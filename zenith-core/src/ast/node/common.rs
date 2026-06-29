//! Shared node-layer types: forward-compat property storage, text spans,
//! geometry primitives, and the top-level [`Node`] enum.

use crate::ast::value::PropertyValue;
use crate::data::DataFormat;

use super::container::{FrameNode, GroupNode, TableNode};
use super::effect::LightNode;
use super::leaf::{
    ChartNode, CodeNode, EllipseNode, ImageNode, LineNode, PatternNode, PolygonNode, PolylineNode,
    RectNode, TextNode,
};
use super::special::{
    ConnectorNode, FieldNode, FootnoteNode, InstanceNode, ShapeNode, TocNode, UnknownNode,
};

/// The typed value of an unrecognized KDL property, preserved for forward-compat.
///
/// Mirrors the KDL v2 value space so that the original KDL type is never
/// discarded during a parse→format→parse round-trip.
#[derive(Debug, Clone, PartialEq)]
pub enum UnknownValue {
    String(String),
    Integer(i128),
    Float(f64),
    Bool(bool),
    Null,
}

/// A typed KDL value retained for an unrecognized property (forward-compat).
///
/// Storing the full `UnknownValue` variant keeps the AST lossless for
/// round-trip: a boolean `magic=#true` round-trips back as a boolean, not
/// as the string `"true"`. Any KDL type annotation on the value (e.g. `px`
/// from `(px)10`) is retained in `ty` so annotated values round-trip
/// byte-identically.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownProperty {
    /// The typed representation of the KDL value.
    pub value: UnknownValue,
    /// The KDL type annotation, if any (e.g. `px` from `(px)10`, `token` from
    /// `(token)"color.navy"`). Preserved so annotated values round-trip losslessly.
    pub ty: Option<String>,
}

/// A text content span — a run of text with optional inline style overrides.
///
/// This is deliberately named `TextSpan` to avoid colliding with the source-
/// location type [`Span`](crate::ast::Span).
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    /// The literal text content.
    pub text: String,
    /// Per-span fill override (usually a token ref).
    pub fill: Option<PropertyValue>,
    /// Per-span font-weight override.
    pub font_weight: Option<PropertyValue>,
    /// Italic override.
    pub italic: Option<bool>,
    /// Underline decoration.
    pub underline: Option<bool>,
    /// Strikethrough decoration.
    pub strikethrough: Option<bool>,
    /// Vertical alignment of the span relative to the run baseline. `Some("super")`
    /// raises the span (superscript); `Some("sub")` lowers it (subscript). Both
    /// typeset the span at a reduced font size. `None` (or any other value) keeps
    /// the span on the baseline at full size. See the scene `compile_text`
    /// super/subscript handling for the exact scale + baseline-shift factors.
    pub vertical_align: Option<String>,
    /// Footnote reference — the id of a page-level [`FootnoteNode`]. When
    /// `Some(id)`, the renderer emits the referenced footnote's auto-number as a
    /// SUPERSCRIPT marker run immediately AFTER this span's text (reusing the
    /// [`TextSpan::vertical_align`] `"super"` rendering: reduced size + raised
    /// baseline). An id that names no footnote on the same page yields an
    /// advisory `footnote.unresolved_ref` and no marker. KDL: `footnote-ref="fn.1"`.
    pub footnote_ref: Option<String>,
    /// Runtime data-field reference for the span's TEXT CONTENT. When `Some(path)`,
    /// the scene compiler's data pre-pass looks `path` up in the active
    /// [`DataContext`](crate::data::DataContext) and REPLACES [`TextSpan::text`]
    /// with the resolved value (styled by [`TextSpan::data_format`] when set). A
    /// missing field emits `data.missing_field` and leaves the authored `text`
    /// (the fallback). `None` keeps the literal `text` unchanged (byte-identical
    /// to a span without the attribute). KDL: `data-ref="revenue.total"`.
    pub data_ref: Option<String>,
    /// Optional display format applied to the resolved [`TextSpan::data_ref`]
    /// value (currency / percent / number, with optional precision + locale). Only
    /// meaningful when `data_ref` is `Some`. `None` substitutes the raw field
    /// value verbatim. KDL: `format="currency" precision=2`.
    pub data_format: Option<DataFormat>,
    /// Per-span highlight (text background) color; usually a token ref. `None` =
    /// no highlight (byte-identical to a span without it). When `Some`, the
    /// renderer emits a filled rect behind the span's glyphs covering the full
    /// ascent-to-descent band so the text reads like a marker-pen highlight.
    /// KDL: `span highlight=(token)"color.mark" "text"`.
    pub highlight: Option<PropertyValue>,
    /// Inline code mark: when `Some(true)`, the span is rendered in the bundled
    /// monospace family ("Noto Sans Mono") with a subtle background rect behind
    /// it (the internal `CODE_BG` default color). `None` or `Some(false)` keeps
    /// the node's own font family (byte-identical to a span without it).
    /// KDL: `span "text" code=#true`.
    pub code: Option<bool>,
    /// Hyperlink URL carried on the span. `None` = no link (byte-identical to a
    /// span without it). When `Some(url)`, the span is rendered with an underline
    /// and the internal `LINK_COLOR` default color when the span has no explicit
    /// `fill`; a span that already has a `fill` keeps its author color. In PDF
    /// output the URL becomes a clickable `/Link` annotation over the span (when
    /// the text is `selectable`). KDL: `span "text" link="https://example.com"`.
    pub link: Option<String>,
}

