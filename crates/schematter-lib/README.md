# schematter-lib

Schema validation for markdown documents: JSON Schema over frontmatter, plus a
structural schema over the section and block tree.

A **document schema** declares the required shape of a markdown page — which
frontmatter fields it carries (validated as literal JSON Schema, draft 2020-12),
which sections it contains and in what order, how headers are written, how deep
the heading tree may nest, and how large each part may grow. This crate is the
integration API: markdown and a schema go in, violations come out. It adds the
markdown parser (`pulldown-cmark`) and the token counter (`tiktoken-rs`) on top
of the markup-agnostic [`schematter-validator`](https://crates.io/crates/schematter-validator)
core.

For the command-line tool, see [`schematter`](https://crates.io/crates/schematter).
For the schema language and the full story, see the
[project README](https://github.com/iwe-org/schematter#readme).

## Usage

The one-call entry point is `validate`, which takes markdown and a schema source
and returns the combined frontmatter + structural violations:

```rust
let schema = "sections:\n  - header: { const: Summary }\n";
let markdown = "# Summary\n\ntext\n";
let violations = schematter_lib::validate(markdown, schema).unwrap();
assert!(violations.is_empty());
```

`validate` returns `Err(Vec<SchemaError>)` when the schema itself doesn't
compile, and `Ok(Vec<Violation>)` otherwise; each `Violation` carries a message,
a breadcrumb into the document, a JSON pointer into the schema, the failing
keyword, and the hint (the nearest schema `description`).

The pieces are also exposed directly for finer control: `compile_schema` turns a
schema source into a `CompiledSchema` (or a list of `SchemaError` load errors),
`build_document` projects markdown into the `Document` model, and
`CompiledSchema::validate` runs one against the other — so a compiled schema can
be reused across many documents:

```rust
use schematter_lib::{build_document, compile_schema};
use schematter_lib::tokens::count_tokens;

let compiled = compile_schema("sections:\n  - header: { const: Summary }\n").unwrap();
for markdown in ["# Summary\n\na\n", "# Other\n\nb\n"] {
    let document = build_document(markdown, count_tokens);
    let violations = compiled.validate(&document);
    println!("{} violations", violations.len());
}
```

External references — the dialect's `$ref` between document schemas, and JSON
Schema `$ref` inside `frontmatter` — are resolved against schemas supplied up
front through `CompileOptions`, with an optional resolver consulted only on a
miss. The crate performs no I/O of its own:

```rust
use schematter_lib::{validate_with, CompileOptions};

let options = CompileOptions::new()
    .with_schema(
        "https://schemas.example.com/shared.yaml",
        serde_json::json!({ "sections": [{ "header": { "const": "Summary" } }] }),
    )
    .with_resolver(|uri: &str| Err(format!("{uri} is not available offline")));

let schema = "sections:\n  - $ref: 'https://schemas.example.com/shared.yaml#/sections/0'\n";
let violations = validate_with("# Summary\n\ntext\n", schema, &options).unwrap();
assert!(violations.is_empty());
```

## Related crates

| Crate                                                                  | Kind    | Contents                                              |
| ---------------------------------------------------------------------- | ------- | ----------------------------------------------------- |
| [`schematter`](https://crates.io/crates/schematter)                    | binary  | the `schematter` command                              |
| [`schematter-lib`](https://crates.io/crates/schematter-lib)            | library | this crate                                            |
| [`schematter-validator`](https://crates.io/crates/schematter-validator) | library | schema language, compiler, matcher, and document model |

## License

Apache-2.0; see [LICENSE-APACHE](https://github.com/iwe-org/schematter/blob/main/LICENSE-APACHE).
