//! Container node structs that own child `Node`s: frame, group, and the table
//! family (table + column/row/cell).

use std::collections::BTreeMap;

use crate::ast::Span;
use crate::ast::value::{Dimension, PropertyValue};

use super::common::{Node, UnknownProperty};

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
    /// Rotation in degrees, applied at render about the node's center (the
    /// subtree rotates with it; see the compile-site notes for clip limitations).
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`](super::RectNode::anchor_sibling).
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`](super::RectNode::anchor_edge).
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`](super::RectNode::anchor_gap).
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
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
    /// Rotation in degrees, applied at render about the node's center (the
    /// subtree rotates with it; see the compile-site notes for clip limitations).
    pub rotate: Option<Dimension>,
    /// Compositing blend mode: `"normal"` (default) or one of the 11 separable
    /// blends. `None`/`"normal"` render source-over (byte-identical).
    pub blend_mode: Option<String>,
    /// Gaussian blur radius applied to the node's own rendered ink (sigma in
    /// the declared unit, resolved to pixels at compile time). `None` / 0 →
    /// no blur (byte-identical to having no attribute).
    pub blur: Option<Dimension>,
    pub style: Option<String>,
    /// Advisory semantic layer role for external tooling (e.g. `"background"`,
    /// `"overlay"`). Non-rendering; distinct from the structural `role` field.
    /// Open-ended string; no value is invalid.
    pub semantic_role: Option<String>,
    /// Advisory visual prominence hint in the range `0.0..=1.0`. Non-rendering;
    /// values outside this range produce a validation warning.
    pub intensity: Option<f64>,
    /// Advisory z-ordering hint for external tooling. Non-rendering; all integer
    /// values are valid.
    pub layer_priority: Option<i64>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`](super::RectNode::anchor_sibling).
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`](super::RectNode::anchor_edge).
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`](super::RectNode::anchor_gap).
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single column declaration in a [`TableNode`] (a `column` child).
///
/// `width` absent means an AUTO column: its width is content-based (the natural
/// width its cells demand), scaled to fit the table's leftover width.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    /// Explicit column width; `None` = auto (content-based width).
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
/// Renders tables with explicit, proportional, or content-based (auto) column
/// widths; separate or collapsed borders; styled header rows; and multi-page
/// flow when a table is taller than its page.
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
    /// First N rows are headers: styled via `header_fill`/`header_style` and
    /// repeated atop each page slice in multi-page flow.
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
    /// Border model: `"separate"` (default) or `"collapse"`; both are rendered.
    pub border_collapse: Option<String>,
    /// Default cell background (token-required color).
    pub fill: Option<PropertyValue>,
    /// Default cell border color (token-required color).
    pub border: Option<PropertyValue>,
    /// Default border width (token/dimension).
    pub border_width: Option<PropertyValue>,
    /// Header-row background override, applied to header cells (precedence:
    /// cell.fill > header_fill > table.fill).
    pub header_fill: Option<PropertyValue>,
    /// Header text style ref, applied to header-row cell text.
    pub header_style: Option<String>,
    /// Default horizontal alignment (`start`(default)/`center`/`end`).
    pub h_align: Option<String>,
    /// Default vertical alignment (`top`(default)/`middle`/`bottom`).
    pub v_align: Option<String>,
    pub style: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Rotation — parsed and preserved but not yet applied at render for tables.
    pub rotate: Option<Dimension>,
    /// Page-relative placement anchor (one of the nine named positions, e.g.
    /// `"bottom-right"`). When present and recognized, the compile step derives
    /// the node's x and/or y from the page and node dimensions. An explicitly-
    /// authored x or y always wins.
    pub anchor: Option<String>,
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Optional sibling node id for sibling-relative anchor positioning.
    /// See [`RectNode::anchor_sibling`](super::RectNode::anchor_sibling).
    pub anchor_sibling: Option<String>,
    /// Adjacent-placement edge relative to `anchor-sibling`: `above`/`below`/`before`/`after`.
    /// See [`RectNode::anchor_edge`](super::RectNode::anchor_edge).
    pub anchor_edge: Option<String>,
    /// Gap (px) between this node and its `anchor-sibling` edge when `anchor-edge` is set.
    /// See [`RectNode::anchor_gap`](super::RectNode::anchor_gap).
    pub anchor_gap: Option<Dimension>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
