# schematter

Command-line validator for markdown document schemas.

A **document schema** declares the required shape of a markdown page — which
frontmatter fields it carries, which sections it contains and in what order, how
headers are written, how deep the heading tree may nest, and how large each part
may grow — and `schematter` checks a document against it, returning a list of
violations. Think JSON Schema, but for the *structure* of a markdown page rather
than the structure of a JSON value.

This crate is the `schematter` command. The schema language, the library API,
and the full story live in the
[project README](https://github.com/iwe-org/schematter#readme).

## Install

```bash
cargo install schematter
```

Prebuilt binaries for Linux, macOS, and Windows are attached to each
[GitHub release](https://github.com/iwe-org/schematter/releases).

## Usage

```bash
schematter validate NOTE.md --schema note.yaml       # text output
schematter validate docs/*.md --schema note.yaml -f json
cat NOTE.md | schematter validate --schema note.yaml # stdin
schematter validate NOTE.md --schema note.yaml --explain
```

Exit codes: `0` clean, `1` violations found, `2` the schema itself is invalid
(or an I/O error) — ready for CI, git hooks, or an agent's write path.

Given `event.yaml`:

```yaml
description: a dated event page in an agent-maintained knowledge base
frontmatter:
  type: object
  required: [type, date]
  properties:
    type: { const: event }
    date: { type: string, format: date }
maxTokens: 400
maxDepth: 1
sections:
  - header: { pattern: '.+\(\d{1,2} [A-Z][a-z]+ \d{4}\)$' }
    description: the title carries the event date, e.g. "Jon visited Paris (28 January 2023)"
    maxContains: 1
    blocks:
      - type: paragraph
additionalSections: false
```

a non-conforming page reports every violation with the nearest schema
`description` as its hint:

```
$ schematter validate bad.md --schema event.yaml
bad › frontmatter › date: "January 28th" is not a "date"
  hint: a dated event page in an agent-maintained knowledge base
bad › Jon visited Paris › Details: heading depth 2 exceeds maximum 1
  hint: a dated event page in an agent-maintained knowledge base
bad: required section matching '.+\(\d{1,2} [A-Z][a-z]+ \d{4}\)$' missing
  hint: the title carries the event date, e.g. "Jon visited Paris (28 January 2023)"
bad › Jon visited Paris: unexpected section
  hint: a dated event page in an agent-maintained knowledge base
```

`-f json` emits the same reports as structured data, one entry per failing
document, each violation carrying the breadcrumb into the document, the JSON
pointer into the schema, the failing keyword, and the hint. `--explain` prints
the binding trace — which section and block bound to which schema entry —
instead of validating, which is how you debug a schema that matches differently
than you expect.

## External references

A schema can point at another with `$ref` — a whole file or one node inside it
— and its `frontmatter` can `$ref` an external JSON Schema. Nothing is fetched
on its own: name the targets up front, or let the CLI read them from disk.

```bash
schematter validate NOTE.md --schema note.yaml \
  --ref https://schemas.example.com/shared.yaml=shared.yaml

schematter validate NOTE.md --schema note.yaml --resolve-refs
```

`--ref URI=FILE` registers one file under one URI and repeats; `--ref FILE`
alone takes the URI from the file's own `$id`. `--resolve-refs` reads whatever
is left from disk, resolved against the schema file's directory — `file:` and
relative references only; `http`/`https` are refused by scheme. An unresolved
reference is a schema error, so the command exits `2` rather than validating.

## The schema language

The full language reference — every keyword, the matching semantics, the block
vocabulary, and the load-error catalog — is in
[docs/document-schema.md](https://github.com/iwe-org/schematter/blob/main/docs/document-schema.md).
A JSON Schema meta-schema for the dialect ships in the
[repository](https://github.com/iwe-org/schematter/blob/main/schema/draft/2026-06/schema.json).

## Related crates

| Crate                                                                  | Kind    | Contents                                              |
| ---------------------------------------------------------------------- | ------- | ----------------------------------------------------- |
| [`schematter`](https://crates.io/crates/schematter)                    | binary  | this command                                          |
| [`schematter-lib`](https://crates.io/crates/schematter-lib)            | library | markdown + schema in, violations out                  |
| [`schematter-validator`](https://crates.io/crates/schematter-validator) | library | schema language, compiler, matcher, and document model |

## License

Apache-2.0; see [LICENSE-APACHE](https://github.com/iwe-org/schematter/blob/main/LICENSE-APACHE).
