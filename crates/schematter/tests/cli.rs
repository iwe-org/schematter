//! Integration tests for the `schematter` binary: exit codes, text/JSON output,
//! stdin input, and the schema/IO error path. Cargo builds the binary and hands
//! us its path in `CARGO_BIN_EXE_schematter`.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use indoc::indoc;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_schematter");

const SCHEMA: &str = indoc! {"
    sections:
      - header: { const: Summary }
        description: open with a summary
      - header: { const: Tasks }
    additionalSections: false
"};

struct Case {
    _dir: TempDir,
    schema: String,
    markdown: String,
}

fn case(schema: &str, markdown: &str) -> Case {
    case_with(schema, markdown, &[])
}

fn case_with(schema: &str, markdown: &str, extras: &[(&str, &str)]) -> Case {
    let dir = TempDir::new().unwrap();
    let schema_path = dir.path().join("note.yaml");
    let markdown_path = dir.path().join("note.md");
    std::fs::write(&schema_path, schema).unwrap();
    std::fs::write(&markdown_path, markdown).unwrap();
    for (name, contents) in extras {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    Case {
        _dir: dir,
        schema: schema_path.to_str().unwrap().to_string(),
        markdown: markdown_path.to_str().unwrap().to_string(),
    }
}

impl Case {
    fn path(&self, name: &str) -> String {
        self._dir.path().join(name).to_str().unwrap().to_string()
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run schematter")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn clean_document_exits_zero_and_is_silent() {
    let case = case(
        SCHEMA,
        indoc! {"
        # Summary

        text

        # Tasks
    "},
    );
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "");
}

#[test]
fn violations_exit_one_with_text_report() {
    let case = case(
        SCHEMA,
        indoc! {"
        # Summary

        # Extra
    "},
    );
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(1));
    let expected = indoc! {"
        note: required section \"Tasks\" is missing
        note › Extra: unexpected section
    "};
    assert_eq!(stdout(&output), expected);
}

#[test]
fn hint_line_follows_its_violation() {
    let case = case(SCHEMA, "# Tasks\n");
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(1));
    // `Tasks` binds the second entry; `Summary` is missing and carries a hint.
    let out = stdout(&output);
    assert!(out.contains(indoc! {"
        note: required section \"Summary\" is missing
          hint: open with a summary
    "}));
}

#[test]
fn json_output_is_an_array_of_reports() {
    let case = case(
        SCHEMA,
        indoc! {"
        # Summary

        # Extra
    "},
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));

    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!([
            {
                "key": "note",
                "schema": "note",
                "violations": [
                    {
                        "breadcrumb": [],
                        "message": "required section \"Tasks\" is missing",
                        "hint": null,
                        "schemaPath": "/sections/1/minContains",
                        "keyword": "minContains"
                    },
                    {
                        "breadcrumb": ["Extra"],
                        "message": "unexpected section",
                        "hint": null,
                        "schemaPath": "/additionalSections",
                        "keyword": "additionalSections"
                    }
                ]
            }
        ])
    );
}

#[test]
fn clean_json_output_is_an_empty_array() {
    let case = case(
        SCHEMA,
        indoc! {"
        # Summary

        # Tasks
    "},
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(parsed, serde_json::json!([]));
}

#[test]
fn markdown_is_read_from_stdin_when_no_path_is_given() {
    let case = case(SCHEMA, "");
    let mut child = Command::new(BIN)
        .args(["validate", "--schema", &case.schema])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(indoc! {b"
            # Summary

            # Extra
        "})
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    // Key falls back to `<stdin>`.
    assert!(stdout(&output).starts_with("<stdin>: required section \"Tasks\" is missing"));
}

#[test]
fn invalid_schema_exits_two_via_stderr() {
    let case = case(
        indoc! {"
        sections:
          - minContains: -1
    "},
        "# Summary\n",
    );
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "schema 'note' /sections/0/minContains: minContains must not be negative\n"
    );
}

#[test]
fn missing_schema_file_exits_two() {
    let case = case(SCHEMA, "# Summary\n");
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        "does-not-exist.yaml",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("reading schema does-not-exist.yaml"));
}

#[test]
fn multiple_files_report_under_their_own_keys() {
    let dir = TempDir::new().unwrap();
    let schema_path = dir.path().join("note.yaml");
    let clean_path = dir.path().join("clean.md");
    let broken_path = dir.path().join("broken.md");
    std::fs::write(&schema_path, SCHEMA).unwrap();
    std::fs::write(
        &clean_path,
        indoc! {"
        # Summary

        # Tasks
    "},
    )
    .unwrap();
    std::fs::write(
        &broken_path,
        indoc! {"
        # Summary

        # Extra
    "},
    )
    .unwrap();

    let output = run(&[
        "validate",
        clean_path.to_str().unwrap(),
        broken_path.to_str().unwrap(),
        "--schema",
        schema_path.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let expected = indoc! {"
        broken: required section \"Tasks\" is missing
        broken › Extra: unexpected section
    "};
    assert_eq!(stdout(&output), expected);
}

#[test]
fn explain_prints_binding_trace_and_exits_zero() {
    let case = case(
        SCHEMA,
        indoc! {"
        # Summary

        # Extra
    "},
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--explain",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let expected = indoc! {"
        note  [schema: note]
        # Summary  ->  sections[0]
        # Extra  ->  additional

    "};
    assert_eq!(stdout(&output), expected);
}

#[test]
fn unparseable_schema_yaml_exits_two_without_pointer() {
    let case = case("sections: [\n", "# Summary\n");
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).starts_with("schema 'note': "));
}

const META_SCHEMA: &str = indoc! {"
    type: object
    required: [status]
"};

const LIB_SCHEMA: &str = indoc! {"
    sections:
      - header: { const: Summary }
"};

#[test]
fn registered_frontmatter_ref_is_resolved() {
    let case = case_with(
        indoc! {"
            frontmatter:
              $ref: https://example.com/meta.json
        "},
        indoc! {"
            # Notes
        "},
        &[("meta.json", META_SCHEMA)],
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--ref",
        &format!("https://example.com/meta.json={}", case.path("meta.json")),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "note \u{203a} frontmatter: \"status\" is a required property\n"
    );
}

#[test]
fn registered_section_ref_is_resolved() {
    let case = case_with(
        indoc! {"
            sections:
              - $ref: 'https://example.com/lib.yaml#/sections/0'
            additionalSections: false
        "},
        indoc! {"
            # Summary

            text
        "},
        &[("lib.yaml", LIB_SCHEMA)],
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--ref",
        &format!("https://example.com/lib.yaml={}", case.path("lib.yaml")),
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");
}

#[test]
fn a_ref_file_may_carry_its_own_id() {
    let case = case_with(
        indoc! {"
            sections:
              - $ref: 'https://example.com/lib.yaml#/sections/0'
        "},
        indoc! {"
            # Notes
        "},
        &[(
            "lib.yaml",
            indoc! {"
                $id: https://example.com/lib.yaml
                sections:
                  - header: { const: Summary }
            "},
        )],
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--ref",
        &case.path("lib.yaml"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "note: required section \"Summary\" is missing\n"
    );
}

#[test]
fn resolve_refs_reads_a_relative_reference_from_disk() {
    let case = case_with(
        indoc! {"
            sections:
              - $ref: ./section.yaml
            additionalSections: false
        "},
        indoc! {"
            # Summary

            text
        "},
        &[("section.yaml", "header: { const: Summary }\n")],
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--resolve-refs",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");
    assert_eq!(stdout(&output), "");
}

#[test]
fn an_external_ref_without_flags_exits_two() {
    let case = case(
        indoc! {"
            frontmatter:
              $ref: https://example.com/meta.json
        "},
        "# Notes\n",
    );
    let output = run(&["validate", &case.markdown, "--schema", &case.schema]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "schema 'note' /frontmatter: external references are not allowed\n"
    );
}

#[test]
fn resolve_refs_refuses_network_schemes() {
    let case = case(
        indoc! {"
            sections:
              - $ref: https://example.com/lib.yaml
        "},
        "# Notes\n",
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--resolve-refs",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("'https:' references are not read from disk"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn resolve_refs_follows_a_chain_across_directories() {
    let case = case_with(
        indoc! {"
            sections:
              - $ref: lib/section.yaml
            additionalSections: false
        "},
        indoc! {"
            # Summary

            text
        "},
        &[
            ("lib/section.yaml", "$ref: ./summary.yaml\nmaxTokens: 500\n"),
            (
                "lib/summary.yaml",
                indoc! {"
                    $schema: https://document-schema.org/draft/2026-06/schema
                    $id: https://schemas.example.com/summary.yaml
                    header: { const: Summary }
                "},
            ),
        ],
    );
    let output = run(&[
        "validate",
        &case.markdown,
        "--schema",
        &case.schema,
        "--resolve-refs",
    ]);
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));
}
