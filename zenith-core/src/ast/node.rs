//! Node types for the renderable layer of a `.zen` document.

use std::collections::BTreeMap;

use super::Span;
use super::value::{Dimension, PropertyValue};
use crate::tokens::SyntaxTheme;

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
/// as the string `"true"`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownProperty {
    /// The typed representation of the KDL value.
    pub value: UnknownValue,
}

/// A text content span — a run of text with optional inline style overrides.
///
/// This is deliberately named `TextSpan` to avoid colliding with the source-
/// location type [`Span`].
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

/// An `image` node — a LEAF that draws a raster (PNG) asset into a declared
/// `[x, y, w, h]` box with a `fit` mode, ALWAYS clipped to that box
/// (normative image box-clip, doc 09 G-22).
///
/// The `asset` field references an [`AssetDecl`](super::AssetDecl) by its
/// stable id, declared in the document's `assets {}` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Required: the referenced asset id (matches an `AssetDecl.id`).
    pub asset: String,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    /// Fit mode string (`contain`/`cover`/`stretch`/`none`); validated, not
    /// enum-typed in the AST so unknown values survive for forward-compat.
    pub fit: Option<String>,
    /// Horizontal object-position anchor (string anchor or `(pct)N`).
    pub object_position_x: Option<ObjectPosition>,
    /// Vertical object-position anchor (string anchor or `(pct)N`).
    pub object_position_y: Option<ObjectPosition>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `rect` node.
#[derive(Debug, Clone, PartialEq)]
pub struct RectNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub radius: Option<PropertyValue>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    pub stroke_alignment: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `line` node (stroke-only; defined by two endpoints x1/y1/x2/y2).
///
/// Unlike `rect` and `ellipse` there is no bounding box, no fill, no radius,
/// no rotate, and no stroke-alignment — a line is a 1-D geometry whose only
/// visual property is its centered stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct LineNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x1: Option<Dimension>,
    pub y1: Option<Dimension>,
    pub x2: Option<Dimension>,
    pub y2: Option<Dimension>,
    pub style: Option<String>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// An `ellipse` node (fill + centered stroke; bounded by x/y/w/h bounding box).
///
/// `stroke-alignment` is not supported for ellipse in v0 — stroke is always
/// centered on the ellipse path. `stroke_alignment` may be added in a later
/// schema version.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipseNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `text` node.
#[derive(Debug, Clone, PartialEq)]
pub struct TextNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub align: Option<String>,
    pub direction: Option<String>,
    pub overflow: Option<String>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub font_family: Option<PropertyValue>,
    pub font_size: Option<PropertyValue>,
    /// Numeric font weight (100–900), usually a `fontWeight` token ref.
    pub font_weight: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Inline text spans.
    pub spans: Vec<TextSpan>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `code` node — a multi-line MONOSPACE text block.
///
/// Structurally this mirrors [`TextNode`] but carries a single verbatim source
/// blob instead of styled `spans`. The blob is stored DECODED (newlines and
/// tabs are literal characters); the formatter re-encodes it with escapes.
///
/// The verbatim source is carried in the KDL as a `content` child node with one
/// escaped string argument (NOT a bare `r#"..."#` raw string): KDL v2 multi-line
/// string dedent semantics make the raw form lossy, whereas a single-line
/// escaped string round-trips `\n \t \" \\` exactly through the `kdl` crate.
/// See `transform_code` / `write_code` for the parse/format sides.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    /// "clip" (default) or "visible"; v0 does not word-wrap.
    pub overflow: Option<String>,
    /// Open string naming the source language; v0 renders plaintext regardless.
    pub language: Option<String>,
    /// Render line numbers (default false); parsed + preserved, NOT acted on in v0.
    pub line_numbers: Option<bool>,
    /// Rendered column width of a tab (default 4).
    pub tab_width: Option<u32>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub font_family: Option<PropertyValue>,
    pub font_size: Option<PropertyValue>,
    /// Optional built-in syntax-highlight color theme; `None` = use default (`Dark`).
    pub syntax_theme: Option<SyntaxTheme>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Verbatim source text (decoded; newlines/tabs are literal characters).
    pub content: String,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// An unrecognized node kind, preserved for forward-compat.
