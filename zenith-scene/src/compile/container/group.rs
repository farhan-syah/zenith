//! `group` container compilation: translate + opacity cascade with optional
//! rotation / blend / blur brackets, plus the bounding-box helpers that
//! determine a group's rotation pivot.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, GroupNode, Node, Point, ResolvedToken, dim_to_px};

use crate::ir::SceneCommand;

use super::super::util::{blend_mode_ir, resolve_geometry_px, rotation_degrees};
use super::super::{NodeCtx, RenderCtx, compile_node};

// NOTE: compile_group → compile_node → compile_group recursion has no depth
// guard.  Pathologically deep group trees can overflow the stack.  This is a
// known v0 limitation; a guard will be added when nested documents are tested.
pub(in crate::compile) fn compile_group(
    group: &GroupNode,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    connector_strokes: &mut Vec<usize>,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false.
    if group.visible == Some(false) {
        return;
    }

    // Cascade opacity: multiply the group's own opacity into the inherited ctx.
    // With a non-normal blend the group's opacity instead rides the PushLayer
    // (emitted after the rotation push below) and children inherit `ctx.opacity`
    // unmultiplied, so opacity is applied exactly once. No-blend path unchanged.
    let group_opacity = group.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let blend = blend_mode_ir(group.blend_mode.as_deref());
    let child_opacity = match blend {
        Some(_) => ctx.opacity,
        None => ctx.opacity * group_opacity,
    };

    // Resolve group x/y to pixels; absent or unsupported-unit → 0.0 (no diagnostic).
    let group_x_px = resolve_geometry_px(group.x.as_ref(), cx.resolved).unwrap_or(0.0);
    let group_y_px = resolve_geometry_px(group.y.as_ref(), cx.resolved).unwrap_or(0.0);

    let child_dx = ctx.dx + group_x_px;
    let child_dy = ctx.dy + group_y_px;

    // Rotation bracket — outermost, wrapping all child commands.
    // Determine the pivot center:
    //   1. If the group has BOTH w and h declared → use the declared box center.
    //   2. Otherwise → compute the union bbox of direct children in device space.
    //   3. If neither yields a center (empty / geometry-less group) → skip
    //      the bracket entirely (v0 limitation, commented below).
    let group_rot = rotation_degrees(group.rotate.as_ref());
    let rot_center: Option<(f64, f64)> = if group_rot.is_some() {
        // Try declared box first.
        let declared = resolve_geometry_px(group.w.as_ref(), cx.resolved)
            .zip(resolve_geometry_px(group.h.as_ref(), cx.resolved))
            .map(|(gw, gh)| (child_dx + gw / 2.0, child_dy + gh / 2.0));
        if declared.is_some() {
            declared
        } else {
            // Fall back to union bbox of direct children in device space.
            // v0 limitation: if the group is empty or contains only
            // geometry-less nodes no center is computable → rotation is
            // silently skipped.
            group_children_center(&group.children, child_dx, child_dy, cx.resolved)
        }
    } else {
        None
    };

    if let (Some(angle), Some((cx_pivot, cy_pivot))) = (group_rot, rot_center) {
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx: cx_pivot,
            cy: cy_pivot,
        });
    }

    // Blend-mode layer bracket (inside the rotation, around all children). The
    // layer composites back with the group's full opacity cascade.
    if let Some(blend_mode) = blend {
        commands.push(SceneCommand::PushLayer {
            opacity: ctx.opacity * group_opacity,
            blend_mode: Some(blend_mode),
        });
    }

    // BLUR bracket (inside blend, around all children). The entire group ink
    // (all children composited) is blurred as a unit.
    let blur_sigma = group
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let has_blur = blur_sigma.is_some();
    if let Some(sigma) = blur_sigma {
        commands.push(SceneCommand::BeginBlur { radius: sigma });
    }

    // Emit children in source order; the group itself produces no command.
    let child_ctx = RenderCtx {
        opacity: child_opacity,
        dx: child_dx,
        dy: child_dy,
        // Page baseline grid cascades unchanged so all text shares one grid.
        baseline_grid: ctx.baseline_grid,
    };
    for child in &group.children {
        compile_node(
            child,
            cx,
            commands,
            diagnostics,
            connector_strokes,
            child_ctx,
        );
    }

    if has_blur {
        commands.push(SceneCommand::EndBlur);
    }

    if blend.is_some() {
        commands.push(SceneCommand::PopLayer);
    }

    if group_rot.is_some() && rot_center.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Compute the axis-aligned bounding box of a `Point` list in authored coords.
