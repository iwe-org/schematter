# schematter-validator

Low-level validation for schematter document schemas via domain objects: the
parsed-document model, the schema compiler, and the matcher, independent of any
markup parser.

A **document schema** declares the required shape of a page — which frontmatter
fields it carries (validated as literal JSON Schema, draft 2020-12), which
sections it contains and in what order, how headers are written, how deep the
heading tree may nest, and how large each part may grow. This crate is the core:
it operates on the already-parsed `Document` model and carries no markdown
parser, so other markup languages can target it with their own builders.

To validate markdown source directly, use
[`schematter-lib`](https://crates.io/crates/schematter-lib), which builds the
`Document` with its markdown parser and token counter and wraps the two steps in
a one-call `validate`. For the command-line tool, see
[`schematter`](https://crates.io/crates/schematter). For the schema language and
the full story, see the
[project README](https://github.com/iwe-org/schematter#readme).

## Usage

`compile_schema` turns a schema source into a `CompiledSchema` (or a list of
`SchemaError` load errors), and `CompiledSchema::validate` checks a `Document`
against it, returning the combined frontmatter + structural `Violation`s:

```rust
use schematter_validator::compile_schema;

let compiled = compile_schema("sections:\n  - header: { const: Summary }\n").unwrap();
// `document` is a schematter_validator::Document built by your own parser
// (or by schematter-lib's markdown builder).
let violations = compiled.validate(&document);
assert!(violations.is_empty());
```

The `Document` model — `Document`, `Section`, `Block`, `BlockKind`, `Item` — is
the contract between a markup parser and the validator. Build it however you
like; the matcher only sees the model, never the source syntax.

## Related crates

| Crate                                                                  | Kind    | Contents                                              |
| ---------------------------------------------------------------------- | ------- | ----------------------------------------------------- |
| [`schematter`](https://crates.io/crates/schematter)                    | binary  | the `schematter` command                              |
| [`schematter-lib`](https://crates.io/crates/schematter-lib)            | library | markdown + schema in, violations out                  |
| [`schematter-validator`](https://crates.io/crates/schematter-validator) | library | this crate                                            |

## License

Apache-2.0; see [LICENSE-APACHE](https://github.com/iwe-org/schematter/blob/main/LICENSE-APACHE).
