//! `block role="…"` style declaration AST type.
//!
//! A `block` decl maps a markdown block role (h1–h6, p, blockquote, li,
//! code-block, hr) to a set of style and spacing properties. It is a
//! declaration child — NOT a renderable node — and may appear at three scopes:
//! document body, page, and text node. The cascade precedence is text > page >
//! document (highest specificity wins). Block decls are data-only in this
//! unit; the layout engine consumes them in a later unit.

use crate::ast::value::{Dimension, PropertyValue};

/// The recognized block-role vocabulary. Used by `zenith schema block` for
/// documentation and by the formatter for canonical KDL emission.
///
/// Values: h1, h2, h3, h4, h5, h6, p, blockquote, li, code-block, hr.
pub const BLOCK_ROLE_VOCAB: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "blockquote",
    "li",
    "code-block",
    "hr",
];

/// Style + spacing declaration for a single markdown block role.
///
/// Declared as a `block role="h1" …` leaf child at document, page, or text
/// scope. All style fields are optional; absent fields fall through to the
/// next scope in the cascade (text > page > document) and ultimately to the
/// node-level default. An empty `block` decl (only `role` set) is valid and
/// simply acts as a noop override.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStyle {
    /// The block role this decl targets (e.g. `"h1"`, `"p"`, `"blockquote"`).
    pub role: String,
    /// Override font family (token ref or literal).
    pub font_family: Option<PropertyValue>,
    /// Override font size (token ref, pixel literal, or dimension ref).
    pub font_size: Option<PropertyValue>,
    /// Override font weight (token ref or literal).
    pub font_weight: Option<PropertyValue>,
    /// Override text fill color (token ref or literal).
    pub fill: Option<PropertyValue>,
    /// Override text alignment: `"left"`, `"center"`, `"right"`, `"justify"`.
    pub align: Option<String>,
    /// Override italic rendering.
    pub italic: Option<bool>,
    /// Extra space inserted above the block (resolved at compile time).
    pub space_before: Option<Dimension>,
    /// Extra space inserted below the block (resolved at compile time).
    pub space_after: Option<Dimension>,
}
