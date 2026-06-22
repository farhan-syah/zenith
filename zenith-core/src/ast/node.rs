//! Node types for the renderable layer of a `.zen` document.

use std::collections::BTreeMap;

use super::Span;
use super::value::{Dimension, PropertyValue};
use crate::tokens::SyntaxTheme;

// ── Page-relative 9-point anchor ─────────────────────────────────────────────

/// One of the nine named page-relative placement anchors.
///
/// An anchor supplies BOTH the x and y of a node from the page dimensions;
/// an explicitly-authored `x` or `y` on the node overrides the corresponding
/// anchor-derived coordinate. The node's `w` and `h` must be present and in a
/// px-convertible unit for derivation to succeed; when they are absent or
/// use a non-px unit the anchor is silently skipped (the node remains incomplete
/// and the compile step emits the usual `scene.missing_geometry` advisory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Parse a string into an [`Anchor`] variant.
///
/// Returns `Some` for the nine recognized names; `None` for any other string.
/// This is the SINGLE source of truth for the anchor name list — both the
/// validator (`anchor.unknown_value`) and the scene pre-pass use this function
/// so the names cannot diverge.
pub fn parse_anchor(s: &str) -> Option<Anchor> {
    match s {
        "top-left" => Some(Anchor::TopLeft),
        "top-center" => Some(Anchor::TopCenter),
        "top-right" => Some(Anchor::TopRight),
        "center-left" => Some(Anchor::CenterLeft),
        "center" => Some(Anchor::Center),
        "center-right" => Some(Anchor::CenterRight),
        "bottom-left" => Some(Anchor::BottomLeft),
        "bottom-center" => Some(Anchor::BottomCenter),
        "bottom-right" => Some(Anchor::BottomRight),
        _ => None,
    }
}

