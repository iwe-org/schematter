//! Differential check: the published meta-schema must agree with the compiler
//! it mirrors. For a large, systematic set of schema sources we run both
//! `compile_schema` (the ground truth) and the meta-schema, and assert their
//! accept/reject verdicts match. The only tolerated disagreements are the
//! handful of rules JSON Schema cannot express; those are pinned separately in
//! `known_divergences` so any change to them is deliberate and visible.

use indoc::indoc;
use jsonschema::{Draft, Validator};
use schematter_validator::compile_schema;
use serde_json::{json, Map, Value};

const METASCHEMA: &str = include_str!("../../../schema/draft/2026-06/schema.json");

fn metaschema() -> Validator {
    let schema: Value = serde_json::from_str(METASCHEMA).expect("meta-schema is valid JSON");
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("meta-schema is a valid draft 2020-12 schema")
}

fn as_json(yaml: &str) -> Value {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(yaml).expect("schema source is valid YAML");
    serde_json::to_value(value).expect("YAML converts to JSON")
}

/// `(compiler accepts, meta-schema accepts)`.
fn verdicts(meta: &Validator, source: &str) -> (bool, bool) {
    (
        compile_schema(source).is_ok(),
        meta.is_valid(&as_json(source)),
    )
}

/// Assert every case draws the same verdict from both, reporting all mismatches.
fn assert_agreement(cases: &[(String, String)]) {
    let meta = metaschema();
    let mut mismatches = Vec::new();
    for (name, source) in cases {
        let (compiler, meta_ok) = verdicts(&meta, source);
        if compiler != meta_ok {
            mismatches.push(format!(
                "[{name}] compiler={compiler} meta={meta_ok}\n----\n{source}----"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} meta-schema/compiler disagreement(s):\n\n{}",
        mismatches.len(),
        mismatches.join(indoc! {"


        "})
    );
}

/// A block list holding one entry built from the given 4-space-indented body
/// fragments; an empty body becomes a wildcard `{}` entry.
fn one_block(body: &str) -> String {
    if body.is_empty() {
        indoc! {"
            blocks:
              - {}
        "}
        .to_string()
    } else {
        let mut source = String::from(indoc! {"
            blocks:
              -
        "});
        source.push_str(body);
        source
    }
}

/// Every block `type` shape the compiler recognizes, plus unions.
const BLOCK_TYPES: &[(&str, &str)] = &[
    ("no-type", ""),
    ("paragraph", "    type: paragraph\n"),
    ("code", "    type: code\n"),
    ("bullet-list", "    type: bullet-list\n"),
    ("ordered-list", "    type: ordered-list\n"),
    ("quote", "    type: quote\n"),
    ("table", "    type: table\n"),
    ("rule", "    type: rule\n"),
    ("list-union", "    type: [bullet-list, ordered-list]\n"),
    ("code-array", "    type: [code]\n"),
    ("quote-array", "    type: [quote]\n"),
    ("mixed-union", "    type: [code, paragraph]\n"),
];

/// Every type-specific keyword, whose applicability the compiler gates on type.
const BLOCK_KEYWORDS: &[(&str, &str)] = &[
    ("none", ""),
    ("lang", "    lang: { const: rust }\n"),
    ("items", "    items: { text: { maxTokens: 5 } }\n"),
    ("minItems", "    minItems: 1\n"),
    ("maxItems", "    maxItems: 2\n"),
    ("blocks", "    blocks: [ { type: paragraph } ]\n"),
    ("blocks-empty", "    blocks: []\n"),
    ("additionalBlocks", "    additionalBlocks: false\n"),
    ("allBlocks", "    allBlocks: { maxTokens: 5 }\n"),
    ("maxTokens", "    maxTokens: 5\n"),
    ("maxContains", "    maxContains: 2\n"),
];

#[test]
fn agree_on_block_type_and_keyword_grid() {
    let mut cases = Vec::new();
    for (tname, tfrag) in BLOCK_TYPES {
        for (kname, kfrag) in BLOCK_KEYWORDS {
            cases.push((
                format!("block/{tname}+{kname}"),
                one_block(&format!("{tfrag}{kfrag}")),
            ));
        }
    }
    assert_agreement(&cases);
}

#[test]
fn agree_on_header_grid() {
    let headers = [
        (
            "none",
            indoc! {"
            sections:
              - {}
        "},
        ),
        (
            "const",
            indoc! {"
            sections:
              - header: { const: A }
        "},
        ),
        (
            "enum",
            indoc! {"
            sections:
              - header: { enum: [A, B] }
        "},
        ),
        (
            "enum-empty",
            indoc! {"
            sections:
              - header: { enum: [] }
        "},
        ),
        (
            "enum-nonstring",
            indoc! {"
            sections:
              - header: { enum: [1, 2] }
        "},
        ),
        (
            "const-and-enum",
            indoc! {"
            sections:
              - header: { const: A, enum: [A] }
        "},
        ),
        (
            "pattern",
            indoc! {"
            sections:
              - header: { pattern: '.+' }
        "},
        ),
        (
            "pattern-and-const",
            indoc! {"
            sections:
              - header: { pattern: '.+', const: A }
        "},
        ),
        (
            "lengths",
            indoc! {"
                sections:
                  - header: { minLength: 1, maxLength: 3, maxTokens: 2 }
            "},
        ),
        (
            "unknown-key",
            indoc! {"
            sections:
              - header: { const: A, weight: 2 }
        "},
        ),
        (
            "negative-length",
            indoc! {"
            sections:
              - header: { minLength: -1 }
        "},
        ),
        (
            "const-number",
            indoc! {"
            sections:
              - header: { const: 2024 }
        "},
        ),
        (
            "const-bool",
            indoc! {"
            sections:
              - header: { const: true }
        "},
        ),
        (
            "pattern-number",
            indoc! {"
            sections:
              - header: { pattern: 1 }
        "},
        ),
        (
            "enum-mixed-scalars",
            indoc! {"
            sections:
              - header: { enum: [draft, 2, true] }
        "},
        ),
    ];
    let cases = headers
        .iter()
        .map(|(name, src)| (format!("header/{name}"), src.to_string()))
        .collect::<Vec<_>>();
    assert_agreement(&cases);
}

#[test]
fn agree_on_reduced_section_contexts() {
    let allowed = [
        "header: { const: A }",
        "maxTokens: 5",
        "maxDepth: 1",
        "description: x",
    ];
    let forbidden = [
        "sections: []",
        "additionalSections: false",
        "allSections: {}",
        "minContains: 1",
        "maxContains: 1",
        "blocks: []",
        "width: 3",
    ];
    let mut cases = Vec::new();
    for host in ["allSections", "additionalSections"] {
        for entry in allowed.iter().chain(forbidden.iter()) {
            cases.push((format!("{host}/{entry}"), format!("{host}:\n  {entry}\n")));
        }
    }
    assert_agreement(&cases);
}

#[test]
fn agree_on_reduced_block_contexts() {
    let allowed = ["text: { const: A }", "maxTokens: 5", "description: x"];
    let forbidden = [
        "type: paragraph",
        "lang: { const: rust }",
        "items: {}",
        "blocks: []",
        "additionalBlocks: false",
        "allBlocks: {}",
        "minContains: 1",
        "maxContains: 1",
        "minItems: 1",
        "maxItems: 1",
        "width: 3",
    ];
    let mut cases = Vec::new();
    for host in ["allBlocks", "additionalBlocks"] {
        for entry in allowed.iter().chain(forbidden.iter()) {
            cases.push((format!("{host}/{entry}"), format!("{host}:\n  {entry}\n")));
        }
    }
    assert_agreement(&cases);
}

#[test]
fn agree_on_document_level_and_types() {
    let cases: Vec<(String, String)> = [
        ("empty", "{}\n"),
        ("unknown-keyword", "width: 3\n"),
        ("description-number", "description: 5\n"),
        (
            "section-description-bool",
            indoc! {"
            sections:
              - description: true
        "},
        ),
        ("negative-maxTokens", "maxTokens: -1\n"),
        ("string-maxTokens", "maxTokens: nope\n"),
        (
            "good-dialect",
            "$schema: https://document-schema.org/draft/2026-06/schema\n",
        ),
        (
            "bad-dialect",
            "$schema: https://document-schema.org/draft/2027-01/schema\n",
        ),
        (
            "unknown-block-type",
            indoc! {"
            blocks:
              - type: heading
        "},
        ),
        (
            "empty-type-list",
            indoc! {"
            blocks:
              - type: []
        "},
        ),
        (
            "unknown-in-union",
            indoc! {"
            blocks:
              - type: [code, heading]
        "},
        ),
        ("sections-not-array", "sections: 5\n"),
        ("frontmatter-bool", "frontmatter: true\n"),
        ("frontmatter-number", "frontmatter: 5\n"),
        (
            "frontmatter-bad-type",
            indoc! {"
            frontmatter:
              type: 5
        "},
        ),
        (
            "frontmatter-object",
            indoc! {"
            frontmatter:
              type: object
              required: [a]
        "},
        ),
        (
            "unknown-in-item",
            indoc! {"
            blocks:
              - type: bullet-list
                items:
                  weird: 1
        "},
        ),
        (
            "nested-lang-on-paragraph",
            indoc! {"
                sections:
                  - blocks:
                      - type: quote
                        blocks:
                          - type: paragraph
                            lang: { const: rust }
            "},
        ),
    ]
    .iter()
    .map(|(name, src)| (format!("doc/{name}"), src.to_string()))
    .collect();
    assert_agreement(&cases);
}

#[test]
fn agree_on_real_schema_corpus() {
    let corpus = [
        (
            "readme-event",
            indoc! {"
                $schema: https://document-schema.org/draft/2026-06/schema
                description: a dated event page in an agent-maintained knowledge base
                frontmatter:
                  type: object
                  required: [type, date]
                  properties:
                    type: { const: event }
                    date: { type: string, format: date }
                    participants:
                      type: array
                      items: { type: string }
                maxTokens: 400
                maxDepth: 1
                sections:
                  - header: { pattern: '.+\\(\\d{1,2} [A-Z][a-z]+ \\d{4}\\)$' }
                    description: the title carries the event date
                    maxContains: 1
                    blocks:
                      - type: paragraph
                additionalSections: false
            "},
        ),
        (
            "compiler-valid-schema",
            indoc! {"
                $schema: https://document-schema.org/draft/2026-06/schema
                frontmatter:
                  type: object
                  required: [status]
                  properties:
                    status: { enum: [draft, published] }
                maxTokens: 1200
                sections:
                  - header: { pattern: '^[A-Z]', maxTokens: 12 }
                    maxContains: 1
                    sections:
                      - header: { const: Summary }
                        maxContains: 1
                additionalSections: false
            "},
        ),
        (
            "compiler-block-schema",
            indoc! {"
                $schema: https://document-schema.org/draft/2026-06/schema
                allBlocks:
                  text: { maxTokens: 40 }
                sections:
                  - header: { pattern: '.+' }
                    blocks:
                      - type: paragraph
                        maxContains: 1
                      - type: bullet-list
                        minItems: 1
                        items:
                          text: { maxTokens: 40 }
                          blocks:
                            - type: quote
                              blocks:
                                - type: paragraph
                      - type: code
                        lang: { enum: [rust, toml] }
                    additionalBlocks: false
            "},
        ),
        (
            "cli-summary-tasks",
            indoc! {"
                sections:
                  - header: { const: Summary }
                    description: open with a summary
                  - header: { const: Tasks }
                additionalSections: false
            "},
        ),
        (
            "table-and-rule",
            indoc! {"
            blocks:
              - type: table
              - type: rule
        "},
        ),
    ];
    let cases = corpus
        .iter()
        .map(|(name, src)| (format!("corpus/{name}"), src.to_string()))
        .collect::<Vec<_>>();
    assert_agreement(&cases);

    // Every corpus schema is expected to be valid on both sides.
    let meta = metaschema();
    for (name, src) in corpus {
        let (compiler, meta_ok) = verdicts(&meta, src);
        assert!(
            compiler && meta_ok,
            "corpus/{name} should be valid: compiler={compiler} meta={meta_ok}"
        );
    }
}

/// Rules the compiler enforces that JSON Schema cannot express, plus the one
/// keyword the meta-schema deliberately flags that the compiler ignores. Each
/// is pinned to its exact `(compiler, meta)` verdict so a change is deliberate.
#[test]
fn known_divergences() {
    let meta = metaschema();

    // (source, compiler-accepts, meta-accepts, why)
    let cases: &[(&str, bool, bool, &str)] = &[
        (
            indoc! {"
                sections:
                  - {}
                  - header: { const: Last }
            "},
            false,
            true,
            "wildcard-before-others: reachability is not expressible in JSON Schema",
        ),
        (
            indoc! {"
                sections:
                  - header: { const: N }
                  - header: { const: N }
            "},
            false,
            true,
            "duplicate section entry: identity comparison across items is not expressible",
        ),
        (
            indoc! {"
                blocks:
                  - type: paragraph
                  - type: paragraph
            "},
            false,
            true,
            "duplicate block entry: identity comparison across items is not expressible",
        ),
        (
            indoc! {"
                sections:
                  - minContains: 3
                    maxContains: 1
            "},
            false,
            true,
            "minContains > maxContains: cross-field arithmetic is not expressible",
        ),
        (
            indoc! {"
                blocks:
                  - type: bullet-list
                    minItems: 4
                    maxItems: 2
            "},
            false,
            true,
            "minItems > maxItems: cross-field arithmetic is not expressible",
        ),
        (
            indoc! {"
                sections:
                  - header: { pattern: \"[\" }
            "},
            false,
            true,
            "invalid regex: pattern compilation is not performed by structural validation",
        ),
        (
            indoc! {"
                frontmatter:
                  $ref: https://example.com/schema.json
            "},
            false,
            true,
            "external $ref in frontmatter: the 2020-12 meta-schema accepts any $ref string",
        ),
        (
            indoc! {"
                blocks:
                  - type: paragraph
                    target: { const: x }
            "},
            true,
            false,
            "target: inert/undocumented compiler field the meta-schema deliberately flags",
        ),
    ];

    let mut wrong = Vec::new();
    for (source, want_compiler, want_meta, why) in cases {
        let (compiler, meta_ok) = verdicts(&meta, source);
        if compiler != *want_compiler || meta_ok != *want_meta {
            wrong.push(format!(
                "[{why}]\n  want compiler={want_compiler} meta={want_meta}\n  got  compiler={compiler} meta={meta_ok}\n{source}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} divergence(s) changed:\n\n{}",
        wrong.len(),
        wrong.join(indoc! {"


        "})
    );
}

// --- Randomized differential fuzzer -----------------------------------------
//
// A seeded generator emits random schema trees, deliberately staying out of the
// zone where compiler and meta-schema are *allowed* to disagree (see
// `known_divergences`): it never emits `target`, keeps every sibling list to at
// most one entry (no duplicate / wildcard-ordering rejections), orders every
// min/max pair, uses only valid regexes, and never puts an external `$ref` in
// frontmatter. Inside that zone the two must agree on every input, so any
// mismatch the fuzzer surfaces is a genuine meta-schema bug.

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
}

const WORDS: &[&str] = &["Summary", "Tasks", "A", "B", "Details", "Notes"];
const REGEXES: &[&str] = &[".+", "^[A-Z]", "\\d+", ".*x"];
const SINGLE_KINDS: &[&str] = &[
    "paragraph",
    "code",
    "bullet-list",
    "ordered-list",
    "quote",
    "table",
    "rule",
];

fn word(rng: &mut Lcg) -> &'static str {
    WORDS[rng.below(WORDS.len() as u64) as usize]
}

/// A small integer, occasionally negative (a lone negative is rejected by both
/// sides, so it stays in the agreement zone).
fn num(rng: &mut Lcg) -> Value {
    if rng.one_in(8) {
        json!(-1)
    } else {
        json!(rng.below(5) as i64)
    }
}

/// A scalar text value: usually a word, sometimes a bare number or bool to
/// exercise the loader's scalar-to-string coercion.
fn text(rng: &mut Lcg) -> Value {
    match rng.below(4) {
        0 => json!(rng.below(3000) as i64),
        1 => json!(rng.one_in(2)),
        _ => json!(word(rng)),
    }
}

fn gen_type(rng: &mut Lcg) -> Value {
    let single = SINGLE_KINDS[rng.below(SINGLE_KINDS.len() as u64) as usize];
    match rng.below(7) {
        0 => json!([single]),
        1 => json!(["bullet-list", "ordered-list"]),
        2 => json!(["code", "paragraph"]),
        3 => json!([]),
        _ => json!(single),
    }
}

fn gen_header(rng: &mut Lcg) -> Value {
    let mut h = Map::new();
    match rng.below(6) {
        0 => {
            h.insert("const".into(), text(rng));
        }
        1 => {
            h.insert("enum".into(), json!([text(rng), text(rng)]));
        }
        2 => {
            h.insert("pattern".into(), json!(REGEXES[rng.below(4) as usize]));
        }
        3 => {
            h.insert("const".into(), json!(word(rng)));
            h.insert("enum".into(), json!([word(rng)]));
        }
        _ => {}
    }
    if rng.one_in(3) {
        h.insert("minLength".into(), num(rng));
    }
    if rng.one_in(3) {
        h.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(8) {
        h.insert("weird".into(), json!(1));
    }
    Value::Object(h)
}

/// Insert an ordered min/max pair, or a single (possibly negative) bound — never
/// an unordered positive pair, which is the one arithmetic case JSON Schema
/// cannot check.
fn insert_bounds(rng: &mut Lcg, obj: &mut Map<String, Value>, min_key: &str, max_key: &str) {
    match rng.below(3) {
        0 => {
            let a = rng.below(4);
            let b = rng.below(4);
            obj.insert(min_key.into(), json!(a.min(b)));
            obj.insert(max_key.into(), json!(a.max(b)));
        }
        1 => {
            obj.insert(min_key.into(), num(rng));
        }
        _ => {
            obj.insert(max_key.into(), json!(rng.below(4)));
        }
    }
}

fn gen_reduced_block(rng: &mut Lcg) -> Value {
    let mut r = Map::new();
    if rng.one_in(2) {
        r.insert("text".into(), gen_header(rng));
    }
    if rng.one_in(2) {
        r.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(4) {
        r.insert("description".into(), text(rng));
    }
    if rng.one_in(6) {
        r.insert("type".into(), json!("paragraph"));
    }
    Value::Object(r)
}

fn gen_reduced_section(rng: &mut Lcg) -> Value {
    let mut r = Map::new();
    if rng.one_in(2) {
        r.insert("header".into(), gen_header(rng));
    }
    if rng.one_in(2) {
        r.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(3) {
        r.insert("maxDepth".into(), num(rng));
    }
    if rng.one_in(6) {
        r.insert("sections".into(), json!([]));
    }
    Value::Object(r)
}

fn gen_item(rng: &mut Lcg, depth: u32) -> Value {
    let mut it = Map::new();
    if rng.one_in(2) {
        it.insert("text".into(), gen_header(rng));
    }
    if rng.one_in(3) {
        it.insert("maxTokens".into(), num(rng));
    }
    if depth > 0 && rng.one_in(3) {
        it.insert("blocks".into(), json!([gen_block(rng, depth - 1)]));
    }
    if rng.one_in(8) {
        it.insert("weird".into(), json!(1));
    }
    Value::Object(it)
}

fn gen_block(rng: &mut Lcg, depth: u32) -> Value {
    let mut b = Map::new();
    if !rng.one_in(5) {
        b.insert("type".into(), gen_type(rng));
    }
    if rng.one_in(3) {
        b.insert("text".into(), gen_header(rng));
    }
    if rng.one_in(4) {
        b.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(4) {
        b.insert("lang".into(), gen_header(rng));
    }
    if rng.one_in(4) {
        b.insert("items".into(), gen_item(rng, depth));
    }
    if rng.one_in(4) {
        insert_bounds(rng, &mut b, "minItems", "maxItems");
    }
    if rng.one_in(4) {
        insert_bounds(rng, &mut b, "minContains", "maxContains");
    }
    if depth > 0 && rng.one_in(4) {
        b.insert("blocks".into(), json!([gen_block(rng, depth - 1)]));
    } else if rng.one_in(6) {
        b.insert("blocks".into(), json!([]));
    }
    if rng.one_in(5) {
        let v = if rng.one_in(2) {
            json!(false)
        } else {
            gen_reduced_block(rng)
        };
        b.insert("additionalBlocks".into(), v);
    }
    if rng.one_in(6) {
        b.insert("allBlocks".into(), gen_reduced_block(rng));
    }
    if rng.one_in(10) {
        b.insert("nope".into(), json!(1));
    }
    Value::Object(b)
}

fn gen_section(rng: &mut Lcg, depth: u32) -> Value {
    let mut s = Map::new();
    if rng.one_in(2) {
        s.insert("header".into(), gen_header(rng));
    }
    if rng.one_in(3) {
        s.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(4) {
        s.insert("maxDepth".into(), num(rng));
    }
    if rng.one_in(3) {
        insert_bounds(rng, &mut s, "minContains", "maxContains");
    }
    if depth > 0 && rng.one_in(3) {
        s.insert("sections".into(), json!([gen_section(rng, depth - 1)]));
    }
    if depth > 0 && rng.one_in(3) {
        s.insert("blocks".into(), json!([gen_block(rng, depth - 1)]));
    }
    if rng.one_in(5) {
        let v = if rng.one_in(2) {
            json!(false)
        } else {
            gen_reduced_section(rng)
        };
        s.insert("additionalSections".into(), v);
    }
    if rng.one_in(6) {
        s.insert("allSections".into(), gen_reduced_section(rng));
    }
    if rng.one_in(6) {
        s.insert("additionalBlocks".into(), json!(false));
    }
    if rng.one_in(10) {
        s.insert("nope".into(), json!(1));
    }
    Value::Object(s)
}

fn gen_frontmatter(rng: &mut Lcg) -> Value {
    match rng.below(5) {
        0 => json!({"type": "object"}),
        1 => json!({"type": "string"}),
        2 => json!(true),
        3 => json!({"type": "object", "required": ["a"], "properties": {"a": {"type": "string"}}}),
        _ => json!({"type": "array", "items": {"type": "string"}}),
    }
}

fn gen_schema(rng: &mut Lcg, depth: u32) -> Value {
    let mut d = Map::new();
    if rng.one_in(3) {
        d.insert(
            "$schema".into(),
            json!("https://document-schema.org/draft/2026-06/schema"),
        );
    }
    if rng.one_in(3) {
        d.insert("description".into(), text(rng));
    }
    if rng.one_in(3) {
        d.insert("frontmatter".into(), gen_frontmatter(rng));
    }
    if rng.one_in(3) {
        d.insert("maxTokens".into(), num(rng));
    }
    if rng.one_in(4) {
        d.insert("maxDepth".into(), num(rng));
    }
    if rng.one_in(3) {
        d.insert("sections".into(), json!([gen_section(rng, depth)]));
    }
    if rng.one_in(3) {
        d.insert("blocks".into(), json!([gen_block(rng, depth)]));
    }
    if rng.one_in(5) {
        let v = if rng.one_in(2) {
            json!(false)
        } else {
            gen_reduced_section(rng)
        };
        d.insert("additionalSections".into(), v);
    }
    if rng.one_in(6) {
        d.insert("additionalBlocks".into(), json!(false));
    }
    if rng.one_in(6) {
        d.insert("allBlocks".into(), gen_reduced_block(rng));
    }
    if rng.one_in(12) {
        d.insert("nope".into(), json!(1));
    }
    Value::Object(d)
}

#[test]
fn agree_on_randomized_schemas() {
    let meta = metaschema();
    let seeds: [u64; 4] = [
        0x9E3779B97F4A7C15,
        0x0123456789ABCDEF,
        0xDEADBEEFCAFEF00D,
        0xA5A5A5A55A5A5A5A,
    ];
    let per_seed = 6000;
    let iterations = seeds.len() * per_seed;
    let mut mismatches = Vec::new();

    for seed in seeds {
        let mut rng = Lcg(seed);
        for i in 0..per_seed {
            let schema = gen_schema(&mut rng, 3);
            let yaml = serde_yaml_ng::to_string(&schema).expect("schema serializes to YAML");
            let compiler = compile_schema(&yaml).is_ok();
            let meta_ok = meta.is_valid(&as_json(&yaml));
            if compiler != meta_ok && mismatches.len() < 10 {
                mismatches.push(format!(
                    "seed={seed:#x} #{i} compiler={compiler} meta={meta_ok}\n----\n{yaml}----"
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "meta-schema/compiler disagreement(s) over {iterations} random schemas:\n\n{}",
        mismatches.join(indoc! {"


        "})
    );
}

/// The loader coerces bare YAML scalars to strings on the direct path
/// (`sections`, `allSections`, `allBlocks`, …) but NOT under the untagged
/// `additionalSections` / `additionalBlocks` unions. The meta-schema must draw
/// the same context-dependent line: loose text there, strict text here.
#[test]
fn agree_on_scalar_coercion_contexts() {
    let cases: Vec<(String, String)> = [
        (
            "all-sections-const-number",
            indoc! {"
                allSections:
                  header: { const: 2024 }
            "},
        ),
        (
            "add-sections-const-number",
            indoc! {"
                additionalSections:
                  header: { const: 2024 }
            "},
        ),
        (
            "all-sections-const-string",
            indoc! {"
                allSections:
                  header: { const: Draft }
            "},
        ),
        (
            "add-sections-const-string",
            indoc! {"
                additionalSections:
                  header: { const: Draft }
            "},
        ),
        (
            "all-sections-desc-number",
            indoc! {"
                allSections:
                  description: 5
            "},
        ),
        (
            "add-sections-desc-number",
            indoc! {"
                additionalSections:
                  description: 5
            "},
        ),
        (
            "all-blocks-text-number",
            indoc! {"
                allBlocks:
                  text: { const: 2024 }
            "},
        ),
        (
            "add-blocks-text-number",
            indoc! {"
                additionalBlocks:
                  text: { const: 2024 }
            "},
        ),
        (
            "add-sections-enum-number",
            indoc! {"
                additionalSections:
                  header: { enum: [1, 2] }
            "},
        ),
        (
            "add-sections-maxtokens",
            indoc! {"
                additionalSections:
                  maxTokens: 5
            "},
        ),
        (
            "section-const-number",
            indoc! {"
                sections:
                  - header: { const: 2024 }
            "},
        ),
    ]
    .iter()
    .map(|(name, src)| (format!("coerce/{name}"), src.to_string()))
    .collect();
    assert_agreement(&cases);
}
