//! Per-node validation: the recursive [`walk_node`] dispatcher and the
//! walk-wide context/position types.
//!
//! The dispatcher runs the shared prologue advisories (frame.child_overflow,
//! off_canvas with rotate-AABB), computes the per-node `geom_required` gate,
//! then dispatches to the per-kind `check_*` helpers in [`node`] and performs
//! the container (frame/group/table/unknown) recursion. All per-kind
//! validation logic lives in the [`node`] submodules.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::node::Node;
use crate::diagnostics::Diagnostic;
use crate::tokens::ResolvedToken;

use node::shared::{node_rotate_deg, pv_to_dim, resolve_axis};

mod node;

pub(super) use node::shared::{
    AnchorParentCtx, check_sibling_anchors, node_bbox, node_id_and_span, node_role,
};

/// Walk-wide immutable validation context (never changes during a page walk).
#[derive(Clone, Copy)]
pub(super) struct WalkCtx<'a> {
    pub(super) resolved_tokens: &'a BTreeMap<String, ResolvedToken>,
    pub(super) declared_asset_ids: &'a BTreeSet<String>,
    pub(super) declared_style_ids: &'a BTreeSet<String>,
    pub(super) declared_component_ids: &'a BTreeSet<String>,
    pub(super) component_local_ids: &'a BTreeMap<String, BTreeSet<String>>,
    pub(super) all_node_ids: &'a BTreeSet<String>,
    pub(super) zone_ids: &'a BTreeSet<&'a str>,
}

