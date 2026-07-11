# schematter

Schema validation for markdown documents. A **document schema** declares the
required shape of a page — which frontmatter fields it carries, which sections
it contains and in what order, how headers are written, how deep the heading
tree may nest, and how large each part may grow — and `schematter` checks a
document against it, returning a list of violations.

Think JSON Schema, but for the *structure* of a markdown page rather than the
structure of a JSON value.

## Why

Markdown knowledge bases are increasingly written by agents: memory stores,
curated note graphs, generated documentation. Agents are good at following
structural recipes and bad at holding stylistic conventions — page budgets, key
formats, required sections — through a long session. Prompt rules decay as
context grows, and when a convention is only stated in a prompt, an agent under
pressure will negotiate with it ("these warnings are expected here") or quietly
trade it away against a competing rule.

Mechanical validation behaves differently. It re-fires on every write, it
cannot be argued with, and it names the exact violation and the fix. While
benchmarking markdown knowledge graphs as agent memory for
[IWE](https://iwe.md), we found that one validation loop did what three rounds
of prompt iteration could not: pages that no instruction could keep in shape
snapped to their budgets, and downstream question-answering scores jumped
accordingly. Better still, validating three generations of stores against the
same schema ordered them exactly as their benchmark scores had — the schema
measured store quality at write time, without asking a single question.

The lesson generalizes into a division of labor: **schemas own shape**
(required sections, token budgets, block types — checkable, nothing to argue
with), **prompts own semantics** (what deserves a page, which date is the
event's). `schematter` is the shape half, extracted as a standalone tool: a
schema language, a CLI, and embeddable libraries, with no dependency on iwe.

The language deliberately tracks JSON Schema. Frontmatter is validated by
literal JSON Schema (draft 2020-12); the body schema mirrors the document's own
structure — a document has `sections`, a section has a `header` and its own
nested `sections` — and keyword names and semantics are borrowed from JSON
Schema wherever the concept maps (`pattern`, `const`, `enum`, `minLength`,
`maxLength`, `minContains`, `maxContains`, `additionalSections`,
`description`). Both humans and models already know these keywords, so the
dialect is readable on sight.

## Example

A schema for a dated event page in an agent-maintained store, `event.yaml`:

```yaml
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
  - header: { pattern: '.+\(\d{1,2} [A-Z][a-z]+ \d{4}\)$' }
    description: the title carries the event date, e.g. "Jon visited Paris (28 January 2023)"
    maxContains: 1
    blocks:
      - type: paragraph
additionalSections: false
```

This page passes:

```markdown
---
type: event
date: 2023-01-28
participants:
- jon
---

# Jon visited Paris (28 January 2023)

Jon visited Paris. He described the experience enthusiastically and shared a
photo from the trip.
```

This page does not — the date is free text, the title carries no date, and a
subsection nests below the depth cap:

```markdown
---
type: event
date: January 28th
---

# Jon visited Paris

Jon visited Paris. He described the experience enthusiastically and shared a
photo from the trip.

## Details

He also mentioned the food.
```

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
$ echo $?
1
```

Every violation carries the nearest schema `description` as its hint, so the
error message documents the convention it enforces — which matters when the
reader recovering from the error is an agent.

## The schema language

A schema is a YAML document describing one kind of page. `schematter` checks
one document against one schema; deciding which schema governs which page — by
path, key pattern, or frontmatter `type` — is the caller's job. The main
pieces:

- **Frontmatter** — the `frontmatter` keyword holds literal JSON Schema
  (draft 2020-12), applied to the page's YAML frontmatter with format
  assertions enabled (`format: date` rejects `January 28th`). A missing
  frontmatter block is validated as an empty object, so `required` fields are
  enforced either way; a block that is present but is not parseable YAML — or
  not a mapping — is itself a violation, never silently ignored. Fields whose
  names start with a reserved prefix (`_`, `$`, `.`, `#`, `@`) belong to
  tooling and are invisible to the schema.
- **Sections** — the `sections` list declares expected sections in order. An
  entry's `header` (`const`, `pattern`, `enum`) is its *binding key*: documents
  bind headers to entries greedily, in order, without backtracking, and an
  entry left unbound reports as missing. Entries require one match by default;
  `minContains` / `maxContains` allow repetition (three dated entries matching
  one pattern) or cap it. Sections nest: an entry takes its own `sections`,
  `blocks`, and budgets. `additionalSections: false` closes a scope;
  `additionalSections` with a reduced schema constrains extras instead.
  `allSections` applies one reduced schema to every section at every depth.
- **Blocks** — the `blocks` list constrains a section's body the same way:
  `type` is one of `paragraph`, `bullet-list`, `ordered-list`, `code`, `quote`,
  `table`. Code blocks take a `lang` constraint, lists take `items` /
  `minItems` / `maxItems`, quotes recurse with their own `blocks`.
  `additionalBlocks` and `allBlocks` mirror their section counterparts.
- **Budgets** — `maxTokens` bounds the document body, a section, a header, a
  block's text, or a list item; `maxDepth` caps heading nesting. Tokens are
  counted with OpenAI's `o200k_base` BPE, so budgets are denominated in the
  units an LLM reader actually pays.

The full language reference — every keyword, the matching semantics, the
block vocabulary, and the load-error catalog — is in
[docs/document-schema.md](docs/document-schema.md). A JSON Schema meta-schema
for the dialect ships at
[schema/draft/2026-06/schema.json](schema/draft/2026-06/schema.json); point a
JSON-Schema-aware YAML editor at it (or add `$schema:
https://document-schema.org/draft/2026-06/schema` to a schema file) for
completion and validation as you write.

## Install

```bash
cargo install schematter
```

Prebuilt binaries for Linux, macOS, and Windows are attached to each
[GitHub release](https://github.com/iwe-org/schematter/releases).

## CLI

```bash
schematter validate NOTE.md --schema note.yaml       # text output
schematter validate docs/*.md --schema note.yaml -f json
cat NOTE.md | schematter validate --schema note.yaml # stdin
schematter validate NOTE.md --schema note.yaml --explain
```

Exit codes: `0` clean, `1` violations found, `2` the schema itself is invalid
(or an I/O error) — ready for CI, git hooks, or an agent's write path.

`-f json` emits the same reports as structured data, one entry per failing
document, each violation carrying the breadcrumb into the document, the JSON
pointer into the schema, the failing keyword, and the hint:

```json
[
  {
    "key": "bad",
    "schema": "event",
    "violations": [
      {
        "breadcrumb": ["frontmatter", "date"],
        "hint": "a dated event page in an agent-maintained knowledge base",
        "keyword": "format",
        "message": "\"January 28th\" is not a \"date\"",
        "schemaPath": "/frontmatter/properties/date/format"
      }
    ]
  }
]
```

`--explain` prints the binding trace — which section and block bound to which
schema entry — instead of validating, which is how you debug a schema that
matches differently than you expect:

```
$ schematter validate good.md --schema event.yaml --explain
good  [schema: event]
# Jon visited Paris (28 January 2023)  ->  sections[0]
  paragraph "Jon visited Paris. He described the expe..."  ->  blocks[0]
```

## Library

```rust
let schema = "sections:\n  - header: { const: Summary }\n";
let markdown = "# Summary\n\ntext\n";
let violations = schematter_lib::validate(markdown, schema).unwrap();
assert!(violations.is_empty());
```

`validate` returns `Err(Vec<SchemaError>)` when the schema itself doesn't
compile, and `Ok(Vec<Violation>)` otherwise; each `Violation` carries a
message, a breadcrumb into the document, a JSON pointer into the schema, the
failing keyword, and the hint.

## Layout

| Crate                  | Kind    | Contents                                                                    |
| ---------------------- | ------- | --------------------------------------------------------------------------- |
| `schematter`           | binary  | the `schematter` command                                                     |
| `schematter-lib`       | library | integration API: markdown + schema in, violations out (parser, token counter, one-call `validate`) |
| `schematter-validator` | library | schema language, compiler, matcher, and the document model — markup-agnostic, no parser dependency |

Each crate depends on the next: `schematter` → `schematter-lib` →
`schematter-validator`. `schematter-lib` parses markdown with `pulldown-cmark`
and counts tokens with `tiktoken-rs`; `schematter-validator` operates on the
already-parsed document model, so other markup languages can target it with
their own builders. Nothing depends on iwe.

## Build

```bash
cargo build
cargo test
```

The minimum supported Rust version is 1.82.

## Releases

CI lives in `.github/workflows/`: `rust.yml` builds and tests every push and
pull request on Linux, macOS, and Windows; `release-plz.yaml` versions and
publishes the crates to crates.io on pushes to `main` (requires the
`RELEASE_PLZ_TOKEN` and `CARGO_REGISTRY_TOKEN` repository secrets);
`release.yaml` attaches prebuilt `schematter` binaries for six targets to each
published GitHub release.

## License

Apache-2.0; see [LICENSE-APACHE](LICENSE-APACHE).
