//! Core entities and validation for schematter document schemas.
//!
//! A document schema declares the required shape of a page: which frontmatter
//! fields it carries (validated as literal JSON Schema, draft 2020-12), which
//! sections it contains and in what order, how headers are written, how deep
//! the heading tree may nest, and how large each part may grow.
//!
//! This crate is independent of any markup parser: it operates on the
//! already-parsed [`Document`] model. [`compile_schema`] turns a schema source
//! into a [`CompiledSchema`] (or a list of [`SchemaError`] load errors), and
//! [`CompiledSchema::validate`] checks a [`Document`] against it, returning the
//! combined frontmatter + structural [`Violation`]s.
//!
//! To validate markdown source directly, use the `schematter-lib` crate, which
//! builds the [`Document`] with its markdown parser and wraps the two steps in
//! a one-call `validate`.

pub mod compile;
pub mod dialect;
pub mod document;
mod refs;
pub mod resolve;
pub mod violation;

pub use compile::{compile_schema, compile_schema_with, CompiledSchema, SchemaError};
pub use document::{Block, BlockKind, Document, Item, Section};
pub use resolve::{CompileOptions, ResolveError, Resolver};
pub use violation::{Crumb, Violation};
