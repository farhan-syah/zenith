//! Vector leaf-node compilation: rect, ellipse, line, polygon, and polyline.
//!
//! Each function mirrors the shared leaf signature
//! `(node, resolved, style_map, commands, diagnostics, ctx)` and emits the same
//! `SceneCommand` stream that the original inline `compile_node` arms produced.

use std::collections::BTreeMap;

use zenith_core::{
    ConnectorNode, Diagnostic, EllipseNode, FontProvider, LineNode, Point, PolygonNode,
    PolylineNode, PropertyValue, RectNode, ResolvedToken, ShapeNode, Span, Style, TextNode,
    dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::{LineCap, SceneCommand, StrokeAlign};

use super::RenderCtx;
use super::anchor::AnchorMap;
use super::chain::ChainAssignments;
use super::paint::{
    NodeEffect, apply_gradient_opacity, emit_node_with_effects, resolve_property_color,
    resolve_property_filter, resolve_property_gradient, resolve_property_mask,
    resolve_property_shadow,
};
use super::style_prop;
use super::text::{MeasureEnv, compile_text, measure_text_wrapped_height, resolve_text_families};
use super::util::{
    blend_mode_ir, missing_geometry_diag, px, resolve_anchored_axis, resolve_property_dimension_px,
    rotation_degrees, unsupported_unit_diag,
};

/// Resolve dashed-stroke parameters from raw node fields.
///
/// Returns `(stroke_dash, stroke_gap, stroke_linecap)`:
/// - `stroke_dash`/`stroke_gap` are `None` when dash is absent or `<= 0`
///   (solid stroke, byte-identical to prior behavior).
/// - `stroke_linecap` is `None` (Butt default) when dash is absent.
fn resolve_dash_params(
    dash_prop: &Option<PropertyValue>,
    gap_prop: &Option<PropertyValue>,
    linecap_str: Option<&str>,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> (Option<f64>, Option<f64>, Option<LineCap>) {
    let dash_px = resolve_property_dimension_px(dash_prop, resolved, -1.0);
    let gap_px = resolve_property_dimension_px(gap_prop, resolved, -1.0);
    let (stroke_dash, stroke_gap) = if dash_px > 0.0 {
        let g = if gap_px >= 0.0 { gap_px } else { dash_px };
        (Some(dash_px), Some(g))
    } else {
        (None, None)
    };
    let stroke_linecap = linecap_str.map(|s| match s {
        "round" => LineCap::Round,
        "square" => LineCap::Square,
        _ => LineCap::Butt,
    });
    // Only emit linecap when dash is active (solid strokes ignore it).
    let stroke_linecap = stroke_dash.and(stroke_linecap);
    (stroke_dash, stroke_gap, stroke_linecap)
}

/// Compile a `rect` leaf node.
pub(super) fn compile_rect(
    rect: &RectNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    anchors: &AnchorMap,
    ctx: RenderCtx,
) {
    // Skip invisible rects.
    if rect.visible == Some(false) {
        return;
    }

    // Resolve geometry — w and h are always required. x and y may be
    // derived from a page-relative anchor when absent.
    let (Some(w_dim), Some(h_dim)) = (&rect.w, &rect.h) else {
        diagnostics.push(missing_geometry_diag("rect", &rect.id, rect.source_span));
        return;
    };
    let Some(w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "rect",
            &rect.id,
            "w",
            rect.source_span,
        ));
        return;
    };
    let Some(h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "rect",
            &rect.id,
            "h",
            rect.source_span,
        ));
        return;
    };

    // Anchor-derived (x, y): look up the pre-pass map when x or y is absent.
    let anchor_xy = anchors.get(&rect.id).copied();

    let Some(x_raw) = resolve_anchored_axis(
        "rect",
        &rect.id,
        "x",
        rect.x.as_ref(),
        anchor_xy.map(|(ax, _)| ax),
        rect.source_span,
        diagnostics,
    ) else {
        return;
    };
    let Some(y_raw) = resolve_anchored_axis(
        "rect",
        &rect.id,
        "y",
        rect.y.as_ref(),
        anchor_xy.map(|(_, ay)| ay),
        rect.source_span,
        diagnostics,
    ) else {
        return;
    };

    // Apply group translation offset.
    let x = x_raw + ctx.dx;
    let y = y_raw + ctx.dy;

    // Apply node opacity then cascade ctx.opacity on top.
    let node_opacity = rect.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

    // Blend-mode layer. When the node specifies a non-normal blend, the whole
    // opacity cascade (`node_opacity * ctx.opacity`) rides on the PushLayer and
    // is applied once, at PopLayer; the colors are then emitted at full alpha
    // (`color_op == 1.0`) so opacity is not double-counted. With no blend (the
    // common case) the layer is absent and `color_op` keeps the prior
    // `node_opacity * ctx.opacity`, leaving the command stream byte-identical.
    let blend = blend_mode_ir(rect.blend_mode.as_deref());
    let (layer_op, color_op) = match blend {
        Some(_) => (node_opacity * ctx.opacity, 1.0),
        None => (1.0, node_opacity * ctx.opacity),
    };

    // Resolve corner radius (optional; 0.0 when absent). Node-local
    // overrides style.
    let radius_prop = rect
        .radius
        .clone()
        .or_else(|| style_prop(&rect.style, style_map, "radius").cloned());
    let radius = resolve_property_dimension_px(&radius_prop, resolved, 0.0);

    // Per-corner radius overrides. When any corner prop is present, build the
    // `radii` array (each corner falls back to uniform `radius`). When NO
    // corner prop is present, `radii` stays `None` → byte-identical to before.
    let has_corner_props = rect.radius_tl.is_some()
        || rect.radius_tr.is_some()
        || rect.radius_br.is_some()
        || rect.radius_bl.is_some();
    let radii: Option<[f64; 4]> = if has_corner_props {
        let tl = rect.radius_tl.as_ref().map_or(radius, |p| {
            resolve_property_dimension_px(&Some(p.clone()), resolved, radius)
        });
        let tr = rect.radius_tr.as_ref().map_or(radius, |p| {
            resolve_property_dimension_px(&Some(p.clone()), resolved, radius)
        });
        let br = rect.radius_br.as_ref().map_or(radius, |p| {
            resolve_property_dimension_px(&Some(p.clone()), resolved, radius)
        });
        let bl = rect.radius_bl.as_ref().map_or(radius, |p| {
            resolve_property_dimension_px(&Some(p.clone()), resolved, radius)
        });
        Some([tl, tr, br, bl])
    } else {
        None
    };

    // A rect is "rounded" when the uniform radius or any per-corner override > 0.
    let is_rounded = radius > 0.0 || radii.as_ref().is_some_and(|a| a.iter().any(|&v| v > 0.0));

    // Rotation bracket (outermost). PushTransform is only emitted when
    // rotate is non-zero; unrotated rects are byte-identical to before.
    let rot = rotation_degrees(rect.rotate.as_ref());
    if let Some(angle) = rot {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // BLEND-MODE layer bracket (inside the rotation, outside the shadow). Only
    // emitted for a non-normal blend; the matching PopLayer rides the arm tail.
    if let Some(blend_mode) = blend {
        commands.push(SceneCommand::PushLayer {
            opacity: layer_op,
            blend_mode: Some(blend_mode),
        });
    }

    // BLUR / SHADOW / FILTER effect (innermost, behind fill+stroke). Blur wins
    // over shadow over filter when several are set: at most one effect is chosen.
    // The single winning effect plus the optional mask bracket the node's DRAWS
    // (FILL + STROKE), emitted via `emit_node_with_effects` below; the
    // stroke-outer / per-side borders are NOT part of the masked/effected draws
    // (they land after the effect bracket, exactly as before).
    let blur_sigma = rect
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let effect: Option<NodeEffect> = if let Some(sigma) = blur_sigma {
        Some(NodeEffect::Blur(sigma))
    } else if let Some(shadows) = rect
        .shadow
        .as_ref()
        .and_then(|p| resolve_property_shadow(p, resolved, &rect.id))
    {
        Some(NodeEffect::Shadow(shadows))
    } else {
        rect.filter
            .as_ref()
            .and_then(|p| resolve_property_filter(p, resolved, &rect.id))
            .map(NodeEffect::Filter)
    };

    // Resolve the optional node mask against the rect's page-absolute box.
    let mask = rect
        .mask
        .as_ref()
        .and_then(|p| resolve_property_mask(p, resolved, (x, y, w, h)));

    // Collect the node's DRAW commands (fill + stroke) into a local buffer so
    // the shared helper can bracket them with the effect and/or mask. With no
    // effect and no mask the helper extends `commands` verbatim → byte-identical.
    let mut draws: Vec<SceneCommand> = Vec::new();

    // FILL (emitted first, under the stroke) — node-local prop overrides
    // style cascade.
    let fill_prop = rect
        .fill
        .as_ref()
        .or_else(|| style_prop(&rect.style, style_map, "fill"));
    if let Some(fill_prop) = fill_prop {
        if let Some(mut gradient) = resolve_property_gradient(fill_prop, resolved, &rect.id) {
            apply_gradient_opacity(&mut gradient, color_op, 1.0);
            if is_rounded {
                draws.push(SceneCommand::FillRoundedRectGradient {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    radii,
                    gradient,
                });
            } else {
                draws.push(SceneCommand::FillRectGradient {
                    x,
                    y,
                    w,
                    h,
                    gradient,
                });
            }
        } else if let Some(mut color) =
            resolve_property_color(fill_prop, resolved, diagnostics, &rect.id)
        {
            color.a = (color.a as f64 * color_op).round() as u8;
            if is_rounded {
                draws.push(SceneCommand::FillRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    radii,
                    color,
                });
            } else {
                draws.push(SceneCommand::FillRect { x, y, w, h, color });
            }
        }
    }

    // STROKE (emitted on top of the fill) — node-local prop overrides
    // style cascade.
    let stroke_prop = rect
        .stroke
        .as_ref()
        .or_else(|| style_prop(&rect.style, style_map, "stroke"));
    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &rect.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        let sw = rect
            .stroke_width
            .clone()
            .or_else(|| style_prop(&rect.style, style_map, "stroke-width").cloned());
        let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);

        // Resolve dashed stroke parameters.
        let (stroke_dash, stroke_gap, stroke_linecap) = resolve_dash_params(
            &rect.stroke_dash,
            &rect.stroke_gap,
            rect.stroke_linecap.as_deref(),
            resolved,
        );

        // Stroke alignment offsets the stroke path relative to the box
        // edge by half the stroke width. `center` (default) straddles the
        // edge; `inside`/`outside` shift the whole stroked rectangle in or
        // out. The fill geometry above is unaffected.
        let half = stroke_width / 2.0;

        // Helper: adjust a single corner radius for stroke alignment. A
        // corner with radius 0 stays sharp (no adjustment).
        let adjust_inside = |v: f64| if v > 0.0 { (v - half).max(0.0) } else { 0.0 };
        let adjust_outside = |v: f64| if v > 0.0 { v + half } else { 0.0 };

        let (sx, sy, sw_geom, sh_geom, sradius, sradii) = match rect.stroke_alignment.as_deref() {
            Some("inside") => (
                x + half,
                y + half,
                w - stroke_width,
                h - stroke_width,
                adjust_inside(radius),
                radii.map(|[tl, tr, br, bl]| {
                    [
                        adjust_inside(tl),
                        adjust_inside(tr),
                        adjust_inside(br),
                        adjust_inside(bl),
                    ]
                }),
            ),
            Some("outside") => (
                x - half,
                y - half,
                w + stroke_width,
                h + stroke_width,
                adjust_outside(radius),
                radii.map(|[tl, tr, br, bl]| {
                    [
                        adjust_outside(tl),
                        adjust_outside(tr),
                        adjust_outside(br),
                        adjust_outside(bl),
                    ]
                }),
            ),
            // "center" (default) and any unrecognized value.
            _ => (x, y, w, h, radius, radii),
        };

        // Whether the stroked shape is still rounded after alignment.
        let stroke_is_rounded =
            sradius > 0.0 || sradii.as_ref().is_some_and(|a| a.iter().any(|&v| v > 0.0));

        // An inside-aligned stroke can shrink the box to nothing; skip
        // rather than emit a degenerate rectangle.
        if sw_geom > 0.0 && sh_geom > 0.0 {
            if stroke_is_rounded {
                draws.push(SceneCommand::StrokeRoundedRect {
                    x: sx,
                    y: sy,
                    w: sw_geom,
                    h: sh_geom,
                    radius: sradius,
                    radii: sradii,
                    color,
                    stroke_width,
                    stroke_dash,
                    stroke_gap,
                    stroke_linecap,
                });
            } else {
                draws.push(SceneCommand::StrokeRect {
                    x: sx,
                    y: sy,
                    w: sw_geom,
                    h: sh_geom,
                    color,
                    stroke_width,
                    stroke_dash,
                    stroke_gap,
                    stroke_linecap,
                });
            }
        }
    }

    // Emit the collected draws into `commands`, bracketed by the winning effect
    // and/or the mask. No effect + no mask → draws appended verbatim, so an
    // unmasked, uneffected rect is byte-identical to the prior command stream.
    emit_node_with_effects(commands, draws, effect, mask);

    // STROKE-OUTER: a second stroke painted OUTSIDE the rect geometry.
    // Emitted after shadow/blur bracket (so it lands on top of the shadow)
    // and outside the blend layer bracket (so it inherits the same opacity
    // cascade). Default-OFF: no commands emitted when stroke_outer is absent.
    if let Some(op) = rect.stroke_outer.as_ref()
        && let Some(mut oc) = resolve_property_color(op, resolved, diagnostics, &rect.id)
    {
        oc.a = (oc.a as f64 * color_op).round() as u8;
        let ow = resolve_property_dimension_px(&rect.stroke_outer_width, resolved, 1.0);
        let half = ow / 2.0;
        let ox = x - half;
        let oy = y - half;
        let ogw = w + ow;
        let ogh = h + ow;
        if ogw > 0.0 && ogh > 0.0 {
            if is_rounded {
                // Expand corner radii outward by half the outer stroke width.
                // A corner with radius 0 stays sharp (no outset).
                let outer_expand = |v: f64| if v > 0.0 { v + half } else { 0.0 };
                let outer_radius = outer_expand(radius);
                let outer_radii = radii.map(|[tl, tr, br, bl]| {
                    [
                        outer_expand(tl),
                        outer_expand(tr),
                        outer_expand(br),
                        outer_expand(bl),
                    ]
                });
                commands.push(SceneCommand::StrokeRoundedRect {
                    x: ox,
                    y: oy,
                    w: ogw,
                    h: ogh,
                    radius: outer_radius,
                    radii: outer_radii,
                    color: oc,
                    stroke_width: ow,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            } else {
                commands.push(SceneCommand::StrokeRect {
                    x: ox,
                    y: oy,
                    w: ogw,
                    h: ogh,
                    color: oc,
                    stroke_width: ow,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            }
        }
    }

    // PER-SIDE BORDERS: straight StrokeLine along each present side.
    // Emitted after the outer stroke. Default-OFF: no commands when all four
    // border_* props are absent.
    let has_border = rect.border_top.is_some()
        || rect.border_bottom.is_some()
        || rect.border_left.is_some()
        || rect.border_right.is_some();
    if has_border {
        // border-width falls back to stroke-width then 1px.
        let bw_prop = rect
            .border_width
            .clone()
            .or_else(|| rect.stroke_width.clone());
        let bw = resolve_property_dimension_px(&bw_prop, resolved, 1.0);
        for (prop, x1, y1, x2, y2) in [
            (rect.border_top.as_ref(), x, y, x + w, y),
            (rect.border_bottom.as_ref(), x, y + h, x + w, y + h),
            (rect.border_left.as_ref(), x, y, x, y + h),
            (rect.border_right.as_ref(), x + w, y, x + w, y + h),
        ] {
            if let Some(sp) = prop
                && let Some(mut sc) = resolve_property_color(sp, resolved, diagnostics, &rect.id)
            {
                sc.a = (sc.a as f64 * color_op).round() as u8;
                commands.push(SceneCommand::StrokeLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: sc,
                    stroke_width: bw,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            }
        }
    }

    if blend.is_some() {
        commands.push(SceneCommand::PopLayer);
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Compile an `ellipse` leaf node.
pub(super) fn compile_ellipse(
    ellipse: &EllipseNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    anchors: &AnchorMap,
    ctx: RenderCtx,
) {
    // Skip invisible ellipses.
    if ellipse.visible == Some(false) {
        return;
    }

    // Resolve geometry — w and h are always required. x and y may be
    // derived from a page-relative anchor when absent.
    let (Some(w_dim), Some(h_dim)) = (&ellipse.w, &ellipse.h) else {
        diagnostics.push(missing_geometry_diag(
            "ellipse",
            &ellipse.id,
            ellipse.source_span,
        ));
        return;
    };
    let Some(w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "ellipse",
            &ellipse.id,
            "w",
            ellipse.source_span,
        ));
        return;
    };
    let Some(h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "ellipse",
            &ellipse.id,
            "h",
            ellipse.source_span,
        ));
        return;
    };

    // Anchor-derived (x, y): look up the pre-pass map when x or y is absent.
    let anchor_xy = anchors.get(&ellipse.id).copied();

    let Some(x_raw) = resolve_anchored_axis(
        "ellipse",
        &ellipse.id,
        "x",
        ellipse.x.as_ref(),
        anchor_xy.map(|(ax, _)| ax),
        ellipse.source_span,
        diagnostics,
    ) else {
        return;
    };
    let Some(y_raw) = resolve_anchored_axis(
        "ellipse",
        &ellipse.id,
        "y",
        ellipse.y.as_ref(),
        anchor_xy.map(|(_, ay)| ay),
        ellipse.source_span,
        diagnostics,
    ) else {
        return;
    };

    // Apply group translation offset.
    let x = x_raw + ctx.dx;
    let y = y_raw + ctx.dy;

    // Apply node opacity then cascade ctx.opacity on top.
    let node_opacity = ellipse.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

    // Blend-mode layer (see compile_rect for the opacity-split rationale). With
    // no blend, `color_op == node_opacity * ctx.opacity` → byte-identical.
    let blend = blend_mode_ir(ellipse.blend_mode.as_deref());
    let (layer_op, color_op) = match blend {
        Some(_) => (node_opacity * ctx.opacity, 1.0),
        None => (1.0, node_opacity * ctx.opacity),
    };

    // Resolve independent semi-axis overrides. When absent → `None` → byte-identical
    // to inscribed-ellipse behavior (renderer uses w/h directly).
    let rx: Option<f64> = ellipse.rx.as_ref().and_then(|p| {
        let v = resolve_property_dimension_px(&Some(p.clone()), resolved, 0.0);
        if v > 0.0 { Some(v) } else { None }
    });
    let ry: Option<f64> = ellipse.ry.as_ref().and_then(|p| {
        let v = resolve_property_dimension_px(&Some(p.clone()), resolved, 0.0);
        if v > 0.0 { Some(v) } else { None }
    });

    // Rotation bracket (outermost). PushTransform only when rotate ≠ 0.
    let rot = rotation_degrees(ellipse.rotate.as_ref());
    if let Some(angle) = rot {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // BLEND-MODE layer bracket (inside rotation, outside shadow).
    if let Some(blend_mode) = blend {
        commands.push(SceneCommand::PushLayer {
            opacity: layer_op,
            blend_mode: Some(blend_mode),
        });
    }

    // BLUR / SHADOW / FILTER effect (behind fill+stroke). Blur > shadow > filter;
    // at most one is chosen. The winning effect plus the optional mask bracket
    // the node's DRAWS (FILL + STROKE) via `emit_node_with_effects` below.
    let blur_sigma = ellipse
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let effect: Option<NodeEffect> = if let Some(sigma) = blur_sigma {
        Some(NodeEffect::Blur(sigma))
    } else if let Some(shadows) = ellipse
        .shadow
        .as_ref()
        .and_then(|p| resolve_property_shadow(p, resolved, &ellipse.id))
    {
        Some(NodeEffect::Shadow(shadows))
    } else {
        ellipse
            .filter
            .as_ref()
            .and_then(|p| resolve_property_filter(p, resolved, &ellipse.id))
            .map(NodeEffect::Filter)
    };

    // Resolve the optional node mask against the ellipse's page-absolute box.
    let mask = ellipse
        .mask
        .as_ref()
        .and_then(|p| resolve_property_mask(p, resolved, (x, y, w, h)));

    // Collect the node's DRAW commands (fill + stroke) into a local buffer.
    let mut draws: Vec<SceneCommand> = Vec::new();

    // FILL (emitted first, under the stroke) — node-local prop overrides
    // style cascade.
    let fill_prop = ellipse
        .fill
        .as_ref()
        .or_else(|| style_prop(&ellipse.style, style_map, "fill"));
    if let Some(fill_prop) = fill_prop {
        if let Some(mut gradient) = resolve_property_gradient(fill_prop, resolved, &ellipse.id) {
            apply_gradient_opacity(&mut gradient, color_op, 1.0);
            draws.push(SceneCommand::FillEllipseGradient {
                x,
                y,
                w,
                h,
                rx,
                ry,
                gradient,
            });
        } else if let Some(mut color) =
            resolve_property_color(fill_prop, resolved, diagnostics, &ellipse.id)
        {
            color.a = (color.a as f64 * color_op).round() as u8;
            draws.push(SceneCommand::FillEllipse {
                x,
                y,
                w,
                h,
                rx,
                ry,
                color,
            });
        }
    }

    // STROKE (emitted on top of the fill) — node-local prop overrides
    // style cascade. Stroke is centered on the ellipse path in v0.
    let stroke_prop = ellipse
        .stroke
        .as_ref()
        .or_else(|| style_prop(&ellipse.style, style_map, "stroke"));
    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &ellipse.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        let sw = ellipse
            .stroke_width
            .clone()
            .or_else(|| style_prop(&ellipse.style, style_map, "stroke-width").cloned());
        let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);

        // Resolve dashed stroke parameters.
        let (stroke_dash, stroke_gap, stroke_linecap) = resolve_dash_params(
            &ellipse.stroke_dash,
            &ellipse.stroke_gap,
            ellipse.stroke_linecap.as_deref(),
            resolved,
        );

        draws.push(SceneCommand::StrokeEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            color,
            stroke_width,
            stroke_dash,
            stroke_gap,
            stroke_linecap,
        });
    }

    // If neither fill nor stroke is present, the ellipse draws nothing —
    // no diagnostic needed (an invisible ellipse is valid in v0).

    // Emit the collected draws, bracketed by the winning effect and/or mask.
    // No effect + no mask → draws appended verbatim (byte-identical).
    emit_node_with_effects(commands, draws, effect, mask);

    if blend.is_some() {
        commands.push(SceneCommand::PopLayer);
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Compile a `line` leaf node.
pub(super) fn compile_line(
    line: &LineNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    // Skip invisible lines.
    if line.visible == Some(false) {
        return;
    }

    // Require all four endpoints; skip if any is absent or bad unit.
    let (Some(x1d), Some(y1d), Some(x2d), Some(y2d)) = (&line.x1, &line.y1, &line.x2, &line.y2)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "line '{}' is missing one or more endpoint properties (x1, y1, x2, y2); \
                 skipped",
                line.id
            ),
            line.source_span,
            Some(line.id.clone()),
        ));
        return;
    };

    let Some(x1_raw) = dim_to_px(x1d.value, &x1d.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "line",
            &line.id,
            "x1",
            line.source_span,
        ));
        return;
    };
    let Some(y1_raw) = dim_to_px(y1d.value, &y1d.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "line",
            &line.id,
            "y1",
            line.source_span,
        ));
        return;
    };
    let Some(x2_raw) = dim_to_px(x2d.value, &x2d.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "line",
            &line.id,
            "x2",
            line.source_span,
        ));
        return;
    };
    let Some(y2_raw) = dim_to_px(y2d.value, &y2d.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "line",
            &line.id,
            "y2",
            line.source_span,
        ));
        return;
    };

    // Apply group translation offset.
    let x1 = x1_raw + ctx.dx;
    let y1 = y1_raw + ctx.dy;
    let x2 = x2_raw + ctx.dx;
    let y2 = y2_raw + ctx.dy;

    // Stroke is optional in validation, but a stroke-less line draws nothing.
    // Cascade: node-local stroke overrides style stroke.
    let stroke_prop = line
        .stroke
        .as_ref()
        .or_else(|| style_prop(&line.style, style_map, "stroke"));
    let Some(stroke_prop) = stroke_prop else {
        return;
    };
    let Some(mut color) = resolve_property_color(stroke_prop, resolved, diagnostics, &line.id)
    else {
        return;
    };

    // Apply node opacity then cascade ctx.opacity on top.
    let node_opacity = line.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

    // Resolve stroke_width to px with style cascade; default 1.0 when absent.
    let sw = line
        .stroke_width
        .clone()
        .or_else(|| style_prop(&line.style, style_map, "stroke-width").cloned());
    let stroke_width: f64 = resolve_property_dimension_px(&sw, resolved, 1.0);

    // Resolve dashed stroke parameters.
    let (stroke_dash, stroke_gap, stroke_linecap) = resolve_dash_params(
        &line.stroke_dash,
        &line.stroke_gap,
        line.stroke_linecap.as_deref(),
        resolved,
    );

    commands.push(SceneCommand::StrokeLine {
        x1,
        y1,
        x2,
        y2,
        color,
        stroke_width,
        stroke_dash,
        stroke_gap,
        stroke_linecap,
    });
}