/// How an `image` node aligns its content within the declared box when the
/// `fit` mode leaves slack on an axis (`contain`, `cover`, `none`).
///
/// `Pct(n)` is an arbitrary 0–100 position; `Start`/`Center`/`End` are the
/// named anchors (equivalent to `Pct(0)`, `Pct(50)`, `Pct(100)`).
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectPosition {
    Start,
    Center,
    End,
    Pct(f64),
}

/// A single vertex in a polygon or polyline point list.
///
/// Both `x` and `y` are `Option` for consistency with line endpoint geometry
/// — validate-time checks enforce their presence.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: Option<crate::ast::value::Dimension>,
    pub y: Option<crate::ast::value::Dimension>,
}

/// A renderable content node within a page.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    // Boxed: `RectNode` grew large enough to trigger `large_enum_variant`.
    // Boxing keeps `Node` compact so moving it around stays cheap.
    // Mirrors the existing `Text(Box<TextNode>)` pattern.
    Rect(Box<RectNode>),
    Ellipse(EllipseNode),
    Line(LineNode),
    // Boxed: `TextNode` is by far the largest node variant (many optional
    // typography/geometry fields). Boxing keeps `Node` compact so moving it
    // around (and the `large_enum_variant` lint) stays cheap.
    Text(Box<TextNode>),
    Code(CodeNode),
    Frame(FrameNode),
    Group(GroupNode),
    Image(ImageNode),
    Polygon(PolygonNode),
    Polyline(PolylineNode),
    Instance(InstanceNode),
    Field(FieldNode),
    Footnote(FootnoteNode),
    /// A compile-time table-of-contents placeholder; resolved to a
    /// tab-leader text block by the scene compiler.
    Toc(TocNode),
    // Boxed: `TableNode` is large (many optional visual fields + nested
    // columns/rows/cells). Boxing keeps `Node` compact for the
    // `large_enum_variant` lint, mirroring `Rect`/`Text`.
    Table(Box<TableNode>),
    // Boxed: `ShapeNode` is large (box geometry + visual fields + owned label
    // spans). Boxing keeps `Node` compact for the `large_enum_variant` lint,
    // mirroring `Rect`/`Text`/`Table`.
    Shape(Box<ShapeNode>),
    // Boxed: `ConnectorNode` carries many optional attrs (from/to + anchors +
    // route + markers + visual fields). Boxing keeps `Node` compact for the
    // `large_enum_variant` lint, mirroring `Rect`/`Text`/`Table`/`Shape`.
    Connector(Box<ConnectorNode>),
    // Boxed: `UnknownNode` now carries preserved props + recursive children for
    // lossless forward-compat round-trip. Boxing keeps `Node` compact for the
    // `large_enum_variant` lint, mirroring `Rect`/`Text`/`Table`/`Shape`.
    Unknown(Box<UnknownNode>),
    // Boxed: `PatternNode` carries the full common-field spread plus a boxed
    // motif. Boxing keeps `Node` compact for the `large_enum_variant` lint,
    // mirroring `Rect`/`Text`/`Table`/`Shape`.
    Pattern(Box<PatternNode>),
    // Boxed: `ChartNode` carries the full common-field spread plus a Vec of
    // series. Boxing keeps `Node` compact for the `large_enum_variant` lint,
    // mirroring `Rect`/`Text`/`Table`/`Shape`/`Pattern`.
    Chart(Box<ChartNode>),
    // Boxed for the same forward-growth reason as the other rich authoring
    // variants: future effect nodes may gain more fields without bloating Node.
    Light(Box<LightNode>),
}