/// Derive the `(x, y)` for the given anchor given the page and node dimensions.
///
/// `page_w`/`page_h` and `node_w`/`node_h` are all in pixels.
pub fn anchor_xy(anchor: Anchor, page_w: f64, page_h: f64, node_w: f64, node_h: f64) -> (f64, f64) {
    match anchor {
        Anchor::TopLeft => (0.0, 0.0),
        Anchor::TopCenter => ((page_w - node_w) / 2.0, 0.0),
        Anchor::TopRight => (page_w - node_w, 0.0),
        Anchor::CenterLeft => (0.0, (page_h - node_h) / 2.0),
        Anchor::Center => ((page_w - node_w) / 2.0, (page_h - node_h) / 2.0),
        Anchor::CenterRight => (page_w - node_w, (page_h - node_h) / 2.0),
        Anchor::BottomLeft => (0.0, page_h - node_h),
        Anchor::BottomCenter => ((page_w - node_w) / 2.0, page_h - node_h),
        Anchor::BottomRight => (page_w - node_w, page_h - node_h),
    }
}

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
    /// Optional source-sub-rectangle: left edge within the source image (pixels).
    /// All four src-* fields must be present together; partial presence is a hard
    /// error (`image.partial_src_rect`). Absent ⇒ the full source image is used.
    pub src_x: Option<Dimension>,
    /// Source-sub-rectangle: top edge within the source image (pixels).
    pub src_y: Option<Dimension>,
    /// Source-sub-rectangle: width within the source image (pixels, must be > 0).
    pub src_w: Option<Dimension>,
    /// Source-sub-rectangle: height within the source image (pixels, must be > 0).
    pub src_h: Option<Dimension>,
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
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    pub style: Option<String>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Per-corner radius overrides (top-left, top-right, bottom-right, bottom-left).
    /// When `Some`, the value overrides the uniform `radius` for that corner only.
    /// When `None`, the uniform `radius` applies. All four are `None` for existing docs.
    pub radius_tl: Option<PropertyValue>,
    pub radius_tr: Option<PropertyValue>,
    pub radius_br: Option<PropertyValue>,
    pub radius_bl: Option<PropertyValue>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    pub stroke_alignment: Option<String>,
    /// Dash segment length in pixels; `None` = solid stroke.
    pub stroke_dash: Option<PropertyValue>,
    /// Gap length in pixels between dashes; defaults to `stroke_dash` when absent.
    pub stroke_gap: Option<PropertyValue>,
    /// Dash end-cap style: `"butt"` (default), `"round"`, or `"square"`.
    pub stroke_linecap: Option<String>,
    /// Per-side border color for the top edge. Token-required (color token).
    /// When `Some`, a `StrokeLine` is emitted along the top edge of the rect.
    pub border_top: Option<PropertyValue>,
    /// Per-side border color for the bottom edge. Token-required (color token).
    pub border_bottom: Option<PropertyValue>,
    /// Per-side border color for the left edge. Token-required (color token).
    pub border_left: Option<PropertyValue>,
    /// Per-side border color for the right edge. Token-required (color token).
    pub border_right: Option<PropertyValue>,
    /// Shared border width for per-side borders. Token-required (dimension).
    /// Falls back to `stroke_width`, then to 1px when absent.
    pub border_width: Option<PropertyValue>,
    /// Outer stroke color: a SECOND stroke painted OUTSIDE the rect geometry.
    /// Token-required (color token). When `Some`, a `StrokeRect` /
    /// `StrokeRoundedRect` is emitted at outset geometry in addition to the
    /// primary stroke. `None` → no outer stroke (byte-identical).
    pub stroke_outer: Option<PropertyValue>,
    /// Outer stroke width for `stroke_outer`. Token-required (dimension).
    /// Defaults to 1px when absent. Only effective when `stroke_outer` is set.
    pub stroke_outer_width: Option<PropertyValue>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends (`multiply`, `screen`, `overlay`, …). `None`/`"normal"` render
    /// source-over (byte-identical to having no blend).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Dash segment length in pixels; `None` = solid stroke.
    pub stroke_dash: Option<PropertyValue>,
    /// Gap length in pixels between dashes; defaults to `stroke_dash` when absent.
    pub stroke_gap: Option<PropertyValue>,
    /// Dash end-cap style: `"butt"` (default), `"round"`, or `"square"`.
    pub stroke_linecap: Option<String>,
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
    /// Explicit x-radius override (half-width of the ellipse). When absent, the
    /// ellipse is inscribed in the bounding box (w/2). Backward-compatible: None
    /// leaves all existing ellipses byte-identical.
    pub rx: Option<PropertyValue>,
    /// Explicit y-radius override (half-height of the ellipse). When absent, the
    /// ellipse is inscribed in the bounding box (h/2). Backward-compatible: None
    /// leaves all existing ellipses byte-identical.
    pub ry: Option<PropertyValue>,
    pub style: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    /// Dash segment length in pixels; `None` = solid stroke.
    pub stroke_dash: Option<PropertyValue>,
    /// Gap length in pixels between dashes; defaults to `stroke_dash` when absent.
    pub stroke_gap: Option<PropertyValue>,
    /// Dash end-cap style: `"butt"` (default), `"round"`, or `"square"`.
    pub stroke_linecap: Option<String>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Glyph outline (stroke) color. Token-required (like `fill`). When `Some`,
    /// each glyph path is filled then stroked with this color. `None` → no
    /// outline; byte-identical to a node without `stroke`. KDL:
    /// `stroke=(token)"color.ink.outline"`.
    pub stroke: Option<PropertyValue>,
    /// Glyph outline width in pixels. Token-required (like `font-size`). Only
    /// effective when `stroke` is also set. `None` / 0 → no outline.
    /// KDL: `stroke-width=(token)"size.stroke.hairline"`.
    pub stroke_width: Option<PropertyValue>,
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
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
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
    /// Auto-aligning list bullet. When `Some(marker)` (a non-empty string like "•",
    /// "–", "1."), the node renders as a hanging-indent list item: the marker is
    /// drawn once in the left margin at the first line's baseline, and ALL text
    /// lines (first and wrapped) are indented to a column at `marker_advance + gap`
    /// from the box left edge, so continuation lines auto-align with the text after
    /// the marker — measured from the marker shaped at the node's own font, hence
    /// font/size-independent. The span text holds only the content (no bullet glyph).
    /// `None` → not a list item (byte-identical to a node without the attribute).
    /// Honored on the plain single-box wrap path; drop-cap/runaround/chain are a
    /// documented v0 follow-up. KDL: `bullet="•"`.
    pub bullet: Option<String>,
    /// Gap between the bullet marker and the text column, in pixels. `None` → a
    /// default proportional to the font size (`0.4 × font_size`). KDL:
    /// `bullet-gap=(px)16`.
    pub bullet_gap: Option<Dimension>,
    /// Inline text spans.
    pub spans: Vec<TextSpan>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `shape` node — a COMPOUND node: a background box that OWNS a centered text
