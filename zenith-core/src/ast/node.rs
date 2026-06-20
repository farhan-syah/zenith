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
    /// Clip-to-shape mode (`"ellipse"`/`"rounded"`/`"rect"`); absent or an
    /// unrecognized value means the default rectangular box-clip. Validated as a
    /// plain string so unknown values survive for forward-compat.
    pub clip: Option<String>,
    /// Corner radius for `clip="rounded"`, as a `(token)` dimension ref. Only
    /// meaningful when `clip="rounded"`; absent → radius 0 (sharp corners).
    pub clip_radius: Option<PropertyValue>,
    /// Horizontal object-position anchor (string anchor or `(pct)N`).
    pub object_position_x: Option<ObjectPosition>,
    /// Vertical object-position anchor (string anchor or `(pct)N`).
    pub object_position_y: Option<ObjectPosition>,
    pub opacity: Option<f64>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
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
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
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
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
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
    /// Overflow-wrap mode. `Some("break-word")` lets the line packer break an
    /// unbreakable token (a long URL/compound with no space or hyphen point) that
    /// is wider than the line box at a CHARACTER boundary, so it no longer
    /// overflows; a forced break emits an advisory `text.forced_break`. `None` or
    /// `"normal"` keeps the default (the overlong token overflows/clips,
    /// byte-identical to a node without the attribute). KDL:
    /// `overflow-wrap="break-word"`.
    pub overflow_wrap: Option<String>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    /// WCAG contrast hint: an explicit background color (token ref) the text
    /// visually sits ON, for nodes placed over an `image` or other non-fillable
    /// backdrop the validator cannot sample. When set, the contrast check uses
    /// THIS color as the background (highest priority, over any detected backdrop
    /// and the page background). Token-only, like `fill`. `None` → unchanged
    /// backdrop detection. KDL: `contrast-bg=(token)"color.photo.shadow"`.
    pub contrast_bg: Option<PropertyValue>,
    pub font_family: Option<PropertyValue>,
    pub font_size: Option<PropertyValue>,
    /// Floor font size for `overflow="autofit"` — the node's font shrinks no
    /// smaller than this when fitting. Token-only, like `font-size`. `None` → a
    /// default floor (`(declared * 0.5).max(8.0)`). KDL:
    /// `font-size-min=(token)"size.min"`.
    pub font_size_min: Option<PropertyValue>,
    /// Numeric font weight (100–900), usually a `fontWeight` token ref.
    pub font_weight: Option<PropertyValue>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Threaded-text-flow chain id. When `Some(id)`, this text node is a member
    /// of the chain named `id`; all text nodes sharing the same `chain` id form
    /// an ordered chain (ordering = document source order). A long article
    /// placed in the FIRST member's spans flows across every member's box in
    /// order: each box consumes as much text as fits, the remainder continues in
    /// the next member. Continuation members carry `chain=id` with empty spans.
    ///
    /// v0 semantics (documented):
    /// - Content source: the first member (source order) that has non-empty
    ///   spans is the sole content source; later members' spans are ignored
    ///   (no concatenation).
    /// - Shared style: all members are assumed to share font family/size/weight/
    ///   fill; the whole chain is shaped with the FIRST member's resolved style.
    ///   Each box re-wraps to its OWN width, so line height stays uniform.
    pub chain: Option<String>,
    /// Drop-cap initial: the FIRST grapheme of the paragraph is typeset large,
    /// spanning `Some(n)` body lines at the top-left, with the first `n` body
    /// lines wrapping to a narrower measure beside it and line `n+1` onward
    /// returning to the full box width. `Some(0)` or `None` disables the drop
    /// cap (rendered byte-identically to a node without the attribute). Honored
    /// only on the single-box wrap path (a box with a width whose text overflows
    /// it); chain/flow integration is a documented v0 follow-up. KDL:
    /// `drop-cap-lines=3`.
    pub drop_cap_lines: Option<u32>,
    /// Knuth–Liang hyphenation toggle. When `Some(true)`, the greedy line packer
    /// may break a word that does not fit the remaining space on a non-empty line
    /// at an embedded (en-US) hyphenation point, placing `fragment-` on the
    /// current line and carrying the remainder to the next. `None`/`Some(false)`
    /// disables hyphenation (byte-identical to a node without the attribute).
    /// KDL: `hyphenate=#true`.
    pub hyphenate: Option<bool>,
    /// Widow/orphan control: keep at least `Some(n)` lines of a paragraph
    /// together across a chain box/page break. `n=2` prevents a lone first line
    /// (orphan) from being stranded at a box bottom and a lone last line (widow)
    /// from starting the next box. Applied only at the CHAIN distribution
    /// boundary, read from the chain source node. `None` disables the control
    /// (byte-identical to a node without the attribute). KDL: `widow-orphan=2`.
    pub widow_orphan: Option<u32>,
    /// Tab-stop leader character. When `Some(s)` with a non-empty `s`, the node
    /// renders in TAB-LEADER mode (table-of-contents rows): the combined span
    /// text is split into rows on `\n`, each row is split on its FIRST `\t` into
    /// a LEFT and RIGHT segment, the LEFT segment is placed at the box left edge,
    /// the RIGHT segment is right-aligned to the box right edge, and the gap
    /// between them is filled with the leader glyph `s` (e.g. `"."`) repeated.
    /// A row with no tab renders left-aligned with no leader. `None` or an empty
    /// string disables tab-leader mode (byte-identical to a node without the
    /// attribute). KDL: `tab-leader="."`.
    pub tab_leader: Option<String>,
    /// Text-runaround exclusion: the id of ANOTHER node on the same page whose
    /// bounding box becomes an exclusion zone this text wraps around. For each
    /// wrapped line whose vertical band intersects the excluded rect, the line
    /// flows into the LARGER free horizontal segment (left or right of the rect);
    /// a line with no segment wide enough is left blank so text flows above and
    /// below a full-width exclusion ("largest-area / jump" wrap). An id naming no
    /// resolvable node yields an advisory `text-exclusion.unresolved_ref` and the
    /// text renders with no exclusion (byte-identical to a node without the
    /// attribute). Honored on the single-box wrap path; chain-member runaround is a
    /// documented v0 follow-up. KDL: `text-exclusion="author.portrait"`.
    pub text_exclusion: Option<String>,
    /// Left padding in pixels applied to EVERY wrapped line (indents the text-box
    /// left edge inward, reducing the measure). Combine with a negative
    /// [`TextNode::text_indent`] for a hanging indent (bulleted lists). `None` → 0.
    /// KDL: `padding-left=(px)44`.
    pub padding_left: Option<Dimension>,
    /// First-line horizontal offset in pixels RELATIVE to the padded left edge.
    /// May be NEGATIVE to pull the first line back out (a hanging bullet glyph sits
    /// left of the wrapped continuation lines). Applies to line 0 of the box only
    /// (per-paragraph first-line indent is a documented v0 follow-up). `None` → 0.
    /// KDL: `text-indent=(px)-44`.
    pub text_indent: Option<Dimension>,
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
    /// Numeric font weight (100–900), usually a `fontWeight` token ref.
    pub font_weight: Option<PropertyValue>,
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
    /// Layout algorithm hint ("absolute"/"flow"/"grid"). `"flow"` activates a
    /// vertical-stack flow layout (uniform `padding` inset + `gap` between
    /// children, resolved from the frame's style); `"grid"` tiles children
    /// row-major into a `columns × rows` grid inside the padded content box with
    /// uniform `gap` gutters; any other value (including `None` and `"absolute"`)
    /// keeps the clip-only absolute-positioning model.
    pub layout: Option<String>,
    /// Grid column count for `layout="grid"` (ignored otherwise). When the frame
    /// uses grid layout, children tile row-major into `columns` columns; absent →
    /// treated as 1 column. KDL: `columns=2`.
    pub columns: Option<u32>,
    /// Grid row count for `layout="grid"` (ignored otherwise). Absent → derived as
    /// `ceil(child_count / columns)` so the grid grows to fit its children. KDL:
    /// `rows=3`.
    pub rows: Option<u32>,
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

