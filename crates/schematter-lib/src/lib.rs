//! Schema validation for markdown documents.
//!
//! A document schema declares the required shape of a markdown page: which
//! frontmatter fields it carries (validated as literal JSON Schema, draft
//! 2020-12), which sections it contains and in what order, how headers are
//! written, how deep the heading tree may nest, and how large each part may
//! grow.
//!
//! # Surface
//!
//! The one-call entry point is [`validate`], which takes markdown and a schema
//! source and returns the combined frontmatter + structural violations:
//!
//! ```
//! let schema = "sections:\n  - header: { const: Summary }\n";
//! let markdown = "# Summary\n\ntext\n";
//! let violations = schematter_lib::validate(markdown, schema).unwrap();
//! assert!(violations.is_empty());
//! ```
//!
//! The pieces are also exposed directly: [`compile_schema`] turns a schema
//! source into a [`CompiledSchema`] (or a list of [`SchemaError`] load errors),
//! [`build_document`] projects markdown into the [`Document`] model, and
//! [`CompiledSchema::validate`] runs one against the other. The schema
//! language, matcher, and document model live in the `schematter-validator`
//! crate (re-exported here); this crate adds the markdown parser and token
//! counter.

pub mod builder;
pub mod tokens;

pub use schematter_validator::{compile, dialect, document, violation};

pub use builder::build_document;
pub use schematter_validator::{compile_schema, CompiledSchema, SchemaError};
pub use schematter_validator::{Block, BlockKind, Document, Item, Section};
pub use schematter_validator::{Crumb, Violation};

/// Validate `markdown` against a `schema` source.
///
/// Returns the list of [`Violation`]s (empty when the document conforms), or the
/// [`SchemaError`] load errors if the schema itself is invalid. Frontmatter is
/// checked as JSON Schema and the body against the structural schema; the two
/// sets of violations are returned together.
pub fn validate(markdown: &str, schema: &str) -> Result<Vec<Violation>, Vec<SchemaError>> {
    let compiled = compile_schema(schema)?;
    let document = build_document(markdown, tokens::count_tokens);
    Ok(compiled.validate(&document))
}
