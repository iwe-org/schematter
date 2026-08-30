//! The published meta-schema (`schema/draft/2026-06/schema.json`) must stay in
//! step with what the compiler accepts. These tests build it as a draft
//! 2020-12 JSON Schema and check that valid schemas pass and malformed ones are
//! flagged — the same distinctions `compile_schema` draws, expressed
//! structurally so editor tooling sees them too.

use indoc::indoc;
use jsonschema::{Draft, Validator};
use serde_json::Value;

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

fn accepts(validator: &Validator, yaml: &str) -> bool {
    validator.is_valid(&as_json(yaml))
}

#[test]
fn empty_schema_is_accepted() {
    assert!(accepts(&metaschema(), "{}"));
}

#[test]
fn readme_event_schema_is_accepted() {
    let source = indoc! {"
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
    "};
    assert!(accepts(&metaschema(), source));
}

#[test]
fn nested_block_schema_is_accepted() {
    let source = indoc! {"
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
    "};
    assert!(accepts(&metaschema(), source));
}

#[test]
fn union_type_and_reduced_additional_schema_are_accepted() {
    let source = indoc! {"
        blocks:
          - type: [bullet-list, ordered-list]
            minItems: 1
        additionalBlocks:
          maxTokens: 20
        additionalSections:
          maxTokens: 300
    "};
    assert!(accepts(&metaschema(), source));
}

#[test]
fn unknown_keyword_is_rejected() {
    assert!(!accepts(&metaschema(), "width: 3\n"));
}

#[test]
fn unknown_block_type_is_rejected() {
    assert!(!accepts(
        &metaschema(),
        indoc! {"
        blocks:
          - type: heading
    "}
    ));
}

#[test]
fn empty_type_list_is_rejected() {
    assert!(!accepts(
        &metaschema(),
        indoc! {"
        blocks:
          - type: []
    "}
    ));
}

#[test]
fn const_and_enum_together_are_rejected() {
    let source = indoc! {"
        sections:
          - header: { const: A, enum: [A, B] }
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn negative_budget_is_rejected() {
    assert!(!accepts(&metaschema(), "maxTokens: -1\n"));
}

#[test]
fn lang_on_non_code_block_is_rejected() {
    let source = indoc! {"
        blocks:
          - type: paragraph
            lang: { const: rust }
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn items_on_non_list_block_is_rejected() {
    let source = indoc! {"
        blocks:
          - type: code
            items:
              text: { maxTokens: 5 }
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn quote_keyword_on_non_quote_block_is_rejected() {
    let source = indoc! {"
        blocks:
          - type: paragraph
            blocks:
              - type: paragraph
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn type_specific_keyword_without_type_is_rejected() {
    assert!(!accepts(
        &metaschema(),
        indoc! {"
        blocks:
          - lang: { const: rust }
    "}
    ));
}

#[test]
fn structural_keyword_in_all_sections_is_rejected() {
    assert!(!accepts(
        &metaschema(),
        indoc! {"
        allSections:
          sections: []
    "}
    ));
}

#[test]
fn structural_keyword_in_all_blocks_is_rejected() {
    let source = indoc! {"
        allBlocks:
          items:
            text: { maxTokens: 5 }
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn references_and_definitions_are_accepted() {
    let source = indoc! {"
        $id: https://example.com/main.yaml
        $defs:
          summary:
            header: { const: Summary }
        sections:
          - $ref: '#/$defs/summary'
          - header: { const: Tasks }
            blocks:
              - $ref: https://example.com/lib.yaml#/blocks/0
    "};
    assert!(accepts(&metaschema(), source));
}

#[test]
fn top_level_reference_is_accepted() {
    let source = indoc! {"
        $ref: https://example.com/lib.yaml
        maxTokens: 400
    "};
    assert!(accepts(&metaschema(), source));
}

#[test]
fn non_string_reference_is_rejected() {
    let source = indoc! {"
        sections:
          - $ref: 3
    "};
    assert!(!accepts(&metaschema(), source));
}

#[test]
fn unknown_dialect_is_rejected() {
    let source = "$schema: https://document-schema.org/draft/2027-01/schema\n";
    assert!(!accepts(&metaschema(), source));
}