/// Resolve an ordered point list into a flat `[x0, y0, x1, y1, …]` pixel-
/// coordinate vector, applying `ctx.dx`/`ctx.dy`.
///
/// Returns `None` on the first point with a missing or unsupported-unit
/// coordinate, after pushing a diagnostic. The minimum-count check is the
/// caller's responsibility (polygon requires ≥ 6 coords, polyline ≥ 4).
fn resolve_flat_points(
    points: &[Point],
    node_kind: &str,
    node_id: &str,
    source_span: Option<Span>,
    ctx: RenderCtx,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<f64>> {
    let mut flat: Vec<f64> = Vec::with_capacity(points.len() * 2);
    for (idx, pt) in points.iter().enumerate() {
        let (Some(xd), Some(yd)) = (&pt.x, &pt.y) else {
            diagnostics.push(Diagnostic::advisory(
                "scene.missing_geometry",
                format!(
                    "{} '{}' point[{}] is missing x or y coordinate; skipped",
                    node_kind, node_id, idx
                ),
                source_span,
                Some(node_id.to_owned()),
            ));
            return None;
        };
        let Some(px) = dim_to_px(xd.value, &xd.unit) else {
            diagnostics.push(unsupported_unit_diag(
                node_kind,
                node_id,
                "point x",
                source_span,
            ));
            return None;
        };
        let Some(py) = dim_to_px(yd.value, &yd.unit) else {
            diagnostics.push(unsupported_unit_diag(
                node_kind,
                node_id,
                "point y",
                source_span,
            ));
            return None;
        };
        flat.push(px + ctx.dx);
        flat.push(py + ctx.dy);
    }
    Some(flat)
}

/// Compile a `polygon` leaf node.
///
/// Emits `FillPolygon` (if fill is present) THEN `StrokePolyline { closed: true }`
/// (if stroke is present) so the stroke draws on top of the fill.
///
/// Points are in absolute document coordinates — `ctx.dx`/`ctx.dy` are added
/// exactly as for `line` endpoints.
pub(super) fn compile_polygon(
    poly: &PolygonNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    if poly.visible == Some(false) {
        return;
    }

    // Build the flat point list: require both x and y for every point.
    let Some(flat_points) = resolve_flat_points(
        &poly.points,
        "polygon",
        &poly.id,
        poly.source_span,
        ctx,
        diagnostics,
    ) else {
        return;
    };

    // Need at least 3 points (6 coordinates) — validate already errors, skip emit.
    if flat_points.len() < 6 {
        return;
    }

    let node_opacity = poly.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let even_odd = poly.fill_rule.as_deref() == Some("evenodd");

    // Rotation bracket: compute centroid-bbox center from the flat point vec.
    // PushTransform only when rotate is non-zero; unrotated polys are unchanged.
    let rot = rotation_degrees(poly.rotate.as_ref());
    if let Some(angle) = rot {
        let (cx, cy) = flat_points_centroid_center(&flat_points);
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // FILL (drawn first, stroke on top) — node-local overrides style cascade.
    let fill_prop = poly
        .fill
        .as_ref()
        .or_else(|| style_prop(&poly.style, style_map, "fill"));
    if let Some(fill_prop) = fill_prop
        && let Some(mut color) = resolve_property_color(fill_prop, resolved, diagnostics, &poly.id)
    {
        color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
        commands.push(SceneCommand::FillPolygon {
            points: flat_points.clone(),
            color,
            even_odd,
        });
    }

    // STROKE (drawn on top of fill) — node-local overrides style cascade.
    let stroke_prop = poly
        .stroke
        .as_ref()
        .or_else(|| style_prop(&poly.style, style_map, "stroke"));
    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &poly.id)
    {
        color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
        let sw = poly
            .stroke_width
            .clone()
            .or_else(|| style_prop(&poly.style, style_map, "stroke-width").cloned());
        let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);
        // stroke-alignment: "inside"/"outside" shift the stroke off the path
        // boundary; anything else (incl. "center", None, or an invalid value)
        // falls back to Center. Validation emits the warning for bad values.
        let align = match poly.stroke_alignment.as_deref() {
            Some("inside") => StrokeAlign::Inside,
            Some("outside") => StrokeAlign::Outside,
            _ => StrokeAlign::Center,
        };
        commands.push(SceneCommand::StrokePolyline {
            points: flat_points,
            color,
            stroke_width,
            closed: true,
            align,
            fill_even_odd: even_odd,
        });
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Compile a `polyline` leaf node.
///
/// Emits `FillPolygon` (if fill is present, renderer closes the path implicitly)
/// THEN `StrokePolyline { closed: false }` (if stroke is present).
///
/// Points are in absolute document coordinates — `ctx.dx`/`ctx.dy` are added
/// exactly as for `line` endpoints.
pub(super) fn compile_polyline(
    poly: &PolylineNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    if poly.visible == Some(false) {
        return;
    }

    // Build the flat point list.
    let Some(flat_points) = resolve_flat_points(
        &poly.points,
        "polyline",
        &poly.id,
        poly.source_span,
        ctx,
        diagnostics,
    ) else {
        return;
    };

    // Need at least 2 points (4 coordinates) — validate already errors, skip emit.
    if flat_points.len() < 4 {
        return;
    }

    let node_opacity = poly.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let even_odd = poly.fill_rule.as_deref() == Some("evenodd");

    // Rotation bracket: compute centroid-bbox center from the flat point vec.
    // PushTransform only when rotate is non-zero; unrotated polylines are unchanged.
    let rot = rotation_degrees(poly.rotate.as_ref());
    if let Some(angle) = rot {
        let (cx, cy) = flat_points_centroid_center(&flat_points);
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // FILL (drawn first; FillPolygon renderer closes the path) — style cascade.
    let fill_prop = poly
        .fill
        .as_ref()
        .or_else(|| style_prop(&poly.style, style_map, "fill"));
    if let Some(fill_prop) = fill_prop
        && let Some(mut color) = resolve_property_color(fill_prop, resolved, diagnostics, &poly.id)
    {
        color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
        commands.push(SceneCommand::FillPolygon {
            points: flat_points.clone(),
            color,
            even_odd,
        });
    }

    // STROKE — open path (closed: false) — style cascade.
    let stroke_prop = poly
        .stroke
        .as_ref()
        .or_else(|| style_prop(&poly.style, style_map, "stroke"));
    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &poly.id)
    {
        color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
        let sw = poly
            .stroke_width
            .clone()
            .or_else(|| style_prop(&poly.style, style_map, "stroke-width").cloned());
        let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);
        commands.push(SceneCommand::StrokePolyline {
            points: flat_points,
            color,
            stroke_width,
            closed: false,
            // polyline is an open path: alignment never applies.
            align: StrokeAlign::Center,
            fill_even_odd: false,
        });
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Which edge of a box an anchor sits on, expressed as the orientation the path
/// must leave/enter through. `Horizontal` = a left/right edge → the path leaves
/// horizontally; `Vertical` = a top/bottom edge → the path leaves vertically.
///
/// Used by orthogonal routing (Unit 3) to guarantee the first/last segment is
/// perpendicular to the box edge, so arrowheads land axis-aligned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AnchorSide {
    Horizontal,
    Vertical,
}

/// Compute the page-absolute anchor point on the edge of a `(x, y, w, h)` box,
/// AND the orientation of that edge ([`AnchorSide`]).
///
/// Named anchors map to the edge centers; `"center"` is the box center (treated
/// as `Horizontal`, a rare degenerate case). `"auto"` (the default for an absent
/// / unrecognized anchor) chooses the edge by the dominant axis toward `toward`
/// (the OTHER box's center): a larger horizontal delta picks left/right
/// (`Horizontal`), otherwise top/bottom (`Vertical`).
///
/// The point math is identical to the pre-Unit-3 anchor resolution, so
/// straight-route output is unchanged.
fn resolve_anchor(
    boxr: (f64, f64, f64, f64),
    anchor: &str,
    toward: (f64, f64),
) -> ((f64, f64), AnchorSide) {
    let (x, y, w, h) = boxr;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    match anchor {
        "top" => ((cx, y), AnchorSide::Vertical),
        "bottom" => ((cx, y + h), AnchorSide::Vertical),
        "left" => ((x, cy), AnchorSide::Horizontal),
        "right" => ((x + w, cy), AnchorSide::Horizontal),
        "center" => ((cx, cy), AnchorSide::Horizontal),
        // "auto" and any unrecognized value: dominant-axis edge toward `toward`.
        _ => {
            let dx = toward.0 - cx;
            let dy = toward.1 - cy;
            if dx.abs() >= dy.abs() {
                let pt = if dx >= 0.0 { (x + w, cy) } else { (x, cy) };
                (pt, AnchorSide::Horizontal)
            } else if dy >= 0.0 {
                ((cx, y + h), AnchorSide::Vertical)
            } else {
                ((cx, y), AnchorSide::Vertical)
            }
        }
    }
}

/// Build a flat right-angle (elbow) point list between two anchors, with the
/// first segment perpendicular to `f`'s edge (`fs`) and the last perpendicular to
/// `t`'s edge (`ts`). Returns 8 coords (4-point Z-route) when both anchors share
/// an orientation, or 6 coords (3-point L-corner) when they differ.
///
/// Collinear/degenerate elbows (e.g. `mx == f.0`) are left as-is — zero-length
/// sub-segments render harmlessly and are never special-cased.
fn orthogonal_route(f: (f64, f64), fs: AnchorSide, t: (f64, f64), ts: AnchorSide) -> Vec<f64> {
    match (fs, ts) {
        // Both side edges → H–V–H Z-route, elbow at the mid x.
        (AnchorSide::Horizontal, AnchorSide::Horizontal) => {
            let mx = (f.0 + t.0) / 2.0;
            vec![f.0, f.1, mx, f.1, mx, t.1, t.0, t.1]
        }
        // Both top/bottom edges → V–H–V Z-route, elbow at the mid y.
        (AnchorSide::Vertical, AnchorSide::Vertical) => {
            let my = (f.1 + t.1) / 2.0;
            vec![f.0, f.1, f.0, my, t.0, my, t.0, t.1]
        }
        // Leaves F horizontally, enters T vertically → corner at (t.0, f.1).
        (AnchorSide::Horizontal, AnchorSide::Vertical) => {
            vec![f.0, f.1, t.0, f.1, t.0, t.1]
        }
        // Leaves F vertically, enters T horizontally → corner at (f.0, t.1).
        (AnchorSide::Vertical, AnchorSide::Horizontal) => {
            vec![f.0, f.1, f.0, t.1, t.0, t.1]
        }
    }
}

/// Bounds-safe read of the `i`-th `(x, y)` point from a flat `[x0,y0,x1,y1,…]`
/// list. Returns `None` if the point is out of range (no panic, no indexing).
fn point_at(pts: &[f64], i: usize) -> Option<(f64, f64)> {
    let x = pts.get(i * 2)?;
    let y = pts.get(i * 2 + 1)?;
    Some((*x, *y))
}

/// Compile a `connector` leaf node — a semantic arrow whose endpoints are
/// DERIVED at compile time from the resolved boxes of its `from`/`to` targets.
///
/// Unit 1 draws a STRAIGHT 2-point line between the resolved edge anchors. Unit 2
/// adds filled-triangle arrowheads at the `to` end (`marker-end="arrow"`) and/or
/// the `from` end (`marker-start="arrow"`), in the line's stroke color and inside
/// the same rotation bracket. Unit 3 adds `route="orthogonal"` — a right-angle
/// elbow path (4-point Z-route or 3-point L-corner) instead of the straight
/// diagonal — and orients arrowheads along the actual first/last routed segment
/// so they land axis-aligned. When `from`/`to` is absent, or a target box is
/// not in `node_boxes` (unresolved), nothing is emitted (graceful — validation
/// warned); markers follow the same guards, so a skipped line skips its heads.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_connector(
    connector: &ConnectorNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
    ctx: RenderCtx,
) {
    if connector.visible == Some(false) {
        return;
    }

    // Both endpoints are required to route; absent → emit nothing (validation
    // already warned via `connector.missing_target`).
    let (Some(from_id), Some(to_id)) = (connector.from.as_deref(), connector.to.as_deref()) else {
        return;
    };

    // Look up the resolved page-absolute boxes of both targets. A missing box
    // (unresolved id, or a target with no authored geometry) → emit nothing.
    let (Some(from_box), Some(to_box)) = (node_boxes.get(from_id), node_boxes.get(to_id)) else {
        return;
    };
    let from_box = *from_box;
    let to_box = *to_box;

    let from_center = (from_box.0 + from_box.2 / 2.0, from_box.1 + from_box.3 / 2.0);
    let to_center = (to_box.0 + to_box.2 / 2.0, to_box.1 + to_box.3 / 2.0);

    // Resolve anchors: each end aims toward the OTHER box's center for "auto".
    let from_anchor = connector.from_anchor.as_deref().unwrap_or("auto");
    let to_anchor = connector.to_anchor.as_deref().unwrap_or("auto");
    let (f_pt, f_side) = resolve_anchor(from_box, from_anchor, to_center);
    let (t_pt, t_side) = resolve_anchor(to_box, to_anchor, from_center);

    // Route selection: `orthogonal` builds a right-angle elbow path; everything
    // else (None / "straight" / unknown — validation already warned) is the
    // straight 2-point line, byte-identical to Unit 1/2.
    let flat_points = match connector.route.as_deref() {
        Some("orthogonal") => orthogonal_route(f_pt, f_side, t_pt, t_side),
        _ => vec![f_pt.0, f_pt.1, t_pt.0, t_pt.1],
    };

    // STROKE — only emit when a stroke color is present (mirrors polyline: no
    // stroke token → nothing drawn). Style cascade for stroke + stroke-width.
    let node_opacity = connector.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let stroke_prop = connector
        .stroke
        .as_ref()
        .or_else(|| style_prop(&connector.style, style_map, "stroke"));
    let Some(stroke_prop) = stroke_prop else {
        return;
    };
    let Some(mut color) = resolve_property_color(stroke_prop, resolved, diagnostics, &connector.id)
    else {
        return;
    };
    color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

    let sw = connector
        .stroke_width
        .clone()
        .or_else(|| style_prop(&connector.style, style_map, "stroke-width").cloned());
    let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);

    // Rotation bracket: rotate about the line's bbox center, matching polyline.
    let rot = rotation_degrees(connector.rotate.as_ref());
    if let Some(angle) = rot {
        let (cx, cy) = flat_points_centroid_center(&flat_points);
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Derive marker endpoints from the ACTUAL routed path BEFORE the Vec is moved
    // into the stroke command, so orthogonal arrowheads orient along the real
    // last/first segment (axis-aligned), not the global anchor line. For a
    // 2-point straight line these reduce to today's (tx,ty)/(fx,fy) endpoints.
    let n = flat_points.len() / 2;
    let end_tip = point_at(&flat_points, n.saturating_sub(1));
    let end_from = point_at(&flat_points, n.saturating_sub(2));
    let start_tip = point_at(&flat_points, 0);
    let start_from = point_at(&flat_points, 1);

    commands.push(SceneCommand::StrokePolyline {
        points: flat_points,
        color,
        stroke_width,
        closed: false,
        align: StrokeAlign::Center,
        fill_even_odd: false,
    });

    // ARROWHEAD MARKERS (Unit 2/3) — filled triangles in the SAME stroke color,
    // INSIDE the rotation bracket so they rotate with the line. The tip sits
    // exactly on the path endpoint; the base extends back along the adjacent
    // segment. Fewer than 2 points → endpoints are `None` and markers are skipped.
    {
        let mut emit_head = |tip, from_pt| {
            if let Some(points) = arrowhead_points(tip, from_pt, stroke_width) {
                commands.push(SceneCommand::FillPolygon {
                    points,
                    color,
                    even_odd: false,
                });
            }
        };
        if connector.marker_end.as_deref() == Some("arrow")
            && let (Some(tip), Some(from_pt)) = (end_tip, end_from)
        {
            emit_head(tip, from_pt);
        }
        if connector.marker_start.as_deref() == Some("arrow")
            && let (Some(tip), Some(from_pt)) = (start_tip, start_from)
        {
            emit_head(tip, from_pt);
        }
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Build a filled-triangle arrowhead whose tip sits at `tip`, arriving along the
/// segment from `from_pt` → `tip` (so the head points in the direction of travel
/// into `tip`). Returns a flat `[x0,y0, x1,y1, x2,y2]` (tip, left base, right
/// base), or `None` if the segment is degenerate (endpoints coincide) and the
/// head cannot be oriented. Size scales with `stroke_width`, clamped so thin
/// strokes still get a visible head.
fn arrowhead_points(tip: (f64, f64), from_pt: (f64, f64), stroke_width: f64) -> Option<Vec<f64>> {
    let vx = tip.0 - from_pt.0;
    let vy = tip.1 - from_pt.1;
    let len = (vx * vx + vy * vy).sqrt();
    if len < 1e-6 {
        return None;
    }
    let (ux, uy) = (vx / len, vy / len);
    let (px, py) = (-uy, ux);
    // head_len: 3.5× stroke; half_w: 2.0× stroke — clamped so hairline strokes
    // still produce a visible 7px × 8px head.
    let head_len = (stroke_width * 3.5).max(7.0);
    let half_w = (stroke_width * 2.0).max(4.0);
    let base_cx = tip.0 - ux * head_len;
    let base_cy = tip.1 - uy * head_len;
    let left_x = base_cx + px * half_w;
    let left_y = base_cy + py * half_w;
    let right_x = base_cx - px * half_w;
    let right_y = base_cy - py * half_w;
    Some(vec![tip.0, tip.1, left_x, left_y, right_x, right_y])
}

/// Compute the center of the bounding box of a flat `[x0, y0, x1, y1, …]` point list.
///
/// Used to determine the rotation pivot for polygon/polyline nodes. The slice
/// must be non-empty and even-length (guaranteed by the callers). If the slice
/// is somehow empty, returns `(0.0, 0.0)` as a safe degenerate fallback
/// (the no-panic contract requires this instead of indexing).
fn flat_points_centroid_center(flat: &[f64]) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut i = 0;
    while i + 1 < flat.len() {
        let px = flat[i];
        let py = flat[i + 1];
        if px < min_x {
            min_x = px;
        }
        if px > max_x {
            max_x = px;
        }
        if py < min_y {
            min_y = py;
        }
        if py > max_y {
            max_y = py;
        }
        i += 2;
    }
    if min_x.is_infinite() {
        return (0.0, 0.0);
    }
    ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
}

/// Compile a `shape` compound node — background + owned centered label.
///
/// Emits the background primitive selected by [`ShapeNode::kind`] (default
/// `"process"` when absent or unrecognized):
/// - `process`   → rounded rect (`FillRoundedRect` + `StrokeRoundedRect`), using
///   `radius`; a plain `FillRect`/`StrokeRect` when `radius` is absent/0.
/// - `terminator`→ rounded rect with corner radius = `h/2` (pill).
/// - `ellipse`   → `FillEllipse` + `StrokeEllipse`.
/// - `decision`  → 4-point diamond polygon (`FillPolygon` + closed
///   `StrokePolyline`) built from the bbox mid-edges.
///
/// AFTER the background (so the label paints ON TOP of the fill), the owned
/// label [`ShapeNode::spans`] are rendered as a synthesized [`TextNode`] laid
/// into the shape's padded content box, REUSING the production
/// [`compile_text`] path. The label is horizontally aligned by `h_align`
/// (default `center`) and vertically aligned by `v_align` (default `middle`,
/// via a measured pre-offset like the table cell), and it shares the SAME
/// `ctx` as the background — so the shape's opacity, rotation, and clip
/// propagate to the label and the two stay locked together.
///
/// `opacity`/`visible`/`rotate` are honored exactly as `compile_rect` does;
/// `stroke_alignment` insets the rounded-rect/ellipse box the same way
/// `compile_rect` handles it. For the decision diamond the stroke is left
/// centered (v0).
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_shape(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    footnote_markers: &BTreeMap<String, String>,
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
    anchors: &AnchorMap,
    ctx: RenderCtx,
) {
    // Skip invisible shapes.
    if shape.visible == Some(false) {
        return;
    }

    // Resolve geometry — w and h are always required. x and y may be
    // derived from a page-relative anchor when absent.
    let (Some(w_dim), Some(h_dim)) = (&shape.w, &shape.h) else {
        diagnostics.push(missing_geometry_diag("shape", &shape.id, shape.source_span));
        return;
    };
    let Some(w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "shape",
            &shape.id,
            "w",
            shape.source_span,
        ));
        return;
    };
    let Some(h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "shape",
            &shape.id,
            "h",
            shape.source_span,
        ));
        return;
    };

    // Anchor-derived (x, y): look up the pre-pass map when x or y is absent.
    let anchor_xy = anchors.get(&shape.id).copied();

    let Some(x_raw) = resolve_anchored_axis(
        "shape",
        &shape.id,
        "x",
        shape.x.as_ref(),
        anchor_xy.map(|(ax, _)| ax),
        shape.source_span,
        diagnostics,
    ) else {
        return;
    };
    let Some(y_raw) = resolve_anchored_axis(
        "shape",
        &shape.id,
        "y",
        shape.y.as_ref(),
        anchor_xy.map(|(_, ay)| ay),
        shape.source_span,
        diagnostics,
    ) else {
        return;
    };

    // Apply group translation offset.
    let x = x_raw + ctx.dx;
    let y = y_raw + ctx.dy;

    // Apply node opacity then cascade ctx.opacity on top (matches compile_rect).
    let node_opacity = shape.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let color_op = node_opacity * ctx.opacity;

    // Resolve fill / stroke once (node-local prop overrides style cascade).
    let fill_prop = shape
        .fill
        .as_ref()
        .or_else(|| style_prop(&shape.style, style_map, "fill"));
    let stroke_prop = shape
        .stroke
        .as_ref()
        .or_else(|| style_prop(&shape.style, style_map, "stroke"));
    let stroke_width = {
        let sw = shape
            .stroke_width
            .clone()
            .or_else(|| style_prop(&shape.style, style_map, "stroke-width").cloned());
        resolve_property_dimension_px(&sw, resolved, 1.0)
    };

    // Rotation bracket (outermost). PushTransform only when rotate ≠ 0.
    let rot = rotation_degrees(shape.rotate.as_ref());
    if let Some(angle) = rot {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Background primitive by kind (default "process").
    match shape.kind.as_deref() {
        Some("ellipse") => {
            emit_shape_ellipse(
                shape,
                resolved,
                diagnostics,
                commands,
                x,
                y,
                w,
                h,
                color_op,
                fill_prop,
                stroke_prop,
                stroke_width,
            );
        }
        Some("decision") => {
            emit_shape_decision(
                shape,
                resolved,
                diagnostics,
                commands,
                x,
                y,
                w,
                h,
                color_op,
                fill_prop,
                stroke_prop,
                stroke_width,
            );
        }
        Some("terminator") => {
            // Pill: corner radius = h/2.
            emit_shape_rounded_rect(
                shape,
                resolved,
                diagnostics,
                commands,
                x,
                y,
                w,
                h,
                h / 2.0,
                color_op,
                fill_prop,
                stroke_prop,
                stroke_width,
            );
        }
        // "process" (default) and any unrecognized value: rounded rect using
        // `radius` (0 → plain rect).
        _ => {
            let radius_prop = shape
                .radius
                .clone()
                .or_else(|| style_prop(&shape.style, style_map, "radius").cloned());
            let radius = resolve_property_dimension_px(&radius_prop, resolved, 0.0);
            emit_shape_rounded_rect(
                shape,
                resolved,
                diagnostics,
                commands,
                x,
                y,
                w,
                h,
                radius,
                color_op,
                fill_prop,
                stroke_prop,
                stroke_width,
            );
        }
    }

    // OWNED LABEL (painted ON TOP of the background). Emitted INSIDE the
    // rotation bracket so the label rotates with the shape, and using the SAME
    // `ctx` so the shape's opacity/clip cascade onto the glyphs too.
    emit_shape_label(
        shape,
        resolved,
        style_map,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        footnote_markers,
        node_boxes,
        anchors,
        x,
        y,
        w,
        h,
        ctx,
    );

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Synthesize a [`TextNode`] for the shape's owned label and render it via the
/// production [`compile_text`] path into the shape's padded content box.
///
/// The label inherits the shape's `ctx` (opacity / rotation / clip). Horizontal
/// alignment maps `h_align` → the text node's `align` (default `center`);
/// vertical alignment is applied by PRE-OFFSETTING the synthetic node's `y`
/// (measured via [`measure_text_wrapped_height`]), exactly like the table cell
/// — `TextNode` has no native v-align. The label defaults to centered both ways.
///
/// v0 simplification: for ALL kinds the content box is the bbox inset by
/// `padding`. The decision diamond inscribes its label in the bbox; an author
/// adds `padding` to keep the text inside the rhombus.
#[allow(clippy::too_many_arguments)]
fn emit_shape_label(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    footnote_markers: &BTreeMap<String, String>,
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
    anchors: &AnchorMap,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    ctx: RenderCtx,
) {
    // Nothing to render when the label has no spans.
    if shape.spans.is_empty() {
        return;
    }

    // Padded content box: bbox inset by `padding` (token → px; 0 when absent).
    let pad = resolve_property_dimension_px(&shape.padding, resolved, 0.0);
    let content_x = x + pad;
    let content_y = y + pad;
    let content_w = (w - 2.0 * pad).max(0.0);
    let content_h = (h - 2.0 * pad).max(0.0);

    // Padding larger than the box collapses the content area; skip rather than
    // emit a degenerate (zero/negative) text box.
    if content_w <= 0.0 || content_h <= 0.0 {
        return;
    }

    // Map the shape's `h_align` to the text node's `align` (default center).
    let align = match shape.h_align.as_deref() {
        Some("end") => Some("end".to_owned()),
        Some("start") => Some("start".to_owned()),
        // "center", any unrecognized value, and absent all center the label.
        _ => Some("center".to_owned()),
    };

    // Synthesize the label as a fresh TextNode laid into the content box. A
    // synthetic id derived from the shape id keeps it unique (never collides).
    let mut synth = TextNode {
        id: format!("{}/label", shape.id),
        name: None,
        role: None,
        x: Some(px(content_x)),
        y: Some(px(content_y)),
        w: Some(px(content_w)),
        h: Some(px(content_h)),
        align,
        direction: None,
        overflow: None,
        overflow_wrap: None,
        style: shape.text_style.clone(),
        fill: None,
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: None,
        font_size: None,
        font_size_min: None,
        font_weight: None,
        shadow: None,
        filter: None,
        mask: None,
        blend_mode: None,
        blur: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        chain: None,
        drop_cap_lines: None,
        hyphenate: None,
        widow_orphan: None,
        tab_leader: None,
        text_exclusion: None,
        padding_left: None,
        text_indent: None,
        bullet: None,
        bullet_gap: None,
        anchor: None,
        spans: shape.spans.clone(),
        source_span: shape.source_span,
        unknown_props: BTreeMap::new(),
    };

    // VERTICAL ALIGNMENT: TextNode has no native v-align, so pre-offset `y` by
    // the measured wrapped height (same approach as the table cell). Default is
    // middle.
    let families = resolve_text_families(&synth, resolved, style_map, fonts, diagnostics);
    let wrapped_h = measure_text_wrapped_height(
        &synth,
        content_w,
        &families,
        &MeasureEnv {
            resolved,
            style_map,
            fonts,
            engine,
        },
        diagnostics,
    )
    .unwrap_or(0.0);
    let v_offset = match shape.v_align.as_deref() {
        Some("top") => 0.0,
        Some("bottom") => (content_h - wrapped_h).max(0.0),
        // "middle", any unrecognized value, and absent center vertically.
        _ => ((content_h - wrapped_h) / 2.0).max(0.0),
    };
    synth.y = Some(px(content_y + v_offset));

    // Emit the label via the production text path. The synth's x/y are ALREADY
    // absolute (the caller resolved `x_raw + ctx.dx`), so the translation must
    // NOT be applied again — `compile_text` adds `ctx.dx/dy` itself. Zero the
    // translation while preserving opacity/baseline-grid so the label still
    // cascades correctly. Without this, a shape inside a group/instance has its
    // label double-translated by the container offset.
    let label_ctx = RenderCtx {
        dx: 0.0,
        dy: 0.0,
        ..ctx
    };
    let _ = compile_text(
        &synth,
        resolved,
        style_map,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        footnote_markers,
        node_boxes,
        anchors,
        label_ctx,
    );
}

/// Emit the rounded-rect (or plain-rect when `radius <= 0`) background for a
/// `process`/`terminator` shape. Stroke alignment insets the stroked box like
/// `compile_rect`.
#[allow(clippy::too_many_arguments)]
fn emit_shape_rounded_rect(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    color_op: f64,
    fill_prop: Option<&PropertyValue>,
    stroke_prop: Option<&PropertyValue>,
    stroke_width: f64,
) {
    let is_rounded = radius > 0.0;

    // FILL (emitted first, under the stroke).
    if let Some(fill_prop) = fill_prop
        && let Some(mut color) = resolve_property_color(fill_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        if is_rounded {
            commands.push(SceneCommand::FillRoundedRect {
                x,
                y,
                w,
                h,
                radius,
                radii: None,
                color,
            });
        } else {
            commands.push(SceneCommand::FillRect { x, y, w, h, color });
        }
    }

    // STROKE (emitted on top of fill). Only when both stroke and width apply.
    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        let half = stroke_width / 2.0;
        let adjust_inside = |v: f64| if v > 0.0 { (v - half).max(0.0) } else { 0.0 };
        let adjust_outside = |v: f64| if v > 0.0 { v + half } else { 0.0 };
        let (sx, sy, sw_geom, sh_geom, sradius) = match shape.stroke_alignment.as_deref() {
            Some("inside") => (
                x + half,
                y + half,
                w - stroke_width,
                h - stroke_width,
                adjust_inside(radius),
            ),
            Some("outside") => (
                x - half,
                y - half,
                w + stroke_width,
                h + stroke_width,
                adjust_outside(radius),
            ),
            // "center" (default) and any unrecognized value.
            _ => (x, y, w, h, radius),
        };
        let stroke_is_rounded = sradius > 0.0;
        if sw_geom > 0.0 && sh_geom > 0.0 {
            if stroke_is_rounded {
                commands.push(SceneCommand::StrokeRoundedRect {
                    x: sx,
                    y: sy,
                    w: sw_geom,
                    h: sh_geom,
                    radius: sradius,
                    radii: None,
                    color,
                    stroke_width,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            } else {
                commands.push(SceneCommand::StrokeRect {
                    x: sx,
                    y: sy,
                    w: sw_geom,
                    h: sh_geom,
                    color,
                    stroke_width,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            }
        }
    }
}

/// Emit an ellipse background (mirrors `compile_ellipse`'s fill/stroke emit).
/// Stroke alignment is not modeled for the ellipse (centered, like `ellipse`).
#[allow(clippy::too_many_arguments)]
fn emit_shape_ellipse(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color_op: f64,
    fill_prop: Option<&PropertyValue>,
    stroke_prop: Option<&PropertyValue>,
    stroke_width: f64,
) {
    if let Some(fill_prop) = fill_prop
        && let Some(mut color) = resolve_property_color(fill_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        commands.push(SceneCommand::FillEllipse {
            x,
            y,
            w,
            h,
            rx: None,
            ry: None,
            color,
        });
    }

    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        commands.push(SceneCommand::StrokeEllipse {
            x,
            y,
            w,
            h,
            rx: None,
            ry: None,
            color,
            stroke_width,
            stroke_dash: None,
            stroke_gap: None,
            stroke_linecap: None,
        });
    }
}

/// Emit a 4-point diamond polygon background for a `decision` shape (mirrors
/// `compile_polygon`'s emit). The diamond vertices are the bbox mid-edges:
/// top-mid, right-mid, bottom-mid, left-mid. Stroke is centered in U1.
#[allow(clippy::too_many_arguments)]
fn emit_shape_decision(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color_op: f64,
    fill_prop: Option<&PropertyValue>,
    stroke_prop: Option<&PropertyValue>,
    stroke_width: f64,
) {
    let flat_points = vec![
        x + w / 2.0,
        y, // top-mid
        x + w,
        y + h / 2.0, // right-mid
        x + w / 2.0,
        y + h, // bottom-mid
        x,
        y + h / 2.0, // left-mid
    ];

    if let Some(fill_prop) = fill_prop
        && let Some(mut color) = resolve_property_color(fill_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        commands.push(SceneCommand::FillPolygon {
            points: flat_points.clone(),
            color,
            even_odd: false,
        });
    }

    if let Some(stroke_prop) = stroke_prop
        && let Some(mut color) =
            resolve_property_color(stroke_prop, resolved, diagnostics, &shape.id)
    {
        color.a = (color.a as f64 * color_op).round() as u8;
        commands.push(SceneCommand::StrokePolyline {
            points: flat_points,
            color,
            stroke_width,
            closed: true,
            align: StrokeAlign::Center,
            fill_even_odd: false,
        });
    }
}
