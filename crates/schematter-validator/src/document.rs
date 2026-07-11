//! The document model the validator runs against.
//!
//! A [`Document`] is the schema-visible projection of a markdown page: its
//! frontmatter as a JSON value, its section tree, and the content blocks that
//! hang off the document root and each section. It is deliberately independent
//! of any particular parser — a builder such as `schematter_lib::build_document`
//! constructs one from markdown source, but the validator only ever sees this
//! model.

use serde_json::Value;

/// A whole markdown page, projected for schema validation.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Frontmatter mapping as JSON (`{}` when the page has none), with
    /// reserved-prefix keys (`_ $ . # @`) removed.
    pub frontmatter: Value,
    /// Why the frontmatter block could not be parsed, when the page has one
    /// that is not valid YAML or not a mapping; `frontmatter` is `{}` then.
    pub frontmatter_error: Option<String>,
    /// Token count of the rendered body, frontmatter excluded.
    pub body_tokens: usize,
    /// Content blocks above the first heading.
    pub blocks: Vec<Block>,
    /// Top-level sections, in document order.
    pub sections: Vec<Section>,
}

/// A heading and everything nested beneath it.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Rendered plain text of the heading, inline markup stripped.
    pub header: String,
    /// Structural depth after normalization (`1` for the shallowest heading).
    pub level: usize,
    /// Token count of the header text.
    pub header_tokens: usize,
    /// Token count of the section subtree, header included.
    pub subtree_tokens: usize,
    /// The section's own content blocks, before its subsections.
    pub blocks: Vec<Block>,
    /// Nested subsections, in document order.
    pub sections: Vec<Section>,
}

/// A single piece of non-section content.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Which of the eight kinds this block is.
    pub kind: BlockKind,
    /// The block's own plain text (empty for containers).
    pub text: String,
    /// Token count of the block's own text.
    pub text_tokens: usize,
    /// Token count of the block's whole subtree.
    pub subtree_tokens: usize,
    /// Fenced-code language, when `kind` is [`BlockKind::Code`].
    pub lang: Option<String>,
    /// List items, when `kind` is a list.
    pub items: Vec<Item>,
    /// Child blocks, when `kind` is [`BlockKind::Quote`].
    pub blocks: Vec<Block>,
}

/// One element of a bullet or ordered list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The item's own plain text.
    pub text: String,
    /// Token count of the item's own text.
    pub text_tokens: usize,
    /// Token count of the item's whole subtree.
    pub subtree_tokens: usize,
    /// The item's nested content blocks.
    pub blocks: Vec<Block>,
}

/// The block kinds recognized by the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    BulletList,
    OrderedList,
    Code,
    Quote,
    Table,
    Rule,
}
