//! `shape` compound-node compilation — background primitive + owned centered
//! label, reusing the production text path for the label.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, PropertyValue, ResolvedToken, ShapeNode, Style, TextNode, dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::{SceneCommand, StrokeAlign};

use super::super::RenderCtx;
use super::super::anchor::AnchorMap;
use super::super::chain::ChainAssignments;
use super::super::paint::resolve_property_color;
use super::super::style_prop;
use super::super::text::{
    MeasureEnv, TextCompileEnv, compile_text, measure_text_wrapped_height, resolve_text_families,
};
use super::super::util::{
    missing_geometry_diag, px, resolve_anchored_axis, resolve_property_dimension_px,
    rotation_degrees, unsupported_unit_diag,
};

/// Read-only borrow + scalar context for [`compile_shape`] and its label
/// emitter.
///
/// Bundles every map/provider borrow plus the per-subtree [`RenderCtx`] so the
/// shape compiler and its label helper stay under the argument-count lint
/// without an `#[allow]`. All fields are borrows/`Copy` scalars held for the
/// duration of a single compile call.
#[derive(Clone, Copy)]
pub(in crate::compile) struct ShapeCompileEnv<'a> {
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    pub(in crate::compile) chains: &'a ChainAssignments,
    pub(in crate::compile) footnote_markers: &'a BTreeMap<String, String>,
    pub(in crate::compile) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    pub(in crate::compile) anchors: &'a AnchorMap,
    pub(in crate::compile) ctx: RenderCtx,
}

/// Page-absolute bounding box of a resolved shape, shared by the background
/// emitters and the label placer.
#[derive(Clone, Copy)]
struct ShapeBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Geometry + paint inputs for the background-primitive emitters, bundled so each
/// emitter stays under the argument-count lint without an `#[allow]`.
#[derive(Clone, Copy)]
struct ShapeBg<'a> {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color_op: f64,
    fill_prop: Option<&'a PropertyValue>,
    stroke_prop: Option<&'a PropertyValue>,
    stroke_width: f64,
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
pub(in crate::compile) fn compile_shape(
    shape: &ShapeNode,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    env: ShapeCompileEnv,
) {
    let resolved = env.resolved;
    let style_map = env.style_map;
    let ctx = env.ctx;

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
    let anchor_xy = env.anchors.get(&shape.id).copied();

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
    let geom = ShapeBox { x, y, w, h };

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
        resolve_property_dimension_px(sw.as_ref(), resolved, 1.0)
    };

    let bg = ShapeBg {
        x,
        y,
        w,
        h,
        color_op,
        fill_prop,
        stroke_prop,
        stroke_width,
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
            emit_shape_ellipse(shape, resolved, diagnostics, commands, bg);
        }
        Some("decision") => {
            emit_shape_decision(shape, resolved, diagnostics, commands, bg);
        }
        Some("terminator") => {
            // Pill: corner radius = h/2.
            emit_shape_rounded_rect(shape, resolved, diagnostics, commands, h / 2.0, bg);
        }
        // "process" (default) and any unrecognized value: rounded rect using
        // `radius` (0 → plain rect).
        _ => {
            let radius_prop = shape
                .radius
                .clone()
                .or_else(|| style_prop(&shape.style, style_map, "radius").cloned());
            let radius = resolve_property_dimension_px(radius_prop.as_ref(), resolved, 0.0);
            emit_shape_rounded_rect(shape, resolved, diagnostics, commands, radius, bg);
        }
    }

    // OWNED LABEL (painted ON TOP of the background). Emitted INSIDE the
    // rotation bracket so the label rotates with the shape, and using the SAME
    // `ctx` so the shape's opacity/clip cascade onto the glyphs too.
    emit_shape_label(shape, commands, diagnostics, env, geom);

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
fn emit_shape_label(
    shape: &ShapeNode,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    env: ShapeCompileEnv,
    geom: ShapeBox,
) {
    let ShapeCompileEnv {
        resolved,
        style_map,
        fonts,
        engine,
        chains,
        footnote_markers,
        node_boxes,
        anchors,
        ctx,
    } = env;
    let ShapeBox { x, y, w, h } = geom;

    // Nothing to render when the label has no spans.
    if shape.spans.is_empty() {
        return;
    }

    // Padded content box: bbox inset by `padding` (token → px; 0 when absent).
    let pad = resolve_property_dimension_px(shape.padding.as_ref(), resolved, 0.0);
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
        anchor_zone: None,
        anchor_parent: None,
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
        MeasureEnv {
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
        TextCompileEnv {
            resolved,
            style_map,
            fonts,
            engine,
            chains,
            footnote_markers,
            node_boxes,
            anchors,
        },
        commands,
        diagnostics,
        label_ctx,
    );
}

/// Emit the rounded-rect (or plain-rect when `radius <= 0`) background for a
/// `process`/`terminator` shape. Stroke alignment insets the stroked box like
/// `compile_rect`.
fn emit_shape_rounded_rect(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    radius: f64,
    bg: ShapeBg,
) {
    let ShapeBg {
        x,
        y,
        w,
        h,
        color_op,
        fill_prop,
        stroke_prop,
        stroke_width,
    } = bg;

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
fn emit_shape_ellipse(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    bg: ShapeBg,
) {
    let ShapeBg {
        x,
        y,
        w,
        h,
        color_op,
        fill_prop,
        stroke_prop,
        stroke_width,
    } = bg;

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
fn emit_shape_decision(
    shape: &ShapeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    commands: &mut Vec<SceneCommand>,
    bg: ShapeBg,
) {
    let ShapeBg {
        x,
        y,
        w,
        h,
        color_op,
        fill_prop,
        stroke_prop,
        stroke_width,
    } = bg;

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
