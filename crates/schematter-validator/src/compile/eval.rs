use super::{
    CompiledAdditional, CompiledBlock, CompiledBlockAdditional, CompiledHeader, CompiledReduced,
    CompiledReducedBlock, CompiledSchema, CompiledSection,
};
use crate::document::{Block, BlockKind, Document, Section};
use crate::violation::{Crumb, Violation};

impl CompiledSchema {
    pub fn validate(&self, document: &Document) -> Vec<Violation> {
        let mut out = Vec::new();

        if let Some(error) = &document.frontmatter_error {
            out.push(Violation {
                breadcrumb: vec![Crumb::Frontmatter],
                message: error.clone(),
                hint: self.description.clone(),
                keyword: "frontmatter".to_string(),
                schema_pointer: String::new(),
            });
        } else if let Some(validator) = &self.frontmatter {
            for error in validator.iter_errors(&document.frontmatter) {
                let schema_path = error.schema_path().as_str().to_string();
                out.push(Violation {
                    breadcrumb: frontmatter_breadcrumb(error.instance_path().as_str()),
                    message: error.to_string(),
                    hint: frontmatter_hint(
                        self.frontmatter_source.as_ref(),
                        &schema_path,
                        self.description.as_deref(),
                    ),
                    keyword: last_segment(&schema_path),
                    schema_pointer: format!("/frontmatter{schema_path}"),
                });
            }
        }

        if let Some(max) = self.max_tokens {
            if document.body_tokens > max {
                out.push(violation(
                    &[],
                    format!(
                        "body is {}, greater than the maximum of {max}",
                        count_noun(document.body_tokens, "token")
                    ),
                    hint(&[], &[self.description.as_deref()]),
                    "/maxTokens".to_string(),
                    "maxTokens",
                ));
            }
        }

        let descs = vec![self.description.as_deref()];

        if let Some(limit) = self.max_depth {
            report_deep(
                &document.sections,
                limit,
                "/maxDepth",
                self.description.as_deref(),
                &descs,
                &[],
                &mut out,
            );
        }

        let block_scope: Vec<&CompiledReducedBlock> = self.all_blocks.iter().collect();
        walk_blocks(
            &self.blocks,
            &self.additional_blocks,
            &block_scope,
            &document.blocks,
            &[],
            &descs,
            &mut out,
        );

        let scope: Vec<&CompiledReduced> = self.all_sections.iter().collect();
        walk(
            &self.sections,
            &self.additional_sections,
            &scope,
            &block_scope,
            &document.sections,
            &[],
            &descs,
            &mut out,
        );

        out
    }
}

