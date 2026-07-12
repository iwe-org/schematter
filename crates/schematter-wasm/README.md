# schematter-wasm

WebAssembly bindings for [schematter](https://github.com/iwe-org/schematter):
validate a markdown document against a document schema in the browser.

A **document schema** declares the required shape of a markdown page — which
frontmatter fields it carries, which sections it contains and in what order, how
headers are written, how deep the heading tree may nest, and how large each part
may grow. This crate wraps [`schematter-lib`](https://crates.io/crates/schematter-lib)
in a `wasm-bindgen` surface so the same validator runs client-side.

This crate is not published to crates.io; build it from the
[repository](https://github.com/iwe-org/schematter).

## Build

```bash
wasm-pack build crates/schematter-wasm --target web
```

## API

Two exported functions:

```js
import init, { validate, dialect } from "./pkg/schematter_wasm.js";

await init();

const result = JSON.parse(validate(markdownSource, schemaSource));
// { ok: true,  violations: [ ... ] }  when the schema compiled
// { ok: false, errors:     [ ... ] }  when the schema itself is invalid

dialect(); // "https://document-schema.org/draft/2026-06/schema"
```

`validate` returns a JSON string: on a valid schema, `ok: true` with the list of
violations (empty when the document conforms); on an invalid schema, `ok: false`
with the load errors, each carrying a `pointer` and a `message`. `dialect`
returns the meta-schema URI for the schema language.

## License

Apache-2.0; see [LICENSE-APACHE](https://github.com/iwe-org/schematter/blob/main/LICENSE-APACHE).
