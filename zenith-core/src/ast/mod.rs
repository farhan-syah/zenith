//! AST type re-exports for zenith-core.

pub mod action;
pub mod asset;
pub mod document;
pub mod library;
pub mod node;
pub mod provenance;
pub mod recipe;
pub mod span;
pub mod style;
pub mod token;
pub mod value;
pub mod variant;

// Flat re-exports used throughout the crate.
pub use action::ActionDef;
pub use asset::{AssetBlock, AssetDecl, AssetKind};
pub use document::{
    ComponentDef, Document, DocumentBody, Fold, MasterDef, Page, Project, SafeZone, SafeZoneType,
    SectionDef,
};
pub use library::LibraryDef;
pub use node::{
    Anchor, AnchorEdge, CodeNode, ConnectorNode, EllipseNode, FieldNode, FootnoteNode, FrameNode,
    GroupNode, ImageNode, InstanceNode, LineNode, Node, ObjectPosition, Override, PatternNode,
    Point, PolygonNode, PolylineNode, ProtectedRegion, RectNode, ShapeNode, TableCell, TableColumn,
    TableNode, TableRow, TextNode, TextSpan, TocNode, UnknownNode, UnknownProperty, UnknownValue,
    anchor_xy, parse_anchor, parse_anchor_edge,
};
pub use provenance::ProvenanceDef;
pub use recipe::{RecipeDef, RecipeParam};
pub use span::Span;
pub use style::{
    STYLE_RECOGNIZED_KEYS, Style, StyleBlock, UnknownStyleProp, canonicalize_style_key,
};
pub use token::{
    FilterKind, FilterLiteral, FilterOp, GradientKind, GradientLiteral, GradientStopRef,
    MaskLiteral, MaskShape, ShadowLayerRef, ShadowLiteral, Token, TokenBlock, TokenLiteral,
    TokenType, TokenValue,
};
pub use value::{Dimension, PropertyValue, Unit, dim_to_px};
pub use variant::{VariantDef, VariantOverride};
