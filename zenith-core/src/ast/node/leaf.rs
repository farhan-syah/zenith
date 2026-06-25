//! Leaf node structs: shapes and text-bearing primitives that have no child
//! `Node`s of their own (rect, line, ellipse, image, text, code, polygon,
//! polyline, pattern, chart).

use std::collections::BTreeMap;

use crate::ast::Span;
use crate::ast::value::{Dimension, PropertyValue};
use crate::tokens::SyntaxTheme;

use super::common::{Node, ObjectPosition, Point, TextSpan, UnknownProperty};

/// An `image` node — a LEAF that draws a raster (PNG) asset into a declared
/// `[x, y, w, h]` box with a `fit` mode, ALWAYS clipped to that box
/// (normative image box-clip).
///
/// The `asset` field references an [`AssetDecl`](crate::ast::AssetDecl) by its
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
    /// Optional safe-zone id selecting the reference rectangle for `anchor`
    /// (page-relative when absent). See [`Anchor`](super::Anchor).
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`].
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
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
    /// Optional safe-zone reference for the anchor. When `Some(id)` and a
    /// safe-zone with that id is declared on the page, the `anchor` is resolved
    /// relative to that zone's rectangle instead of the full page. Requires
    /// `anchor` to be set; `anchor_zone` without `anchor` has no effect and
    /// triggers an `anchor.zone_without_anchor` warning.
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// Requires `anchor` to be set; `anchor_sibling` without `anchor` has no
    /// effect and triggers an `anchor.sibling_without_anchor` warning.
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// When `Some`, positions this node's corresponding edge flush to the named
    /// edge of `anchor-sibling`. Requires `anchor-sibling` to be set.
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// A positive value pushes the node away from the sibling; negative pulls it closer.
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. When `Some(true)` AND a recognized
    /// `anchor` is present (and `anchor_zone` is absent), the `anchor` is
    /// resolved relative to this node's DIRECT PARENT CONTAINER's box (a frame
    /// or group) instead of the full page. An explicitly-authored `x`/`y` still
    /// wins. `anchor_zone` takes precedence when both are set. Requires the node
    /// to be inside a frame/group with a usable box; otherwise the validator
    /// emits `anchor.unresolvable_parent`. `anchor_parent` without `anchor`
    /// triggers an `anchor.parent_without_anchor` warning. `None`/`Some(false)`
    /// keeps page/zone-relative behavior (byte-identical).
    pub anchor_parent: Option<bool>,
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`].
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`].
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
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
    /// Vertical text-block alignment within the box (`top`/`middle`/`bottom`,
    /// default `top` = today's behavior: no y offset applied). When the box
    /// height exceeds the laid-out text block height, the block is offset by
    /// `0` (top), `(box_h - text_h)/2` (middle), or `box_h - text_h`
    /// (bottom). Unknown values are treated as `top` (byte-identical to absent).
    pub v_align: Option<String>,
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`].
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`].
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
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
    /// Open string naming the source language; drives built-in syntax
    /// highlighting when the language is supported, otherwise renders as plain text.
    pub language: Option<String>,
    /// Render a line-number gutter (default false).
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`].
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`].
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
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
    /// Stroke alignment: `"center"` (default), `"inside"`, or `"outside"`.
    /// `inside`/`outside` shift closed-shape strokes; open paths stroke centered.
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

/// A `pattern` node — a compact procedural primitive.
///
/// A `pattern` carries one TEMPLATE child — the [`motif`](PatternNode::motif) —
/// a single [`Node`] that will be expanded deterministically into many native
/// shapes (a grid or scatter of the motif). The node currently renders nothing;
/// expansion is not yet implemented. The motif is NOT an addressable/rendered
/// node — id-collection, validation, anchor, and tx passes treat the pattern as
/// a LEAF and never descend into the motif.
///
/// The common visual/geometry fields mirror [`RectNode`]; the pattern-specific
/// fields (`kind`, `seed`, `count`, `spacing`, `jitter`) describe the expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub radius: Option<PropertyValue>,
    /// Per-corner radius overrides (top-left, top-right, bottom-right, bottom-left).
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
    pub border_top: Option<PropertyValue>,
    /// Per-side border color for the bottom edge. Token-required (color token).
    pub border_bottom: Option<PropertyValue>,
    /// Per-side border color for the left edge. Token-required (color token).
    pub border_left: Option<PropertyValue>,
    /// Per-side border color for the right edge. Token-required (color token).
    pub border_right: Option<PropertyValue>,
    /// Shared border width for per-side borders. Token-required (dimension).
    pub border_width: Option<PropertyValue>,
    /// Outer stroke color: a SECOND stroke painted OUTSIDE the geometry.
    pub stroke_outer: Option<PropertyValue>,
    /// Outer stroke width for `stroke_outer`. Token-required (dimension).
    pub stroke_outer_width: Option<PropertyValue>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the separable blends.
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink.
    pub blur: Option<Dimension>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor. See [`RectNode::anchor`].
    pub anchor: Option<String>,
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`].
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
    /// Required: the pattern kind (`"grid"` | `"scatter"`; freeform, validated later).
    pub kind: String,
    /// Deterministic jitter seed.
    pub seed: Option<i64>,
    /// Scatter: number of instances.
    pub count: Option<i64>,
    /// Grid: cell spacing.
    pub spacing: Option<Dimension>,
    /// Positional jitter amount in `0..1`.
    pub jitter: Option<f64>,
    /// The single template child shape expanded by the pattern (mandatory).
    /// This is a TEMPLATE, NOT an addressable/rendered node: id-collection,
    /// validation, anchor, and tx passes never descend into it.
    pub motif: Box<Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// One data series within a [`ChartNode`].
