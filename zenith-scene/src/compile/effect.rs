//! Scene compilation for effect-producing nodes.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, LightNode, MeshNode, ResolvedToken};

use crate::ir::{GradientPaint, GradientStop, Paint, SceneCommand};

use super::RenderCtx;
use super::leaf::resolve_dash_params;
use super::paint::resolve_property_color;
use super::util::resolve_property_dimension_px;

pub(super) fn compile_light(
    light: &LightNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    if light.visible == Some(false) {
        return;
    }

    let x = resolve_property_dimension_px(light.x.as_ref(), resolved, 0.0) + ctx.dx;
    let y = resolve_property_dimension_px(light.y.as_ref(), resolved, 0.0) + ctx.dy;
    let radius = resolve_property_dimension_px(light.radius.as_ref(), resolved, 0.0);
    if !radius.is_finite() || radius <= 0.0 {
        return;
    }

    let Some(color_prop) = light.color.as_ref() else {
        return;
    };
    let Some(mut color) = resolve_property_color(color_prop, resolved, diagnostics, &light.id)
    else {
        return;
    };
    let opacity = light.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * ctx.opacity;
    color.a = (color.a as f64 * opacity).round() as u8;
    let mut transparent = color;
    transparent.a = 0;

    commands.push(SceneCommand::FillEllipse {
        x: x - radius,
        y: y - radius,
        w: radius * 2.0,
        h: radius * 2.0,
        rx: Some(radius),
        ry: Some(radius),
        paint: Paint::Gradient(GradientPaint {
            angle_deg: 0.0,
            stops: vec![
                GradientStop { offset: 0.0, color },
                GradientStop {
                    offset: 1.0,
                    color: transparent,
                },
            ],
            radial: true,
            center_x: Some(0.5),
            center_y: Some(0.5),
            radius_frac: Some(1.0),
        }),
    });
}

#[derive(Clone, Copy)]
struct MeshBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    extend: f64,
}

#[derive(Clone, Copy)]
struct MeshStroke {
    color: crate::ir::Color,
    stroke_width: f64,
    stroke_dash: Option<f64>,
    stroke_gap: Option<f64>,
    stroke_linecap: Option<crate::ir::LineCap>,
}

pub(super) fn compile_mesh(
    mesh: &MeshNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    if mesh.visible == Some(false) {
        return;
    }

    let Some(stroke_prop) = mesh.stroke.as_ref() else {
        return;
    };
    let Some(mut color) = resolve_property_color(stroke_prop, resolved, diagnostics, &mesh.id)
    else {
        return;
    };

    let opacity = mesh.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * ctx.opacity;
    color.a = (color.a as f64 * opacity).round() as u8;
    if color.a == 0 {
        return;
    }

    let box_px = MeshBox {
        x: resolve_property_dimension_px(mesh.x.as_ref(), resolved, 0.0) + ctx.dx,
        y: resolve_property_dimension_px(mesh.y.as_ref(), resolved, 0.0) + ctx.dy,
        w: resolve_property_dimension_px(mesh.w.as_ref(), resolved, 0.0),
        h: resolve_property_dimension_px(mesh.h.as_ref(), resolved, 0.0),
        extend: resolve_property_dimension_px(mesh.extend.as_ref(), resolved, 0.0).max(0.0),
    };
    if !box_px.x.is_finite()
        || !box_px.y.is_finite()
        || !box_px.w.is_finite()
        || !box_px.h.is_finite()
        || box_px.w <= 0.0
        || box_px.h <= 0.0
    {
        return;
    }

    let (stroke_dash, stroke_gap, stroke_linecap) = resolve_dash_params(
        mesh.stroke_dash.as_ref(),
        mesh.stroke_gap.as_ref(),
        mesh.stroke_linecap.as_deref(),
        resolved,
    );
    let stroke = MeshStroke {
        color,
        stroke_width: resolve_property_dimension_px(mesh.stroke_width.as_ref(), resolved, 1.0),
        stroke_dash,
        stroke_gap,
        stroke_linecap,
    };
    if !stroke.stroke_width.is_finite() || stroke.stroke_width <= 0.0 {
        return;
    }

    match mesh.kind.as_deref().unwrap_or("orthographic") {
        "perspective" => compile_perspective_mesh(mesh, resolved, box_px, stroke, commands),
        "orthographic" => compile_orthographic_mesh(mesh, box_px, stroke, commands),
        _ => compile_orthographic_mesh(mesh, box_px, stroke, commands),
    }
}

