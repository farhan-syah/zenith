//! Shared helpers for the per-node checks: geometry resolution, bounding-box
//! and AABB computation, role/id extraction, and the anchor/dimension/style-ref
//! validators reused by every per-kind `check_*` function.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::node::{Node, TextSpan, parse_anchor, parse_anchor_edge};
use crate::ast::value::{Dimension, PropertyValue, Unit, dim_to_px};
use crate::diagnostics::Diagnostic;
use crate::tokens::ResolvedToken;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

// ── off_canvas geometry helpers ───────────────────────────────────────────────

/// Resolve a single geometry axis dimension to pixels.
///
/// `Pct` is resolved against `basis` (e.g. page_w for x/w, page_h for y/h).
/// All other convertible units delegate to [`dim_to_px`]; `None` on failure.
pub(in crate::validate::check) fn resolve_axis(dim: &Dimension, basis: f64) -> Option<f64> {
    if dim.unit == Unit::Pct {
        Some(dim.value / 100.0 * basis)
    } else {
        dim_to_px(dim.value, &dim.unit)
    }
}

/// Whether `s` is one of the 12 recognized `blend-mode` values (`normal` plus
/// the 11 separable blends). Unknown values warn at validation time.
pub(super) fn is_valid_blend_mode(s: &str) -> bool {
    matches!(
        s,
        "normal"
            | "multiply"
            | "screen"
            | "overlay"
            | "darken"
            | "lighten"
            | "color-dodge"
            | "color-burn"
            | "hard-light"
            | "soft-light"
            | "difference"
            | "exclusion"
    )
}

