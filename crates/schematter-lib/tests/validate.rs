//! End-to-end tests of the public `validate()` surface: real markdown source
//! fed through the fresh builder into the validator. The unit tests cover each
//! stage in isolation (the builder against `Document` literals, the validator
//! against hand-built documents); these cover the two composed.

use indoc::indoc;
use schematter_lib::tokens::count_tokens;
use schematter_lib::{validate, Violation};

/// Render each violation as `"<breadcrumb>: <message>  [<keyword> @ <pointer>]"`
/// — every field that matters, in one comparable line.
fn rendered(markdown: &str, schema: &str) -> Vec<String> {
    hits(markdown, schema)
        .iter()
        .map(|violation| {
            let breadcrumb = violation.breadcrumb_text();
            let head = if breadcrumb.is_empty() {
                violation.message.clone()
            } else {
                format!("{breadcrumb}: {}", violation.message)
            };
            format!(
                "{head}  [{} @ {}]",
                violation.keyword, violation.schema_pointer
            )
        })
        .collect()
}

fn hits(markdown: &str, schema: &str) -> Vec<Violation> {
    validate(markdown, schema).expect("schema compiles")
}

// --- sections -------------------------------------------------------------

#[test]
fn clean_document_passes() {
    let schema = indoc! {"
        sections:
          - header: { const: Summary }
          - header: { const: Tasks }
        additionalSections: false
    "};
    let markdown = indoc! {"
        # Summary

        text

        # Tasks
    "};
    assert_eq!(hits(markdown, schema), vec![]);
}

#[test]
fn empty_schema_passes_anything() {
    assert_eq!(
        hits(
            indoc! {"
        # Anything

        goes
    "},
            "{}"
        ),
        vec![]
    );
}

#[test]
fn missing_required_section_carries_its_description_as_hint() {
    let schema = indoc! {"
        sections:
          - header: { const: Summary }
            description: every note opens with a summary
    "};
    let violations = hits("# Other\n", schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].message,
        "required section \"Summary\" is missing"
    );
    assert_eq!(
        violations[0].hint.as_deref(),
        Some("every note opens with a summary")
    );
    assert_eq!(violations[0].schema_pointer, "/sections/0/minContains");
}

#[test]
fn additional_section_is_rejected_when_closed() {
    let schema = indoc! {"
        sections:
          - header: { const: Summary }
        additionalSections: false
    "};
    assert_eq!(
        rendered(
            indoc! {"
            # Summary

            # Extra
        "},
            schema
        ),
        vec!["Extra: unexpected section  [additionalSections @ /additionalSections]"],
    );
}

#[test]
fn out_of_order_section_leaves_the_earlier_entry_unmatched() {
    // Greedy, no-backtracking: `Two` binds the second entry, then `One` can only
    // see a closed entry, so entry `One` reports missing and `One` is additional.
    let schema = indoc! {"
        sections:
          - header: { const: One }
          - header: { const: Two }
    "};
    assert_eq!(
        rendered(
            indoc! {"
            # Two

            # One
        "},
            schema
        ),
        vec!["required section \"One\" is missing  [minContains @ /sections/0/minContains]"],
    );
}

#[test]
fn repeated_shape_meets_min_contains() {
    let schema = indoc! {r#"
        sections:
          - header: { pattern: '^\d{4}-\d{2}-\d{2}$' }
            minContains: 3
    "#};
    assert_eq!(
        hits(
            indoc! {"
            # 2026-01-01

            # 2026-01-02

            # 2026-01-03
        "},
            schema
        ),
        vec![]
    );
    let short = hits(
        indoc! {"
        # 2026-01-01

        # 2026-01-02
    "},
        schema,
    );
    assert_eq!(short.len(), 1);
    assert_eq!(
        short[0].message,
        "section matching \"^\\d{4}-\\d{2}-\\d{2}$\" appears 2 times, less than the minimum of 3"
    );
}

// --- headers --------------------------------------------------------------

#[test]
fn header_pattern_is_the_binding_key_not_a_violation() {
    // A `header` in a `sections` entry decides binding: a header that fails the
    // pattern does not raise a pattern violation, it just leaves the entry
    // unbound (missing). Pattern-as-violation is exercised via `allSections`.
    let schema = indoc! {"
        sections:
          - header: { pattern: \"^[A-Z]\" }
    "};
    assert_eq!(
        rendered("# lower start\n", schema),
        vec!["required section matching \"^[A-Z]\" is missing  [minContains @ /sections/0/minContains]"],
    );
}

#[test]
fn header_token_budget_is_checked_once_bound() {
    // With a matching binding key (`.+`), the section binds and its remaining
    // header keywords — here maxTokens — are then validated.
    let schema = indoc! {"
        sections:
          - header: { pattern: \".+\", maxTokens: 2 }
    "};
    let header = "lower case words here";
    assert_eq!(
        rendered(&format!("# {header}\n"), schema),
        vec![format!(
            "{header}: header is {} tokens, greater than the maximum of 2  [maxTokens @ /sections/0/header/maxTokens]",
            count_tokens(header)
        )],
    );
}

#[test]
fn nested_sections_bind_and_report_missing_children() {
    let schema = indoc! {"
        sections:
          - header: { pattern: \".+\" }
            maxContains: 1
            sections:
              - header: { const: Installation }
              - header: { const: Usage }
    "};
    assert_eq!(
        rendered(indoc! {"
            # Guide

            ## Installation
        "}, schema),
        vec!["Guide: required section \"Usage\" is missing  [minContains @ /sections/0/sections/1/minContains]"],
    );
}

#[test]
fn max_depth_forbids_deeper_headings() {
    let schema = "maxDepth: 1\n";
    assert_eq!(
        rendered(
            indoc! {"
            # Top

            ## Too Deep
        "},
            schema
        ),
        vec!["Top › Too Deep: heading is nested 2 levels deep, greater than the maximum of 1  [maxDepth @ /maxDepth]"],
    );
}

#[test]
fn all_sections_applies_at_every_depth() {
    let schema = indoc! {"
        allSections:
          header: { pattern: \"^[A-Z]\" }
    "};
    let violations = hits(
        indoc! {"
        # Top

        ## lower
    "},
        schema,
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].breadcrumb_text(), "Top › lower");
    assert_eq!(violations[0].keyword, "pattern");
}

// --- token budgets --------------------------------------------------------

#[test]
fn body_token_budget_counts_the_whole_body() {
    let markdown = indoc! {"
        # Title

        several extra words beyond the budget
    "};
    let schema = "maxTokens: 2\n";
    let violations = hits(markdown, schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].message,
        format!(
            "body is {} tokens, greater than the maximum of 2",
            count_tokens(markdown)
        )
    );
    assert_eq!(violations[0].schema_pointer, "/maxTokens");
}

// --- blocks ---------------------------------------------------------------

#[test]
fn additional_block_is_rejected_when_closed() {
    let schema = indoc! {"
        sections:
          - header: { const: Notes }
            blocks:
              - type: paragraph
            additionalBlocks: false
    "};
    assert_eq!(
        rendered(indoc! {"
            # Notes

            a paragraph

            - a list item
        "}, schema),
        vec!["Notes › blocks[1]: unexpected block  [additionalBlocks @ /sections/0/additionalBlocks]"],
    );
}

#[test]
fn code_block_language_participates_in_binding() {
    let schema = indoc! {"
        sections:
          - header: { const: Sample }
            blocks:
              - type: code
                lang: { enum: [rust, toml] }
            additionalBlocks: false
    "};
    // A rust block satisfies the lang identity and binds cleanly.
    assert_eq!(
        hits(
            indoc! {"
            # Sample

            ```rust
            fn x() {}
            ```
        "},
            schema
        ),
        vec![]
    );
    // A python block fails the lang identity: the entry is unbound (missing) and
    // the block itself is additional.
    let messages: Vec<String> = hits(
        indoc! {"
        # Sample

        ```python
        x = 1
        ```
    "},
        schema,
    )
    .iter()
    .map(|violation| violation.message.clone())
    .collect();
    assert_eq!(
        messages,
        vec![
            "required block code is missing".to_string(),
            "unexpected block".to_string(),
        ],
    );
}

#[test]
fn list_length_bounds_are_checked() {
    let schema = indoc! {"
        sections:
          - header: { const: Steps }
            blocks:
              - type: bullet-list
                minItems: 2
    "};
    assert_eq!(
        hits(
            indoc! {"
        # Steps

        - one
        - two
    "},
            schema
        ),
        vec![]
    );
    let short = hits(
        indoc! {"
        # Steps

        - only one
    "},
        schema,
    );
    assert_eq!(short.len(), 1);
    assert_eq!(
        short[0].message,
        "list has 1 item, less than the minimum of 2"
    );
}

#[test]
fn all_blocks_bounds_every_block_text() {
    let schema = indoc! {"
        allBlocks:
          text: { maxTokens: 2 }
    "};
    let long = "a paragraph with far too many words in it";
    let violations = hits(&format!("# H\n\n{long}\n"), schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].message,
        format!(
            "text is {} tokens, greater than the maximum of 2",
            count_tokens(long)
        )
    );
}

#[test]
fn ordered_and_bullet_lists_are_distinct_types() {
    let schema = indoc! {"
        sections:
          - header: { const: List }
            blocks:
              - type: ordered-list
            additionalBlocks: false
    "};
    // A bullet list does not satisfy an `ordered-list` entry.
    let violations = hits(
        indoc! {"
        # List

        - a
        - b
    "},
        schema,
    );
    let messages: Vec<&str> = violations.iter().map(|v| v.message.as_str()).collect();
    assert!(messages.contains(&"required block ordered-list is missing"));
}

// --- frontmatter ----------------------------------------------------------

#[test]
fn frontmatter_required_field_is_enforced_even_when_absent() {
    let schema = indoc! {"
        frontmatter:
          type: object
          required: [status]
    "};
    let violations = hits("# No Frontmatter\n", schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].keyword, "required");
    assert_eq!(violations[0].breadcrumb_text(), "frontmatter");
}

#[test]
fn frontmatter_enum_violation_points_into_the_schema() {
    let schema = indoc! {"
        frontmatter:
          type: object
          properties:
            status: { enum: [draft, published] }
    "};
    let markdown = indoc! {"
        ---
        status: archived
        ---
        # Body
    "};
    let violations = hits(markdown, schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].keyword, "enum");
    assert_eq!(
        violations[0].schema_pointer,
        "/frontmatter/properties/status/enum"
    );
    assert_eq!(violations[0].breadcrumb_text(), "frontmatter › status");
}

#[test]
fn frontmatter_format_assertions_are_active() {
    let schema = indoc! {"
        frontmatter:
          type: object
          properties:
            date: { type: string, format: date }
    "};
    let bad = indoc! {"
        ---
        date: not-a-date
        ---
        # Body
    "};
    let violations = hits(bad, schema);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].keyword, "format");

    let good = indoc! {"
        ---
        date: 2026-01-05
        ---
        # Body
    "};
    assert_eq!(hits(good, schema), vec![]);
}

#[test]
fn reserved_prefix_frontmatter_is_invisible_to_the_schema() {
    let schema = indoc! {"
        frontmatter:
          type: object
          additionalProperties: false
          properties:
            status: { type: string }
    "};
    // `_internal` would trip additionalProperties if it were visible.
    let markdown = indoc! {"
        ---
        status: ok
        _internal: whatever
        ---
        # Body
    "};
    assert_eq!(hits(markdown, schema), vec![]);
}

#[test]
fn unparseable_frontmatter_is_a_violation_even_under_an_empty_schema() {
    let violations = hits(
        indoc! {"
        ---
        status: [unclosed
        ---
        # Body
    "},
        "{}",
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].breadcrumb_text(), "frontmatter");
    assert!(violations[0].message.starts_with("invalid YAML: "));
    assert_eq!(violations[0].keyword, "frontmatter");
    assert_eq!(violations[0].schema_pointer, "");
}

#[test]
fn unparseable_frontmatter_replaces_the_schema_frontmatter_checks() {
    // Without the parse violation, `{}` would misleadingly report `status`
    // as a missing required property.
    let schema = indoc! {"
        frontmatter:
          type: object
          required: [status]
    "};
    let violations = hits(
        indoc! {"
        ---
        status: [unclosed
        ---
        # Body
    "},
        schema,
    );
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.starts_with("invalid YAML: "));
}

#[test]
fn duplicate_frontmatter_keys_are_a_violation() {
    let violations = hits(
        indoc! {"
        ---
        status: a
        status: b
        ---
        # Body
    "},
        "{}",
    );
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.starts_with("invalid YAML: "));
}

#[test]
fn non_mapping_frontmatter_is_a_violation() {
    let violations = hits(
        indoc! {"
        ---
        - item
        ---
        # Body
    "},
        "{}",
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].message, "not a YAML mapping");
}

#[test]
fn empty_frontmatter_block_is_not_a_violation() {
    assert_eq!(
        hits(
            indoc! {"
        ---
        ---
        # Body
    "},
            "{}"
        ),
        vec![]
    );
}

// --- schema load errors ---------------------------------------------------

#[test]
fn invalid_schema_returns_load_errors() {
    let errors = validate(
        "# Body\n",
        indoc! {"
        sections:
          - minContains: -1
    "},
    )
    .unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].pointer, "/sections/0/minContains");
    assert_eq!(errors[0].message, "minContains must not be negative");
}

#[test]
fn ref_type_is_no_longer_a_known_block_type() {
    let errors = validate(
        "# Body\n",
        indoc! {"
        blocks:
          - type: ref
    "},
    )
    .unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "unknown block type 'ref'");
}

#[test]
fn header_length_bounds_are_enforced() {
    let schema = indoc! {"
        allSections:
          header: { minLength: 3, maxLength: 8 }
    "};
    assert_eq!(
        rendered("# Ok\n", schema),
        vec!["Ok: header \"Ok\" is 2 characters, less than the minimum of 3  [minLength @ /allSections/header/minLength]"]
    );
    assert_eq!(
        rendered("# Much Too Long\n", schema),
        vec![
            "Much Too Long: header \"Much Too Long\" is 13 characters, greater than the maximum of 8  [maxLength @ /allSections/header/maxLength]"
        ]
    );
}

#[test]
fn list_max_items_is_enforced() {
    let schema = indoc! {"
        sections:
          - header: { const: Steps }
            blocks:
              - type: bullet-list
                maxItems: 2
    "};
    assert_eq!(
        hits(
            indoc! {"
        # Steps

        - one
        - two
    "},
            schema
        ),
        vec![]
    );
    let long = hits(
        indoc! {"
        # Steps

        - one
        - two
        - three
    "},
        schema,
    );
    assert_eq!(long.len(), 1);
    assert_eq!(
        long[0].message,
        "list has 3 items, greater than the maximum of 2"
    );
}