///
/// When a `.zen` document contains a node kind that this binary does not
/// recognise (e.g. authored with a newer version), the node is wrapped in this
/// variant instead of triggering a hard error.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownNode {
    /// The KDL node name (e.g. `"sparkle"`, `"table"`, `"chart"`).
    pub kind: String,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// A `frame` node — a container that CLIPS its children to its rectangular
/// bounds and renders them in source order (first child = bottom of z-order).
///
/// Unlike `group`, a frame has **required** geometry (x, y, w, h): these four
/// dimensions define the clip rectangle. Children are rendered at their
/// **absolute** page coordinates — frame does NOT translate children (dx/dy
/// are unchanged). The frame only clips; it has no fill of its own in v0.
///
/// Opacity cascades (multiplies) into all descendant node alphas, exactly as
/// in `GroupNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Required: clip-rectangle left edge in page coordinates.
    pub x: Option<Dimension>,
    /// Required: clip-rectangle top edge in page coordinates.
    pub y: Option<Dimension>,
    /// Required: clip-rectangle width.
    pub w: Option<Dimension>,
    /// Required: clip-rectangle height.
    pub h: Option<Dimension>,
    /// Layout algorithm hint ("absolute"/"flow") — parsed and preserved but
    /// NOT acted on in v0; flow layout is not implemented.
    pub layout: Option<String>,
    /// Opacity that cascades (multiplies) into all descendant node alphas.
    pub opacity: Option<f64>,
    /// When `Some(false)` the entire subtree (including the clip) is excluded
    /// from the render.
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Rotation — parsed and preserved but DEFERRED (not applied at render,
    /// consistent with the universal rotate deferral on all node types).
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `group` node — a container that holds child nodes and renders them in
/// source order (first child = bottom of z-order).
///
/// Groups introduce recursive nesting: a group can contain any mix of leaf
/// nodes and further groups.  The group itself emits no scene command; it
/// only propagates a render context (opacity cascade + translation offset)
/// to its descendants.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Advisory x-translation offset applied to the subtree (default 0).
    pub x: Option<Dimension>,
    /// Advisory y-translation offset applied to the subtree (default 0).
    pub y: Option<Dimension>,
    /// Advisory bounding width — NOT used to scale children.
    pub w: Option<Dimension>,
    /// Advisory bounding height — NOT used to scale children.
    pub h: Option<Dimension>,
    /// Opacity that cascades (multiplies) into all descendant node alphas.
    pub opacity: Option<f64>,
    /// When `Some(false)` the entire subtree is excluded from the render.
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Rotation — parsed and preserved but DEFERRED (not applied at render,
    /// consistent with the universal rotate deferral on all node types).
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single vertex in a polygon or polyline point list.
///
/// Both `x` and `y` are `Option` for consistency with line endpoint geometry
/// — validate-time checks enforce their presence.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
}

/// A `polygon` node — a CLOSED filled shape defined by an ordered list of
/// `point` child nodes.
///
/// `polygon` supports both fill and stroke (stroke is centered in v0).
/// `fill-rule` controls the winding rule for self-intersecting fills.
/// `stroke-alignment` is parsed and preserved for future use but the stroke
/// is ALWAYS rendered centered in v0.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    /// DEFERRED: stroke-alignment offset (rendered centered in v0)
    pub stroke_alignment: Option<String>,
    /// `"nonzero"` (default) or `"evenodd"`.
    pub fill_rule: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Ordered vertex list parsed from `point` child nodes.
    pub points: Vec<Point>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `polyline` node — an OPEN stroked path defined by an ordered list of
/// `point` child nodes.
///
/// `polyline` has stroke (required for visible output) and optional fill.
/// Unlike `polygon`, `polyline` does NOT support `stroke-alignment` (doc 09).
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    /// `"nonzero"` (default) or `"evenodd"`.
    pub fill_rule: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Ordered vertex list parsed from `point` child nodes.
    pub points: Vec<Point>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A renderable content node within a page.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Rect(RectNode),
    Ellipse(EllipseNode),
    Line(LineNode),
    Text(TextNode),
    Code(CodeNode),
    Frame(FrameNode),
    Group(GroupNode),
    Image(ImageNode),
    Polygon(PolygonNode),
    Polyline(PolylineNode),
    Unknown(UnknownNode),
}
