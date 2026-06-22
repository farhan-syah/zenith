//! Specialized node structs: the compound shape, the derived connector,
//! component instances + overrides, the forward-compat unknown node, and the
//! book-interior furniture (field, footnote, toc).

use std::collections::BTreeMap;

use crate::ast::Span;
use crate::ast::value::{Dimension, PropertyValue};

use super::common::{Node, TextSpan, UnknownProperty};

/// A `shape` node — a COMPOUND node: a background box that OWNS a centered text
/// label (like a flowchart process box).
///
/// Structurally this mirrors [`TextNode`](super::TextNode): it carries box geometry + visual
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
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

/// An instance-local override applied to a single descendant of the referenced
/// component when an [`InstanceNode`] is expanded at compile time.
///
/// An `override` is an `override ref="<local-descendant-id>" { … }` child of an
/// instance. `ref_id` names a descendant by its component-LOCAL id (the id as
/// declared inside the [`ComponentDef`](crate::ast::ComponentDef), before instance-id prefixing).
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

/// An `instance` node — a placement of a declared [`ComponentDef`](crate::ast::ComponentDef) at an origin
/// `(x, y)`, with an optional opacity/visible cascade and instance-local
/// overrides.
///
/// At compile time the instance expands to the component's child subtree treated
/// as a GROUP translated by `(x, y)`, cascading `opacity`/`visible` exactly like
/// a [`GroupNode`](super::GroupNode). Every expanded descendant id is PREFIXED with the instance id
/// (`<instance-id>/<local-id>`) so multiple instances of the same component never
/// collide. The instance node itself emits no scene command; its expanded subtree
/// does. Expansion happens at COMPILE time only — the instance stays a single node
/// in the canonical AST so parse→format→parse round-trips.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// The referenced [`ComponentDef`](crate::ast::ComponentDef) id.
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A `footnote` node — page-level book-interior furniture that auto-numbers and
/// renders in a reserved zone at the bottom of the page.
///
/// A footnote is NOT positioned by the author: it has NO `x`/`y`/`w`/`h`. At
/// compile time every `footnote` that is a DIRECT child of a [`Page`](crate::ast::Page) is
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
/// The synthesised [`TextNode`](super::TextNode) uses `tab-leader` mode so the text engine
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
    /// Optional safe-zone reference for the anchor. See [`RectNode::anchor_zone`](super::RectNode::anchor_zone).
    pub anchor_zone: Option<String>,
    /// Parent-relative anchor toggle. See [`RectNode::anchor_parent`](super::RectNode::anchor_parent).
    pub anchor_parent: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
