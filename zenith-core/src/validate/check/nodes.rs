//! Per-node validation: the recursive [`walk_node`] dispatcher, the
//! polygon/polyline checks, geometry validation, and the off_canvas geometry
//! helpers.

use std::collections::{BTreeMap, HashSet};

use crate::ast::node::{FieldNode, FootnoteNode, InstanceNode, Node, PolygonNode, PolylineNode};
use crate::ast::value::{Dimension, PropertyValue, Unit, dim_to_px};
use crate::diagnostics::Diagnostic;
use crate::tokens::ResolvedToken;

use super::register_id;
use super::visual::{VisualExpect, check_visual_prop};

/// Recursively walk a [`Node`], collecting all diagnostics.
///
/// `referenced_token_ids` accumulates every token id actually used so that
/// the unused-token check (done after the walk) can diff against defined ids.
///
/// `page_px_bounds` is `Some((page_w, page_h))` when the page's dimensions
/// resolved successfully; `None` means off_canvas checks are skipped for this
/// page (page unit was bad — already diagnosed).
///
/// # Known limitation
/// Recursion through `Node::Group` and `Node::Frame` children has no depth
/// guard.  Pathologically deep trees can overflow the stack.  This is an
/// accepted v0 limitation.
#[allow(clippy::too_many_arguments)]
pub(super) fn walk_node(
    node: &Node,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_asset_ids: &HashSet<String>,
    declared_style_ids: &HashSet<String>,
    declared_component_ids: &HashSet<String>,
    component_local_ids: &BTreeMap<String, HashSet<String>>,
    all_node_ids: &HashSet<String>,
    page_px_bounds: Option<(f64, f64)>,
    in_flow_parent: bool,
    enclosing_frame: Option<(f64, f64, f64, f64)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── frame.child_overflow advisory ─────────────────────────────────────
    // When this node is a direct (or group-nested) child of a frame whose px
    // box resolved, advise if the child's AUTHORED bbox protrudes beyond the
    // frame box on any side. `node_bbox` returns None for flow-supplied
    // (missing) geometry, so such children are naturally skipped.
    if let Some((fx, fy, fw, fh)) = enclosing_frame
        && let Some((page_w, page_h)) = page_px_bounds
        && let Some((nx, ny, nw, nh)) = node_bbox(node, page_w, page_h)
    {
        const EPSILON: f64 = 0.5;
        let over_left = nx < fx - EPSILON;
        let over_top = ny < fy - EPSILON;
        let over_right = nx + nw > fx + fw + EPSILON;
        let over_bottom = ny + nh > fy + fh + EPSILON;
        if over_left || over_top || over_right || over_bottom {
            let (node_id, node_span) = node_id_and_span(node);
            diagnostics.push(Diagnostic::advisory(
                "frame.child_overflow",
                format!(
                    "node '{}' (bbox {nx}, {ny}, {nw}, {nh}) protrudes beyond its \
                     enclosing frame (bbox {fx}, {fy}, {fw}, {fh})",
                    node_id
                ),
                node_span,
                Some(node_id.to_owned()),
            ));
        }
    }

    // Direct children of a `layout="flow"` frame have their x/y (and, when
    // omitted, w/h) supplied by the flow algorithm, so geometry is optional.
    let geom_required = !in_flow_parent;
    // ── off_canvas advisory ───────────────────────────────────────────────
    // Check whether the node's authored bounding box exceeds the page rect
    // [0, 0, page_w, page_h]. This uses authored coordinates only — group
    // translation offsets are NOT accumulated (v0 advisory behavior; render-
    // time offset accumulation is a scene-compiler concern, not validation).
    //
    // When the node carries a non-zero `rotate` (deg), the check uses the
    // axis-aligned bounding box (AABB) of the four rotated corners instead of
    // the authored box. Unrotated nodes (no rotate or 0°) use the authored
    // box unchanged, keeping byte-identical advisory behavior for those nodes.
    if let Some((page_w, page_h)) = page_px_bounds
        && let Some((nx, ny, nw, nh)) = node_bbox(node, page_w, page_h)
    {
        // Compute the effective (ax, ay, aw, ah) used for the bounds check.
        let (ax, ay, aw, ah) = match node_rotate_deg(node) {
            Some(deg) if deg != 0.0 => {
                // Rotate the four corners of the authored bbox around its center,
                // then take the min/max to produce the rotated AABB.
                let rad = deg.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                let cx = nx + nw / 2.0;
                let cy = ny + nh / 2.0;
                // Half-extents relative to center.
                let hw = nw / 2.0;
                let hh = nh / 2.0;
                // Four corners in local space (relative to center).
                let locals: [(f64, f64); 4] = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for (lx, ly) in locals {
                    let rx = cx + lx * cos - ly * sin;
                    let ry = cy + lx * sin + ly * cos;
                    min_x = min_x.min(rx);
                    min_y = min_y.min(ry);
                    max_x = max_x.max(rx);
                    max_y = max_y.max(ry);
                }
                (min_x, min_y, max_x - min_x, max_y - min_y)
            }
            // Unrotated (or no rotate field / non-deg unit): use authored box as-is.
            _ => (nx, ny, nw, nh),
        };

        if ax < 0.0 || ay < 0.0 || ax + aw > page_w || ay + ah > page_h {
            let (node_id, node_span) = node_id_and_span(node);
            diagnostics.push(Diagnostic::advisory(
                "off_canvas",
                format!(
                    "node '{}' extends outside the page bounds (0, 0, {page_w}, {page_h})",
                    node_id
                ),
                node_span,
                Some(node_id.to_owned()),
            ));
        }
    }

    match node {
        Node::Rect(r) => {
            register_id(&r.id, seen_ids, diagnostics);
            check_style_ref(
                &r.id,
                r.style.as_deref(),
                declared_style_ids,
                r.source_span,
                diagnostics,
            );

            // Required geometry: x, y, w, h must all be present.
            check_optional_dim(
                &r.id,
                "x",
                r.x.as_ref(),
                geom_required,
                r.source_span,
                diagnostics,
            );
            check_optional_dim(
                &r.id,
                "y",
                r.y.as_ref(),
                geom_required,
                r.source_span,
                diagnostics,
            );
            check_optional_dim(
                &r.id,
                "w",
                r.w.as_ref(),
                geom_required,
                r.source_span,
                diagnostics,
            );
            check_optional_dim(
                &r.id,
                "h",
                r.h.as_ref(),
                geom_required,
                r.source_span,
                diagnostics,
            );

            // Visual properties.
            check_visual_prop(
                &r.id,
                "fill",
                r.fill.as_ref(),
                VisualExpect::ColorOrGradient,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "stroke",
                r.stroke.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "stroke-width",
                r.stroke_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "stroke-dash",
                r.stroke_dash.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = r.stroke_dash.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("rect '{}': stroke-dash must be >= 0", r.id),
                    r.source_span,
                    Some(r.id.clone()),
                ));
            }
            check_visual_prop(
                &r.id,
                "stroke-gap",
                r.stroke_gap.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = r.stroke_gap.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("rect '{}': stroke-gap must be >= 0", r.id),
                    r.source_span,
                    Some(r.id.clone()),
                ));
            }
            if let Some(lc) = r.stroke_linecap.as_deref()
                && !matches!(lc, "butt" | "round" | "square")
            {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "rect '{}': stroke-linecap '{}' is not one of butt/round/square",
                        r.id, lc
                    ),
                    r.source_span,
                    Some(r.id.clone()),
                ));
            }
            check_visual_prop(
                &r.id,
                "radius",
                r.radius.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "shadow",
                r.shadow.as_ref(),
                VisualExpect::Shadow,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in r.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "rect '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        r.id, prop_name
                    ),
                    r.source_span,
                    Some(r.id.clone()),
                ));
            }
        }

        Node::Ellipse(e) => {
            register_id(&e.id, seen_ids, diagnostics);
            check_style_ref(
                &e.id,
                e.style.as_deref(),
                declared_style_ids,
                e.source_span,
                diagnostics,
            );

            // Required geometry: x, y, w, h must all be present.
            check_optional_dim(
                &e.id,
                "x",
                e.x.as_ref(),
                geom_required,
                e.source_span,
                diagnostics,
            );
            check_optional_dim(
                &e.id,
                "y",
                e.y.as_ref(),
                geom_required,
                e.source_span,
                diagnostics,
            );
            check_optional_dim(
                &e.id,
                "w",
                e.w.as_ref(),
                geom_required,
                e.source_span,
                diagnostics,
            );
            check_optional_dim(
                &e.id,
                "h",
                e.h.as_ref(),
                geom_required,
                e.source_span,
                diagnostics,
            );

            // Visual properties.
            check_visual_prop(
                &e.id,
                "fill",
                e.fill.as_ref(),
                VisualExpect::ColorOrGradient,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &e.id,
                "stroke",
                e.stroke.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &e.id,
                "stroke-width",
                e.stroke_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &e.id,
                "stroke-dash",
                e.stroke_dash.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = e.stroke_dash.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("ellipse '{}': stroke-dash must be >= 0", e.id),
                    e.source_span,
                    Some(e.id.clone()),
                ));
            }
            check_visual_prop(
                &e.id,
                "stroke-gap",
                e.stroke_gap.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = e.stroke_gap.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("ellipse '{}': stroke-gap must be >= 0", e.id),
                    e.source_span,
                    Some(e.id.clone()),
                ));
            }
            if let Some(lc) = e.stroke_linecap.as_deref()
                && !matches!(lc, "butt" | "round" | "square")
            {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "ellipse '{}': stroke-linecap '{}' is not one of butt/round/square",
                        e.id, lc
                    ),
                    e.source_span,
                    Some(e.id.clone()),
                ));
            }
            check_visual_prop(
                &e.id,
                "shadow",
                e.shadow.as_ref(),
                VisualExpect::Shadow,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in e.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "ellipse '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        e.id, prop_name
                    ),
                    e.source_span,
                    Some(e.id.clone()),
                ));
            }
        }

        Node::Line(l) => {
            register_id(&l.id, seen_ids, diagnostics);
            check_style_ref(
                &l.id,
                l.style.as_deref(),
                declared_style_ids,
                l.source_span,
                diagnostics,
            );

            // Required geometry: x1, y1, x2, y2 must all be present.
            check_optional_dim(&l.id, "x1", l.x1.as_ref(), true, l.source_span, diagnostics);
            check_optional_dim(&l.id, "y1", l.y1.as_ref(), true, l.source_span, diagnostics);
            check_optional_dim(&l.id, "x2", l.x2.as_ref(), true, l.source_span, diagnostics);
            check_optional_dim(&l.id, "y2", l.y2.as_ref(), true, l.source_span, diagnostics);

            // Visual properties (stroke-only; no fill for line).
            // stroke is optional — only type-checked if present (a stroke-less
            // line draws nothing, but it is not an error to omit stroke).
            check_visual_prop(
                &l.id,
                "stroke",
                l.stroke.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &l.id,
                "stroke-width",
                l.stroke_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &l.id,
                "stroke-dash",
                l.stroke_dash.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = l.stroke_dash.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("line '{}': stroke-dash must be >= 0", l.id),
                    l.source_span,
                    Some(l.id.clone()),
                ));
            }
            check_visual_prop(
                &l.id,
                "stroke-gap",
                l.stroke_gap.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            if let Some(PropertyValue::Dimension(d)) = l.stroke_gap.as_ref()
                && d.value < 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "node.invalid_geometry",
                    format!("line '{}': stroke-gap must be >= 0", l.id),
                    l.source_span,
                    Some(l.id.clone()),
                ));
            }
            if let Some(lc) = l.stroke_linecap.as_deref()
                && !matches!(lc, "butt" | "round" | "square")
            {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "line '{}': stroke-linecap '{}' is not one of butt/round/square",
                        l.id, lc
                    ),
                    l.source_span,
                    Some(l.id.clone()),
                ));
            }

            // Unknown properties.
            for prop_name in l.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "line '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        l.id, prop_name
                    ),
                    l.source_span,
                    Some(l.id.clone()),
                ));
            }
        }

        Node::Text(t) => {
            register_id(&t.id, seen_ids, diagnostics);
            check_style_ref(
                &t.id,
                t.style.as_deref(),
                declared_style_ids,
                t.source_span,
                diagnostics,
            );

            // Required geometry.
            check_optional_dim(
                &t.id,
                "x",
                t.x.as_ref(),
                geom_required,
                t.source_span,
                diagnostics,
            );
            check_optional_dim(
                &t.id,
                "y",
                t.y.as_ref(),
                geom_required,
                t.source_span,
                diagnostics,
            );
            check_optional_dim(
                &t.id,
                "w",
                t.w.as_ref(),
                geom_required,
                t.source_span,
                diagnostics,
            );
            check_optional_dim(
                &t.id,
                "h",
                t.h.as_ref(),
                geom_required,
                t.source_span,
                diagnostics,
            );

            // Visual properties.
            check_visual_prop(
                &t.id,
                "fill",
                t.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "contrast-bg",
                t.contrast_bg.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-family",
                t.font_family.as_ref(),
                VisualExpect::FontFamily,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-size",
                t.font_size.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-size-min",
                t.font_size_min.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-weight",
                t.font_weight.as_ref(),
                VisualExpect::FontWeight,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "shadow",
                t.shadow.as_ref(),
                VisualExpect::Shadow,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Per-span visual properties. Spans inherit the node id as their
            // subject so token refs in `span ... fill=(token)".." font-weight=..`
            // are registered (otherwise the token is falsely flagged unused) and
            // get the same existence/type/raw-literal validation as node props.
            for span in &t.spans {
                check_visual_prop(
                    &t.id,
                    "fill",
                    span.fill.as_ref(),
                    VisualExpect::Color,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
                check_visual_prop(
                    &t.id,
                    "font-weight",
                    span.font_weight.as_ref(),
                    VisualExpect::FontWeight,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
            }

            // Text-runaround exclusion: an `text-exclusion` naming an id that
            // does not exist among the document's node ids is advisory (the
            // render proceeds with no exclusion, byte-identical to a node
            // without the attribute). Mirrors `field.unresolved_ref`.
            if let Some(target) = &t.text_exclusion
                && !all_node_ids.contains(target)
            {
                diagnostics.push(Diagnostic::warning(
                    "text-exclusion.unresolved_ref",
                    format!(
                        "text '{}': text-exclusion '{}' matches no node id in the document",
                        t.id, target
                    ),
                    t.source_span,
                    Some(t.id.clone()),
                ));
            }

            // Unknown properties.
            for prop_name in t.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "text '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        t.id, prop_name
                    ),
                    t.source_span,
                    Some(t.id.clone()),
                ));
            }
        }

        Node::Code(c) => {
            register_id(&c.id, seen_ids, diagnostics);
            check_style_ref(
                &c.id,
                c.style.as_deref(),
                declared_style_ids,
                c.source_span,
                diagnostics,
            );

            // Geometry (advisory box for v0; only unit-checked if present).
            check_optional_dim(
                &c.id,
                "x",
                c.x.as_ref(),
                geom_required,
                c.source_span,
                diagnostics,
            );
            check_optional_dim(
                &c.id,
                "y",
                c.y.as_ref(),
                geom_required,
                c.source_span,
                diagnostics,
            );
            check_optional_dim(
                &c.id,
                "w",
                c.w.as_ref(),
                geom_required,
                c.source_span,
                diagnostics,
            );
            check_optional_dim(
                &c.id,
                "h",
                c.h.as_ref(),
                geom_required,
                c.source_span,
                diagnostics,
            );

            // Visual properties (mirror text; overflow is not enum-validated,
            // matching how text.overflow is currently handled).
            check_visual_prop(
                &c.id,
                "fill",
                c.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &c.id,
                "font-family",
                c.font_family.as_ref(),
                VisualExpect::FontFamily,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &c.id,
                "font-size",
                c.font_size.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &c.id,
                "font-weight",
                c.font_weight.as_ref(),
                VisualExpect::FontWeight,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in c.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "code '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        c.id, prop_name
                    ),
                    c.source_span,
                    Some(c.id.clone()),
                ));
            }
        }

        Node::Frame(f) => {
            register_id(&f.id, seen_ids, diagnostics);
            check_style_ref(
                &f.id,
                f.style.as_deref(),
                declared_style_ids,
                f.source_span,
                diagnostics,
            );

            // Frames REQUIRE all four geometry dimensions (unlike groups).
            check_optional_dim(
                &f.id,
                "x",
                f.x.as_ref(),
                geom_required,
                f.source_span,
                diagnostics,
            );
            check_optional_dim(
                &f.id,
                "y",
                f.y.as_ref(),
                geom_required,
                f.source_span,
                diagnostics,
            );
            check_optional_dim(
                &f.id,
                "w",
                f.w.as_ref(),
                geom_required,
                f.source_span,
                diagnostics,
            );
            check_optional_dim(
                &f.id,
                "h",
                f.h.as_ref(),
                geom_required,
                f.source_span,
                diagnostics,
            );

            // Grid layout advisory: `layout="grid"` without a positive `columns`
            // defaults the scene to a single column. Non-fatal.
            if f.layout.as_deref() == Some("grid") && f.columns.unwrap_or(0) == 0 {
                diagnostics.push(Diagnostic::advisory(
                    "grid.missing_columns",
                    format!(
                        "frame '{}' uses layout=\"grid\" without a positive `columns`; \
                         defaulting to 1 column",
                        f.id
                    ),
                    f.source_span,
                    Some(f.id.clone()),
                ));
            }

            // Unknown properties.
            for prop_name in f.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "frame '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        f.id, prop_name
                    ),
                    f.source_span,
                    Some(f.id.clone()),
                ));
            }

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check. Direct
            // children of a flow OR grid frame have layout-supplied geometry,
            // so their own x/y/w/h are optional.
            let children_in_flow = matches!(f.layout.as_deref(), Some("flow") | Some("grid"));

            // Compute this frame's own px box; children are checked for
            // overflow against it. If any of x/y/w/h is missing or has a bad
            // unit, pass None so no spurious overflow advisory is produced.
            let frame_box = match page_px_bounds {
                Some((page_w, page_h)) => {
                    f.x.as_ref()
                        .and_then(|d| resolve_axis(d, page_w))
                        .zip(f.y.as_ref().and_then(|d| resolve_axis(d, page_h)))
                        .zip(f.w.as_ref().and_then(|d| resolve_axis(d, page_w)))
                        .zip(f.h.as_ref().and_then(|d| resolve_axis(d, page_h)))
                        .map(|(((x, y), w), h)| (x, y, w, h))
                }
                None => None,
            };

            for child in &f.children {
                walk_node(
                    child,
                    seen_ids,
                    referenced_token_ids,
                    resolved_tokens,
                    declared_asset_ids,
                    declared_style_ids,
                    declared_component_ids,
                    component_local_ids,
                    all_node_ids,
                    page_px_bounds,
                    children_in_flow,
                    frame_box,
                    diagnostics,
                );
            }
        }

        Node::Group(g) => {
            register_id(&g.id, seen_ids, diagnostics);
            check_style_ref(
                &g.id,
                g.style.as_deref(),
                declared_style_ids,
                g.source_span,
                diagnostics,
            );

            // Groups have NO required geometry — x/y/w/h are all advisory.

            // Unknown properties.
            for prop_name in g.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "group '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        g.id, prop_name
                    ),
                    g.source_span,
                    Some(g.id.clone()),
                ));
            }

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check. Groups do
            // not lay out children, so geometry remains required for them.
            // Groups don't clip, so the enclosing frame (if any) is propagated
            // unchanged: a group inside a frame still has the frame as the
            // clipping ancestor.
            for child in &g.children {
                walk_node(
                    child,
                    seen_ids,
                    referenced_token_ids,
                    resolved_tokens,
                    declared_asset_ids,
                    declared_style_ids,
                    declared_component_ids,
                    component_local_ids,
                    all_node_ids,
                    page_px_bounds,
                    false,
                    enclosing_frame,
                    diagnostics,
                );
            }
        }

        Node::Image(img) => {
            register_id(&img.id, seen_ids, diagnostics);
            check_style_ref(
                &img.id,
                img.style.as_deref(),
                declared_style_ids,
                img.source_span,
                diagnostics,
            );

            // Required geometry: x, y, w, h must all be present (mirror rect).
            check_optional_dim(
                &img.id,
                "x",
                img.x.as_ref(),
                geom_required,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "y",
                img.y.as_ref(),
                geom_required,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "w",
                img.w.as_ref(),
                geom_required,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "h",
                img.h.as_ref(),
                geom_required,
                img.source_span,
                diagnostics,
            );

            // src-rect: all-four-or-none rule.
            let src_present_count = [
                img.src_x.as_ref(),
                img.src_y.as_ref(),
                img.src_w.as_ref(),
                img.src_h.as_ref(),
            ]
            .iter()
            .filter(|d| d.is_some())
            .count();
            if src_present_count > 0 && src_present_count < 4 {
                diagnostics.push(Diagnostic::error(
                    "image.partial_src_rect",
                    format!(
                        "image '{}': src-x/src-y/src-w/src-h must all be present together; \
                         found {src_present_count} of 4",
                        img.id
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // src-w/src-h must be > 0 when present (and unit is resolvable to px).
            if let Some(sw) = &img.src_w
                && let Some(sw_px) = dim_to_px(sw.value, &sw.unit)
                && sw_px <= 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "image.invalid_src_rect",
                    format!("image '{}': src-w must be > 0 (got {})", img.id, sw.value,),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }
            if let Some(sh) = &img.src_h
                && let Some(sh_px) = dim_to_px(sh.value, &sh.unit)
                && sh_px <= 0.0
            {
                diagnostics.push(Diagnostic::error(
                    "image.invalid_src_rect",
                    format!("image '{}': src-h must be > 0 (got {})", img.id, sh.value,),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // Unit validation for each src-* field (required=false: partial is already caught above).
            check_optional_dim(
                &img.id,
                "src-x",
                img.src_x.as_ref(),
                false,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "src-y",
                img.src_y.as_ref(),
                false,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "src-w",
                img.src_w.as_ref(),
                false,
                img.source_span,
                diagnostics,
            );
            check_optional_dim(
                &img.id,
                "src-h",
                img.src_h.as_ref(),
                false,
                img.source_span,
                diagnostics,
            );

            // The referenced asset must exist in the document's assets block.
            if !declared_asset_ids.contains(&img.asset) {
                diagnostics.push(Diagnostic::error(
                    "asset.unknown_reference",
                    format!(
                        "image '{}': references asset '{}' which is not declared in the \
                         assets block",
                        img.id, img.asset
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // Validate fit (version-relative; forward-compat warning).
            if let Some(fit) = &img.fit
                && !matches!(fit.as_str(), "contain" | "cover" | "stretch" | "none")
            {
                diagnostics.push(Diagnostic::warning(
                    "image.invalid_fit",
                    format!(
                        "image '{}': unrecognized fit '{}' (version-relative; allowed \
                         values are contain, cover, stretch, none)",
                        img.id, fit
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // Visual properties.
            // clip-radius is a dimension token (mirror rect `radius`); only
            // meaningful for clip="rounded" but type-checked whenever present.
            check_visual_prop(
                &img.id,
                "clip-radius",
                img.clip_radius.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &img.id,
                "shadow",
                img.shadow.as_ref(),
                VisualExpect::Shadow,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in img.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "image '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        img.id, prop_name
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }
            // Image is a leaf — no child recursion.
        }

        Node::Polygon(poly) => {
            check_polygon(
                poly,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                diagnostics,
            );
        }

        Node::Polyline(poly) => {
            check_polyline(
                poly,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                diagnostics,
            );
        }

        Node::Instance(inst) => {
            check_instance(
                inst,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_component_ids,
                component_local_ids,
                diagnostics,
            );
        }

        Node::Field(field) => {
            check_field(
                field,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                all_node_ids,
                diagnostics,
            );
        }

        Node::Footnote(footnote) => {
            check_footnote(
                footnote,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                diagnostics,
            );
        }

        Node::Unknown(u) => {
            diagnostics.push(Diagnostic::warning(
                "node.unknown_kind",
                format!(
                    "unknown node kind '{}' (forward-compatibility; \
                     this kind may be valid in a later schema version)",
                    u.kind
                ),
                u.source_span,
                None,
            ));
            // Unknown nodes have no children in the v0 AST; nothing to recurse.
        }
    }
}

// ── off_canvas geometry helpers ───────────────────────────────────────────────

/// Resolve a single geometry axis dimension to pixels.
///
/// `Pct` is resolved against `basis` (e.g. page_w for x/w, page_h for y/h).
/// All other convertible units delegate to [`dim_to_px`]; `None` on failure.
fn resolve_axis(dim: &Dimension, basis: f64) -> Option<f64> {
    if dim.unit == Unit::Pct {
        Some(dim.value / 100.0 * basis)
    } else {
        dim_to_px(dim.value, &dim.unit)
    }
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
pub(super) fn node_bbox(node: &Node, page_w: f64, page_h: f64) -> Option<(f64, f64, f64, f64)> {
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
        Node::Group(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Footnote(_)
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
/// `Text`, `Code`, `Group`, `Polygon`, `Polyline`.
/// Not covered: `Line`, `Instance`, `Field`, `Footnote`, `Unknown`.
fn node_rotate_deg(node: &Node) -> Option<f64> {
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
        Node::Line(_)
        | Node::Instance(_)
        | Node::Field(_)
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
pub(super) fn node_role(node: &Node) -> Option<&str> {
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
        Node::Footnote(n) => n.role.as_deref(),
        Node::Unknown(_) => None,
    }
}

/// Extract the string id and source span from any node variant.
pub(super) fn node_id_and_span(node: &Node) -> (&str, Option<crate::ast::Span>) {
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
        Node::Footnote(n) => (&n.id, n.source_span),
        Node::Unknown(n) => (&n.kind, n.source_span),
    }
}

// ── polygon / polyline validation ─────────────────────────────────────────────

fn check_polygon(
    poly: &PolygonNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y (both must be present with a known unit).
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
    }

    // polygon requires at least 3 points.
    if poly.points.len() < 3 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polygon '{}': requires at least 3 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polygon is a LEAF: no child-node recursion (points are sub-data).
}

fn check_polyline(
    poly: &PolylineNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y.
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
    }

    // polyline requires at least 2 points.
    if poly.points.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polyline '{}': requires at least 2 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polyline is a LEAF: no child-node recursion (points are sub-data).
}

// ── instance validation ───────────────────────────────────────────────────────

/// Validate an `instance` node:
/// - its own `id` participates in GLOBAL uniqueness;
/// - `component` must reference a declared component → else
///   `component.unknown_reference` (Error);
/// - each override `ref` must match a LOCAL descendant id of the referenced
///   component → else `component.unknown_override_target` (Warning).
///
/// The instance is a container-ish node but it does NOT recurse here: its
/// expanded subtree (and the component definition's own ids) are validated at
/// the component definition site, not per-instance, so token/asset refs are
/// checked once.
fn check_instance(
    inst: &InstanceNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_component_ids: &HashSet<String>,
    component_local_ids: &BTreeMap<String, HashSet<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&inst.id, seen_ids, diagnostics);

    let component_known = declared_component_ids.contains(&inst.component);
    if !component_known {
        diagnostics.push(Diagnostic::error(
            "component.unknown_reference",
            format!(
                "instance '{}': references component '{}' which is not declared in the \
                 components block",
                inst.id, inst.component
            ),
            inst.source_span,
            Some(inst.id.clone()),
        ));
    }

    // Override targets are only checkable when the component is known. Look up the
    // referenced component's local-id set; an override `ref` that matches no local
    // descendant id → warning.
    let local_ids = component_local_ids.get(&inst.component);
    for ov in &inst.overrides {
        // Validate (and register as referenced) any token refs the override
        // carries, so an override-only token is not falsely flagged unused and
        // a bad override fill/span fill is type-checked like a node fill.
        check_visual_prop(
            &inst.id,
            "fill",
            ov.fill.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        if let Some(spans) = &ov.spans {
            for span in spans {
                check_visual_prop(
                    &inst.id,
                    "fill",
                    span.fill.as_ref(),
                    VisualExpect::Color,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
            }
        }

        let target_known = local_ids
            .map(|ids| ids.contains(&ov.ref_id))
            .unwrap_or(false);
        if component_known && !target_known {
            diagnostics.push(Diagnostic::warning(
                "component.unknown_override_target",
                format!(
                    "instance '{}': override ref '{}' matches no descendant id in component '{}'",
                    inst.id, ov.ref_id, inst.component
                ),
                ov.source_span.or(inst.source_span),
                Some(inst.id.clone()),
            ));
        }
    }

    // Unknown properties on the instance node.
    for prop_name in inst.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "instance '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                inst.id, prop_name
            ),
            inst.source_span,
            Some(inst.id.clone()),
        ));
    }
}

// ── field validation ──────────────────────────────────────────────────────────

/// The known v0 field types.
const KNOWN_FIELD_TYPES: &[&str] = &["running-head", "page-number", "page-ref", "page-count"];

/// Validate a `field` node:
/// - its own `id` participates in GLOBAL uniqueness;
/// - `type` must be one of the known field types → else `field.unknown_type`
///   (Warning);
/// - a `page-ref` field whose `target` matches no node id anywhere in the
///   document → `field.unresolved_ref` (Warning);
/// - `style`/`fill`/`font-family`/`font-size` are validated like a text node's,
///   and any token refs are registered so they are not flagged unused.
///
/// A field is a leaf — it does not recurse. Geometry is optional (an absent
/// x/w defaults to the page live area at compile time), so no missing-geometry
/// error is raised here.
fn check_field(
    field: &FieldNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    all_node_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&field.id, seen_ids, diagnostics);
    check_style_ref(
        &field.id,
        field.style.as_deref(),
        declared_style_ids,
        field.source_span,
        diagnostics,
    );

    // Unknown field type → Warning (never a hard error; the field simply renders
    // nothing at compile time).
    if !KNOWN_FIELD_TYPES.contains(&field.field_type.as_str()) {
        diagnostics.push(Diagnostic::warning(
            "field.unknown_type",
            format!(
                "field '{}': unknown type '{}'; expected one of {}",
                field.id,
                field.field_type,
                KNOWN_FIELD_TYPES.join(", ")
            ),
            field.source_span,
            Some(field.id.clone()),
        ));
    }

    // A page-ref field with an unresolvable target → Warning. A page-ref with no
    // target at all is also unresolved (nothing to point at).
    if field.field_type == "page-ref" {
        let resolved = field
            .target
            .as_ref()
            .map(|t| all_node_ids.contains(t))
            .unwrap_or(false);
        if !resolved {
            diagnostics.push(Diagnostic::warning(
                "field.unresolved_ref",
                format!(
                    "field '{}': page-ref target {} matches no node id in the document",
                    field.id,
                    field
                        .target
                        .as_deref()
                        .map(|t| format!("'{t}'"))
                        .unwrap_or_else(|| "(absent)".to_owned())
                ),
                field.source_span,
                Some(field.id.clone()),
            ));
        }
    }

    // Visual properties (mirror the text-node checks).
    check_visual_prop(
        &field.id,
        "fill",
        field.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &field.id,
        "font-family",
        field.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &field.id,
        "font-size",
        field.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Unknown properties on the field node.
    for prop_name in field.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "field '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                field.id, prop_name
            ),
            field.source_span,
            Some(field.id.clone()),
        ));
    }
}

/// Validate a [`FootnoteNode`]: id uniqueness, style ref, the content-span and
/// node visual properties (fill/font-family/font-size, plus per-span fill/weight
/// so raw visual literals are surfaced like any text), and unknown properties.
///
/// The structural `footnote.unresolved_ref` check (a span `footnote-ref` that
/// names no footnote on the same page) is done at the PAGE level (it needs the
/// page's footnote-id set), not here. A footnote has no geometry, so there are
/// no geometry checks.
fn check_footnote(
    footnote: &FootnoteNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&footnote.id, seen_ids, diagnostics);
    check_style_ref(
        &footnote.id,
        footnote.style.as_deref(),
        declared_style_ids,
        footnote.source_span,
        diagnostics,
    );

    check_visual_prop(
        &footnote.id,
        "fill",
        footnote.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &footnote.id,
        "font-family",
        footnote.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &footnote.id,
        "font-size",
        footnote.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Per-span visual props (mirror the text-node span checks) so token refs are
    // registered and raw visual literals are flagged `token.raw_visual_literal`.
    for span in &footnote.spans {
        check_visual_prop(
            &footnote.id,
            "fill",
            span.fill.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            &footnote.id,
            "font-weight",
            span.font_weight.as_ref(),
            VisualExpect::FontWeight,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }

    for prop_name in footnote.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "footnote '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                footnote.id, prop_name
            ),
            footnote.source_span,
            Some(footnote.id.clone()),
        ));
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Check a single optional geometry dimension (`x`, `y`, `w`, `h`):
/// - absent AND `required` → `node.missing_geometry` (Error).
/// - absent AND NOT `required` (e.g. a direct child of a `layout="flow"`
///   frame, whose position/size is supplied by the flow algorithm) → no
///   diagnostic.
/// - present but `Unit::Unknown` → `node.invalid_geometry` (Error) regardless
///   of `required`.
fn check_optional_dim(
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

/// Check that a node's `style` attribute references a declared style id.
///
/// Called for every node kind that carries a `style` field.
fn check_style_ref(
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