fn compile_orthographic_mesh(
    mesh: &MeshNode,
    box_px: MeshBox,
    stroke: MeshStroke,
    commands: &mut Vec<SceneCommand>,
) {
    let rows = mesh.rows.unwrap_or(1).max(1);
    let columns = mesh.columns.unwrap_or(1).max(1);

    for index in 0..=rows {
        let t = f64::from(index) / f64::from(rows);
        let y = box_px.y + box_px.h * t;
        push_mesh_line(
            commands,
            box_px.x - box_px.extend,
            y,
            box_px.x + box_px.w + box_px.extend,
            y,
            stroke,
        );
    }
    for index in 0..=columns {
        let t = f64::from(index) / f64::from(columns);
        let x = box_px.x + box_px.w * t;
        push_mesh_line(
            commands,
            x,
            box_px.y - box_px.extend,
            x,
            box_px.y + box_px.h + box_px.extend,
            stroke,
        );
    }
}

fn compile_perspective_mesh(
    mesh: &MeshNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    box_px: MeshBox,
    stroke: MeshStroke,
    commands: &mut Vec<SceneCommand>,
) {
    let rows = mesh.rows.unwrap_or(1).max(1);
    let columns = mesh.columns.unwrap_or(1).max(1);
    let vx = resolve_property_dimension_px(mesh.vanishing_x.as_ref(), resolved, box_px.x);
    let vy = resolve_property_dimension_px(mesh.vanishing_y.as_ref(), resolved, box_px.y);
    if !vx.is_finite() || !vy.is_finite() {
        return;
    }

    let bottom_y = box_px.y + box_px.h + box_px.extend;
    for index in 0..=columns {
        let t = f64::from(index) / f64::from(columns);
        let bottom_x = box_px.x - box_px.extend + (box_px.w + box_px.extend * 2.0) * t;
        let (x2, y2) = extend_toward(bottom_x, bottom_y, vx, vy, box_px.extend);
        push_mesh_line(commands, bottom_x, bottom_y, x2, y2, stroke);
    }

    for index in 0..=rows {
        let t = f64::from(index) / f64::from(rows);
        let y = box_px.y + box_px.h * t;
        let left = interpolate_ray_at_y(box_px.x - box_px.extend, bottom_y, vx, vy, y);
        let right = interpolate_ray_at_y(box_px.x + box_px.w + box_px.extend, bottom_y, vx, vy, y);
        if let (Some(x1), Some(x2)) = (left, right) {
            push_mesh_line(commands, x1, y, x2, y, stroke);
        }
    }
}

fn interpolate_ray_at_y(x0: f64, y0: f64, vx: f64, vy: f64, y: f64) -> Option<f64> {
    let denom = vy - y0;
    if denom.abs() < 0.000_001 {
        return None;
    }
    let t = (y - y0) / denom;
    Some(x0 + (vx - x0) * t)
}

fn extend_toward(x0: f64, y0: f64, vx: f64, vy: f64, extend: f64) -> (f64, f64) {
    let dx = vx - x0;
    let dy = vy - y0;
    let len = dx.hypot(dy);
    if !len.is_finite() || len <= 0.000_001 {
        return (vx, vy);
    }
    let target = (len + extend) / len;
    (x0 + dx * target, y0 + dy * target)
}

fn push_mesh_line(
    commands: &mut Vec<SceneCommand>,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    stroke: MeshStroke,
) {
    if !x1.is_finite() || !y1.is_finite() || !x2.is_finite() || !y2.is_finite() {
        return;
    }
    commands.push(SceneCommand::StrokeLine {
        x1,
        y1,
        x2,
        y2,
        color: stroke.color,
        stroke_width: stroke.stroke_width,
        stroke_dash: stroke.stroke_dash,
        stroke_gap: stroke.stroke_gap,
        stroke_linecap: stroke.stroke_linecap,
    });
}
