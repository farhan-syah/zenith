//! Vector leaf-node compilation: rect, ellipse, line, polygon, and polyline.
//!
//! Each function mirrors the shared leaf signature
//! `(node, resolved, style_map, commands, diagnostics, ctx)` and emits the same
//! `SceneCommand` stream that the original inline `compile_node` arms produced.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, EllipseNode, LineNode, Point, PolygonNode, PolylineNode, PropertyValue, RectNode,
    ResolvedToken, Span, Style, dim_to_px,
};

use crate::ir::{LineCap, SceneCommand};

use super::RenderCtx;
use super::paint::{
    apply_gradient_opacity, resolve_property_color, resolve_property_gradient,
    resolve_property_shadow,
};
use super::style_prop;
use super::util::{
    blend_mode_ir, resolve_property_dimension_px, rotation_degrees, unsupported_unit_diag,
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
    ctx: RenderCtx,
) {
    // Skip invisible rects.
    if rect.visible == Some(false) {
        return;
    }

    // Resolve geometry — all four are required; skip if any is absent
    // or uses an unsupported unit.
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) = (&rect.x, &rect.y, &rect.w, &rect.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "rect '{}' is missing one or more geometry properties (x, y, w, h); \
                 skipped",
                rect.id
            ),
            rect.source_span,
            Some(rect.id.clone()),
        ));
        return;
    };

    let Some(x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "rect",
            &rect.id,
            "x",
            rect.source_span,
        ));
        return;
    };
    let Some(y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "rect",
            &rect.id,
            "y",
            rect.source_span,
        ));
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

    // BLUR / SHADOW bracket (innermost, behind fill+stroke). Blur wins over
    // shadow when both are set: only one capture bracket is opened at a time.
    let blur_sigma = rect
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let has_blur = blur_sigma.is_some();
    if let Some(sigma) = blur_sigma {
        commands.push(SceneCommand::BeginBlur { radius: sigma });
    }
    let has_shadow = !has_blur
        && match rect
            .shadow
            .as_ref()
            .and_then(|p| resolve_property_shadow(p, resolved, &rect.id))
        {
            Some(shadows) => {
                commands.push(SceneCommand::BeginShadow { shadows });
                true
            }
            None => false,
        };

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
                commands.push(SceneCommand::FillRoundedRectGradient {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    radii,
                    gradient,
                });
            } else {
                commands.push(SceneCommand::FillRectGradient {
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
                commands.push(SceneCommand::FillRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    radii,
                    color,
                });
            } else {
                commands.push(SceneCommand::FillRect { x, y, w, h, color });
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
                commands.push(SceneCommand::StrokeRoundedRect {
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
                commands.push(SceneCommand::StrokeRect {
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

    if has_shadow {
        commands.push(SceneCommand::EndShadow);
    }
    if has_blur {
        commands.push(SceneCommand::EndBlur);
    }

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
    ctx: RenderCtx,
) {
    // Skip invisible ellipses.
    if ellipse.visible == Some(false) {
        return;
    }

    // Resolve geometry — all four are required; skip if any is absent
    // or uses an unsupported unit.
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
        (&ellipse.x, &ellipse.y, &ellipse.w, &ellipse.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "ellipse '{}' is missing one or more geometry properties (x, y, w, h); \
                 skipped",
                ellipse.id
            ),
            ellipse.source_span,
            Some(ellipse.id.clone()),
        ));
        return;
    };

    let Some(x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "ellipse",
            &ellipse.id,
            "x",
            ellipse.source_span,
        ));
        return;
    };
    let Some(y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "ellipse",
            &ellipse.id,
            "y",
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

    // BLUR / SHADOW bracket (behind fill+stroke). Blur wins when both set.
    let blur_sigma = ellipse
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let has_blur = blur_sigma.is_some();
    if let Some(sigma) = blur_sigma {
        commands.push(SceneCommand::BeginBlur { radius: sigma });
    }
    let has_shadow = !has_blur
        && match ellipse
            .shadow
            .as_ref()
            .and_then(|p| resolve_property_shadow(p, resolved, &ellipse.id))
        {
            Some(shadows) => {
                commands.push(SceneCommand::BeginShadow { shadows });
                true
            }
            None => false,
        };

    // FILL (emitted first, under the stroke) — node-local prop overrides
    // style cascade.
    let fill_prop = ellipse
        .fill
        .as_ref()
        .or_else(|| style_prop(&ellipse.style, style_map, "fill"));
    if let Some(fill_prop) = fill_prop {
        if let Some(mut gradient) = resolve_property_gradient(fill_prop, resolved, &ellipse.id) {
            apply_gradient_opacity(&mut gradient, color_op, 1.0);
            commands.push(SceneCommand::FillEllipseGradient {
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
            commands.push(SceneCommand::FillEllipse {
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

        commands.push(SceneCommand::StrokeEllipse {
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

    if has_shadow {
        commands.push(SceneCommand::EndShadow);
    }
    if has_blur {
        commands.push(SceneCommand::EndBlur);
    }

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
        commands.push(SceneCommand::StrokePolyline {
            points: flat_points,
            color,
            stroke_width,
            closed: true,
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
        });
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
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