/// label (like a flowchart process box).
///
/// Structurally this mirrors [`TextNode`]: it carries box geometry + visual
/// properties AND a list of owned label [`TextSpan`]s (NOT child `Node`s). The
/// background primitive emitted depends on [`ShapeNode::kind`]
/// (`process`/`decision`/`terminator`/`ellipse`, default `process`). The owned
/// label text is centered inside the box (label rendering is a later unit).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    /// Shape kind string (`process`/`decision`/`terminator`/`ellipse`).
    /// Validated, not enum-typed, so unknown values survive for forward-compat.
    /// Absent or unrecognized is treated as `"process"` at compile time.
    pub kind: Option<String>,
    pub fill: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    /// Corner radius for the `process` rounded-rect (token-required dimension).
    pub radius: Option<PropertyValue>,
    /// Stroke alignment (`inside`/`center`/`outside`), same model as `rect`.
    pub stroke_alignment: Option<String>,
    /// Text inset inside the box (token-required dimension). Carried; applied to
    /// the owned label in a later unit.
    pub padding: Option<PropertyValue>,
    /// Horizontal label alignment in the box (`start`/`center`/`end`). Carried;
    /// applied to the owned label in a later unit.
    pub h_align: Option<String>,
    /// Vertical label alignment in the box (`top`/`middle`/`bottom`). Carried;
    /// applied to the owned label in a later unit.
    pub v_align: Option<String>,
    /// Style ref for the owned label text. Carried; applied in a later unit.
    pub text_style: Option<String>,
    /// The owned label spans (same model as a `text` node's spans). Carried +
    /// parsed/formatted/validated now; rendered in a later unit.
    pub spans: Vec<TextSpan>,
    /// Box style ref.
    pub style: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `connector` node — a semantic arrow that declares `from`/`to` target node
/// ids and, at COMPILE time, resolves those targets' bounding boxes to draw a
/// straight line between anchor points on their edges.
///
/// A connector has NO authored geometry (`x`/`y`/`w`/`h`): its endpoints are
/// DERIVED from the resolved boxes of `from` and `to`, so when a target moves
/// the connector reroutes automatically (the boxes are recomputed each compile).
/// It is a stroke-only LEAF: it has a `stroke`/`stroke_width` (no `fill`), no
/// text/spans, and no children.
///
/// Unit 1 renders a STRAIGHT line between the two resolved anchors with NO
/// arrowhead markers (Unit 2) and NO orthogonal routing (Unit 3); the `route`
/// and `marker_*` attributes are stored + validated now but render straight /
/// headless until those units land.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectorNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// The source target node id (the box the arrow starts from).
    pub from: Option<String>,
    /// The destination target node id (the box the arrow points to).
    pub to: Option<String>,
    /// Source-edge anchor (`top`/`bottom`/`left`/`right`/`center`/`auto`).
    /// Absent or unrecognized is treated as `"auto"` at compile time.
    pub from_anchor: Option<String>,
    /// Destination-edge anchor (`top`/`bottom`/`left`/`right`/`center`/`auto`).
    pub to_anchor: Option<String>,
    /// Routing mode (`straight`(default)/`orthogonal`). Orthogonal is Unit 3:
    /// stored + validated now, rendered as a straight line until then.
    pub route: Option<String>,
    /// Start-cap marker (`none`(default)/`arrow`). Markers are Unit 2: stored +
    /// validated now, rendered headless until then.
    pub marker_start: Option<String>,
    /// End-cap marker (`none`(default)/`arrow`). Markers are Unit 2.
    pub marker_end: Option<String>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
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

/// An unrecognized node kind, preserved for forward-compat.
///
/// When a `.zen` document contains a node kind that this binary does not
/// recognise (e.g. authored with a newer version), the node is wrapped in this
/// variant instead of triggering a hard error.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownNode {
    /// The KDL node name (e.g. `"sparkle"`, `"table"`, `"chart"`).
    pub kind: String,
    /// The node's `id` attribute, if present. Captured first-class so unknown
    /// nodes are addressable and participate in duplicate-id detection.
    pub id: Option<String>,
    /// All other attributes, preserved with typed values + annotations.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
    /// Child nodes (may be known OR unknown), preserved for lossless round-trip.
    pub children: Vec<Node>,
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
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub style: Option<String>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub style: Option<String>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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
    /// Folio numbering style for numeric fields (`page-number`, `page-count`,
    /// `page-ref`): `"decimal"` (default), `"lower-roman"`, or `"upper-roman"`.
    /// Ignored by `running-head`. Unknown values fall back to decimal.
    pub folio_style: Option<String>,
    /// When `true`, a numeric field renders nothing on document page 1 (the
    /// title page). Used to suppress the folio on the first page.
    pub suppress_first: Option<bool>,
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
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
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

