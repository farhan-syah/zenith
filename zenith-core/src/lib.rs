//! Foundation crate for Zenith.
//!
//! Owns the KDL-v2 parser adapter, semantic AST types, canonical formatter,
//! token types and resolution, validation engine with the full diagnostic set,
//! AST-based migrations, and deterministic font and asset resolution.
//! No other Zenith crate is a dependency.

pub mod ast;
pub mod diagnostics;
pub mod error;
pub mod font;
pub mod format;
pub mod parse;
pub mod tokens;
pub mod validate;

// Curated flat re-exports for the most-used public surface.
pub use ast::{
    Dimension, Document, DocumentBody, Node, Page, Project, PropertyValue, RectNode, Span,
    StyleBlock, TextNode, TextSpan, Token, TokenBlock, TokenLiteral, TokenType, TokenValue, Unit,
    UnknownNode, UnknownProperty, UnknownValue,
};
pub use diagnostics::{Diagnostic, Severity};
pub use error::{FormatError, ParseError, ParseErrorCode};
pub use font::{BytesFontProvider, FontData, FontProvider, FontStyle, default_provider};
pub use parse::{KdlAdapter, KdlSource};
pub use tokens::{ResolvedToken, ResolvedValue, TokenResolution, resolve_tokens};
pub use validate::{ValidationReport, validate};
