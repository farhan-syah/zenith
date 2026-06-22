//! Shared helpers for the per-node checks: geometry resolution, bounding-box
//! and AABB computation, role/id extraction, and the anchor/dimension/style-ref
//! validators reused by every per-kind `check_*` function.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::ast::node::{Node, TextSpan, parse_anchor};
use crate::ast::value::{Dimension, Unit, dim_to_px};
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
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Ellipse(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Image(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Frame(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Text(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Code(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
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
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Shape(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
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
        Node::Unknown(n) => (n.id.as_deref().unwrap_or(&n.kind), n.source_span),
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Validate the `anchor` and `anchor_zone` properties on a node.
///
/// Returns `true` when `anchor` is present and recognized (anchor active:
/// x/y geometry is NOT required even outside a flow parent), `false` otherwise.
///
/// Diagnostics pushed:
/// - `anchor.unknown_value` (Error) — `anchor` present with an unrecognized value.
/// - `anchor.zone_without_anchor` (Warning) — `anchor_zone` set but `anchor` absent.
/// - `anchor.unresolved_zone` (Error) — `anchor_zone` names a zone not on this page.
pub(super) fn check_anchor(
    node_id: &str,
    anchor: Option<&str>,
    anchor_zone: Option<&str>,
    zone_ids: &BTreeSet<&str>,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
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

    anchor_active
}

/// - absent AND `required` (e.g. a non-flow-positioned leaf) → `node.missing_geometry` (Error).
/// - absent AND NOT `required` (e.g. a direct child of a `layout="flow"`
///   frame, whose position/size is supplied by the flow algorithm) → no
///   diagnostic.
/// - present but `Unit::Unknown` → `node.invalid_geometry` (Error) regardless
///   of `required`.
pub(super) fn check_optional_dim(
    node_id: &str,
    prop: &str,
    dim: Option<&crate::ast::value::Dimension>,
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
        None => {
            // Flow-positioned child: geometry is supplied by the parent.
        }
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
        Some(_) => {
            // valid
        }
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
    referenced_token_ids: &mut HashSet<String>,
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
    }
}

/// Check that a node's `style` attribute references a declared style id.
///
/// Called for every node kind that carries a `style` field.
pub(super) fn check_style_ref(
    node_id: &str,
    style_opt: Option<&str>,
    declared_style_ids: &HashSet<String>,
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
