//! Scene compilation for effect-producing nodes.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, LightNode, ResolvedToken};

use crate::ir::{GradientPaint, GradientStop, Paint, SceneCommand};

use super::RenderCtx;
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