/// A `toc` node — a compile-time table-of-contents placeholder.
///
/// A `toc` is a LEAF that, at compile time, resolves to a multi-line
/// tab-leader text block by collecting all heading nodes across the whole
/// document that match its selector (`match-role` and/or `match-style`).
/// Each row in the output is formatted as:
/// `{heading text}\t{page number}`, joined by newlines.
///
/// The synthesised [`TextNode`] uses `tab-leader` mode so the text engine
/// fills the gap between heading text and page number with the leader glyph
/// (default `"."`), and right-aligns the page number.
///
/// At least one of `match_role` or `match_style` must be set; when both are
/// absent the toc collects nothing and an advisory `toc.no_selector` is
/// emitted by the validator.
#[derive(Debug, Clone, PartialEq)]
pub struct TocNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Select heading nodes whose `role` equals this. `None` = no role filter.
    pub match_role: Option<String>,
    /// Select heading nodes whose `style` equals this. `None` = no style filter.
    pub match_style: Option<String>,
    /// Leader glyph for the dotted fill between title and page number
    /// (default `"."` when omitted).
    pub leader: Option<String>,
    /// Folio numbering style for the page numbers
    /// (`"decimal"` / `"lower-roman"` / `"upper-roman"`).
    pub folio_style: Option<String>,
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
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single column declaration in a [`TableNode`] (a `column` child).
///
/// `width` absent means an AUTO column: in this unit auto columns share the
/// leftover table width equally (content-based auto-sizing is a later unit).
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    /// Explicit column width; `None` = auto (equal share of leftover width).
    pub width: Option<Dimension>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single cell in a [`TableRow`] (a `cell` child).
///
/// A cell holds ordinary child nodes (text/rect/image/…) — the same node model
/// used by `frame`/`group` children — and may span multiple columns/rows via
/// `colspan`/`rowspan` (HTML-table cell flow).
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    /// Number of columns this cell spans (default 1).
    pub colspan: u32,
    /// Number of rows this cell spans (default 1).
    pub rowspan: u32,
    /// Cell content — ordinary nodes in source order.
    pub children: Vec<Node>,
    /// Per-cell background fill override (token-required color).
    pub fill: Option<PropertyValue>,
    /// Per-cell border color override (token-required color).
    pub border: Option<PropertyValue>,
    /// Per-cell border width override (token/dimension).
    pub border_width: Option<PropertyValue>,
    /// Per-cell horizontal alignment override (`start`/`center`/`end`).
    pub h_align: Option<String>,
    /// Per-cell vertical alignment override (`top`/`middle`/`bottom`).
    pub v_align: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single row in a [`TableNode`] (a `row` child), holding cells left→right.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    /// Cells in source order (left→right).
    pub cells: Vec<TableCell>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `table` node — a grid container of `column`/`row`/`cell` children.
///
/// This unit renders single-page tables with EXPLICIT/PROPORTIONAL column
/// widths and SEPARATE borders only. Content-based auto-sizing, border-collapse
/// mode, header-row styling, and multi-page flow are LATER units; their AST
/// fields are carried here so the schema stays stable.
#[derive(Debug, Clone, PartialEq)]
pub struct TableNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Required: table box left edge in page coordinates.
    pub x: Option<Dimension>,
    /// Required: table box top edge in page coordinates.
    pub y: Option<Dimension>,
    /// Required: table box width.
    pub w: Option<Dimension>,
    /// Required: table box height.
    pub h: Option<Dimension>,
    /// Column declarations, order = left→right.
    pub columns: Vec<TableColumn>,
    /// Row declarations, order = top→bottom.
    pub rows: Vec<TableRow>,
    /// First N rows are headers (styling + repeat in later units).
    pub header_rows: Option<u32>,
    /// Multi-page flow id. Tables sharing a `flows` id form ONE logical table:
    /// the FIRST member (page-order, then source-order) is the SOURCE carrying
    /// all rows + columns; continuation members declare the same id with empty
    /// rows and receive the body-row slice that fits their box, with header rows
    /// repeated. Mirrors the text-node `chain` field. `None` = standalone table.
    pub flows: Option<String>,
    /// Uniform gutter between cells in px (token or literal).
    pub gap: Option<PropertyValue>,
    /// Inset inside each cell in px (token or literal).
    pub cell_padding: Option<PropertyValue>,
    /// Border model: `"separate"` (default) or `"collapse"`. Only `"separate"`
    /// is rendered in this unit.
    pub border_collapse: Option<String>,
    /// Default cell background (token-required color).
    pub fill: Option<PropertyValue>,
    /// Default cell border color (token-required color).
    pub border: Option<PropertyValue>,
    /// Default border width (token/dimension).
    pub border_width: Option<PropertyValue>,
    /// Header-row background override (carried; applied in a later unit).
    pub header_fill: Option<PropertyValue>,
    /// Header text style ref (carried; applied in a later unit).
    pub header_style: Option<String>,
    /// Default horizontal alignment (`start`(default)/`center`/`end`).
    pub h_align: Option<String>,
    /// Default vertical alignment (`top`(default)/`middle`/`bottom`).
    pub v_align: Option<String>,
    pub style: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Rotation — parsed and preserved but DEFERRED at render, consistent with
    /// the universal rotate deferral on all node types.
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
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
}