/// Compute the authored bounding box `(x, y, w, h)` of a node in pixels.
///
/// Returns `None` when the node has no resolvable bounding box (Group, Unknown,
/// or any node with a missing/unresolvable required dimension). Callers should
/// treat `None` as "no check possible" and produce no advisory.
///
/// v0 NOTE: authored coordinates are used as-is. Group translation offsets are
/// NOT accumulated here (that is a scene-compiler / render-time concern). The
/// off_canvas advisory documents this v0 behavior: it checks authored geometry
/// against the page rectangle, not render-time geometry.
pub(in crate::validate::check) fn node_bbox(
    node: &Node,
    page_w: f64,
    page_h: f64,
) -> Option<(f64, f64, f64, f64)> {
    match node {
        Node::Rect(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Ellipse(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Image(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Frame(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Text(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Code(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Line(n) => {
            let x1 = resolve_axis(n.x1.as_ref()?, page_w)?;
            let y1 = resolve_axis(n.y1.as_ref()?, page_h)?;
            let x2 = resolve_axis(n.x2.as_ref()?, page_w)?;
            let y2 = resolve_axis(n.y2.as_ref()?, page_h)?;
            let bx = x1.min(x2);
            let by = y1.min(y2);
            let bw = (x2 - x1).abs();
            let bh = (y2 - y1).abs();
            Some((bx, by, bw, bh))
        }
        Node::Polygon(n) => points_bbox(&n.points, page_w, page_h),
        Node::Polyline(n) => points_bbox(&n.points, page_w, page_h),
        // Groups have no authoritative bbox in v0 — children are checked
        // individually. An instance likewise has no authoritative bbox: its
        // expanded subtree (a translated group) is the renderable geometry. A
        // field's box defaults to the page live area at compile time (x/w may be
        // omitted), so there is no authored bbox to check against the page here.
        // A toc likewise defaults to the live area; no authored bbox check.
        Node::Table(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Shape(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Pattern(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Chart(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Light(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let r = resolve_axis(pv_to_dim(n.radius.as_ref())?, page_w.max(page_h))?;
            Some((x - r, y - r, r * 2.0, r * 2.0))
        }
        Node::Mesh(n) => {
            let x = resolve_axis(pv_to_dim(n.x.as_ref())?, page_w)?;
            let y = resolve_axis(pv_to_dim(n.y.as_ref())?, page_h)?;
            let w = resolve_axis(pv_to_dim(n.w.as_ref())?, page_w)?;
            let h = resolve_axis(pv_to_dim(n.h.as_ref())?, page_h)?;
            Some((x, y, w, h))
        }
        // A connector has no authored bbox: its endpoints are DERIVED from the
        // resolved boxes of its `from`/`to` targets at compile time and are not
        // known at validate time, so there is no off_canvas check here.
        Node::Group(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => None,
    }
}

/// Compute the bounding box of a slice of [`Point`]s, resolving each coordinate
/// against the given page axis bases.
///
/// Returns `Some((min_x, min_y, w, h))` when at least one point resolves
/// successfully, `None` when no point has resolvable coordinates.
fn points_bbox(
    points: &[crate::ast::node::Point],
    page_w: f64,
    page_h: f64,
) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;
    for pt in points {
        if let (Some(px_val), Some(py_val)) = (
            pt.x.as_ref().and_then(|d| resolve_axis(d, page_w)),
            pt.y.as_ref().and_then(|d| resolve_axis(d, page_h)),
        ) {
            min_x = min_x.min(px_val);
            min_y = min_y.min(py_val);
            max_x = max_x.max(px_val);
            max_y = max_y.max(py_val);
            any = true;
        }
    }
    if any {
        Some((min_x, min_y, max_x - min_x, max_y - min_y))
    } else {
        None
    }
}

/// Read the authored rotation of a node in degrees, if the node carries a
/// `rotate` field and the stored unit is `Deg`.
///
/// Returns `Some(degrees)` for rotate-bearing node kinds when the stored unit
/// is `Unit::Deg`. Returns `None` when the node has no `rotate` field, the
/// field is absent (`None`), or the unit is not `Deg` (e.g. an exotic unit
/// produced by forward-compat).
///
/// Covered (have a `rotate` field): `Rect`, `Ellipse`, `Frame`, `Image`,
/// `Text`, `Code`, `Group`, `Polygon`, `Polyline`, `Table`, `Shape`,
/// `Connector`.
/// Not covered: `Line`, `Instance`, `Field`, `Footnote`, `Unknown`.
pub(in crate::validate::check) fn node_rotate_deg(node: &Node) -> Option<f64> {
    let dim = match node {
        Node::Rect(n) => n.rotate.as_ref(),
        Node::Ellipse(n) => n.rotate.as_ref(),
        Node::Frame(n) => n.rotate.as_ref(),
        Node::Image(n) => n.rotate.as_ref(),
        Node::Text(n) => n.rotate.as_ref(),
        Node::Code(n) => n.rotate.as_ref(),
        Node::Group(n) => n.rotate.as_ref(),
        Node::Polygon(n) => n.rotate.as_ref(),
        Node::Polyline(n) => n.rotate.as_ref(),
        Node::Table(n) => n.rotate.as_ref(),
        Node::Shape(n) => n.rotate.as_ref(),
        Node::Connector(n) => n.rotate.as_ref(),
        Node::Pattern(n) => n.rotate.as_ref(),
        Node::Chart(n) => n.rotate.as_ref(),
        Node::Light(_) => None,
        Node::Mesh(_) => None,
        Node::Line(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => None,
    }?;
    (dim.unit == Unit::Deg).then_some(dim.value)
}

/// Extract the optional `role` attribute from any node variant.
///
/// Returns `None` for nodes that have no role set (or none in the AST at all,
/// such as `Unknown`). Used by the margin advisory to exempt `role="guide"`
/// nodes, which intentionally live in the page margins.
pub(in crate::validate::check) fn node_role(node: &Node) -> Option<&str> {
    match node {
        Node::Rect(n) => n.role.as_deref(),
        Node::Ellipse(n) => n.role.as_deref(),
        Node::Line(n) => n.role.as_deref(),
        Node::Text(n) => n.role.as_deref(),
        Node::Code(n) => n.role.as_deref(),
        Node::Frame(n) => n.role.as_deref(),
        Node::Group(n) => n.role.as_deref(),
        Node::Image(n) => n.role.as_deref(),
        Node::Polygon(n) => n.role.as_deref(),
        Node::Polyline(n) => n.role.as_deref(),
        Node::Instance(n) => n.role.as_deref(),
        Node::Field(n) => n.role.as_deref(),
        Node::Toc(n) => n.role.as_deref(),
        Node::Footnote(n) => n.role.as_deref(),
        Node::Table(n) => n.role.as_deref(),
        Node::Shape(n) => n.role.as_deref(),
        Node::Connector(n) => n.role.as_deref(),
        Node::Pattern(n) => n.role.as_deref(),
        Node::Chart(n) => n.role.as_deref(),
        Node::Light(n) => n.role.as_deref(),
        Node::Mesh(n) => n.role.as_deref(),
        Node::Unknown(_) => None,
    }
}

/// Extract the string id and source span from any node variant.
pub(in crate::validate::check) fn node_id_and_span(
    node: &Node,
) -> (&str, Option<crate::ast::Span>) {
    match node {
        Node::Rect(n) => (&n.id, n.source_span),
        Node::Ellipse(n) => (&n.id, n.source_span),
        Node::Line(n) => (&n.id, n.source_span),
        Node::Text(n) => (&n.id, n.source_span),
        Node::Code(n) => (&n.id, n.source_span),
        Node::Frame(n) => (&n.id, n.source_span),
        Node::Group(n) => (&n.id, n.source_span),
        Node::Image(n) => (&n.id, n.source_span),
        Node::Polygon(n) => (&n.id, n.source_span),
        Node::Polyline(n) => (&n.id, n.source_span),
        Node::Instance(n) => (&n.id, n.source_span),
        Node::Field(n) => (&n.id, n.source_span),
        Node::Toc(n) => (&n.id, n.source_span),
        Node::Footnote(n) => (&n.id, n.source_span),
        Node::Table(n) => (&n.id, n.source_span),
        Node::Shape(n) => (&n.id, n.source_span),
        Node::Connector(n) => (&n.id, n.source_span),
        Node::Pattern(n) => (&n.id, n.source_span),
        Node::Chart(n) => (&n.id, n.source_span),
        Node::Light(n) => (&n.id, n.source_span),
        Node::Mesh(n) => (&n.id, n.source_span),
        Node::Unknown(n) => (n.id.as_deref().unwrap_or(&n.kind), n.source_span),
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Container context for the parent-relative anchor checks.
///
/// `in_container` is `true` when the node is a direct (or group-nested) child of
/// a `frame`/`group`. `parent_box_known` is `true` when that enclosing container
/// has a usable reference box (frame: always; group: only when it declares both
/// `w` and `h`). At the page root both are `false`.
#[derive(Clone, Copy)]
pub(in crate::validate::check) struct AnchorParentCtx {
    pub(in crate::validate::check) in_container: bool,
    pub(in crate::validate::check) parent_box_known: bool,
}

/// The anchor property reads, bundled so [`check_anchor`] stays
/// within the argument-count lint without suppression.
#[derive(Clone, Copy)]
pub(in crate::validate::check) struct AnchorProps<'a> {
    pub(in crate::validate::check) anchor: Option<&'a str>,
    pub(in crate::validate::check) anchor_zone: Option<&'a str>,
    pub(in crate::validate::check) anchor_sibling: Option<&'a str>,
    pub(in crate::validate::check) anchor_parent: bool,
    pub(in crate::validate::check) anchor_edge: Option<&'a str>,
    pub(in crate::validate::check) anchor_gap: Option<&'a Dimension>,
}

/// Validate the `anchor`, `anchor_zone`, `anchor_sibling`, `anchor_parent`,
/// `anchor_edge`, and `anchor_gap` properties on a node.
///
/// Returns `true` when `anchor` is present and recognized, OR when
/// `anchor_sibling` + `anchor_edge` are both present (edge-placement mode:
/// x/y geometry is NOT required in that case either), `false` otherwise.
///
/// Diagnostics pushed:
/// - `anchor.unknown_value` (Error) — `anchor` present with an unrecognized value.
/// - `anchor.zone_without_anchor` (Warning) — `anchor_zone` set but `anchor` absent.
/// - `anchor.unresolved_zone` (Error) — `anchor_zone` names a zone not on this page.
/// - `anchor.sibling_without_anchor` (Warning) — `anchor_sibling` set but `anchor` absent
///   and `anchor_edge` is also absent (edge-placement makes `anchor` optional).
///   (The sibling-reference graph — `anchor.unresolved_sibling` / `anchor.cycle` —
///   is validated per-scope by [`check_sibling_anchors`], not here.)
/// - `anchor.parent_without_anchor` (Warning) — `anchor_parent` set but `anchor` absent.
/// - `anchor.unresolvable_parent` (Error) — `anchor_parent` set but the node is
///   not inside a frame/group container, or the parent container's box is unknown
///   (a group without `w`/`h`).
/// - `anchor.edge_without_sibling` (Warning) — `anchor_edge` set but `anchor_sibling` absent.
/// - `anchor.unknown_edge` (Error) — `anchor_edge` value is not one of the four
///   recognized directional values.
/// - `anchor.gap_invalid_unit` (Warning) — `anchor_gap` unit cannot be resolved to px.
pub(super) fn check_anchor(
    node_id: &str,
    props: AnchorProps,
    parent_ctx: AnchorParentCtx,
    zone_ids: &BTreeSet<&str>,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let AnchorProps {
        anchor,
        anchor_zone,
        anchor_sibling,
        anchor_parent,
        anchor_edge,
        anchor_gap,
    } = props;
    // When anchor-zone is present without anchor, emit a warning and treat zone as
    // irrelevant (anchor-zone has no effect without an anchor value).
    if anchor_zone.is_some() && anchor.is_none() {
        diagnostics.push(Diagnostic::warning(
            "anchor.zone_without_anchor",
            format!(
                "node '{}': anchor-zone is set but anchor is absent; \
                 anchor-zone has no effect without an anchor value",
                node_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-sibling is present without anchor, emit a warning — BUT only
    // when anchor-edge is also absent. When anchor-edge is set, anchor-sibling
    // enables edge-placement mode and anchor is intentionally optional.
    if anchor_sibling.is_some() && anchor.is_none() && anchor_edge.is_none() {
        diagnostics.push(Diagnostic::warning(
            "anchor.sibling_without_anchor",
            format!(
                "node '{}': anchor-sibling is set but anchor is absent; \
                 anchor-sibling has no effect without an anchor value \
                 (unless anchor-edge is set)",
                node_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-parent is set without anchor, emit a warning (anchor-parent has
    // no effect without an anchor value to position).
    if anchor_parent && anchor.is_none() {
        diagnostics.push(Diagnostic::warning(
            "anchor.parent_without_anchor",
            format!(
                "node '{}': anchor-parent is set but anchor is absent; \
                 anchor-parent has no effect without an anchor value",
                node_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-parent is set, the node must live inside a frame/group whose
    // reference box is resolvable; otherwise the parent-relative anchor cannot be
    // derived. `anchor_zone` takes precedence and disables parent mode, so only
    // flag when no zone is set.
    if anchor_parent
        && anchor_zone.is_none()
        && (!parent_ctx.in_container || !parent_ctx.parent_box_known)
    {
        diagnostics.push(Diagnostic::error(
            "anchor.unresolvable_parent",
            format!(
                "node '{}': anchor-parent is set but the node is not inside a \
                 frame/group container with a usable box",
                node_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-edge is present without anchor-sibling, it has no effect.
    if anchor_edge.is_some() && anchor_sibling.is_none() {
        diagnostics.push(Diagnostic::warning(
            "anchor.edge_without_sibling",
            format!(
                "node '{}': anchor-edge is set but anchor-sibling is absent; \
                 it has no effect without an anchor-sibling target",
                node_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-edge is present, validate that the value is one of the four
    // recognized directional values. `parse_anchor_edge` is the single source
    // of truth for the valid names (shared with the scene pre-pass).
    if let Some(edge) = anchor_edge
        && parse_anchor_edge(edge).is_none()
    {
        diagnostics.push(Diagnostic::error(
            "anchor.unknown_edge",
            format!(
                "node '{}': anchor-edge value '{}' is not recognized; \
                 valid values are above, below, before, after",
                node_id, edge
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // When anchor-gap is present, the unit must be px-convertible.
    if let Some(gap) = anchor_gap
        && dim_to_px(gap.value, &gap.unit).is_none()
    {
        diagnostics.push(Diagnostic::warning(
            "anchor.gap_invalid_unit",
            format!(
                "node '{}': anchor-gap unit '{}' cannot be resolved to px; \
                 gap must resolve to px",
                node_id,
                gap.unit.as_annotation()
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    let anchor_active = match anchor {
        None => false,
        Some(s) => {
            if parse_anchor(s).is_some() {
                true
            } else {
                diagnostics.push(Diagnostic::error(
                    "anchor.unknown_value",
                    format!(
                        "node '{}': anchor value '{}' is not recognized; \
                         valid values are top-left, top-center, top-right, \
                         center-left, center, center-right, \
                         bottom-left, bottom-center, bottom-right",
                        node_id, s
                    ),
                    span,
                    Some(node_id.to_owned()),
                ));
                false
            }
        }
    };

    // When anchor-zone names a zone, check that it exists on the page.
    if let Some(zone_id) = anchor_zone
        && !zone_ids.contains(zone_id)
    {
        diagnostics.push(Diagnostic::error(
            "anchor.unresolved_zone",
            format!(
                "node '{}': anchor-zone '{}' does not name a safe-zone on this page",
                node_id, zone_id
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }

    // Edge-placement mode: anchor_sibling + anchor_edge together supply both x
    // and y (the engine positions the node relative to the sibling edge), so
    // x/y geometry is not required even without a nine-point anchor.
    anchor_active || (anchor_sibling.is_some() && anchor_edge.is_some())
}

/// Per-node sibling-anchor read: the node's id, its `anchor_sibling` target (if
/// any), and its span — for anchor-bearing node kinds only. Kinds that never
/// carry an `anchor` return `None` (they are not valid sibling targets and
/// cannot themselves reference a sibling). The match is EXHAUSTIVE over `Node`
/// so a new kind forces a decision here.
fn node_sibling_fields(node: &Node) -> Option<(&str, Option<&str>, Option<crate::ast::Span>)> {
    let f = match node {
        Node::Rect(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Ellipse(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Text(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Code(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Image(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Frame(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Group(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Shape(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Table(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Field(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Toc(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Pattern(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Chart(n) => (n.id.as_str(), n.anchor_sibling.as_deref(), n.source_span),
        Node::Light(_) => return None,
        Node::Mesh(_) => return None,
        // Kinds that never carry an `anchor` are not sibling-bearing.
        Node::Line(_)
        | Node::Connector(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Footnote(_)
        | Node::Instance(_)
        | Node::Unknown(_) => return None,
    };
    Some(f)
}

/// Validate the sibling-anchor (`anchor-sibling`) graph of one container scope
/// (`children` = the direct children of a page / frame / group).
///
/// Diagnostics pushed:
/// - `anchor.unresolved_sibling` (Error) — a node names an `anchor-sibling`
///   target that is not an in-scope anchor-bearing node id.
/// - `anchor.cycle` (Error) — a node participates in a sibling-anchor reference
///   cycle within this scope. Each cyclic node is reported at most once.
///
/// The cycle detection mirrors the iterative visited-set chain-follow used by
/// the `token.cyclic_reference` detector in `tokens::resolve::driver`: each walk
/// follows the in-scope `id → target` map with a per-walk `BTreeSet` visited;
/// revisiting an id signals a cycle. A scope-wide `BTreeSet` of already-reported
/// ids dedupes across walks. Bounded by scope size; no recursion, no panic.
pub(in crate::validate::check) fn check_sibling_anchors(
    children: &[Node],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // In-scope anchor-bearing node ids (valid sibling targets).
    let mut in_scope: BTreeSet<&str> = BTreeSet::new();
    for child in children {
        if let Some((id, _, _)) = node_sibling_fields(child) {
            in_scope.insert(id);
        }
    }

    // Unresolved-reference pass, plus build the in-scope id → target edge map for
    // cycle detection (only edges whose target is itself in-scope and anchor-
    // bearing form the graph; an out-of-scope target is reported as unresolved
    // and never enters the graph).
    let mut edges: BTreeMap<&str, &str> = BTreeMap::new();
    for child in children {
        let Some((id, anchor_sibling, span)) = node_sibling_fields(child) else {
            continue;
        };
        if let Some(target) = anchor_sibling {
            if in_scope.contains(target) {
                edges.insert(id, target);
            } else {
                diagnostics.push(Diagnostic::error(
                    "anchor.unresolved_sibling",
                    format!(
                        "node '{}': anchor-sibling '{}' does not name a sibling \
                         node in the same container",
                        id, target
                    ),
                    span,
                    Some(id.to_owned()),
                ));
            }
        }
    }

    // Cycle detection: follow each id's chain through `edges` with a per-walk
    // visited set. A revisit means a cycle; report it once per node.
    let mut reported: BTreeSet<&str> = BTreeSet::new();
    let span_of: BTreeMap<&str, Option<crate::ast::Span>> = children
        .iter()
        .filter_map(|c| node_sibling_fields(c).map(|(id, _, span)| (id, span)))
        .collect();
    for &start in edges.keys() {
        if reported.contains(start) {
            continue;
        }
        let mut visited: BTreeSet<&str> = BTreeSet::new();
        let mut current = start;
        visited.insert(current);
        while let Some(&next) = edges.get(current) {
            if visited.contains(next) {
                // `next` closes a cycle. Report `start` (the walk origin) once.
                if reported.insert(start) {
                    diagnostics.push(Diagnostic::error(
                        "anchor.cycle",
                        format!(
                            "node '{}': anchor-sibling chain reaches a cycle \
                             (at '{}'); its position cannot be resolved",
                            start, next
                        ),
                        span_of.get(start).copied().flatten(),
                        Some(start.to_owned()),
                    ));
                }
                break;
            }
            visited.insert(next);
            current = next;
        }
    }
}

/// Borrowed token-validation context passed to geometry helpers.
///
/// Bundles the two token-related arguments that `check_optional_dim` and its
/// callees need so the function stays within the 7-argument clippy limit without
/// an `#[allow]`.
pub(super) struct TokenEnv<'a> {
    pub(super) referenced: &'a mut BTreeSet<String>,
    pub(super) resolved: &'a BTreeMap<String, ResolvedToken>,
}

/// - absent AND `required` (e.g. a non-flow-positioned leaf) → `node.missing_geometry` (Error).
/// - absent AND NOT `required` (e.g. a direct child of a `layout="flow"`
///   frame, whose position/size is supplied by the flow algorithm) → no
///   diagnostic.
/// - present but `Unit::Unknown` → `node.invalid_geometry` (Error) regardless
///   of `required`.
///
/// A geometry property accepts EITHER a raw dimension literal (`(px)N`) OR a
/// `(token)"id"` dimension token ref (exactly like `font-size`). The dispatch:
/// - absent + required → `node.missing_geometry`.
/// - `Dimension` with `Unit::Unknown` → `node.invalid_geometry`; a known unit is ok.
/// - `TokenRef(id)` → PRESENT and geometrically valid; existence + dimension-type
///   validation and reference registration are delegated to [`check_visual_prop`]
///   with [`VisualExpect::Dimension`] (which, on a token ref, never emits
///   `token.raw_visual_literal` — raw px geometry is intentionally allowed).
/// - `Literal` / `DataRef` → `node.invalid_geometry` (geometry can't be a bare
///   string or data ref).
pub(super) fn check_optional_dim(
    node_id: &str,
    prop: &str,
    value: Option<&PropertyValue>,
    required: bool,
    span: Option<crate::ast::Span>,
    tokens: &mut TokenEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match value {
        None if required => {
            diagnostics.push(Diagnostic::error(
                "node.missing_geometry",
                format!(
                    "node '{}': required geometry property '{}' is missing",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        None => {
            // Flow-positioned child: geometry is supplied by the parent.
        }
        Some(PropertyValue::Dimension(d)) if matches!(d.unit, Unit::Unknown(_)) => {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "node '{}': geometry property '{}' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        Some(PropertyValue::Dimension(_)) => {
            // valid raw dimension literal.
        }
        Some(pv @ PropertyValue::TokenRef(_)) => {
            // Present + valid for the geometry check. Existence, dimension-type
            // compatibility, and reference registration are handled by the shared
            // visual-prop machinery (a token ref never trips raw_visual_literal).
            check_visual_prop(
                node_id,
                prop,
                Some(pv),
                VisualExpect::Dimension,
                &mut *tokens.referenced,
                tokens.resolved,
                diagnostics,
            );
        }
        Some(PropertyValue::Literal(_) | PropertyValue::DataRef(_)) => {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "node '{}': geometry property '{}' must be a dimension literal \
                     (e.g. (px)100) or a dimension token ref; a bare string or \
                     data ref is not allowed",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
    }
}

/// Narrow an optional geometry [`PropertyValue`] to a raw [`Dimension`].
///
/// Returns `Some(&Dimension)` only for a `PropertyValue::Dimension`; a token ref
/// (or any non-dimension variant) yields `None`, so geometry expressed as a
/// `(token)` ref is treated as "not resolvable at validate time" by the
/// off-canvas / bbox checks (tokens resolve in a later pass).
pub(in crate::validate::check) fn pv_to_dim(pv: Option<&PropertyValue>) -> Option<&Dimension> {
    match pv? {
        PropertyValue::Dimension(d) => Some(d),
        PropertyValue::TokenRef(_) | PropertyValue::Literal(_) | PropertyValue::DataRef(_) => None,
    }
}

/// Validate a RAW [`Dimension`] geometry property (no token-ref support).
///
/// Used for geometry axes that are still typed `Option<Dimension>` and do NOT
/// accept a `(token)` ref — e.g. the `line` endpoints `x1`/`y1`/`x2`/`y2`. Same
/// missing/invalid-unit diagnostics as the dimension arm of [`check_optional_dim`].
pub(super) fn check_dimension_geom(
    node_id: &str,
    prop: &str,
    dim: Option<&Dimension>,
    required: bool,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match dim {
        None if required => {
            diagnostics.push(Diagnostic::error(
                "node.missing_geometry",
                format!(
                    "node '{}': required geometry property '{}' is missing",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        None => {}
        Some(d) if matches!(d.unit, Unit::Unknown(_)) => {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "node '{}': geometry property '{}' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        Some(_) => {}
    }
}

// ── Style helpers ─────────────────────────────────────────────────────────────

/// Validate the `fill` and `font-weight` visual properties on a slice of
/// [`TextSpan`]s, registering any token references so they are not falsely
/// flagged as unused.
///
/// Used by every node kind that carries a `spans` field (`text`, `shape`,
/// `footnote`). The `node_id` is the PARENT node's id (spans have no id of
/// their own).
pub(super) fn check_spans(
    node_id: &str,
    spans: &[TextSpan],
    referenced_token_ids: &mut BTreeSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for span in spans {
        check_visual_prop(
            node_id,
            "fill",
            span.fill.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            node_id,
            "font-weight",
            span.font_weight.as_ref(),
            VisualExpect::FontWeight,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            node_id,
            "highlight",
            span.highlight.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }
}

/// Check that a node's `style` attribute references a declared style id.
///
/// Called for every node kind that carries a `style` field.
pub(super) fn check_style_ref(
    node_id: &str,
    style_opt: Option<&str>,
    declared_style_ids: &BTreeSet<String>,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(sid) = style_opt
        && !declared_style_ids.contains(sid)
    {
        diagnostics.push(Diagnostic::error(
            "style.unknown_reference",
            format!(
                "node '{}': references style '{}' which is not declared in the styles block",
                node_id, sid
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }
}

// ── Shared visual-property block ──────────────────────────────────────────────

/// A borrowed view of every visual property shared between `rect` and `pattern`.
///
/// The two node kinds carry an identical visual-property surface (fill, stroke,
/// per-side borders, blend-mode, shadow/filter/mask, blur), so the validation of
/// that surface lives in one place ([`check_visual_props`]). The only structural
/// difference is the corner-radius set: `rect` has `radius`/`radius-*`, while
/// `pattern` has none. A `PatternNode` therefore passes all radius fields as
/// `None`; `check_visual_prop` on a `None` value is a no-op and the per-corner
/// guard only fires on `Some(Dimension)`, so a pattern emits nothing for radius
/// while a rect keeps its exact ordering (radius between blend-mode and shadow).
///
/// All fields are `Copy` (`Option<&_>` / `Option<&str>`), so the whole view is
/// `Copy` and can be passed by value without bundling extra arguments.
#[derive(Clone, Copy)]
pub(super) struct VisualProps<'a> {
    pub(super) fill: Option<&'a PropertyValue>,
    pub(super) stroke: Option<&'a PropertyValue>,
    pub(super) stroke_width: Option<&'a PropertyValue>,
    pub(super) stroke_dash: Option<&'a PropertyValue>,
    pub(super) stroke_gap: Option<&'a PropertyValue>,
    pub(super) stroke_linecap: Option<&'a str>,
    pub(super) border_top: Option<&'a PropertyValue>,
    pub(super) border_bottom: Option<&'a PropertyValue>,
    pub(super) border_left: Option<&'a PropertyValue>,
    pub(super) border_right: Option<&'a PropertyValue>,
    pub(super) stroke_outer: Option<&'a PropertyValue>,
    pub(super) border_width: Option<&'a PropertyValue>,
    pub(super) stroke_outer_width: Option<&'a PropertyValue>,
    pub(super) blend_mode: Option<&'a str>,
    /// Corner radius props: supplied by `rect`, all `None` for `pattern`.
    pub(super) radius: Option<&'a PropertyValue>,
    pub(super) radius_tl: Option<&'a PropertyValue>,
    pub(super) radius_tr: Option<&'a PropertyValue>,
    pub(super) radius_br: Option<&'a PropertyValue>,
    pub(super) radius_bl: Option<&'a PropertyValue>,
    pub(super) shadow: Option<&'a PropertyValue>,
    pub(super) filter: Option<&'a PropertyValue>,
    pub(super) mask: Option<&'a PropertyValue>,
    pub(super) blur: Option<&'a Dimension>,
}

/// Validate the shared visual-property block for `rect` and `pattern`, pushing
/// diagnostics in exactly the order the original inline blocks did:
/// fill/stroke/stroke-width → stroke-dash (+ negative guard) → stroke-gap
/// (+ negative guard) → stroke-linecap → per-side borders → border widths →
/// blend-mode → radius (uniform + per-corner, each with a negative guard) →
/// shadow/filter/mask → blur (negative guard). `kind` is substituted into every
/// message ("rect" or "pattern").
pub(super) fn check_visual_props(
    kind: &str,
    id: &str,
    source_span: Option<crate::ast::Span>,
    props: VisualProps<'_>,
    referenced_token_ids: &mut BTreeSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    check_visual_prop(
        id,
        "fill",
        props.fill,
        VisualExpect::ColorOrGradient,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        id,
        "stroke",
        props.stroke,
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        id,
        "stroke-width",
        props.stroke_width,
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        id,
        "stroke-dash",
        props.stroke_dash,
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    if let Some(PropertyValue::Dimension(d)) = props.stroke_dash
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("{kind} '{id}': stroke-dash must be >= 0"),
            source_span,
            Some(id.to_owned()),
        ));
    }
    check_visual_prop(
        id,
        "stroke-gap",
        props.stroke_gap,
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    if let Some(PropertyValue::Dimension(d)) = props.stroke_gap
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("{kind} '{id}': stroke-gap must be >= 0"),
            source_span,
            Some(id.to_owned()),
        ));
    }
    if let Some(lc) = props.stroke_linecap
        && !matches!(lc, "butt" | "round" | "square")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!("{kind} '{id}': stroke-linecap '{lc}' is not one of butt/round/square"),
            source_span,
            Some(id.to_owned()),
        ));
    }
    // Per-side border colors (token-required color props).
    for (prop_name, prop_val) in [
        ("border-top", props.border_top),
        ("border-bottom", props.border_bottom),
        ("border-left", props.border_left),
        ("border-right", props.border_right),
        ("stroke-outer", props.stroke_outer),
    ] {
        check_visual_prop(
            id,
            prop_name,
            prop_val,
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }
    // Per-side border width + outer stroke width (token-required dimension props).
    for (prop_name, prop_val) in [
        ("border-width", props.border_width),
        ("stroke-outer-width", props.stroke_outer_width),
    ] {
        check_visual_prop(
            id,
            prop_name,
            prop_val,
            VisualExpect::Dimension,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }
    if let Some(bm) = props.blend_mode
        && !is_valid_blend_mode(bm)
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!("{kind} '{id}': blend-mode '{bm}' is not a recognized value"),
            source_span,
            Some(id.to_owned()),
        ));
    }
    // Corner radius: uniform then per-corner overrides. Absent for patterns
    // (all radius fields `None`), so a pattern emits nothing here while a rect
    // keeps radius ordered between blend-mode and shadow.
    check_visual_prop(
        id,
        "radius",
        props.radius,
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    for (prop_name, prop_val) in [
        ("radius-tl", props.radius_tl),
        ("radius-tr", props.radius_tr),
        ("radius-br", props.radius_br),
        ("radius-bl", props.radius_bl),
    ] {
        check_visual_prop(
            id,
            prop_name,
            prop_val,
            VisualExpect::Dimension,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        if let Some(PropertyValue::Dimension(d)) = prop_val
            && d.value < 0.0
        {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!("{kind} '{id}': {prop_name} must be >= 0"),
                source_span,
                Some(id.to_owned()),
            ));
        }
    }
    check_visual_prop(
        id,
        "shadow",
        props.shadow,
        VisualExpect::Shadow,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        id,
        "filter",
        props.filter,
        VisualExpect::Filter,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        id,
        "mask",
        props.mask,
        VisualExpect::Mask,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    if let Some(d) = props.blur
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("{kind} '{id}': blur must be >= 0"),
            source_span,
            Some(id.to_owned()),
        ));
    }
}
