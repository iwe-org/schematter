//! Building the [`Document`] model from markdown source.
//!
//! This is the crate's independence boundary: iwe projects its arena graph into
//! the [`Document`] model, but here we fold a `pulldown-cmark` event stream into
//! the same shape directly, so the validator has no dependency on iwe's graph.
//!
//! # Token counting
//!
//! `subtree_tokens` and `body_tokens` are counted over the **raw markdown source
//! span** of the node — the slice of the original document that produced it —
//! rather than over a re-rendered canonical form. This is a deliberate,
//! documented convention: it needs no renderer and is faithful to the document
//! as written. It differs from iwe, which re-renders each subtree and so, for
//! example, omits a list item's own bullet marker from the item's count; here
//! the marker and any indentation are part of the source slice and are counted.
//! `text_tokens` and `header_tokens` count the node's own plain text.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde_json::{Map, Value};

use crate::document::{Block, BlockKind, Document, Item, Section};

/// Build a [`Document`] from markdown `source`, counting tokens with `count`.
///
/// `count` is injected so the model stays tokenizer-agnostic; the CLI and the
/// [`crate::validate`] convenience wire in [`crate::tokens::count_tokens`].
pub fn build_document(source: &str, count: impl Fn(&str) -> usize + Copy) -> Document {
    let (frontmatter, frontmatter_error, body) = split_frontmatter(source);
    let body_tokens = count(body);

    let mut builder = Builder {
        source: body,
        count,
        events: Parser::new_ext(body, options()).into_offset_iter(),
    };
    let (blocks, headings) = builder.parse_top_level();

    let mut index = 0;
    let sections = build_sections(&headings, &mut index, 0, 1, body.len(), body, count);

    Document {
        frontmatter,
        frontmatter_error,
        body_tokens,
        blocks,
        sections,
    }
}

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_WIKILINKS | Options::ENABLE_STRIKETHROUGH
}

/// A heading and the content blocks directly beneath it, before any subheading.
struct HeadingRec {
    raw_level: usize,
    header: String,
    header_tokens: usize,
    start: usize,
    blocks: Vec<Block>,
}

struct Builder<'a, I, F> {
    source: &'a str,
    count: F,
    events: I,
}

