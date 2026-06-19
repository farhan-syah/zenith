//! Foundation crate for Zenith.
//!
//! Owns the KDL-v2 parser adapter, semantic AST types, canonical formatter,
//! token types and resolution, validation engine with the full diagnostic set,
//! AST-based migrations, and deterministic font and asset resolution.
//! No other Zenith crate is a dependency.

pub mod asset;
pub mod ast;
pub mod diagnostics;
pub mod error;
pub mod font;
pub mod format;
pub mod parse;
pub mod tokens;
pub mod validate;

// Curated flat re-exports for the most-used public surface.
pub use asset::{AssetData, AssetProvider, BytesAssetProvider};
pub use ast::{
    AssetBlock, AssetDecl, AssetKind, Dimension, Document, DocumentBody, EllipseNode, FrameNode,
    GroupNode, ImageNode, LineNode, Node, ObjectPosition, Page, Point, PolygonNode, PolylineNode,
    Project, PropertyValue, RectNode, Span, Style, StyleBlock, TextNode, TextSpan, Token,
    TokenBlock, TokenLiteral, TokenType, TokenValue, Unit, UnknownNode, UnknownProperty,
    UnknownStyleProp, UnknownValue,
};
pub use diagnostics::{Diagnostic, Severity};
pub use error::{FormatError, ParseError, ParseErrorCode};
pub use font::{BytesFontProvider, FontData, FontProvider, FontStyle, default_provider};
pub use parse::{KdlAdapter, KdlSource};
pub use tokens::{ResolvedToken, ResolvedValue, TokenResolution, resolve_tokens};
pub use validate::{ValidationReport, validate};