#[allow(clippy::too_many_arguments)]
fn walk<'a>(
    entries: &'a [CompiledSection],
    additional: &'a CompiledAdditional,
    all_scope: &[&'a CompiledReduced],
    block_scope: &[&'a CompiledReducedBlock],
    sections: &[Section],
    crumbs: &[Crumb],
    descs: &[Option<&'a str>],
    out: &mut Vec<Violation>,
) {
    let binds = bind_sections(entries, sections);
    let mut counts = vec![0usize; entries.len()];
    for entry in binds.iter().flatten() {
        counts[*entry] += 1;
    }

    let deny_additional = matches!(additional, CompiledAdditional::Deny { .. });
    let mut out_of_order: Vec<Option<usize>> = vec![None; sections.len()];
    let mut suppressed = vec![false; entries.len()];
    if deny_additional {
        for (index, section) in sections.iter().enumerate() {
            if binds[index].is_none() {
                if let Some(entry) = entries
                    .iter()
                    .position(|entry| entry_identity_matches(entry, &section.header))
                {
                    out_of_order[index] = Some(entry);
                    suppressed[entry] = true;
                }
            }
        }
    }

    for (index, entry) in entries.iter().enumerate() {
        let count = counts[index];
        if count < entry.min_contains && !suppressed[index] {
            let message = if count == 0 {
                format!("required section {} is missing", entry_name(entry, index))
            } else {
                format!(
                    "section {} appears {}, less than the minimum of {}",
                    entry_name(entry, index),
                    count_noun(count, "time"),
                    entry.min_contains
                )
            };
            out.push(violation(
                crumbs,
                message,
                hint(
                    &[
                        header_description(&entry.header),
                        entry.description.as_deref(),
                    ],
                    descs,
                ),
                format!("{}/minContains", entry.pointer),
                "minContains",
            ));
        }
        if let Some(max) = entry.max_contains {
            if count > max {
                out.push(violation(
                    crumbs,
                    format!(
                        "section {} appears {}, greater than the maximum of {max}",
                        entry_name(entry, index),
                        count_noun(count, "time")
                    ),
                    hint(&[entry.description.as_deref()], descs),
                    format!("{}/maxContains", entry.pointer),
                    "maxContains",
                ));
            }
        }
    }

    for (index, section) in sections.iter().enumerate() {
        let mut child_crumbs = crumbs.to_vec();
        child_crumbs.push(section_crumb(section, index));

        for reduced in all_scope {
            apply_reduced(reduced, section, &child_crumbs, descs, out);
        }

        match binds[index] {
            Some(entry_index) => {
                let entry = &entries[entry_index];
                if let Some(header) = &entry.header {
                    check_text(
                        header,
                        "header",
                        &section.header,
                        Some(section.header_tokens),
                        false,
                        entry.description.as_deref(),
                        descs,
                        &child_crumbs,
                        out,
                    );
                }
                if let Some(max) = entry.max_tokens {
                    if section.subtree_tokens > max {
                        out.push(violation(
                            &child_crumbs,
                            format!(
                                "section is {}, greater than the maximum of {max}",
                                count_noun(section.subtree_tokens, "token")
                            ),
                            hint(&[entry.description.as_deref()], descs),
                            format!("{}/maxTokens", entry.pointer),
                            "maxTokens",
                        ));
                    }
                }
                if let Some(depth) = entry.max_depth {
                    report_deep(
                        &section.sections,
                        section.level + depth,
                        &format!("{}/maxDepth", entry.pointer),
                        entry.description.as_deref(),
                        descs,
                        &child_crumbs,
                        out,
                    );
                }

                let mut child_scope = all_scope.to_vec();
                if let Some(reduced) = &entry.all_sections {
                    child_scope.push(reduced);
                }
                let mut child_block_scope = block_scope.to_vec();
                if let Some(reduced) = &entry.all_blocks {
                    child_block_scope.push(reduced);
                }
                let mut child_descs = descs.to_vec();
                child_descs.push(entry.description.as_deref());
                walk_blocks(
                    &entry.blocks,
                    &entry.additional_blocks,
                    &child_block_scope,
                    &section.blocks,
                    &child_crumbs,
                    &child_descs,
                    out,
                );
                walk(
                    &entry.sections,
                    &entry.additional_sections,
                    &child_scope,
                    &child_block_scope,
                    &section.sections,
                    &child_crumbs,
                    &child_descs,
                    out,
                );
            }
            None => {
                match additional {
                    CompiledAdditional::Allow => {}
                    CompiledAdditional::Deny { pointer } => {
                        let message = if out_of_order[index].is_some() {
                            "section is out of order".to_string()
                        } else {
                            "unexpected section".to_string()
                        };
                        out.push(violation(
                            &child_crumbs,
                            message,
                            hint(&[], descs),
                            pointer.clone(),
                            "additionalSections",
                        ));
                    }
                    CompiledAdditional::Schema(reduced) => {
                        apply_reduced(reduced, section, &child_crumbs, descs, out);
                    }
                }
                walk_blocks(
                    &[],
                    &CompiledBlockAdditional::Allow,
                    block_scope,
                    &section.blocks,
                    &child_crumbs,
                    descs,
                    out,
                );
                walk(
                    &[],
                    &CompiledAdditional::Allow,
                    all_scope,
                    block_scope,
                    &section.sections,
                    &child_crumbs,
                    descs,
                    out,
                );
            }
        }
    }
}

fn apply_reduced<'a>(
    reduced: &'a CompiledReduced,
    section: &Section,
    crumbs: &[Crumb],
    descs: &[Option<&'a str>],
    out: &mut Vec<Violation>,
) {
    if let Some(header) = &reduced.header {
        check_text(
            header,
            "header",
            &section.header,
            Some(section.header_tokens),
            true,
            reduced.description.as_deref(),
            descs,
            crumbs,
            out,
        );
    }
    if let Some(max) = reduced.max_tokens {
        if section.subtree_tokens > max {
            out.push(violation(
                crumbs,
                format!(
                    "section is {}, greater than the maximum of {max}",
                    count_noun(section.subtree_tokens, "token")
                ),
                hint(&[reduced.description.as_deref()], descs),
                format!("{}/maxTokens", reduced.pointer),
                "maxTokens",
            ));
        }
    }
    if let Some(depth) = reduced.max_depth {
        report_deep(
            &section.sections,
            section.level + depth,
            &format!("{}/maxDepth", reduced.pointer),
            reduced.description.as_deref(),
            descs,
            crumbs,
            out,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_blocks<'a>(
    entries: &'a [CompiledBlock],
    additional: &'a CompiledBlockAdditional,
    all_scope: &[&'a CompiledReducedBlock],
    blocks: &[Block],
    crumbs: &[Crumb],
    descs: &[Option<&'a str>],
    out: &mut Vec<Violation>,
) {
    let binds = bind_blocks(entries, blocks);
    let mut counts = vec![0usize; entries.len()];
    for entry in binds.iter().flatten() {
        counts[*entry] += 1;
    }

    let deny_additional = matches!(additional, CompiledBlockAdditional::Deny { .. });
    let mut out_of_order: Vec<Option<usize>> = vec![None; blocks.len()];
    let mut suppressed = vec![false; entries.len()];
    if deny_additional {
        for (index, block) in blocks.iter().enumerate() {
            if binds[index].is_none() {
                if let Some(entry) = entries
                    .iter()
                    .position(|entry| block_identity_matches(entry, block))
                {
                    out_of_order[index] = Some(entry);
                    suppressed[entry] = true;
                }
            }
        }
    }

    for (index, entry) in entries.iter().enumerate() {
        let count = counts[index];
        if count < entry.min_contains && !suppressed[index] {
            let message = if count == 0 {
                format!("required block {} is missing", block_entry_name(entry, index))
            } else {
                format!(
                    "block {} appears {}, less than the minimum of {}",
                    block_entry_name(entry, index),
                    count_noun(count, "time"),
                    entry.min_contains
                )
            };
            out.push(violation(
                crumbs,
                message,
                hint(
                    &[
                        block_text_description(&entry.text),
                        entry.description.as_deref(),
                    ],
                    descs,
                ),
                format!("{}/minContains", entry.pointer),
                "minContains",
            ));
        }
        if let Some(max) = entry.max_contains {
            if count > max {
                out.push(violation(
                    crumbs,
                    format!(
                        "block {} appears {}, greater than the maximum of {max}",
                        block_entry_name(entry, index),
                        count_noun(count, "time")
                    ),
                    hint(&[entry.description.as_deref()], descs),
                    format!("{}/maxContains", entry.pointer),
                    "maxContains",
                ));
            }
        }
    }

    for (index, block) in blocks.iter().enumerate() {
        let mut child_crumbs = crumbs.to_vec();
        child_crumbs.push(Crumb::Block(index));

        for reduced in all_scope {
            apply_reduced_block(
                reduced,
                &block.text,
                block.text_tokens,
                block.subtree_tokens,
                &child_crumbs,
                descs,
                out,
            );
        }

        let entry = binds[index].map(|entry_index| &entries[entry_index]);
        match entry {
            Some(entry) => check_bound_block(entry, block, descs, &child_crumbs, out),
            None => match additional {
                CompiledBlockAdditional::Allow => {}
                CompiledBlockAdditional::Deny { pointer } => {
                    let message = if out_of_order[index].is_some() {
                        "block is out of order".to_string()
                    } else {
                        "unexpected block".to_string()
                    };
                    out.push(violation(
                        &child_crumbs,
                        message,
                        hint(&[], descs),
                        pointer.clone(),
                        "additionalBlocks",
                    ));
                }
                CompiledBlockAdditional::Schema(reduced) => {
                    apply_reduced_block(
                        reduced,
                        &block.text,
                        block.text_tokens,
                        block.subtree_tokens,
                        &child_crumbs,
                        descs,
                        out,
                    );
                }
            },
        }

        recurse_block_containers(entry, block, all_scope, &child_crumbs, descs, out);
    }
}

fn check_bound_block<'a>(
    entry: &'a CompiledBlock,
    block: &Block,
    descs: &[Option<&'a str>],
    crumbs: &[Crumb],
    out: &mut Vec<Violation>,
) {
    if let Some(text) = &entry.text {
        check_text(
            text,
            "text",
            &block.text,
            Some(block.text_tokens),
            false,
            entry.description.as_deref(),
            descs,
            crumbs,
            out,
        );
    }
    if let Some(max) = entry.max_tokens {
        if block.subtree_tokens > max {
            out.push(violation(
                crumbs,
                format!(
                    "block is {}, greater than the maximum of {max}",
                    count_noun(block.subtree_tokens, "token")
                ),
                hint(&[entry.description.as_deref()], descs),
                format!("{}/maxTokens", entry.pointer),
                "maxTokens",
            ));
        }
    }
    if let Some(lang) = &entry.lang {
        check_text(
            lang,
            "lang",
            block.lang.as_deref().unwrap_or(""),
            None,
            false,
            entry.description.as_deref(),
            descs,
            crumbs,
            out,
        );
    }
    if let Some(min) = entry.min_items {
        if block.items.len() < min {
            out.push(violation(
                crumbs,
                format!(
                    "list has {}, less than the minimum of {min}",
                    count_noun(block.items.len(), "item")
                ),
                hint(&[entry.description.as_deref()], descs),
                format!("{}/minItems", entry.pointer),
                "minItems",
            ));
        }
    }
    if let Some(max) = entry.max_items {
        if block.items.len() > max {
            out.push(violation(
                crumbs,
                format!(
                    "list has {}, greater than the maximum of {max}",
                    count_noun(block.items.len(), "item")
                ),
                hint(&[entry.description.as_deref()], descs),
                format!("{}/maxItems", entry.pointer),
                "maxItems",
            ));
        }
    }
}

fn recurse_block_containers<'a>(
    entry: Option<&'a CompiledBlock>,
    block: &Block,
    all_scope: &[&'a CompiledReducedBlock],
    crumbs: &[Crumb],
    descs: &[Option<&'a str>],
    out: &mut Vec<Violation>,
) {
    if block.kind == BlockKind::Quote {
        let mut child_scope = all_scope.to_vec();
        let mut child_descs = descs.to_vec();
        let (entries, additional): (&[CompiledBlock], &CompiledBlockAdditional) = match entry {
            Some(entry) => {
                if let Some(reduced) = &entry.all_blocks {
                    child_scope.push(reduced);
                }
                child_descs.push(entry.description.as_deref());
                (&entry.blocks, &entry.additional_blocks)
            }
            None => (&[], &CompiledBlockAdditional::Allow),
        };
        walk_blocks(
            entries,
            additional,
            &child_scope,
            &block.blocks,
            crumbs,
            &child_descs,
            out,
        );
    }

    if block.items.is_empty() {
        return;
    }

    let item_schema = entry.and_then(|entry| entry.items.as_deref());
    for (index, item) in block.items.iter().enumerate() {
        let mut item_crumbs = crumbs.to_vec();
        item_crumbs.push(Crumb::Item(index));

        for reduced in all_scope {
            apply_reduced_block(
                reduced,
                &item.text,
                item.text_tokens,
                item.subtree_tokens,
                &item_crumbs,
                descs,
                out,
            );
        }

        let mut child_scope = all_scope.to_vec();
        let mut child_descs = descs.to_vec();
        let (entries, additional): (&[CompiledBlock], &CompiledBlockAdditional) = match item_schema
        {
            Some(schema) => {
                if let Some(text) = &schema.text {
                    check_text(
                        text,
                        "text",
                        &item.text,
                        Some(item.text_tokens),
                        true,
                        schema.description.as_deref(),
                        descs,
                        &item_crumbs,
                        out,
                    );
                }
                if let Some(max) = schema.max_tokens {
                    if item.subtree_tokens > max {
                        out.push(violation(
                            &item_crumbs,
                            format!(
                                "item is {}, greater than the maximum of {max}",
                                count_noun(item.subtree_tokens, "token")
                            ),
                            hint(&[schema.description.as_deref()], descs),
                            format!("{}/maxTokens", schema.pointer),
                            "maxTokens",
                        ));
                    }
                }
                if let Some(reduced) = &schema.all_blocks {
                    child_scope.push(reduced);
                }
                child_descs.push(schema.description.as_deref());
                (&schema.blocks, &schema.additional_blocks)
            }
            None => (&[], &CompiledBlockAdditional::Allow),
        };
        walk_blocks(
            entries,
            additional,
            &child_scope,
            &item.blocks,
            &item_crumbs,
            &child_descs,
            out,
        );
    }
}

fn apply_reduced_block<'a>(
    reduced: &'a CompiledReducedBlock,
    text: &str,
    text_tokens: usize,
    subtree_tokens: usize,
    crumbs: &[Crumb],
    descs: &[Option<&'a str>],
    out: &mut Vec<Violation>,
) {
    if let Some(schema) = &reduced.text {
        check_text(
            schema,
            "text",
            text,
            Some(text_tokens),
            true,
            reduced.description.as_deref(),
            descs,
            crumbs,
            out,
        );
    }
    if let Some(max) = reduced.max_tokens {
        if subtree_tokens > max {
            out.push(violation(
                crumbs,
                format!(
                    "block is {}, greater than the maximum of {max}",
                    count_noun(subtree_tokens, "token")
                ),
                hint(&[reduced.description.as_deref()], descs),
                format!("{}/maxTokens", reduced.pointer),
                "maxTokens",
            ));
        }
    }
}

fn block_identity_matches(entry: &CompiledBlock, block: &Block) -> bool {
    if let Some(kinds) = &entry.kinds {
        if !kinds.contains(&block.kind) {
            return false;
        }
    }
    if let Some(text) = &entry.text {
        if !header_identity_matches(text, &block.text) {
            return false;
        }
    }
    if let Some(lang) = &entry.lang {
        if !header_identity_matches(lang, block.lang.as_deref().unwrap_or("")) {
            return false;
        }
    }
    true
}

fn block_entry_name(entry: &CompiledBlock, index: usize) -> String {
    if let Some(text) = &entry.text {
        if let Some(expected) = &text.konst {
            return quoted(expected);
        }
        if let Some(choices) = &text.choices {
            return format!("one of {}", or_join(choices));
        }
        if let Some(pattern) = &text.pattern {
            return format!("matching {}", quoted_pattern(pattern.as_str()));
        }
    }
    if let Some(kinds) = &entry.kinds {
        if let [kind] = kinds.as_slice() {
            return block_kind_name(*kind).to_string();
        }
        return format!(
            "one of {}",
            kinds
                .iter()
                .map(|kind| block_kind_name(*kind))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    format!("at position {index}")
}

fn block_kind_name(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::BulletList => "bullet-list",
        BlockKind::OrderedList => "ordered-list",
        BlockKind::Code => "code",
        BlockKind::Quote => "quote",
        BlockKind::Table => "table",
        BlockKind::Rule => "rule",
    }
}

#[allow(clippy::too_many_arguments)]
fn check_text<'a>(
    header: &'a CompiledHeader,
    label: &str,
    text: &str,
    token_count: Option<usize>,
    include_identity: bool,
    outer_desc: Option<&'a str>,
    descs: &[Option<&'a str>],
    crumbs: &[Crumb],
    out: &mut Vec<Violation>,
) {
    let chain = [header.description.as_deref(), outer_desc];

    if include_identity {
        if let Some(pattern) = &header.pattern {
            if !pattern.is_match(text) {
                out.push(violation(
                    crumbs,
                    format!(
                        "{label} {} does not match {}",
                        quoted(text),
                        quoted_pattern(pattern.as_str())
                    ),
                    hint(&chain, descs),
                    format!("{}/pattern", header.pointer),
                    "pattern",
                ));
            }
        }
        if let Some(expected) = &header.konst {
            if text != expected {
                out.push(violation(
                    crumbs,
                    format!(
                        "{label} {} was expected to be {}",
                        quoted(text),
                        quoted(expected)
                    ),
                    hint(&chain, descs),
                    format!("{}/const", header.pointer),
                    "const",
                ));
            }
        }
        if let Some(choices) = &header.choices {
            if !choices.iter().any(|choice| choice == text) {
                out.push(violation(
                    crumbs,
                    format!(
                        "{label} {} is not one of {}",
                        quoted(text),
                        or_join(choices)
                    ),
                    hint(&chain, descs),
                    format!("{}/enum", header.pointer),
                    "enum",
                ));
            }
        }
    }

    let length = text.chars().count();
    if let Some(min) = header.min_length {
        if length < min {
            out.push(violation(
                crumbs,
                format!(
                    "{label} {} is {}, less than the minimum of {min}",
                    quoted(text),
                    count_noun(length, "character")
                ),
                hint(&chain, descs),
                format!("{}/minLength", header.pointer),
                "minLength",
            ));
        }
    }
    if let Some(max) = header.max_length {
        if length > max {
            out.push(violation(
                crumbs,
                format!(
                    "{label} {} is {}, greater than the maximum of {max}",
                    quoted(text),
                    count_noun(length, "character")
                ),
                hint(&chain, descs),
                format!("{}/maxLength", header.pointer),
                "maxLength",
            ));
        }
    }
    if let Some(max) = header.max_tokens {
        if let Some(count) = token_count {
            if count > max {
                out.push(violation(
                    crumbs,
                    format!(
                        "{label} is {}, greater than the maximum of {max}",
                        count_noun(count, "token")
                    ),
                    hint(&chain, descs),
                    format!("{}/maxTokens", header.pointer),
                    "maxTokens",
                ));
            }
        }
    }
}

fn report_deep(
    sections: &[Section],
    limit: usize,
    pointer: &str,
    own_desc: Option<&str>,
    descs: &[Option<&str>],
    base_crumbs: &[Crumb],
    out: &mut Vec<Violation>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut crumbs = base_crumbs.to_vec();
        crumbs.push(section_crumb(section, index));
        if section.level > limit {
            out.push(violation(
                &crumbs,
                format!(
                    "heading is nested {} deep, greater than the maximum of {limit}",
                    count_noun(section.level, "level")
                ),
                hint(&[own_desc], descs),
                pointer.to_string(),
                "maxDepth",
            ));
        }
        report_deep(
            &section.sections,
            limit,
            pointer,
            own_desc,
            descs,
            &crumbs,
            out,
        );
    }
}

fn bind_sections(entries: &[CompiledSection], sections: &[Section]) -> Vec<Option<usize>> {
    let mut pointer = 0;
    let mut binds = Vec::with_capacity(sections.len());
    for section in sections {
        let mut matched = None;
        let mut candidate = pointer;
        while candidate < entries.len() {
            if entry_identity_matches(&entries[candidate], &section.header) {
                matched = Some(candidate);
                break;
            }
            candidate += 1;
        }
        if let Some(entry) = matched {
            pointer = entry;
        }
        binds.push(matched);
    }
    binds
}

fn bind_blocks(entries: &[CompiledBlock], blocks: &[Block]) -> Vec<Option<usize>> {
    let mut pointer = 0;
    let mut binds = Vec::with_capacity(blocks.len());
    for block in blocks {
        let mut matched = None;
        let mut candidate = pointer;
        while candidate < entries.len() {
            if block_identity_matches(&entries[candidate], block) {
                matched = Some(candidate);
                break;
            }
            candidate += 1;
        }
        if let Some(entry) = matched {
            pointer = entry;
        }
        binds.push(matched);
    }
    binds
}

fn entry_identity_matches(entry: &CompiledSection, text: &str) -> bool {
    match &entry.header {
        None => true,
        Some(header) => header_identity_matches(header, text),
    }
}

fn header_description(header: &Option<CompiledHeader>) -> Option<&str> {
    header
        .as_ref()
        .and_then(|header| header.description.as_deref())
}

fn block_text_description(text: &Option<CompiledHeader>) -> Option<&str> {
    text.as_ref().and_then(|text| text.description.as_deref())
}

fn header_identity_matches(header: &CompiledHeader, text: &str) -> bool {
    if let Some(pattern) = &header.pattern {
        if !pattern.is_match(text) {
            return false;
        }
    }
    if let Some(expected) = &header.konst {
        if text != expected {
            return false;
        }
    }
    if let Some(choices) = &header.choices {
        if !choices.iter().any(|choice| choice == text) {
            return false;
        }
    }
    true
}

fn entry_name(entry: &CompiledSection, index: usize) -> String {
    if let Some(header) = &entry.header {
        if let Some(expected) = &header.konst {
            return quoted(expected);
        }
        if let Some(choices) = &header.choices {
            return format!("one of {}", or_join(choices));
        }
        if let Some(pattern) = &header.pattern {
            return format!("matching {}", quoted_pattern(pattern.as_str()));
        }
    }
    format!("at position {index}")
}

fn section_crumb(section: &Section, index: usize) -> Crumb {
    if section.header.is_empty() {
        Crumb::Position(index)
    } else {
        Crumb::Header(section.header.clone())
    }
}

/// Wrap an instance or literal value the way `jsonschema` renders it: as a
/// JSON string, so embedded quotes and backslashes are escaped.
fn quoted(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

/// Wrap a regular expression the way `jsonschema` renders `pattern`: in literal
/// double quotes, without JSON-escaping its backslashes.
fn quoted_pattern(pattern: &str) -> String {
    format!("\"{pattern}\"")
}

/// Join choices the way `jsonschema` renders an `enum` mismatch: each value
/// quoted, separated by `, `, with ` or ` before the last.
fn or_join(values: &[String]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| quoted(value)).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => format!("{} or {last}", head.join(", ")),
    }
}

/// Render a count with its unit, pluralized: `1 token`, `3 tokens`.
fn count_noun(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

fn hint(chain: &[Option<&str>], descs: &[Option<&str>]) -> Option<String> {
    chain
        .iter()
        .flatten()
        .next()
        .or_else(|| descs.iter().rev().flatten().next())
        .map(|value| value.to_string())
}

fn violation(
    crumbs: &[Crumb],
    message: String,
    hint: Option<String>,
    schema_pointer: String,
    keyword: &str,
) -> Violation {
    Violation {
        breadcrumb: crumbs.to_vec(),
        message,
        hint,
        schema_pointer,
        keyword: keyword.to_string(),
    }
}

fn frontmatter_breadcrumb(instance_path: &str) -> Vec<Crumb> {
    let mut crumbs = vec![Crumb::Frontmatter];
    for segment in instance_path.split('/').filter(|part| !part.is_empty()) {
        crumbs.push(Crumb::Field(segment.to_string()));
    }
    crumbs
}

fn frontmatter_hint(
    source: Option<&serde_json::Value>,
    schema_path: &str,
    document_description: Option<&str>,
) -> Option<String> {
    if let Some(source) = source {
        let segments: Vec<&str> = schema_path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        for end in (0..segments.len()).rev() {
            let pointer = if end == 0 {
                String::new()
            } else {
                format!("/{}", segments[..end].join("/"))
            };
            if let Some(serde_json::Value::Object(map)) = source.pointer(&pointer) {
                if let Some(serde_json::Value::String(description)) = map.get("description") {
                    return Some(description.clone());
                }
            }
        }
    }
    document_description.map(|value| value.to_string())
}

fn last_segment(schema_path: &str) -> String {
    schema_path
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_string()
}

impl CompiledSchema {
    pub fn explain(&self, document: &Document) -> String {
        let mut out = String::new();
        explain_blocks(&self.blocks, &document.blocks, 0, &mut out);
        explain_sections(&self.sections, &document.sections, 0, &mut out);
        if out.is_empty() {
            out.push_str("(no matched content)\n");
        }
        out
    }
}

fn explain_sections(
    entries: &[CompiledSection],
    sections: &[Section],
    depth: usize,
    out: &mut String,
) {
    let binds = bind_sections(entries, sections);
    for (index, section) in sections.iter().enumerate() {
        let indent = "  ".repeat(depth);
        let (label, entry) = match binds[index] {
            Some(entry) => (format!("sections[{entry}]"), Some(&entries[entry])),
            None => ("additional".to_string(), None),
        };
        out.push_str(&format!("{indent}# {}  ->  {label}\n", section.header));
        let (child_blocks, child_sections): (&[CompiledBlock], &[CompiledSection]) = match entry {
            Some(entry) => (&entry.blocks, &entry.sections),
            None => (&[], &[]),
        };
        explain_blocks(child_blocks, &section.blocks, depth + 1, out);
        explain_sections(child_sections, &section.sections, depth + 1, out);
    }
}

fn explain_blocks(entries: &[CompiledBlock], blocks: &[Block], depth: usize, out: &mut String) {
    let binds = bind_blocks(entries, blocks);
    for (index, block) in blocks.iter().enumerate() {
        let indent = "  ".repeat(depth);
        let (label, entry) = match binds[index] {
            Some(entry) => (format!("blocks[{entry}]"), Some(&entries[entry])),
            None => ("additional".to_string(), None),
        };
        out.push_str(&format!("{indent}{}  ->  {label}\n", block_label(block)));
        if block.kind == BlockKind::Quote {
            let child = entry.map(|entry| entry.blocks.as_slice()).unwrap_or(&[]);
            explain_blocks(child, &block.blocks, depth + 1, out);
        }
    }
}

fn block_label(block: &Block) -> String {
    let kind = block_kind_name(block.kind);
    let text = block.text.trim();
    if text.is_empty() {
        kind.to_string()
    } else {
        let preview: String = text.chars().take(40).collect();
        let suffix = if text.chars().count() > 40 { "..." } else { "" };
        format!("{kind} \"{preview}{suffix}\"")
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use serde_json::json;

    use crate::compile::compile_schema;
    use crate::document::{Block, BlockKind, Document, Item, Section};
    use crate::violation::{Crumb, Violation};

    fn blk(kind: BlockKind, text: &str) -> Block {
        Block {
            kind,
            text: text.to_string(),
            text_tokens: 0,
            subtree_tokens: 0,
            lang: None,
            items: vec![],
            blocks: vec![],
        }
    }

    fn itm(text: &str) -> Item {
        Item {
            text: text.to_string(),
            text_tokens: 0,
            subtree_tokens: 0,
            blocks: vec![],
        }
    }

    fn block_document(blocks: Vec<Block>) -> Document {
        Document {
            frontmatter: json!({}),
            frontmatter_error: None,
            body_tokens: 0,
            blocks,
            sections: vec![],
        }
    }

    fn node(
        header: &str,
        level: usize,
        header_tokens: usize,
        subtree_tokens: usize,
        sections: Vec<Section>,
    ) -> Section {
        Section {
            header: header.to_string(),
            level,
            header_tokens,
            subtree_tokens,
            blocks: vec![],
            sections,
        }
    }

    fn leaf(header: &str, level: usize) -> Section {
        node(header, level, 1, 1, vec![])
    }

    fn parent(header: &str, level: usize, sections: Vec<Section>) -> Section {
        node(header, level, 1, 1, sections)
    }

    fn document(sections: Vec<Section>) -> Document {
        Document {
            frontmatter: json!({}),
            frontmatter_error: None,
            body_tokens: 0,
            blocks: vec![],
            sections,
        }
    }

    fn header_crumb(text: &str) -> Crumb {
        Crumb::Header(text.to_string())
    }

    fn violation(
        breadcrumb: Vec<Crumb>,
        message: &str,
        hint: Option<&str>,
        schema_pointer: &str,
        keyword: &str,
    ) -> Violation {
        Violation {
            breadcrumb,
            message: message.to_string(),
            hint: hint.map(str::to_string),
            schema_pointer: schema_pointer.to_string(),
            keyword: keyword.to_string(),
        }
    }

    fn validate(schema: &str, document: &Document) -> Vec<Violation> {
        compile_schema(schema).unwrap().validate(document)
    }

    #[test]
    fn empty_schema_passes_any_document() {
        let document = document(vec![parent("A", 1, vec![leaf("B", 2)])]);
        assert_eq!(validate("{}", &document), vec![]);
    }

    #[test]
    fn missing_required_and_closed_template() {
        let schema = indoc! {"
            sections:
              - header: { const: Summary }
              - header: { const: Tasks }
            additionalSections: false
        "};
        let document = document(vec![leaf("Notes", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![],
                    "required section \"Summary\" is missing",
                    None,
                    "/sections/0/minContains",
                    "minContains",
                ),
                violation(
                    vec![],
                    "required section \"Tasks\" is missing",
                    None,
                    "/sections/1/minContains",
                    "minContains",
                ),
                violation(
                    vec![header_crumb("Notes")],
                    "unexpected section",
                    None,
                    "/additionalSections",
                    "additionalSections",
                ),
            ]
        );
    }

    #[test]
    fn binding_by_header_validates_the_rest() {
        let schema = indoc! {"
            sections:
              - header: { const: Tasks }
                sections:
                  - header: { const: Done }
        "};
        let document = document(vec![parent("Tasks", 1, vec![leaf("Todo", 2)])]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Tasks")],
                "required section \"Done\" is missing",
                None,
                "/sections/0/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn consecutive_run_below_min_contains() {
        let schema = indoc! {"
            sections:
              - header: { pattern: '^\\d{4}-\\d{2}-\\d{2}$' }
                minContains: 3
        "};
        let document = document(vec![
            leaf("2026-01-01", 1),
            leaf("2026-01-02", 1),
            leaf("Notes", 1),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "section matching \"^\\d{4}-\\d{2}-\\d{2}$\" appears 2 times, less than the minimum of 3",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn additional_interloper_does_not_break_min_contains_run() {
        let schema = indoc! {"
            sections:
              - header: { pattern: '^\\d{4}-\\d{2}-\\d{2}$' }
                minContains: 3
        "};
        let document = document(vec![
            leaf("2026-01-01", 1),
            leaf("2026-01-02", 1),
            leaf("Notes", 1),
            leaf("2026-01-03", 1),
        ]);
        assert_eq!(validate(schema, &document), vec![]);
    }

    #[test]
    fn additional_interloper_counts_toward_max_contains() {
        let schema = indoc! {"
            sections:
              - header: { pattern: '^\\d{4}-\\d{2}-\\d{2}$' }
                maxContains: 2
        "};
        let document = document(vec![
            leaf("2026-01-01", 1),
            leaf("2026-01-02", 1),
            leaf("Notes", 1),
            leaf("2026-01-03", 1),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "section matching \"^\\d{4}-\\d{2}-\\d{2}$\" appears 3 times, greater than the maximum of 2",
                None,
                "/sections/0/maxContains",
                "maxContains",
            )]
        );
    }

    #[test]
    fn binding_to_a_later_entry_closes_the_repeated_entry() {
        let schema = indoc! {"
            sections:
              - header: { pattern: '^\\d{4}-\\d{2}-\\d{2}$' }
                minContains: 3
              - header: { const: Footer }
                minContains: 0
        "};
        let document = document(vec![
            leaf("2026-01-01", 1),
            leaf("2026-01-02", 1),
            leaf("Footer", 1),
            leaf("2026-01-03", 1),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "section matching \"^\\d{4}-\\d{2}-\\d{2}$\" appears 2 times, less than the minimum of 3",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn explain_traces_block_and_section_bindings() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
            sections:
              - header: { const: Intro }
        "};
        let document = Document {
            frontmatter: json!({}),
            frontmatter_error: None,
            body_tokens: 0,
            blocks: vec![blk(BlockKind::Paragraph, "lead")],
            sections: vec![leaf("Intro", 1), leaf("Extra", 1)],
        };
        let compiled = compile_schema(schema).unwrap();
        assert_eq!(
            compiled.explain(&document),
            indoc! {"
                paragraph \"lead\"  ->  blocks[0]
                # Intro  ->  sections[0]
                # Extra  ->  additional
            "}
        );
    }

    #[test]
    fn out_of_order_section_is_additional() {
        let schema = indoc! {"
            sections:
              - header: { const: A }
              - header: { const: B }
            additionalSections: true
        "};
        let document = document(vec![leaf("B", 1), leaf("A", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "required section \"A\" is missing",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn header_description_hints_a_missing_section() {
        let schema = indoc! {"
            sections:
              - header: { const: Summary, description: open with a summary }
        "};
        let document = document(vec![leaf("Other", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "required section \"Summary\" is missing",
                Some("open with a summary"),
                "/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn out_of_order_section_reports_once_under_deny() {
        let schema = indoc! {"
            sections:
              - header: { const: A }
              - header: { const: B }
            additionalSections: false
        "};
        let document = document(vec![leaf("B", 1), leaf("A", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("A")],
                "section is out of order",
                None,
                "/additionalSections",
                "additionalSections",
            )]
        );
    }

    #[test]
    fn all_sections_applies_at_every_depth() {
        let schema = indoc! {"
            allSections:
              header: { pattern: \"^[A-Z]\" }
        "};
        let document = document(vec![parent("Intro", 1, vec![leaf("lower", 2)])]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Intro"), header_crumb("lower")],
                "header \"lower\" does not match \"^[A-Z]\"",
                None,
                "/allSections/header/pattern",
                "pattern",
            )]
        );
    }

    #[test]
    fn document_max_depth_flags_deep_section() {
        let schema = "maxDepth: 1\n";
        let document = document(vec![parent("A", 1, vec![leaf("B", 2)])]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("A"), header_crumb("B")],
                "heading is nested 2 levels deep, greater than the maximum of 1",
                None,
                "/maxDepth",
                "maxDepth",
            )]
        );
    }

    #[test]
    fn header_and_section_token_budgets() {
        let schema = indoc! {"
            sections:
              - header: { const: Big, maxTokens: 1 }
                maxTokens: 5
        "};
        let document = document(vec![node("Big", 1, 3, 10, vec![])]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![header_crumb("Big")],
                    "header is 3 tokens, greater than the maximum of 1",
                    None,
                    "/sections/0/header/maxTokens",
                    "maxTokens",
                ),
                violation(
                    vec![header_crumb("Big")],
                    "section is 10 tokens, greater than the maximum of 5",
                    None,
                    "/sections/0/maxTokens",
                    "maxTokens",
                ),
            ]
        );
    }

    #[test]
    fn additional_sections_schema_validates_leftover() {
        let schema = indoc! {"
            sections:
              - header: { const: Intro }
            additionalSections:
              maxTokens: 5
        "};
        let document = document(vec![
            node("Intro", 1, 1, 3, vec![]),
            node("Extra", 1, 1, 10, vec![]),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Extra")],
                "section is 10 tokens, greater than the maximum of 5",
                None,
                "/additionalSections/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn frontmatter_enum_reports_with_description_hint() {
        let schema = indoc! {"
            frontmatter:
              type: object
              required: [status]
              properties:
                status:
                  enum: [draft, published]
                  description: every note declares a status
        "};
        let mut document = document(vec![]);
        document.frontmatter = json!({ "status": "archived" });
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![Crumb::Frontmatter, Crumb::Field("status".to_string())],
                "\"archived\" is not one of \"draft\" or \"published\"",
                Some("every note declares a status"),
                "/frontmatter/properties/status/enum",
                "enum",
            )]
        );
    }

    #[test]
    fn frontmatter_error_reports_and_suppresses_schema_checks() {
        let schema = indoc! {"
            frontmatter:
              type: object
              required: [status]
        "};
        let mut document = document(vec![]);
        document.frontmatter_error = Some("invalid YAML: oops".to_string());
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![Crumb::Frontmatter],
                "invalid YAML: oops",
                None,
                "",
                "frontmatter",
            )]
        );
    }

    #[test]
    fn frontmatter_error_reports_even_under_an_empty_schema() {
        let mut document = document(vec![]);
        document.frontmatter_error = Some("not a YAML mapping".to_string());
        assert_eq!(
            validate("{}", &document),
            vec![violation(
                vec![Crumb::Frontmatter],
                "not a YAML mapping",
                None,
                "",
                "frontmatter",
            )]
        );
    }

    fn block_crumb(index: usize) -> Crumb {
        Crumb::Block(index)
    }

    fn item_crumb(index: usize) -> Crumb {
        Crumb::Item(index)
    }

    #[test]
    fn binding_by_type_flags_missing_and_extra() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
              - type: code
            additionalBlocks: false
        "};
        let document = block_document(vec![
            blk(BlockKind::Paragraph, "lead"),
            blk(BlockKind::Table, "cells"),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![],
                    "required block code is missing",
                    None,
                    "/blocks/1/minContains",
                    "minContains",
                ),
                violation(
                    vec![block_crumb(1)],
                    "unexpected block",
                    None,
                    "/additionalBlocks",
                    "additionalBlocks",
                ),
            ]
        );
    }

    #[test]
    fn text_description_hints_a_missing_block() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
                text: { const: Intro, description: lead with intro }
        "};
        let document = block_document(vec![blk(BlockKind::Paragraph, "Other")]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "required block \"Intro\" is missing",
                Some("lead with intro"),
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn out_of_order_block_reports_once_under_deny() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
              - type: code
            additionalBlocks: false
        "};
        let document = block_document(vec![
            blk(BlockKind::Code, "body"),
            blk(BlockKind::Paragraph, "lead"),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(1)],
                "block is out of order",
                None,
                "/additionalBlocks",
                "additionalBlocks",
            )]
        );
    }

    #[test]
    fn type_union_binds_either_listed_kind() {
        let schema = indoc! {"
            blocks:
              - type: [bullet-list, ordered-list]
                maxContains: 2
            additionalBlocks: false
        "};
        let document = block_document(vec![
            blk(BlockKind::BulletList, ""),
            blk(BlockKind::OrderedList, ""),
        ]);
        assert_eq!(validate(schema, &document), vec![]);
    }

    #[test]
    fn type_union_names_the_disjunction_in_missing() {
        let schema = indoc! {"
            blocks:
              - type: [bullet-list, ordered-list]
        "};
        let document = block_document(vec![blk(BlockKind::Code, "body")]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "required block one of bullet-list, ordered-list is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn binding_by_text_const_names_the_missing_entry() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
                text: { const: Intro }
        "};
        let document = block_document(vec![blk(BlockKind::Paragraph, "Other")]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "required block \"Intro\" is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn additional_blocks_true_allows_leftover() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
            additionalBlocks: true
        "};
        let document = block_document(vec![
            blk(BlockKind::Paragraph, "lead"),
            blk(BlockKind::Table, "cells"),
        ]);
        assert_eq!(validate(schema, &document), vec![]);
    }

    #[test]
    fn additional_blocks_schema_validates_leftover() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
            additionalBlocks:
              maxTokens: 5
        "};
        let document = block_document(vec![
            blk(BlockKind::Paragraph, "lead"),
            Block {
                subtree_tokens: 10,
                ..blk(BlockKind::Code, "body")
            },
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(1)],
                "block is 10 tokens, greater than the maximum of 5",
                None,
                "/additionalBlocks/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn max_contains_flags_repeat() {
        let schema = indoc! {"
            blocks:
              - type: paragraph
                maxContains: 1
            additionalBlocks: true
        "};
        let document = block_document(vec![
            blk(BlockKind::Paragraph, "a"),
            blk(BlockKind::Paragraph, "b"),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![],
                "block paragraph appears 2 times, greater than the maximum of 1",
                None,
                "/blocks/0/maxContains",
                "maxContains",
            )]
        );
    }

    #[test]
    fn min_items_and_items_schema_apply_to_every_item() {
        let schema = indoc! {"
            blocks:
              - type: bullet-list
                minItems: 2
                items:
                  text: { maxTokens: 3 }
        "};
        let document = block_document(vec![Block {
            items: vec![Item {
                text_tokens: 5,
                ..itm("way too long an item")
            }],
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![block_crumb(0)],
                    "list has 1 item, less than the minimum of 2",
                    None,
                    "/blocks/0/minItems",
                    "minItems",
                ),
                violation(
                    vec![block_crumb(0), item_crumb(0)],
                    "text is 5 tokens, greater than the maximum of 3",
                    None,
                    "/blocks/0/items/text/maxTokens",
                    "maxTokens",
                ),
            ]
        );
    }

    #[test]
    fn all_blocks_reaches_every_item_and_nested_block() {
        let schema = indoc! {"
            allBlocks:
              text: { maxTokens: 2 }
        "};
        let document = block_document(vec![Block {
            items: vec![Item {
                text_tokens: 5,
                subtree_tokens: 9,
                blocks: vec![Block {
                    text_tokens: 4,
                    ..blk(BlockKind::Paragraph, "nested")
                }],
                ..itm("long item")
            }],
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![block_crumb(0), item_crumb(0)],
                    "text is 5 tokens, greater than the maximum of 2",
                    None,
                    "/allBlocks/text/maxTokens",
                    "maxTokens",
                ),
                violation(
                    vec![block_crumb(0), item_crumb(0), block_crumb(0)],
                    "text is 4 tokens, greater than the maximum of 2",
                    None,
                    "/allBlocks/text/maxTokens",
                    "maxTokens",
                ),
            ]
        );
    }

    #[test]
    fn quote_recursion_reports_nested_missing_block() {
        let schema = indoc! {"
            blocks:
              - type: quote
                blocks:
                  - type: paragraph
                    text: { const: Warning }
        "};
        let document = block_document(vec![Block {
            blocks: vec![blk(BlockKind::Paragraph, "Other")],
            ..blk(BlockKind::Quote, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0)],
                "required block \"Warning\" is missing",
                None,
                "/blocks/0/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn list_subtree_budget_trips_while_own_text_budget_does_not() {
        let schema = indoc! {"
            blocks:
              - type: bullet-list
                maxTokens: 5
                text: { maxTokens: 1 }
        "};
        let document = block_document(vec![Block {
            subtree_tokens: 10,
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0)],
                "block is 10 tokens, greater than the maximum of 5",
                None,
                "/blocks/0/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn lang_participates_in_binding_identity() {
        let schema = indoc! {"
            blocks:
              - type: code
                lang: { enum: [rust, toml] }
            additionalBlocks: false
        "};
        let document = block_document(vec![Block {
            lang: Some("python".to_string()),
            ..blk(BlockKind::Code, "body")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![
                violation(
                    vec![],
                    "required block code is missing",
                    None,
                    "/blocks/0/minContains",
                    "minContains",
                ),
                violation(
                    vec![block_crumb(0)],
                    "unexpected block",
                    None,
                    "/additionalBlocks",
                    "additionalBlocks",
                ),
            ]
        );
    }

    #[test]
    fn section_blocks_are_validated_under_the_section() {
        let schema = indoc! {"
            sections:
              - header: { const: Notes }
                blocks:
                  - type: paragraph
                    maxContains: 1
                additionalBlocks: false
        "};
        let mut section = node("Notes", 1, 1, 1, vec![]);
        section.blocks = vec![
            blk(BlockKind::Paragraph, "one"),
            blk(BlockKind::Table, "cells"),
        ];
        let document = document(vec![section]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Notes"), block_crumb(1)],
                "unexpected block",
                None,
                "/sections/0/additionalBlocks",
                "additionalBlocks",
            )]
        );
    }

    #[test]
    fn header_length_bounds_flag_short_and_long() {
        let schema = indoc! {"
            sections:
              - header: { minLength: 3, maxLength: 5 }
        "};
        let short = document(vec![leaf("ab", 1)]);
        assert_eq!(
            validate(schema, &short),
            vec![violation(
                vec![header_crumb("ab")],
                "header \"ab\" is 2 characters, less than the minimum of 3",
                None,
                "/sections/0/header/minLength",
                "minLength",
            )]
        );
        let long = document(vec![leaf("toolong!", 1)]);
        assert_eq!(
            validate(schema, &long),
            vec![violation(
                vec![header_crumb("toolong!")],
                "header \"toolong!\" is 8 characters, greater than the maximum of 5",
                None,
                "/sections/0/header/maxLength",
                "maxLength",
            )]
        );
    }

    #[test]
    fn max_items_flags_long_list() {
        let schema = indoc! {"
            blocks:
              - type: bullet-list
                maxItems: 1
        "};
        let document = block_document(vec![Block {
            items: vec![itm("a"), itm("b")],
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0)],
                "list has 2 items, greater than the maximum of 1",
                None,
                "/blocks/0/maxItems",
                "maxItems",
            )]
        );
    }

    #[test]
    fn all_sections_const_reports_mismatch() {
        let schema = indoc! {"
            allSections:
              header: { const: Fixed }
        "};
        let document = document(vec![leaf("Other", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Other")],
                "header \"Other\" was expected to be \"Fixed\"",
                None,
                "/allSections/header/const",
                "const",
            )]
        );
    }

    #[test]
    fn all_sections_enum_reports_choices() {
        let schema = indoc! {"
            allSections:
              header: { enum: [Alpha, Beta] }
        "};
        let document = document(vec![leaf("Gamma", 1)]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Gamma")],
                "header \"Gamma\" is not one of \"Alpha\" or \"Beta\"",
                None,
                "/allSections/header/enum",
                "enum",
            )]
        );
    }

    #[test]
    fn section_enum_names_missing_choices() {
        let schema = indoc! {"
            sections:
              - header: { enum: [Alpha, Beta] }
        "};
        assert_eq!(
            validate(schema, &document(vec![])),
            vec![violation(
                vec![],
                "required section one of \"Alpha\" or \"Beta\" is missing",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn block_text_enum_and_pattern_name_missing_entries() {
        let enum_schema = indoc! {"
            blocks:
              - type: paragraph
                text: { enum: [One, Two] }
        "};
        assert_eq!(
            validate(enum_schema, &block_document(vec![])),
            vec![violation(
                vec![],
                "required block one of \"One\" or \"Two\" is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
        let pattern_schema = indoc! {"
            blocks:
              - type: paragraph
                text: { pattern: '^Note' }
        "};
        assert_eq!(
            validate(pattern_schema, &block_document(vec![])),
            vec![violation(
                vec![],
                "required block matching \"^Note\" is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn wildcard_entries_report_positionally_when_missing() {
        let section_schema = indoc! {"
            sections:
              - maxTokens: 5
        "};
        assert_eq!(
            validate(section_schema, &document(vec![])),
            vec![violation(
                vec![],
                "required section at position 0 is missing",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
        let block_schema = indoc! {"
            blocks:
              - maxTokens: 5
        "};
        assert_eq!(
            validate(block_schema, &block_document(vec![])),
            vec![violation(
                vec![],
                "required block at position 0 is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn section_max_depth_limits_relative_depth() {
        let schema = indoc! {"
            sections:
              - header: { const: Guide }
                maxDepth: 1
        "};
        let document = document(vec![parent(
            "Guide",
            1,
            vec![parent("Child", 2, vec![leaf("Deep", 3)])],
        )]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![
                    header_crumb("Guide"),
                    header_crumb("Child"),
                    header_crumb("Deep")
                ],
                "heading is nested 3 levels deep, greater than the maximum of 2",
                None,
                "/sections/0/maxDepth",
                "maxDepth",
            )]
        );
    }

    #[test]
    fn additional_sections_max_depth_applies_to_leftover() {
        let schema = indoc! {"
            sections:
              - header: { const: Intro }
            additionalSections:
              maxDepth: 1
        "};
        let document = document(vec![
            leaf("Intro", 1),
            parent("Extra", 1, vec![parent("Child", 2, vec![leaf("Deep", 3)])]),
        ]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![
                    header_crumb("Extra"),
                    header_crumb("Child"),
                    header_crumb("Deep")
                ],
                "heading is nested 3 levels deep, greater than the maximum of 2",
                None,
                "/additionalSections/maxDepth",
                "maxDepth",
            )]
        );
    }

    #[test]
    fn item_max_tokens_flags_long_item() {
        let schema = indoc! {"
            blocks:
              - type: bullet-list
                items:
                  maxTokens: 2
        "};
        let document = block_document(vec![Block {
            items: vec![Item {
                subtree_tokens: 5,
                ..itm("long")
            }],
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0), item_crumb(0)],
                "item is 5 tokens, greater than the maximum of 2",
                None,
                "/blocks/0/items/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn section_all_blocks_bounds_its_blocks() {
        let schema = indoc! {"
            sections:
              - header: { const: Notes }
                allBlocks:
                  text: { maxTokens: 1 }
        "};
        let mut section = node("Notes", 1, 1, 1, vec![]);
        section.blocks = vec![Block {
            text_tokens: 3,
            ..blk(BlockKind::Paragraph, "long text here")
        }];
        let document = document(vec![section]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Notes"), block_crumb(0)],
                "text is 3 tokens, greater than the maximum of 1",
                None,
                "/sections/0/allBlocks/text/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn section_all_sections_bounds_its_children() {
        let schema = indoc! {"
            sections:
              - header: { const: Top }
                allSections:
                  header: { pattern: \"^[A-Z]\" }
        "};
        let document = document(vec![parent("Top", 1, vec![leaf("lower", 2)])]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![header_crumb("Top"), header_crumb("lower")],
                "header \"lower\" does not match \"^[A-Z]\"",
                None,
                "/sections/0/allSections/header/pattern",
                "pattern",
            )]
        );
    }

    #[test]
    fn quote_all_blocks_bounds_nested_blocks() {
        let schema = indoc! {"
            blocks:
              - type: quote
                allBlocks:
                  text: { maxTokens: 1 }
        "};
        let document = block_document(vec![Block {
            blocks: vec![Block {
                text_tokens: 3,
                ..blk(BlockKind::Paragraph, "long text here")
            }],
            ..blk(BlockKind::Quote, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0), block_crumb(0)],
                "text is 3 tokens, greater than the maximum of 1",
                None,
                "/blocks/0/allBlocks/text/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn unbound_quote_children_still_see_all_blocks_scope() {
        let schema = indoc! {"
            allBlocks:
              text: { maxTokens: 1 }
        "};
        let document = block_document(vec![Block {
            blocks: vec![Block {
                text_tokens: 3,
                ..blk(BlockKind::Paragraph, "long text here")
            }],
            ..blk(BlockKind::Quote, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0), block_crumb(0)],
                "text is 3 tokens, greater than the maximum of 1",
                None,
                "/allBlocks/text/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn item_all_blocks_bounds_item_children() {
        let schema = indoc! {"
            blocks:
              - type: bullet-list
                items:
                  allBlocks:
                    text: { maxTokens: 1 }
        "};
        let document = block_document(vec![Block {
            items: vec![Item {
                blocks: vec![Block {
                    text_tokens: 3,
                    ..blk(BlockKind::Paragraph, "long text here")
                }],
                ..itm("a")
            }],
            ..blk(BlockKind::BulletList, "")
        }]);
        assert_eq!(
            validate(schema, &document),
            vec![violation(
                vec![block_crumb(0), item_crumb(0), block_crumb(0)],
                "text is 3 tokens, greater than the maximum of 1",
                None,
                "/blocks/0/items/allBlocks/text/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn frontmatter_hint_falls_back_to_document_description() {
        let schema = indoc! {"
            description: keep notes tidy
            frontmatter:
              type: object
              required: [status]
        "};
        let violations = validate(schema, &document(vec![]));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].keyword, "required");
        assert_eq!(violations[0].hint.as_deref(), Some("keep notes tidy"));
    }

    #[test]
    fn frontmatter_hint_uses_nearest_schema_description() {
        let schema = indoc! {"
            frontmatter:
              type: object
              description: notes carry status
              required: [status]
        "};
        let violations = validate(schema, &document(vec![]));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].hint.as_deref(), Some("notes carry status"));
    }

    #[test]
    fn explain_empty_document_notes_no_content() {
        let compiled = compile_schema("{}").unwrap();
        assert_eq!(
            compiled.explain(&document(vec![])),
            "(no matched content)\n"
        );
    }

    #[test]
    fn explain_traces_quote_children_and_additional_blocks() {
        let schema = indoc! {"
            blocks:
              - type: quote
                blocks:
                  - type: paragraph
        "};
        let document = block_document(vec![
            Block {
                blocks: vec![blk(BlockKind::Paragraph, "inner")],
                ..blk(BlockKind::Quote, "")
            },
            blk(BlockKind::Table, "cells"),
        ]);
        let compiled = compile_schema(schema).unwrap();
        assert_eq!(
            compiled.explain(&document),
            indoc! {"
                quote  ->  blocks[0]
                  paragraph \"inner\"  ->  blocks[0]
                table \"cells\"  ->  additional
            "}
        );
    }

    #[test]
    fn identity_less_entries_bind_and_validate() {
        let section_schema = indoc! {"
            sections:
              - maxTokens: 5
        "};
        let document_with_section = document(vec![node("Any", 1, 1, 10, vec![])]);
        assert_eq!(
            validate(section_schema, &document_with_section),
            vec![violation(
                vec![header_crumb("Any")],
                "section is 10 tokens, greater than the maximum of 5",
                None,
                "/sections/0/maxTokens",
                "maxTokens",
            )]
        );
        let block_schema = indoc! {"
            blocks:
              - maxTokens: 5
        "};
        let document_with_block = block_document(vec![Block {
            subtree_tokens: 10,
            ..blk(BlockKind::Paragraph, "text")
        }]);
        assert_eq!(
            validate(block_schema, &document_with_block),
            vec![violation(
                vec![block_crumb(0)],
                "block is 10 tokens, greater than the maximum of 5",
                None,
                "/blocks/0/maxTokens",
                "maxTokens",
            )]
        );
    }

    #[test]
    fn identity_less_headers_name_missing_entries_positionally() {
        let section_schema = indoc! {"
            sections:
              - header: { minLength: 3 }
        "};
        assert_eq!(
            validate(section_schema, &document(vec![])),
            vec![violation(
                vec![],
                "required section at position 0 is missing",
                None,
                "/sections/0/minContains",
                "minContains",
            )]
        );
        let block_schema = indoc! {"
            blocks:
              - type: paragraph
                text: { maxTokens: 5 }
        "};
        assert_eq!(
            validate(block_schema, &block_document(vec![])),
            vec![violation(
                vec![],
                "required block paragraph is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn rule_entries_are_named_by_kind() {
        let schema = indoc! {"
            blocks:
              - type: rule
        "};
        assert_eq!(
            validate(schema, &block_document(vec![])),
            vec![violation(
                vec![],
                "required block rule is missing",
                None,
                "/blocks/0/minContains",
                "minContains",
            )]
        );
    }

    #[test]
    fn empty_header_uses_position_crumb() {
        let schema = indoc! {"
            allSections:
              header: { minLength: 1 }
        "};
        let document = document(vec![leaf("", 1)]);
        let violations = validate(schema, &document);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].breadcrumb_text(), "sections[0]");
        assert_eq!(
            violations,
            vec![violation(
                vec![Crumb::Position(0)],
                "header \"\" is 0 characters, less than the minimum of 1",
                None,
                "/allSections/header/minLength",
                "minLength",
            )]
        );
    }
}