///
/// Returns `(x_min, y_min, w, h)` in authored (pre-`base_dx`/`base_dy`) space,
/// or `None` when the list is empty or every point has a missing / unsupported-
/// unit coordinate.  Used by both the `Polygon` and `Polyline` arms of
/// [`group_children_center`] to avoid duplicating the accumulation loop.
fn points_bbox(pts: &[Point]) -> Option<(f64, f64, f64, f64)> {
    let mut px_min = f64::INFINITY;
    let mut py_min = f64::INFINITY;
    let mut px_max = f64::NEG_INFINITY;
    let mut py_max = f64::NEG_INFINITY;
    for pt in pts {
        let (Some(xd), Some(yd)) = (&pt.x, &pt.y) else {
            continue;
        };
        let (Some(px), Some(py)) = (dim_to_px(xd.value, &xd.unit), dim_to_px(yd.value, &yd.unit))
        else {
            continue;
        };
        px_min = px_min.min(px);
        py_min = py_min.min(py);
        px_max = px_max.max(px);
        py_max = py_max.max(py);
    }
    if px_min.is_finite() {
        Some((px_min, py_min, px_max - px_min, py_max - py_min))
    } else {
        None
    }
}

/// Compute the device-space center of a group's direct-child union bounding box.
///
/// `base_dx`/`base_dy` are the device-space origin of the group (i.e.
/// `ctx.dx + group_x_px` and `ctx.dy + group_y_px`).  Children are positioned
/// relative to those origins.
///
/// Returns `None` when no child yields a computable bbox (e.g. an empty group
/// or one containing only unknown/geometry-less nodes).
fn group_children_center(
    children: &[Node],
    base_dx: f64,
    base_dy: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> Option<(f64, f64)> {
    // Accumulate min/max in device space.
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for child in children {
        // Helper: expand accumulated bounds by a device-space rect.
        macro_rules! expand {
            ($x:expr, $y:expr, $w:expr, $h:expr) => {
                if $w > 0.0 || $h > 0.0 {
                    min_x = min_x.min($x);
                    min_y = min_y.min($y);
                    max_x = max_x.max($x + $w);
                    max_y = max_y.max($y + $h);
                }
            };
        }

        match child {
            Node::Rect(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Ellipse(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Text(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Code(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Image(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Frame(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Line(n) => {
                // Line bbox is the degenerate rect spanning (x1,y1)-(x2,y2).
                let (Some(x1d), Some(y1d), Some(x2d), Some(y2d)) = (&n.x1, &n.y1, &n.x2, &n.y2)
                else {
                    continue;
                };
                let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    dim_to_px(x1d.value, &x1d.unit),
                    dim_to_px(y1d.value, &y1d.unit),
                    dim_to_px(x2d.value, &x2d.unit),
                    dim_to_px(y2d.value, &y2d.unit),
                ) else {
                    continue;
                };
                let lx = x1.min(x2);
                let ly = y1.min(y2);
                let lw = (x2 - x1).abs();
                let lh = (y2 - y1).abs();
                expand!(base_dx + lx, base_dy + ly, lw, lh);
            }
            Node::Polygon(n) => {
                if let Some((x, y, w, h)) = points_bbox(&n.points) {
                    expand!(base_dx + x, base_dy + y, w, h);
                }
            }
            Node::Polyline(n) => {
                if let Some((x, y, w, h)) = points_bbox(&n.points) {
                    expand!(base_dx + x, base_dy + y, w, h);
                }
            }
            Node::Group(n) => {
                // Nested group: use its declared w/h if available, else skip.
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Table(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Shape(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(wd), resolved),
                    resolve_geometry_px(Some(hd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Light(n) => {
                let (Some(xd), Some(yd), Some(rd)) = (&n.x, &n.y, &n.radius) else {
                    continue;
                };
                let (Some(x), Some(y), Some(r)) = (
                    resolve_geometry_px(Some(xd), resolved),
                    resolve_geometry_px(Some(yd), resolved),
                    resolve_geometry_px(Some(rd), resolved),
                ) else {
                    continue;
                };
                expand!(base_dx + x - r, base_dy + y - r, r * 2.0, r * 2.0);
            }
            // Instances have no authoritative bbox (their expanded subtree is
            // the geometry); a field's/toc's box is resolved at projection time,
            // not here; unknown nodes have no geometry — skip all.
            // A footnote has no authored bbox (it renders in the footnote zone).
            Node::Instance(_)
            | Node::Field(_)
            | Node::Toc(_)
            | Node::Footnote(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Unknown(_) => {}
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(((min_x + max_x) / 2.0, (min_y + max_y) / 2.0))
    } else {
        None
    }
}