/// An instance-local override applied to a single descendant of the referenced
/// component when an [`InstanceNode`] is expanded at compile time.
///
/// An `override` is an `override ref="<local-descendant-id>" { … }` child of an
/// instance. `ref_id` names a descendant by its component-LOCAL id (the id as
/// declared inside the [`ComponentDef`], before instance-id prefixing).
///
/// v0 supported override set (documented; richer overrides are a follow-up):
/// - `spans` — replaces the target text node's `spans` wholesale (the override's
///   `span` children become the target's new spans).
/// - `fill` — replaces the target node's `fill` visual property.
/// - `visible` — replaces the target node's `visible` flag.
///
/// Each field is `None` when the override does not touch that aspect; a `None`
/// field leaves the corresponding property on the cloned target untouched.
#[derive(Debug, Clone, PartialEq)]
pub struct Override {
    /// The component-LOCAL id of the descendant this override targets.
    pub ref_id: String,
    /// Replacement text spans (only meaningful for a text target).
    pub spans: Option<Vec<TextSpan>>,
    /// Replacement fill (color token ref or literal — validated like any fill).
    pub fill: Option<PropertyValue>,
    /// Replacement visibility flag.
    pub visible: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// An `instance` node — a placement of a declared [`ComponentDef`] at an origin
/// `(x, y)`, with an optional opacity/visible cascade and instance-local
/// overrides.
///
/// At compile time the instance expands to the component's child subtree treated
/// as a GROUP translated by `(x, y)`, cascading `opacity`/`visible` exactly like
/// a [`GroupNode`]. Every expanded descendant id is PREFIXED with the instance id
/// (`<instance-id>/<local-id>`) so multiple instances of the same component never
/// collide. The instance node itself emits no scene command; its expanded subtree
/// does. Expansion happens at COMPILE time only — the instance stays a single node
/// in the canonical AST so parse→format→parse round-trips.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// The referenced [`ComponentDef`] id.
    pub component: String,
    /// Instance origin x-translation applied to the expanded subtree (default 0).
    pub x: Option<Dimension>,
    /// Instance origin y-translation applied to the expanded subtree (default 0).
    pub y: Option<Dimension>,
    /// Opacity that cascades (multiplies) into all expanded descendant alphas.
    pub opacity: Option<f64>,
    /// When `Some(false)` the entire expanded subtree is excluded from the render.
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Instance-local overrides applied to component descendants on expansion.
    pub overrides: Vec<Override>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `field` node — an auto-resolved text placeholder for book interiors.
///
/// A field is a LEAF that, at compile time, resolves to a single-line text run
/// against the page it is projected onto. It is the building block of the
/// master-page / running-head / folio system: a master declares a field once
/// (e.g. a running head or a page-number) and every page that uses the master
/// gets the field resolved against that page's index and parity.
///
/// Field types (v0):
/// - `"running-head"` → renders [`FieldNode::recto`] on odd (recto) pages and
///   [`FieldNode::verso`] on even (verso) pages; an absent side renders nothing.
/// - `"page-number"` → renders the page's folio (its 1-based index in
///   `doc.body.pages`) as a decimal string.
/// - `"page-ref"` → renders the 1-based page index of the page that CONTAINS the
///   node whose id equals [`FieldNode::target`] (document-wide search). A missing
///   target produces an advisory `field.unresolved_ref` and renders nothing.
///
/// Geometry: when `x`/`w` are omitted the field defaults to the page's live
/// area (so a running head auto-mirrors recto/verso x via the page margins).
/// `y`/`h` default to the live area's top/height when omitted. The resolved run
/// is shaped like a single-line text node: `running-head` / `page-number`
/// default to `align="center"`, `page-ref` to `align="start"`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// The field kind string (`"running-head"`/`"page-number"`/`"page-ref"`).
    /// Validated, not enum-typed, so unknown values survive for forward-compat.
    pub field_type: String,
    /// Recto-side text for a `running-head` field (odd, 1-based pages).
    pub recto: Option<String>,
    /// Verso-side text for a `running-head` field (even pages).
    pub verso: Option<String>,
    /// Target node id for a `page-ref` field.
    pub target: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub font_family: Option<PropertyValue>,
    pub font_size: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `footnote` node — page-level book-interior furniture that auto-numbers and
/// renders in a reserved zone at the bottom of the page.
///
/// A footnote is NOT positioned by the author: it has NO `x`/`y`/`w`/`h`. At
/// compile time every `footnote` that is a DIRECT child of a [`Page`] is
/// collected in source order, auto-numbered `1..N` (a footnote that declares an
/// explicit [`marker`](FootnoteNode::marker) uses that string instead of a
/// number but still occupies a slot), and rendered stacked above the page's
/// bottom margin with a separator rule. A [`TextSpan`] that carries a matching
/// [`footnote_ref`](TextSpan::footnote_ref) gets the footnote's marker emitted
/// inline as a superscript after its text.
///
/// KDL: `footnote id="fn.1" { span "See also Chapter 4." }`. The content is a
/// list of [`TextSpan`]s (the same span model as a `text` node), so it inherits
/// the text shaping/wrap path verbatim.
#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Explicit marker override. When `Some(s)`, the footnote renders `s` as its
    /// marker (both inline and in the zone) instead of its auto-number; the
    /// footnote still occupies a numbering slot. `None` → use the auto-number.
    pub marker: Option<String>,
    /// The footnote's content spans (same model as a `text` node's spans).
    pub spans: Vec<TextSpan>,
    pub style: Option<String>,
    /// Fill for the footnote content + the separator rule. `None` → a sensible
    /// muted default for the rule and opaque black for the text.
    pub fill: Option<PropertyValue>,
    pub font_family: Option<PropertyValue>,
    pub font_size: Option<PropertyValue>,
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
    Unknown(UnknownNode),
}
