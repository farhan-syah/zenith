//! Foundation crate for Zenith.
//!
//! Owns the KDL-v2 parser adapter, semantic AST types, canonical formatter,
//! token types and resolution, validation engine with the full diagnostic set,
//! AST-based migrations, and deterministic font and asset resolution.
//! No other Zenith crate is a dependency.

pub mod asset;
pub mod ast;
pub mod color;
pub mod diagnostics;
pub mod error;
pub mod font;
pub mod format;
pub mod parse;
pub mod theme;
pub mod tokens;
pub mod util;
pub mod validate;

// Curated flat re-exports for the most-used public surface.
pub use asset::{AssetData, AssetProvider, BytesAssetProvider};
pub use ast::{
    ActionDef, Anchor, AssetBlock, AssetDecl, AssetKind, CodeNode, ComponentDef, ConnectorNode,
    Dimension, Document, DocumentBody, EllipseNode, FieldNode, FilterKind, FilterLiteral, FilterOp,
    FootnoteNode, FrameNode, GradientKind, GradientLiteral, GradientStopRef, GroupNode, ImageNode,
    InstanceNode, LibraryDef, LineNode, MaskLiteral, MaskShape, MasterDef, Node, ObjectPosition,
    Override, Page, PatternNode, Point, PolygonNode, PolylineNode, Project, PropertyValue,
    ProvenanceDef, RecipeDef, RecipeParam, RectNode, STYLE_RECOGNIZED_KEYS, SafeZone, SafeZoneType,
    SectionDef, ShadowLayerRef, ShadowLiteral, ShapeNode, Span, Style, StyleBlock, TableCell,
    TableColumn, TableNode, TableRow, TextNode, TextSpan, TocNode, Token, TokenBlock, TokenLiteral,
    TokenType, TokenValue, Unit, UnknownNode, UnknownProperty, UnknownStyleProp, UnknownValue,
    VariantDef, VariantOverride, anchor_xy, canonicalize_style_key, dim_to_px, parse_anchor,
};
pub use color::{
    Cmyk, cmyk_to_hex, cmyk_to_srgb, contrast_ratio, parse_cmyk, parse_rgb, relative_luminance,
};
pub use diagnostics::{Diagnostic, Severity};
pub use error::{FormatError, ParseError, ParseErrorCode};
pub use font::{BytesFontProvider, FontData, FontProvider, FontStyle, default_provider};
pub use parse::{KdlAdapter, KdlSource};
pub use tokens::{
    HighlightToken, ResolvedFilter, ResolvedFilterOp, ResolvedGradient, ResolvedMask,
    ResolvedShadow, ResolvedShadowLayer, ResolvedToken, ResolvedValue, SyntaxTheme, TokenKind,
    TokenResolution, builtin_color, is_supported, resolve_tokens, scan, token_id_for_kind,
};
pub use util::hash_unit;
pub use util::pattern::{PatternLayout, pattern_positions};
pub use validate::{ValidationReport, validate};