impl<'a, I, F> Builder<'a, I, F>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
    F: Fn(&str) -> usize + Copy,
{
    fn tokens(&self, span: Range<usize>) -> usize {
        (self.count)(&self.source[span])
    }

    /// Walk the top-level event stream, collecting the leading blocks (before
    /// the first heading) and one [`HeadingRec`] per heading.
    fn parse_top_level(&mut self) -> (Vec<Block>, Vec<HeadingRec>) {
        let mut leading = Vec::new();
        let mut headings: Vec<HeadingRec> = Vec::new();

        while let Some((event, span)) = self.events.next() {
            let block = match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    let (header, _) = self.read_heading_text();
                    let header_tokens = (self.count)(&header);
                    headings.push(HeadingRec {
                        raw_level: level as usize,
                        header,
                        header_tokens,
                        start: span.start,
                        blocks: Vec::new(),
                    });
                    continue;
                }
                Event::Rule => Some(self.rule_block(span)),
                Event::Start(tag) => self.build_block(tag, span.start),
                _ => None,
            };

            if let Some(block) = block {
                match headings.last_mut() {
                    Some(heading) => heading.blocks.push(block),
                    None => leading.push(block),
                }
            }
        }

        (leading, headings)
    }

    /// Consume a heading's inline events up to its `End`, returning the plain
    /// text (markup stripped) and the heading's end offset.
    fn read_heading_text(&mut self) -> (String, usize) {
        let mut text = String::new();
        let mut end = 0;
        for (event, span) in self.events.by_ref() {
            match event {
                Event::End(TagEnd::Heading(_)) => {
                    end = span.end;
                    break;
                }
                Event::Text(chunk) | Event::Code(chunk) => text.push_str(&chunk),
                _ => {}
            }
        }
        (text, end)
    }

    /// Build a single block from the `Start` tag that opens it, consuming the
    /// stream through its matching `End`. Returns `None` for tags that do not
    /// begin a content block.
    fn build_block(&mut self, tag: Tag<'a>, start: usize) -> Option<Block> {
        let block = match tag {
            Tag::Paragraph => {
                let (text, end) = self.read_paragraph();
                self.paragraph_block(text, start..end)
            }
            Tag::CodeBlock(kind) => self.code_block(kind, start),
            Tag::List(first) => self.list_block(first.is_some(), start),
            Tag::BlockQuote(_) => {
                let (blocks, end) = self.read_quote_blocks();
                Block {
                    kind: BlockKind::Quote,
                    text: String::new(),
                    text_tokens: 0,
                    subtree_tokens: self.tokens(start..end),
                    lang: None,
                    items: Vec::new(),
                    blocks,
                }
            }
            Tag::Table(_) => self.table_block(start),
            _ => return None,
        };
        Some(block)
    }

    /// A paragraph's plain text, with inline markup (including link display
    /// text) flattened. Returns the text and the paragraph's end offset.
    fn read_paragraph(&mut self) -> (String, usize) {
        let mut text = String::new();
        let mut end = 0;
        for (event, span) in self.events.by_ref() {
            match event {
                Event::End(TagEnd::Paragraph) => {
                    end = span.end;
                    break;
                }
                Event::Text(chunk) | Event::Code(chunk) => text.push_str(&chunk),
                Event::SoftBreak | Event::HardBreak => text.push(' '),
                _ => {}
            }
        }
        (text, end)
    }

    fn paragraph_block(&self, text: String, span: Range<usize>) -> Block {
        let text_tokens = (self.count)(&text);
        let subtree_tokens = self.tokens(span);
        Block {
            kind: BlockKind::Paragraph,
            text,
            text_tokens,
            subtree_tokens,
            lang: None,
            items: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn code_block(&mut self, kind: CodeBlockKind<'a>, start: usize) -> Block {
        let lang = match kind {
            CodeBlockKind::Fenced(info) => info
                .split_whitespace()
                .next()
                .filter(|token| !token.is_empty())
                .map(str::to_string),
            CodeBlockKind::Indented => None,
        };

        let mut text = String::new();
        let mut end = 0;
        for (event, span) in self.events.by_ref() {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    end = span.end;
                    break;
                }
                Event::Text(chunk) => text.push_str(&chunk),
                _ => {}
            }
        }

        let text_tokens = (self.count)(&text);
        Block {
            kind: BlockKind::Code,
            text,
            text_tokens,
            subtree_tokens: self.tokens(start..end),
            lang,
            items: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn list_block(&mut self, ordered: bool, start: usize) -> Block {
        let mut items = Vec::new();
        let mut end = 0;
        while let Some((event, span)) = self.events.next() {
            match event {
                Event::End(TagEnd::List(_)) => {
                    end = span.end;
                    break;
                }
                Event::Start(Tag::Item) => items.push(self.read_item(span.start)),
                _ => {}
            }
        }

        Block {
            kind: if ordered {
                BlockKind::OrderedList
            } else {
                BlockKind::BulletList
            },
            text: String::new(),
            text_tokens: 0,
            subtree_tokens: self.tokens(start..end),
            lang: None,
            items,
            blocks: Vec::new(),
        }
    }

    /// A list item's own text is its leading inline run (tight) or first
    /// paragraph (loose); everything after becomes its child blocks. A leading
    /// ref does not become the item text — it is kept as a child block.
    fn read_item(&mut self, start: usize) -> Item {
        let mut text = String::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut text_taken = false;
        let mut end = 0;

        while let Some((event, span)) = self.events.next() {
            match event {
                Event::End(TagEnd::Item) => {
                    end = span.end;
                    break;
                }
                Event::Start(Tag::Paragraph) => {
                    let (para_text, para_end) = self.read_paragraph();
                    if !text_taken && blocks.is_empty() && text.is_empty() {
                        text = para_text;
                    } else {
                        blocks.push(self.paragraph_block(para_text, span.start..para_end));
                    }
                    text_taken = true;
                }
                Event::Rule => {
                    text_taken = true;
                    blocks.push(self.rule_block(span));
                }
                Event::Start(tag) => {
                    text_taken = true;
                    if let Some(block) = self.build_block(tag, span.start) {
                        blocks.push(block);
                    }
                }
                Event::Text(chunk) | Event::Code(chunk) if !text_taken && blocks.is_empty() => {
                    text.push_str(&chunk);
                }
                Event::SoftBreak | Event::HardBreak if !text_taken && blocks.is_empty() => {
                    text.push(' ');
                }
                _ => {}
            }
        }

        let text_tokens = (self.count)(&text);
        Item {
            text,
            text_tokens,
            subtree_tokens: self.tokens(start..end),
            blocks,
        }
    }

    fn read_quote_blocks(&mut self) -> (Vec<Block>, usize) {
        let mut blocks = Vec::new();
        let mut end = 0;
        while let Some((event, span)) = self.events.next() {
            match event {
                Event::End(TagEnd::BlockQuote(_)) => {
                    end = span.end;
                    break;
                }
                Event::Start(Tag::Heading { .. }) => {
                    self.read_heading_text();
                }
                Event::Rule => blocks.push(self.rule_block(span)),
                Event::Start(tag) => {
                    if let Some(block) = self.build_block(tag, span.start) {
                        blocks.push(block);
                    }
                }
                _ => {}
            }
        }
        (blocks, end)
    }

    fn table_block(&mut self, start: usize) -> Block {
        let mut cells: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_cell = false;
        let mut end = 0;

        for (event, span) in self.events.by_ref() {
            match event {
                Event::End(TagEnd::Table) => {
                    end = span.end;
                    break;
                }
                Event::Start(Tag::TableCell) => {
                    in_cell = true;
                    current = String::new();
                }
                Event::End(TagEnd::TableCell) => {
                    in_cell = false;
                    if !current.is_empty() {
                        cells.push(std::mem::take(&mut current));
                    }
                }
                Event::Text(chunk) | Event::Code(chunk) if in_cell => {
                    current.push_str(&chunk);
                }
                _ => {}
            }
        }

        let text = cells.join(" ");
        let text_tokens = (self.count)(&text);
        Block {
            kind: BlockKind::Table,
            text,
            text_tokens,
            subtree_tokens: self.tokens(start..end),
            lang: None,
            items: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn rule_block(&self, span: Range<usize>) -> Block {
        Block {
            kind: BlockKind::Rule,
            text: String::new(),
            text_tokens: 0,
            subtree_tokens: self.tokens(span),
            lang: None,
            items: Vec::new(),
            blocks: Vec::new(),
        }
    }
}

/// Fold the flat heading list into a section tree by structural nesting: a
/// heading nests under the nearest shallower one, and its structural `level` is
/// its depth in that tree (`1` for the shallowest), independent of the absolute
/// `#` count — the normalization the spec describes.
fn build_sections(
    headings: &[HeadingRec],
    index: &mut usize,
    parent_raw_level: usize,
    depth: usize,
    body_len: usize,
    source: &str,
    count: impl Fn(&str) -> usize + Copy,
) -> Vec<Section> {
    let mut sections = Vec::new();
    while *index < headings.len() && headings[*index].raw_level > parent_raw_level {
        let current = *index;
        let raw_level = headings[current].raw_level;
        *index += 1;
        let children = build_sections(
            headings,
            index,
            raw_level,
            depth + 1,
            body_len,
            source,
            count,
        );
        let end = headings
            .get(*index)
            .map(|heading| heading.start)
            .unwrap_or(body_len);
        let subtree_tokens = count(&source[headings[current].start..end]);
        sections.push(Section {
            header: headings[current].header.clone(),
            level: depth,
            header_tokens: headings[current].header_tokens,
            subtree_tokens,
            blocks: headings[current].blocks.clone(),
            sections: children,
        });
    }
    sections
}

/// Split a leading `---` … `---` (or `...`) frontmatter block off `source`,
/// returning the parsed mapping (`{}` when absent), the parse failure when the
/// block is present but unparseable, and the remaining body.
fn split_frontmatter(source: &str) -> (Value, Option<String>, &str) {
    match frontmatter_split(source) {
        Some((yaml, body)) => {
            let (frontmatter, error) = parse_frontmatter(yaml);
            (frontmatter, error, body)
        }
        None => (Value::Object(Map::new()), None, source),
    }
}

fn frontmatter_split(source: &str) -> Option<(&str, &str)> {
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }

    None
}

fn parse_frontmatter(yaml: &str) -> (Value, Option<String>) {
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) {
        Ok(serde_yaml_ng::Value::Mapping(mapping)) => {
            (Value::Object(yaml_mapping_to_object(&mapping)), None)
        }
        Ok(serde_yaml_ng::Value::Null) => (Value::Object(Map::new()), None),
        Ok(_) => (
            Value::Object(Map::new()),
            Some("not a YAML mapping".to_string()),
        ),
        Err(error) => (
            Value::Object(Map::new()),
            Some(format!("invalid YAML: {error}")),
        ),
    }
}

/// Reserved-prefix frontmatter fields are invisible to the schema, mirroring the
/// iwe query engine.
fn is_reserved_segment(name: &str) -> bool {
    matches!(name.chars().next(), Some('_' | '$' | '.' | '#' | '@'))
}

fn yaml_mapping_to_object(mapping: &serde_yaml_ng::Mapping) -> Map<String, Value> {
    let mut object = Map::new();
    for (key, value) in mapping {
        if let Some(name) = key.as_str() {
            if is_reserved_segment(name) {
                continue;
            }
            object.insert(name.to_string(), yaml_to_json(value));
        }
    }
    object
}

fn yaml_to_json(value: &serde_yaml_ng::Value) -> Value {
    match value {
        serde_yaml_ng::Value::Null => Value::Null,
        serde_yaml_ng::Value::Bool(boolean) => Value::Bool(*boolean),
        serde_yaml_ng::Value::Number(number) => yaml_number_to_json(number),
        serde_yaml_ng::Value::String(text) => Value::String(text.clone()),
        serde_yaml_ng::Value::Sequence(items) => {
            Value::Array(items.iter().map(yaml_to_json).collect())
        }
        serde_yaml_ng::Value::Mapping(nested) => Value::Object(yaml_mapping_to_object(nested)),
        serde_yaml_ng::Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

fn yaml_number_to_json(number: &serde_yaml_ng::Number) -> Value {
    if let Some(integer) = number.as_i64() {
        Value::Number(integer.into())
    } else if let Some(unsigned) = number.as_u64() {
        Value::Number(unsigned.into())
    } else if let Some(float) = number.as_f64() {
        serde_json::Number::from_f64(float)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use serde_json::json;

    use super::*;

    fn zero(_: &str) -> usize {
        0
    }

    fn words(text: &str) -> usize {
        text.split_whitespace().count()
    }

    fn block(kind: BlockKind, text: &str) -> Block {
        Block {
            kind,
            text: text.to_string(),
            text_tokens: 0,
            subtree_tokens: 0,
            lang: None,
            items: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn item(text: &str, blocks: Vec<Block>) -> Item {
        Item {
            text: text.to_string(),
            text_tokens: 0,
            subtree_tokens: 0,
            blocks,
        }
    }

    fn section(header: &str, level: usize, blocks: Vec<Block>, sections: Vec<Section>) -> Section {
        Section {
            header: header.to_string(),
            level,
            header_tokens: 0,
            subtree_tokens: 0,
            blocks,
            sections,
        }
    }

    #[test]
    fn builds_section_tree_with_levels_and_strips_reserved_frontmatter() {
        let source = indoc! {"
            ---
            status: draft
            _internal: secret
            ---
            # Summary

            text

            ## Details

            # Tasks
        "};
        let document = build_document(source, zero);
        assert_eq!(
            document,
            Document {
                frontmatter: json!({ "status": "draft" }),
                frontmatter_error: None,
                body_tokens: 0,
                blocks: vec![],
                sections: vec![
                    section(
                        "Summary",
                        1,
                        vec![block(BlockKind::Paragraph, "text")],
                        vec![section("Details", 2, vec![], vec![])],
                    ),
                    section("Tasks", 1, vec![], vec![]),
                ],
            }
        );
    }

    #[test]
    fn absent_frontmatter_is_empty_object() {
        let document = build_document("# Title\n", zero);
        assert_eq!(document.frontmatter, json!({}));
        assert_eq!(document.frontmatter_error, None);
    }

    #[test]
    fn unparseable_frontmatter_sets_the_error_and_keeps_the_body() {
        let document = build_document(
            indoc! {"
            ---
            status: [unclosed
            ---
            # Title
        "},
            zero,
        );
        assert_eq!(document.frontmatter, json!({}));
        let error = document.frontmatter_error.as_deref().unwrap();
        assert!(error.starts_with("invalid YAML: "), "got: {error}");
        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].header, "Title");
    }

    #[test]
    fn duplicate_frontmatter_keys_set_the_error() {
        let document = build_document(
            indoc! {"
            ---
            status: a
            status: b
            ---
            # Title
        "},
            zero,
        );
        let error = document.frontmatter_error.as_deref().unwrap();
        assert!(error.starts_with("invalid YAML: "), "got: {error}");
    }

    #[test]
    fn non_mapping_frontmatter_sets_the_error() {
        let document = build_document(
            indoc! {"
            ---
            - item
            ---
            # Title
        "},
            zero,
        );
        assert_eq!(document.frontmatter, json!({}));
        assert_eq!(
            document.frontmatter_error.as_deref(),
            Some("not a YAML mapping")
        );
    }

    #[test]
    fn empty_frontmatter_block_is_empty_object_without_error() {
        let document = build_document(
            indoc! {"
            ---
            ---
            # Title
        "},
            zero,
        );
        assert_eq!(document.frontmatter, json!({}));
        assert_eq!(document.frontmatter_error, None);
    }

    #[test]
    fn skipped_heading_level_normalizes_to_structural_depth() {
        let document = build_document(
            indoc! {"
            # Top

            ### Deep
        "},
            zero,
        );
        assert_eq!(
            document.sections,
            vec![section(
                "Top",
                1,
                vec![],
                vec![section("Deep", 2, vec![], vec![])],
            )],
        );
    }

    #[test]
    fn extracts_every_block_kind_and_splits_subsections() {
        let source = indoc! {"
            # Section

            para one

            - a
            - b

            ```rust
            fn x() {}
            ```

            > quoted

            | H |
            | - |
            | c |

            [link](other)

            ---

            ## Sub

            sub para
        "};
        let document = build_document(source, zero);
        assert_eq!(
            document.sections,
            vec![section(
                "Section",
                1,
                vec![
                    block(BlockKind::Paragraph, "para one"),
                    Block {
                        items: vec![item("a", vec![]), item("b", vec![])],
                        ..block(BlockKind::BulletList, "")
                    },
                    Block {
                        lang: Some("rust".to_string()),
                        ..block(BlockKind::Code, "fn x() {}\n")
                    },
                    Block {
                        blocks: vec![block(BlockKind::Paragraph, "quoted")],
                        ..block(BlockKind::Quote, "")
                    },
                    block(BlockKind::Table, "H c"),
                    block(BlockKind::Paragraph, "link"),
                    block(BlockKind::Rule, ""),
                ],
                vec![section(
                    "Sub",
                    2,
                    vec![block(BlockKind::Paragraph, "sub para")],
                    vec![],
                )],
            )],
        );
    }

    #[test]
    fn extracts_document_blocks_above_the_first_heading() {
        let document = build_document(
            indoc! {"
            lead

            - a

            # Section
        "},
            zero,
        );
        assert_eq!(
            document.blocks,
            vec![
                block(BlockKind::Paragraph, "lead"),
                Block {
                    items: vec![item("a", vec![])],
                    ..block(BlockKind::BulletList, "")
                },
            ]
        );
        assert_eq!(
            document.sections,
            vec![section("Section", 1, vec![], vec![])]
        );
    }

    #[test]
    fn counts_header_and_body_tokens_over_source() {
        let document = build_document(
            indoc! {"
            # Two Words

            body text here
        "},
            words,
        );
        assert_eq!(document.sections[0].header, "Two Words");
        assert_eq!(document.sections[0].header_tokens, 2);
        // whole body source: "# Two Words\n\nbody text here\n"
        assert_eq!(document.body_tokens, words("# Two Words body text here"));
    }

    #[test]
    fn nested_item_takes_first_paragraph_as_text_and_keeps_rest_as_blocks() {
        let source = indoc! {"
            # Section

            - outer one

                nested para

                - inner one
                - inner two
            - outer two
        "};
        let document = build_document(source, zero);
        let list = &document.sections[0].blocks[0];
        assert_eq!(list.kind, BlockKind::BulletList);
        assert_eq!(
            list.items,
            vec![
                item(
                    "outer one",
                    vec![
                        block(BlockKind::Paragraph, "nested para"),
                        Block {
                            items: vec![item("inner one", vec![]), item("inner two", vec![])],
                            ..block(BlockKind::BulletList, "")
                        },
                    ],
                ),
                item("outer two", vec![]),
            ],
        );
    }

    #[test]
    fn parses_ordered_lists() {
        let document = build_document(
            indoc! {"
            # List

            1. one
            2. two
        "},
            zero,
        );
        assert_eq!(
            document.sections[0].blocks,
            vec![Block {
                items: vec![item("one", vec![]), item("two", vec![])],
                ..block(BlockKind::OrderedList, "")
            }]
        );
    }

    #[test]
    fn frontmatter_converts_scalars_sequences_and_nested_maps() {
        let source = indoc! {"
            ---
            count: 3
            ratio: 1.5
            done: true
            missing: null
            tags: [a, b]
            meta:
              owner: dh
            ---
            # Title
        "};
        let document = build_document(source, zero);
        assert_eq!(
            document.frontmatter,
            json!({
                "count": 3,
                "ratio": 1.5,
                "done": true,
                "missing": null,
                "tags": ["a", "b"],
                "meta": { "owner": "dh" }
            })
        );
    }

    #[test]
    fn unterminated_frontmatter_is_body_without_error() {
        let document = build_document(
            indoc! {"
            ---
            status: draft
            # Title
        "},
            zero,
        );
        assert_eq!(document.frontmatter, json!({}));
        assert_eq!(document.frontmatter_error, None);
    }

    #[test]
    fn indented_code_block_has_no_lang() {
        let document = build_document(
            indoc! {"
            # Code

                let x = 1;
        "},
            zero,
        );
        assert_eq!(
            document.sections[0].blocks,
            vec![Block {
                lang: None,
                ..block(BlockKind::Code, "let x = 1;\n")
            }]
        );
    }

    #[test]
    fn line_breaks_join_paragraph_text() {
        let document = build_document(
            indoc! {"
            # Title

            line one
            line two\\
            line three
        "},
            zero,
        );
        assert_eq!(
            document.sections[0].blocks,
            vec![block(BlockKind::Paragraph, "line one line two line three")]
        );
    }

    #[test]
    fn line_breaks_join_item_text() {
        let document = build_document(
            indoc! {"
            # Title

            - first line
              second line
        "},
            zero,
        );
        let list = &document.sections[0].blocks[0];
        assert_eq!(list.items, vec![item("first line second line", vec![])]);
    }

    #[test]
    fn heading_inside_quote_is_skipped() {
        let document = build_document(
            indoc! {"
            # Title

            > # Quoted
            >
            > para
        "},
            zero,
        );
        assert_eq!(
            document.sections[0].blocks,
            vec![Block {
                blocks: vec![block(BlockKind::Paragraph, "para")],
                ..block(BlockKind::Quote, "")
            }]
        );
    }

    #[test]
    fn rule_inside_list_item_becomes_child_block() {
        let document = build_document(
            indoc! {"
            # Title

            - outer

              ***
        "},
            zero,
        );
        let list = &document.sections[0].blocks[0];
        assert_eq!(
            list.items,
            vec![item("outer", vec![block(BlockKind::Rule, "")])]
        );
    }

    #[test]
    fn html_blocks_are_ignored() {
        let document = build_document(
            indoc! {"
            <div>
            hi
            </div>

            # Title
        "},
            zero,
        );
        assert_eq!(document.blocks, vec![]);
        assert_eq!(document.sections, vec![section("Title", 1, vec![], vec![])]);
    }

    #[test]
    fn heading_text_strips_inline_markup() {
        let document = build_document("# *Big* `deal`\n", zero);
        assert_eq!(document.sections[0].header, "Big deal");
    }

    #[test]
    fn rule_inside_quote_becomes_child_block() {
        let document = build_document(
            indoc! {"
            # Title

            > before
            >
            > ***
        "},
            zero,
        );
        assert_eq!(
            document.sections[0].blocks,
            vec![Block {
                blocks: vec![
                    block(BlockKind::Paragraph, "before"),
                    block(BlockKind::Rule, ""),
                ],
                ..block(BlockKind::Quote, "")
            }]
        );
    }

    #[test]
    fn frontmatter_skips_non_string_keys_and_unwraps_tags() {
        let source = indoc! {"
            ---
            1: numeric key
            big: 18446744073709551615
            tagged: !note hello
            ---
            # Title
        "};
        let document = build_document(source, zero);
        assert_eq!(
            document.frontmatter,
            json!({
                "big": 18446744073709551615u64,
                "tagged": "hello"
            })
        );
        assert_eq!(document.frontmatter_error, None);
    }
}