/// Per-recursion position state (changes as the walk descends frames/groups).
#[derive(Clone, Copy)]
pub(super) struct WalkPos {
    pub(super) page_px_bounds: Option<(f64, f64)>,
    pub(super) in_flow_parent: bool,
    pub(super) enclosing_frame: Option<(f64, f64, f64, f64)>,
    /// `true` when this node is a direct (or group-nested) child of a
    /// `frame`/`group` — the anchor-parent container context.
    pub(super) in_container: bool,
    /// `true` when the enclosing container's reference box is usable (frame:
    /// always; group: only when it declares both `w` and `h`).
    pub(super) parent_box_known: bool,
}

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
pub(super) fn walk_node(
    node: &Node,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    pos: WalkPos,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── frame.child_overflow advisory ─────────────────────────────────────
    // When this node is a direct (or group-nested) child of a frame whose px
    // box resolved, advise if the child's AUTHORED bbox protrudes beyond the
    // frame box on any side. `node_bbox` returns None for flow-supplied
    // (missing) geometry, so such children are naturally skipped.
    if let Some((fx, fy, fw, fh)) = pos.enclosing_frame
        && let Some((page_w, page_h)) = pos.page_px_bounds
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
    let geom_required = !pos.in_flow_parent;
    // Parent-relative anchor context for this node: whether it sits inside a
    // frame/group container and whether that container's reference box is usable.
    let parent_ctx = AnchorParentCtx {
        in_container: pos.in_container,
        parent_box_known: pos.parent_box_known,
    };
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
    if let Some((page_w, page_h)) = pos.page_px_bounds
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
                "layout.off_canvas",
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
            node::check_rect(
                r,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Ellipse(e) => {
            node::check_ellipse(
                e,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Line(l) => {
            node::check_line(l, ctx, seen_ids, referenced_token_ids, diagnostics);
        }
        Node::Text(t) => {
            node::check_text(
                t,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Code(c) => {
            node::check_code(
                c,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Image(img) => {
            node::check_image(
                img,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Shape(s) => {
            node::check_shape(
                s,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Pattern(p) => {
            // The pattern is validated as a leaf; its motif is a TEMPLATE, so its
            // own ids must not enter the uniqueness set and its validity is owned
            // by the compile-time probe — hence id-collection / motif diagnostics
            // are not done here.
            node::check_pattern(
                p,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
            // The motif still RENDERS (replicated once per instance), so tokens
            // referenced only inside it are genuinely used. Walk it into throwaway
            // id/diagnostic sinks to collect those token references — without
            // registering the template's ids or re-surfacing its diagnostics.
            let mut motif_seen: BTreeSet<String> = BTreeSet::new();
            let mut motif_diags: Vec<Diagnostic> = Vec::new();
            walk_node(
                &p.motif,
                ctx,
                &mut motif_seen,
                referenced_token_ids,
                pos,
                &mut motif_diags,
            );
        }
        Node::Chart(c) => {
            // The chart is validated as a leaf; its series children are pure DATA
            // (not renderable nodes), so they are never descended into here.
            node::check_chart(
                c,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Light(l) => {
            node::check_light(
                l,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                diagnostics,
            );
        }
        Node::Mesh(m) => {
            node::check_mesh(
                m,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                diagnostics,
            );
        }
        Node::Polygon(poly) => {
            node::check_polygon(poly, ctx, seen_ids, referenced_token_ids, diagnostics);
        }
        Node::Polyline(poly) => {
            node::check_polyline(poly, ctx, seen_ids, referenced_token_ids, diagnostics);
        }
        Node::Instance(inst) => {
            node::check_instance(inst, ctx, seen_ids, referenced_token_ids, diagnostics);
        }
        Node::Field(field) => {
            node::check_field(
                field,
                ctx,
                seen_ids,
                referenced_token_ids,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Toc(toc) => {
            node::check_toc(
                toc,
                ctx,
                seen_ids,
                referenced_token_ids,
                parent_ctx,
                diagnostics,
            );
        }
        Node::Footnote(footnote) => {
            node::check_footnote(footnote, ctx, seen_ids, referenced_token_ids, diagnostics);
        }
        Node::Connector(c) => {
            node::check_connector(c, ctx, seen_ids, referenced_token_ids, diagnostics);
        }

        Node::Frame(f) => {
            node::check_frame(
                f,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check. Direct
            // children of a flow OR grid frame have layout-supplied geometry,
            // so their own x/y/w/h are optional.
            let children_in_flow = matches!(f.layout.as_deref(), Some("flow") | Some("grid"));

            // Compute this frame's own px box; children are checked for
            // overflow against it. If any of x/y/w/h is missing or has a bad
            // unit, pass None so no spurious overflow advisory is produced.
            let frame_box = match pos.page_px_bounds {
                Some((page_w, page_h)) => pv_to_dim(f.x.as_ref())
                    .and_then(|d| resolve_axis(d, page_w))
                    .zip(pv_to_dim(f.y.as_ref()).and_then(|d| resolve_axis(d, page_h)))
                    .zip(pv_to_dim(f.w.as_ref()).and_then(|d| resolve_axis(d, page_w)))
                    .zip(pv_to_dim(f.h.as_ref()).and_then(|d| resolve_axis(d, page_h)))
                    .map(|(((x, y), w), h)| (x, y, w, h)),
                None => None,
            };

            // Validate this frame's sibling-anchor graph (one scope = its
            // direct children) once, before descending.
            check_sibling_anchors(&f.children, diagnostics);

            for child in &f.children {
                walk_node(
                    child,
                    ctx,
                    seen_ids,
                    referenced_token_ids,
                    WalkPos {
                        page_px_bounds: pos.page_px_bounds,
                        in_flow_parent: children_in_flow,
                        enclosing_frame: frame_box,
                        // A frame is always an anchor-parent container with a
                        // usable box (its geometry is required + validated).
                        in_container: true,
                        parent_box_known: true,
                    },
                    diagnostics,
                );
            }
        }

        Node::Group(g) => {
            node::check_group(
                g,
                ctx,
                seen_ids,
                referenced_token_ids,
                parent_ctx,
                diagnostics,
            );

            // A group is an anchor-parent container; its reference box is
            // usable only when it declares both `w` and `h`.
            let group_box_known = g.w.is_some() && g.h.is_some();

            // Validate this group's sibling-anchor graph (one scope = its
            // direct children) once, before descending.
            check_sibling_anchors(&g.children, diagnostics);

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check. Groups do
            // not lay out children, so geometry remains required for them.
            // Groups don't clip, so the enclosing frame (if any) is propagated
            // unchanged: a group inside a frame still has the frame as the
            // clipping ancestor.
            for child in &g.children {
                walk_node(
                    child,
                    ctx,
                    seen_ids,
                    referenced_token_ids,
                    WalkPos {
                        page_px_bounds: pos.page_px_bounds,
                        in_flow_parent: false,
                        enclosing_frame: pos.enclosing_frame,
                        in_container: true,
                        parent_box_known: group_box_known,
                    },
                    diagnostics,
                );
            }
        }

        Node::Table(t) => {
            node::check_table(
                t,
                ctx,
                seen_ids,
                referenced_token_ids,
                geom_required,
                parent_ctx,
                diagnostics,
            );

            // Recurse into every cell's children with the normal walk so nested
            // node ids are registered/validated. A table cell positions and
            // sizes its children (auto-box/wrap/align), exactly like a
            // `frame layout="grid"/"flow"`, so cell children are flow-positioned
            // and their x/y/w/h are OPTIONAL.
            for row in &t.rows {
                for cell in &row.cells {
                    for child in &cell.children {
                        walk_node(
                            child,
                            ctx,
                            seen_ids,
                            referenced_token_ids,
                            WalkPos {
                                page_px_bounds: pos.page_px_bounds,
                                in_flow_parent: true,
                                enclosing_frame: pos.enclosing_frame,
                                // A table cell is NOT an anchor-parent
                                // container; its children's direct parent is the
                                // cell, so anchor-parent there is unresolvable.
                                in_container: false,
                                parent_box_known: false,
                            },
                            diagnostics,
                        );
                    }
                }
            }
        }

        Node::Unknown(u) => {
            node::check_unknown(u, seen_ids, diagnostics);

            // Recurse into children so nested KNOWN nodes (e.g. a `rect` inside
            // an unknown parent) are still validated for token refs, duplicate
            // ids, etc. The unknown parent's layout semantics are unknown, so
            // children must NOT trigger `node.missing_geometry`: pass
            // `in_flow_parent = true` to make their geometry optional.
            for child in &u.children {
                walk_node(
                    child,
                    ctx,
                    seen_ids,
                    referenced_token_ids,
                    WalkPos {
                        page_px_bounds: pos.page_px_bounds,
                        in_flow_parent: true,
                        enclosing_frame: pos.enclosing_frame,
                        // An unknown parent is not a known anchor-parent container.
                        in_container: false,
                        parent_box_known: false,
                    },
                    diagnostics,
                );
            }
        }
    }
}