///
/// A series is PURE DATA — it is not a renderable [`Node`] and is never
/// descended into by id-collection, validation, anchor, or tx passes. It
/// carries an ordered list of numeric values and optional legend/styling hints.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// Optional legend or category label for this series.
    pub label: Option<String>,
    /// Optional series color; a `(token)` color ref. When absent the renderer
    /// picks a palette color by series index.
    pub color: Option<PropertyValue>,
    /// Per-series value-label color override; falls back to the chart
    /// `value_color` then the default on-fill contrasting color.
    pub label_color: Option<PropertyValue>,
    /// Optional binding to a whole series from a [`DataContext`] field.
    /// `None` means the values are inline in [`ChartSeries::values`].
    pub data_ref: Option<String>,
    /// Ordered numeric data points for this series.
    pub values: Vec<f64>,
}

/// A `chart` node — a compact data-visualization primitive.
///
/// A `chart` declares its data inline via [`series`](ChartNode::series) children
/// (one child KDL node per series, each with positional f64 arguments) and
/// paints into its `[x, y, w, h]` bounding box. The node currently renders
/// nothing; chart rendering is deferred. The series children are pure DATA,
/// not renderable nodes: id-collection, validation, anchor, and tx passes
/// treat the chart as a LEAF and never descend into them.
///
/// The common visual/geometry fields mirror [`PatternNode`]; the chart-specific
/// fields (`kind`, `title`, `caption`, `legend`, `axis_*`, `bar_mode`,
/// `point_placement`, `value_labels`, `value_color`, `label_colors`, `categories`,
/// `series`) describe the chart content.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    pub w: Option<Dimension>,
    pub h: Option<Dimension>,
    pub radius: Option<PropertyValue>,
    /// Per-corner radius overrides (top-left, top-right, bottom-right, bottom-left).
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
    pub border_top: Option<PropertyValue>,
    /// Per-side border color for the bottom edge. Token-required (color token).
    pub border_bottom: Option<PropertyValue>,
    /// Per-side border color for the left edge. Token-required (color token).
    pub border_left: Option<PropertyValue>,
    /// Per-side border color for the right edge. Token-required (color token).
    pub border_right: Option<PropertyValue>,
    /// Shared border width for per-side borders. Token-required (dimension).
    pub border_width: Option<PropertyValue>,
    /// Outer stroke color: a SECOND stroke painted OUTSIDE the geometry.
    pub stroke_outer: Option<PropertyValue>,
    /// Outer stroke width for `stroke_outer`. Token-required (dimension).
    pub stroke_outer_width: Option<PropertyValue>,
    /// Drop shadow / outer glow, as a `(token)` ref to a `shadow` token.
    pub shadow: Option<PropertyValue>,
    /// Color/image filter ops, as a `(token)` ref to a `filter` token.
    pub filter: Option<PropertyValue>,
    /// Spatial coverage mask, as a `(token)` ref to a `mask` token.
    pub mask: Option<PropertyValue>,
    /// Compositing blend mode: `"normal"` (default) or one of the separable blends.
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink.
    pub blur: Option<Dimension>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor. See [`RectNode::anchor`].
    pub anchor: Option<String>,
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`].
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`].
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`].
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`].
    pub anchor_parent: Option<bool>,
    /// Required: the chart kind (`"bar"` | `"line"` | `"sparkline"` | `"pie"` | `"donut"`;
    /// freeform, validated later).
    pub kind: String,
    /// Optional chart title rendered above the plot area.
    pub title: Option<String>,
    /// Optional caption rendered below the chart.
    pub caption: Option<String>,
    /// Whether to render a legend. `None` defers to the renderer default.
    pub legend: Option<bool>,
    /// Minimum value for the value axis. `None` = auto-fit to data.
    pub axis_min: Option<f64>,
    /// Maximum value for the value axis. `None` = auto-fit to data.
    pub axis_max: Option<f64>,
    /// Style string for the axis (e.g. `"hidden"`, `"minimal"`); freeform for now.
    pub axis_style: Option<String>,
    /// Bar layout mode: `"grouped"` (default) | `"stacked"`; freeform,
    /// validated later. Mirrors how `kind` is typed/documented.
    pub bar_mode: Option<String>,
    /// X placement for line/area points: `"edge"` (default; first point on the
    /// value axis, last at the right edge) | `"center"` (category-band centers).
    /// freeform, validated later.
    pub point_placement: Option<String>,
    /// Value-label display/placement: `"auto"` (default) | `"none"` | `"top"` |
    /// `"center"`. freeform, validated later.
    pub value_labels: Option<String>,
    /// Explicit color (token) for value labels; when absent the renderer
    /// auto-picks a contrasting color.
    pub value_color: Option<PropertyValue>,
    /// Per-slice value-label colors for pie/donut (one per category, in order);
    /// empty = use the chart `value_color` or the white on-fill default.
    /// Populated from a `label-colors` child node whose positional arguments
    /// are each a `PropertyValue` (e.g. `(token)"color.x"`).
    pub label_colors: Vec<PropertyValue>,
    /// X-axis category labels (one per category slot); empty = derive index
    /// labels at render. Populated from a `categories` child node whose
    /// positional arguments are the label strings.
    pub categories: Vec<String>,
    /// Ordered data series. Each series carries labels, an optional color, and
    /// a list of f64 data points.
    pub series: Vec<ChartSeries>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `polyline` node — an OPEN stroked path defined by an ordered list of
/// `point` child nodes.
///
/// `polyline` has stroke (required for visible output) and optional fill.
/// Unlike `polygon`, `polyline` does NOT support `stroke-alignment`.
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
