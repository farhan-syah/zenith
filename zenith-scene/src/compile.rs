//! Scene compilation: `Document` → `CompileResult`.
//!
//! Entry point: [`compile`].
//!
//! Rect, ellipse, line, text, and group nodes are compiled; the page
//! background is emitted first; unknown nodes produce an advisory diagnostic
//! and are skipped.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Document, FontProvider, FontStyle, FrameNode, GroupNode, ImageNode, Node,
    ObjectPosition, Point, PolygonNode, PolylineNode, PropertyValue, ResolvedToken, ResolvedValue,
    Span, Style, Unit, resolve_tokens,
};
use zenith_layout::{RustybuzzEngine, ShapeRequest, TextLayoutEngine};

use crate::color::parse_srgb_hex;
use crate::ir::{Color, FitMode, Scene, SceneCommand, SceneGlyph};

// ── Render context ────────────────────────────────────────────────────────────

/// Per-subtree rendering context that cascades through the node tree.
///
/// Each field accumulates transformations as we descend:
/// - `opacity` — multiplied together at each group boundary; leaf nodes
///   apply it on top of their own node-level opacity.
/// - `dx`/`dy` — translation offset accumulated from all ancestor groups
///   with an `x`/`y` property; added to every leaf geometry position.
#[derive(Clone, Copy)]
struct RenderCtx {
    /// Accumulated opacity multiplier (1.0 = fully opaque).
    opacity: f64,
    /// Accumulated x-translation in pixels.
    dx: f64,
    /// Accumulated y-translation in pixels.
    dy: f64,
}

impl RenderCtx {
    fn root() -> Self {
        RenderCtx {
            opacity: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }
}

// ── Public result type ────────────────────────────────────────────────────────

/// The result of compiling a [`Document`] into a [`Scene`].
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// The compiled display list.
    pub scene: Scene,
    /// All diagnostics collected during compilation (may include token-resolution
    /// diagnostics, unit advisories, and unsupported-node advisories).
    pub diagnostics: Vec<Diagnostic>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

// ── Style cascade helper ──────────────────────────────────────────────────────

/// Look up a style property value by (style_ref, style_map, key).
///
/// Returns `None` when there is no style reference, the style id is not in the
/// map, or the style does not carry the requested key.
fn style_prop<'a>(
    style_ref: &Option<String>,
    style_map: &'a BTreeMap<&str, &Style>,
    key: &str,
) -> Option<&'a PropertyValue> {
    let sid = style_ref.as_deref()?;
    style_map.get(sid)?.properties.get(key)
}

/// Compile `doc` into a [`CompileResult`], using `fonts` to shape text nodes.
///
/// Only the first page is compiled.  If the document has no pages an empty
/// scene is returned with an advisory diagnostic.
///
/// Pass `&zenith_core::default_provider()` to use the bundled Noto Sans
/// font, which is sufficient for basic text rendering.
///
/// # No-panic guarantee
///
/// This function never calls `unwrap`, `expect`, `panic!`, `todo!`,
/// `unimplemented!`, or performs unchecked indexing.  All failure paths push a
/// diagnostic and continue.
pub fn compile(doc: &Document, fonts: &dyn FontProvider) -> CompileResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // ── Step 1: resolve tokens ────────────────────────────────────────────
    let token_resolution = resolve_tokens(&doc.tokens);
    diagnostics.extend(token_resolution.diagnostics);
    let resolved = &token_resolution.resolved;

    // ── Step 1b: build style lookup map ──────────────────────────────────
    let style_map: BTreeMap<&str, &Style> = doc
        .styles
        .styles
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    // ── Step 2: select the first page ────────────────────────────────────
    let Some(page) = doc.body.pages.first() else {
        diagnostics.push(Diagnostic::advisory(
            "scene.no_pages",
            "document has no pages; an empty scene is returned",
            None,
            Some(doc.body.id.clone()),
        ));
        return CompileResult {
            scene: Scene::new(0.0, 0.0),
            diagnostics,
        };
    };

    // ── Step 3: page dimensions → pixels ─────────────────────────────────
    let page_w = match dim_to_px(page.width.value, &page.width.unit) {
        Some(v) => v,
        None => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_unit",
                format!(
                    "page '{}' width uses an unsupported unit; cannot compile scene",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
            return CompileResult {
                scene: Scene::new(0.0, 0.0),
                diagnostics,
            };
        }
    };
    let page_h = match dim_to_px(page.height.value, &page.height.unit) {
        Some(v) => v,
        None => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_unit",
                format!(
                    "page '{}' height uses an unsupported unit; cannot compile scene",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
            return CompileResult {
                scene: Scene::new(0.0, 0.0),
                diagnostics,
            };
        }
    };

    let mut scene = Scene::new(page_w, page_h);

    // ── Step 4: outermost page-edge clip (doc 09 normative rule) ─────────
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: page_w,
        h: page_h,
    });

    // ── Step 5: optional page background ─────────────────────────────────
    if let Some(bg_prop) = &page.background
        && let Some(color) = resolve_property_color(bg_prop, resolved, &mut diagnostics, &page.id)
    {
        scene.commands.push(SceneCommand::FillRect {
            x: 0.0,
            y: 0.0,
            w: page_w,
            h: page_h,
            color,
        });
    }

    // ── Step 6: children in source order (z-order: first = bottom) ───────
    let engine = RustybuzzEngine::new();
    for node in &page.children {
        compile_node(
            node,
            resolved,
            &style_map,
            fonts,
            &engine,
            &mut scene.commands,
            &mut diagnostics,
            RenderCtx::root(),
        );
    }

    // ── Step 7: close the outermost clip ─────────────────────────────────
    scene.commands.push(SceneCommand::PopClip);

    CompileResult { scene, diagnostics }
}

// ── Node dispatch ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn compile_node(
    node: &Node,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    match node {
        Node::Rect(rect) => {
            // Skip invisible rects.
            if rect.visible == Some(false) {
                return;
            }

            // Resolve geometry — all four are required; skip if any is absent
            // or uses an unsupported unit.
            let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
                (&rect.x, &rect.y, &rect.w, &rect.h)
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

            // Resolve fill color — node-local prop overrides style cascade.
            let fill_prop = rect
                .fill
                .as_ref()
                .or_else(|| style_prop(&rect.style, style_map, "fill"));
            let Some(fill_prop) = fill_prop else {
                // No fill → nothing to draw for a fill-only skeleton.
                return;
            };
            let Some(mut color) =
                resolve_property_color(fill_prop, resolved, diagnostics, &rect.id)
            else {
                return;
            };

            // Apply node opacity then cascade ctx.opacity on top.
            let node_opacity = rect.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
            color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

            commands.push(SceneCommand::FillRect { x, y, w, h, color });
        }

        Node::Ellipse(ellipse) => {
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

            // Resolve fill color — node-local prop overrides style cascade.
            let fill_prop = ellipse
                .fill
                .as_ref()
                .or_else(|| style_prop(&ellipse.style, style_map, "fill"));
            let Some(fill_prop) = fill_prop else {
                // No fill → nothing to draw for a fill-only primitive.
                return;
            };
            let Some(mut color) =
                resolve_property_color(fill_prop, resolved, diagnostics, &ellipse.id)
            else {
                return;
            };

            // Apply node opacity then cascade ctx.opacity on top.
            let node_opacity = ellipse.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
            color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

            commands.push(SceneCommand::FillEllipse { x, y, w, h, color });
        }

        Node::Text(text) => {
            // Skip invisible text nodes.
            if text.visible == Some(false) {
                return;
            }

            // Resolve geometry — x and y are required; skip if absent or bad unit.
            let (Some(x_dim), Some(y_dim)) = (&text.x, &text.y) else {
                diagnostics.push(Diagnostic::advisory(
                    "scene.missing_geometry",
                    format!(
                        "text node '{}' is missing x or y geometry; skipped",
                        text.id
                    ),
                    text.source_span,
                    Some(text.id.clone()),
                ));
                return;
            };

            let Some(text_x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "text node",
                    &text.id,
                    "x",
                    text.source_span,
                ));
                return;
            };
            let Some(text_y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "text node",
                    &text.id,
                    "y",
                    text.source_span,
                ));
                return;
            };

            // Apply group translation offset.
            let text_x = text_x_raw + ctx.dx;
            let text_y = text_y_raw + ctx.dy;

            // Concatenate span text; skip silently if empty (nothing to draw).
            let content: String = text.spans.iter().map(|s| s.text.as_str()).collect();
            if content.is_empty() {
                return;
            }

            // Resolve font family with style cascade.
            // Priority: node-local font_family → style font-family → default "Noto Sans".
            let font_family_prop = text
                .font_family
                .as_ref()
                .or_else(|| style_prop(&text.style, style_map, "font-family"));
            let family_name: String = match font_family_prop {
                Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
                    Some(rt) => match &rt.value {
                        ResolvedValue::FontFamily(name) => name.clone(),
                        _ => "Noto Sans".to_owned(),
                    },
                    None => "Noto Sans".to_owned(),
                },
                Some(PropertyValue::Literal(name)) => name.clone(),
                None => "Noto Sans".to_owned(),
            };
            let families = vec![family_name];

            // Resolve font size in pixels with style cascade; default to 16.0 if absent.
            let font_size_prop = text
                .font_size
                .clone()
                .or_else(|| style_prop(&text.style, style_map, "font-size").cloned());
            let font_size: f32 =
                resolve_property_dimension_px(&font_size_prop, resolved, 16.0) as f32;

            // Resolve fill color with style cascade; default to opaque black.
            let fill_prop = text
                .fill
                .as_ref()
                .or_else(|| style_prop(&text.style, style_map, "fill"));
            let mut color = fill_prop
                .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
                .unwrap_or(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                });

            // Apply node opacity then cascade ctx.opacity on top.
            let node_opacity = text.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
            color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

            // Shape the text.
            // Weight and style are hardcoded to 400/Normal — the TextNode AST
            // does not yet carry weight/style fields (future unit).
            let req = ShapeRequest {
                text: &content,
                families: &families,
                weight: 400,
                style: FontStyle::Normal,
                font_size,
            };

            match engine.shape(&req, fonts) {
                Err(e) => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.text_unshaped",
                        format!("text node '{}' could not be shaped: {}", text.id, e.message),
                        text.source_span,
                        Some(text.id.clone()),
                    ));
                }
                Ok(run) => {
                    let baseline_y = text_y + run.ascent as f64;
                    let glyphs: Vec<SceneGlyph> = run
                        .glyphs
                        .iter()
                        .map(|g| SceneGlyph {
                            glyph_id: g.glyph_id,
                            dx: g.x,
                            dy: g.y,
                        })
                        .collect();

                    commands.push(SceneCommand::DrawGlyphRun {
                        x: text_x,
                        y: baseline_y,
                        font_id: run.font_id,
                        font_size: run.font_size,
                        color,
                        glyphs,
                    });
                }
            }
        }

        Node::Line(line) => {
            // Skip invisible lines.
            if line.visible == Some(false) {
                return;
            }

            // Require all four endpoints; skip if any is absent or bad unit.
            let (Some(x1d), Some(y1d), Some(x2d), Some(y2d)) =
                (&line.x1, &line.y1, &line.x2, &line.y2)
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
            let Some(mut color) =
                resolve_property_color(stroke_prop, resolved, diagnostics, &line.id)
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

            commands.push(SceneCommand::StrokeLine {
                x1,
                y1,
                x2,
                y2,
                color,
                stroke_width,
            });
        }

        Node::Frame(frame) => {
            compile_frame(
                frame,
                resolved,
                style_map,
                fonts,
                engine,
                commands,
                diagnostics,
                ctx,
            );
        }

        Node::Group(group) => {
            compile_group(
                group,
                resolved,
                style_map,
                fonts,
                engine,
                commands,
                diagnostics,
                ctx,
            );
        }

        Node::Image(image) => {
            compile_image(image, commands, diagnostics, ctx);
        }

        Node::Polygon(poly) => {
            compile_polygon(poly, resolved, style_map, commands, diagnostics, ctx);
        }

        Node::Polyline(poly) => {
            compile_polyline(poly, resolved, style_map, commands, diagnostics, ctx);
        }

        Node::Unknown(unknown) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_node",
                format!(
                    "unknown node kind '{}' cannot be compiled; the node is skipped \
                     (forward-compatibility: this kind may be supported in a later version)",
                    unknown.kind
                ),
                unknown.source_span,
                None,
            ));
        }
    }
}

// NOTE: compile_frame → compile_node → compile_frame recursion has no depth
// guard, consistent with the compile_group limitation in v0.
#[allow(clippy::too_many_arguments)]
fn compile_frame(
    frame: &FrameNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false (no PushClip emitted).
    if frame.visible == Some(false) {
        return;
    }

    // All four geometry dimensions are required for a frame clip rectangle.
    // Resolve them BEFORE pushing any PushClip to keep push/pop balanced.
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
        (&frame.x, &frame.y, &frame.w, &frame.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "frame '{}' is missing one or more geometry properties (x, y, w, h); \
                 skipped",
                frame.id
            ),
            frame.source_span,
            Some(frame.id.clone()),
        ));
        return;
    };

    let Some(frame_x) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "x",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_y) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "y",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "w",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "h",
            frame.source_span,
        ));
        return;
    };

    // Clip rectangle is the frame's own bbox.
    commands.push(SceneCommand::PushClip {
        x: frame_x,
        y: frame_y,
        w: frame_w,
        h: frame_h,
    });

    // Frame clips only — it does NOT translate children (dx/dy unchanged).
    // Opacity cascades into all descendant alphas exactly as group does.
    // DEFERRED: frame rotate (universal rotate deferral — not applied here).
    let child_ctx = RenderCtx {
        opacity: ctx.opacity * frame.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
        dx: ctx.dx, // clip-only: no translation
        dy: ctx.dy, // clip-only: no translation
    };

    for child in &frame.children {
        compile_node(
            child,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            child_ctx,
        );
    }

    commands.push(SceneCommand::PopClip);
    // Frame emits no fill of its own in v0.
}

// NOTE: compile_group → compile_node → compile_group recursion has no depth
// guard.  Pathologically deep group trees can overflow the stack.  This is a
// known v0 limitation; a guard will be added when nested documents are tested.
#[allow(clippy::too_many_arguments)]
fn compile_group(
    group: &GroupNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false.
    if group.visible == Some(false) {
        return;
    }

    // Cascade opacity: multiply the group's own opacity into the inherited ctx.
    let group_opacity = group.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let child_opacity = ctx.opacity * group_opacity;

    // Resolve group x/y to pixels; absent or unsupported-unit → 0.0 (no diagnostic).
    let group_x_px = group
        .x
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .unwrap_or(0.0);
    let group_y_px = group
        .y
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .unwrap_or(0.0);

    let child_dx = ctx.dx + group_x_px;
    let child_dy = ctx.dy + group_y_px;

    // DEFERRED: group rotate — consistent with the universal rotate deferral
    // (no node applies rotate yet).

    // Emit children in source order; the group itself produces no command.
    let child_ctx = RenderCtx {
        opacity: child_opacity,
        dx: child_dx,
        dy: child_dy,
    };
    for child in &group.children {
        compile_node(
            child,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            child_ctx,
        );
    }
}

/// Compile an `image` leaf node.
///
/// Mirrors the frame box-clip pattern: resolve geometry first (so early
/// returns stay push/pop balanced), then emit `PushClip(box)` → `DrawImage` →
/// `PopClip`. The box-clip is the normative image box-clip (doc 09 G-22): the
/// raster is ALWAYS clipped to its declared `[x, y, w, h]` box. `compile_node`
/// needs no asset provider here — the asset id string is enough; bytes are
/// resolved at render time.
fn compile_image(
    image: &ImageNode,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    // Skip invisible images.
    if image.visible == Some(false) {
        return;
    }

    // All four geometry dimensions are required. Resolve BEFORE PushClip so
    // any early return keeps push/pop balanced.
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
        (&image.x, &image.y, &image.w, &image.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "image '{}' is missing one or more geometry properties (x, y, w, h); skipped",
                image.id
            ),
            image.source_span,
            Some(image.id.clone()),
        ));
        return;
    };

    let Some(x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "image",
            &image.id,
            "x",
            image.source_span,
        ));
        return;
    };
    let Some(y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "image",
            &image.id,
            "y",
            image.source_span,
        ));
        return;
    };
    let Some(w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "image",
            &image.id,
            "w",
            image.source_span,
        ));
        return;
    };
    let Some(h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "image",
            &image.id,
            "h",
            image.source_span,
        ));
        return;
    };

    // Apply group translation offset.
    let x = x_raw + ctx.dx;
    let y = y_raw + ctx.dy;

    // Effective opacity: node opacity × cascaded ctx opacity.
    let opacity = image.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * ctx.opacity;

    // Map fit string → FitMode. Default (absent or unknown) = Stretch.
    let fit = match image.fit.as_deref() {
        Some("contain") => FitMode::Contain,
        Some("cover") => FitMode::Cover,
        Some("none") => FitMode::None,
        _ => FitMode::Stretch,
    };

    let pos_x = object_pos_to_f64(&image.object_position_x);
    let pos_y = object_pos_to_f64(&image.object_position_y);

    // Box-clip (G-22): push the box, draw the image, pop. The image is always
    // clipped to its declared box ∩ enclosing clips.
    commands.push(SceneCommand::PushClip { x, y, w, h });
    commands.push(SceneCommand::DrawImage {
        x,
        y,
        w,
        h,
        asset_id: image.asset.clone(),
        fit,
        pos_x,
        pos_y,
        opacity,
    });
    commands.push(SceneCommand::PopClip);
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
fn compile_polygon(
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
}

/// Compile a `polyline` leaf node.
///
/// Emits `FillPolygon` (if fill is present, renderer closes the path implicitly)
/// THEN `StrokePolyline { closed: false }` (if stroke is present).
///
/// Points are in absolute document coordinates — `ctx.dx`/`ctx.dy` are added
/// exactly as for `line` endpoints.
fn compile_polyline(
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
}

/// Resolve an object-position anchor to `0.0..=100.0`.
///
/// `None` defaults to `50.0` (centered); `Start`→0, `Center`→50, `End`→100,
/// `Pct(n)`→`n` clamped to `0..=100`.
fn object_pos_to_f64(pos: &Option<ObjectPosition>) -> f64 {
    match pos {
        None => 50.0,
        Some(ObjectPosition::Start) => 0.0,
        Some(ObjectPosition::Center) => 50.0,
        Some(ObjectPosition::End) => 100.0,
        Some(ObjectPosition::Pct(n)) => n.clamp(0.0, 100.0),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a dimension value + unit to pixels.
///
/// Returns `None` for unsupported / unknown units (caller pushes advisory).
fn dim_to_px(value: f64, unit: &Unit) -> Option<f64> {
    match unit {
        Unit::Px => Some(value),
        Unit::Pt => Some(value * 96.0 / 72.0),
        Unit::Pct | Unit::Deg | Unit::Unknown(_) => None,
    }
}

/// Build a `scene.unsupported_unit` advisory for a named geometry field.
///
/// `kind` is the human-readable node kind (e.g. `"rect"`, `"ellipse"`,
/// `"line"`, `"text node"`) used in the diagnostic message.
fn unsupported_unit_diag(kind: &str, node_id: &str, field: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::advisory(
        "scene.unsupported_unit",
        format!(
            "{} '{}' field '{}' uses an unsupported unit; the {} is skipped",
            kind, node_id, field, kind
        ),
        span,
        Some(node_id.to_owned()),
    )
}

/// Resolve an optional dimension-valued property to pixels.
///
/// Returns `default` when the property is absent, is a raw literal, references
/// a non-dimension (or unresolved) token, or carries an unsupported unit. The
/// idiomatic path is a token ref resolving to a `Dimension`. Shared by
/// font-size and stroke-width resolution.
fn resolve_property_dimension_px(
    prop: &Option<PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: f64,
) -> f64 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::Dimension(dim) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
                _ => default,
            },
            None => default,
        },
        _ => default,
    }
}

/// Resolve a `PropertyValue` to a `Color`, or push a diagnostic and return
/// `None`.
///
/// Accepts:
/// - `TokenRef(id)` → looks up in `resolved`, must be a `ResolvedValue::Color`.
/// - `Literal(hex)` → parses as sRGB hex string directly.
fn resolve_property_color(
    prop: &PropertyValue,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    subject_id: &str,
) -> Option<Color> {
    match prop {
        PropertyValue::TokenRef(token_id) => {
            match resolved.get(token_id.as_str()) {
                Some(rt) => match &rt.value {
                    ResolvedValue::Color(hex) => match parse_srgb_hex(hex) {
                        Some(c) => Some(c),
                        None => {
                            // Should not happen — token resolution validates hex —
                            // but be robust.
                            diagnostics.push(Diagnostic::advisory(
                                "scene.invalid_color",
                                format!(
                                    "token '{}' resolved to '{}' which is not a valid \
                                     sRGB hex color; skipped",
                                    token_id, hex
                                ),
                                None,
                                Some(subject_id.to_owned()),
                            ));
                            None
                        }
                    },
                    other => {
                        diagnostics.push(Diagnostic::advisory(
                            "scene.wrong_token_type",
                            format!(
                                "node '{}' references token '{}' which resolved to a \
                                 non-color value ({:?}); skipped",
                                subject_id, token_id, other
                            ),
                            None,
                            Some(subject_id.to_owned()),
                        ));
                        None
                    }
                },
                None => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.unresolved_token",
                        format!(
                            "node '{}' references token '{}' which did not resolve \
                             (check token diagnostics); skipped",
                            subject_id, token_id
                        ),
                        None,
                        Some(subject_id.to_owned()),
                    ));
                    None
                }
            }
        }
        PropertyValue::Literal(hex) => match parse_srgb_hex(hex) {
            Some(c) => Some(c),
            None => {
                diagnostics.push(Diagnostic::advisory(
                    "scene.invalid_color",
                    format!(
                        "node '{}' has a fill literal '{}' that is not a valid \
                         sRGB hex color; skipped",
                        subject_id, hex
                    ),
                    None,
                    Some(subject_id.to_owned()),
                ));
                None
            }
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_core::{KdlAdapter, KdlSource, default_provider};

    // ── Helper to parse a .zen source string ──────────────────────────────

    fn parse(src: &str) -> Document {
        KdlAdapter
            .parse(src.as_bytes())
            .expect("test document must parse")
    }

    // ── Minimal single-rect document ──────────────────────────────────────

    /// A page with a single full-page rect filled via a token color.
    /// Expected scene: PushClip → FillRect (bg from token) → FillRect (rect) → PopClip.
    /// In this test the page has no background, so background FillRect is absent.
    #[test]
    fn single_rect_token_fill_compiles_correctly() {
        let src = r##"zenith version=1 {
  project id="proj.t1" name="T1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.t1" title="T1" {
    page id="page.t1" w=(px)640 h=(px)360 {
      rect id="rect.t1" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

        assert!(
            matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 640.0 && h == 360.0),
            "first command must be PushClip covering the page"
        );

        match &cmds[1] {
            SceneCommand::FillRect { x, y, w, h, color } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*w, 640.0);
                assert_eq!(*h, 360.0);
                // #f8fafc → r=0xf8=248, g=0xfa=250, b=0xfc=252, a=255
                assert_eq!(color.r, 0xf8);
                assert_eq!(color.g, 0xfa);
                assert_eq!(color.b, 0xfc);
                assert_eq!(color.a, 255);
            }
            other => panic!("expected FillRect, got {other:?}"),
        }

        assert!(
            matches!(cmds[2], SceneCommand::PopClip),
            "last command must be PopClip"
        );
    }

    // ── Two rects → two FillRects in source order ─────────────────────────

    #[test]
    fn two_rects_emitted_in_source_order() {
        let src = r##"zenith version=1 {
  project id="proj.t2" name="T2"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#111111"
    token id="color.b" type="color" value="#222222"
  }
  styles {}
  document id="doc.t2" title="T2" {
    page id="page.t2" w=(px)100 h=(px)100 {
      rect id="rect.a" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.a"
      rect id="rect.b" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.b"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect(a), FillRect(b), PopClip
        assert_eq!(cmds.len(), 4, "expected 4 commands, got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x11),
            other => panic!("expected FillRect for rect.a, got {other:?}"),
        }
        match &cmds[2] {
            SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x22),
            other => panic!("expected FillRect for rect.b, got {other:?}"),
        }
    }

    // ── visible=false rect is not emitted ─────────────────────────────────

    #[test]
    fn invisible_rect_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.t3" name="T3"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#abcdef"
  }
  styles {}
  document id="doc.t3" title="T3" {
    page id="page.t3" w=(px)100 h=(px)100 {
      rect id="rect.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" visible=#false
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        // No diagnostics expected (visible=false is a normal skip, not an error).
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Only PushClip + PopClip; no FillRect.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── JSON schema field is "zenith-scene-v1" ────────────────────────────

    #[test]
    fn json_schema_field_value() {
        let src = r##"zenith version=1 {
  project id="proj.t5" name="T5"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.t5" title="T5" {
    page id="page.t5" w=(px)100 h=(px)100 {}
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let json = result.scene.to_json().expect("serialize must succeed");
        assert!(
            json.contains(r#""schema": "zenith-scene-v1""#),
            "JSON must contain schema field; got snippet: {}",
            &json[..json.len().min(200)]
        );
    }

    // ── JSON determinism ──────────────────────────────────────────────────

    #[test]
    fn json_serialization_is_deterministic() {
        let src = r##"zenith version=1 {
  project id="proj.t6" name="T6"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#aabbcc"
  }
  styles {}
  document id="doc.t6" title="T6" {
    page id="page.t6" w=(px)200 h=(px)100 {
      rect id="rect.t6" x=(px)10 y=(px)20 w=(px)100 h=(px)50 fill=(token)"color.fill"
    }
  }
}
"##;
        let doc = parse(src);
        let r1 = compile(&doc, &default_provider());
        let r2 = compile(&doc, &default_provider());
        let j1 = r1.scene.to_json().expect("serialize 1");
        let j2 = r2.scene.to_json().expect("serialize 2");
        assert_eq!(
            j1, j2,
            "two compiles of the same doc must produce identical JSON"
        );
    }

    // ── Page background emitted as first FillRect ─────────────────────────

    #[test]
    fn page_background_emitted_before_children() {
        let src = r##"zenith version=1 {
  project id="proj.t7" name="T7"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
    token id="color.fill" type="color" value="#000000"
  }
  styles {}
  document id="doc.t7" title="T7" {
    page id="page.t7" w=(px)100 h=(px)100 background=(token)"color.bg" {
      rect id="rect.t7" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect(bg=white), FillRect(rect=black), PopClip
        assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);

        // Background fill must be white.
        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => {
                assert_eq!(color.r, 255, "bg must be white");
                assert_eq!(color.g, 255);
                assert_eq!(color.b, 255);
            }
            other => panic!("expected background FillRect, got {other:?}"),
        }

        // Child rect must be black.
        match &cmds[2] {
            SceneCommand::FillRect { color, .. } => {
                assert_eq!(color.r, 0, "child rect must be black");
                assert_eq!(color.g, 0);
                assert_eq!(color.b, 0);
            }
            other => panic!("expected child FillRect, got {other:?}"),
        }
    }

    // ── Opacity multiplied into alpha ─────────────────────────────────────

    #[test]
    fn opacity_applied_to_fill_alpha() {
        // A full-alpha color (#ffffff, a=255) with opacity=0.5 → a≈128.
        let src = r##"zenith version=1 {
  project id="proj.t8" name="T8"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.t8" title="T8" {
    page id="page.t8" w=(px)100 h=(px)100 {
      rect id="rect.t8" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" opacity=0.5
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        match &result.scene.commands[1] {
            SceneCommand::FillRect { color, .. } => {
                // 255 * 0.5 = 127.5 → rounds to 128.
                assert_eq!(color.a, 128, "opacity 0.5 must give a=128; got {}", color.a);
            }
            other => panic!("expected FillRect, got {other:?}"),
        }
    }

    // ── Text node with token-resolved fill/font/size → DrawGlyphRun ───────

    #[test]
    fn text_node_token_resolved_compiles_to_draw_glyph_run() {
        // A page with a text node whose fill, font-family, and font-size all
        // reference tokens.  Shaping uses the bundled Noto Sans provider.
        let src = r##"zenith version=1 {
  project id="proj.tx1" name="TX1"
  tokens format="zenith-token-v1" {
    token id="color.ink"     type="color"      value="#111827"
    token id="font.body"     type="fontFamily" value="Noto Sans"
    token id="size.body"     type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.tx1" title="TX1" {
    page id="page.tx1" w=(px)400 h=(px)200 {
      text id="label.tx1" x=(px)10 y=(px)20 w=(px)380 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
        span "Hello Zenith"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        // No shaping errors expected.
        let unshaped: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == "scene.text_unshaped")
            .collect();
        assert!(
            unshaped.is_empty(),
            "no text_unshaped diagnostics expected; got: {:?}",
            result.diagnostics
        );

        // Commands: PushClip, DrawGlyphRun, PopClip.
        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[2], SceneCommand::PopClip));

        match &cmds[1] {
            SceneCommand::DrawGlyphRun {
                x,
                y,
                font_id,
                font_size,
                color,
                glyphs,
            } => {
                // x is the text-box origin x.
                assert_eq!(*x, 10.0, "x must be text-box origin (10px)");
                // y is baseline = text_y + ascent; ascent > 0, so y > 20.0.
                assert!(*y > 20.0, "baseline y must be > text_y (20px); got {}", y);
                // font_id must be the stable Noto Sans id.
                assert_eq!(
                    font_id, "noto-sans-400-normal",
                    "font_id must be noto-sans-400-normal"
                );
                assert_eq!(*font_size, 24.0, "font_size must be 24px");
                // Fill color: #111827 → r=0x11=17, g=0x18=24, b=0x27=39.
                assert_eq!(color.r, 0x11, "color.r must be 0x11");
                assert_eq!(color.g, 0x18, "color.g must be 0x18");
                assert_eq!(color.b, 0x27, "color.b must be 0x27");
                assert_eq!(color.a, 255, "color.a must be 255 (opaque)");
                // Glyph run must be non-empty.
                assert!(
                    !glyphs.is_empty(),
                    "glyphs must be non-empty for 'Hello Zenith'"
                );
            }
            other => panic!("expected DrawGlyphRun, got {other:?}"),
        }
    }

    // ── Rect then text → FillRect before DrawGlyphRun (z-order) ──────────

    #[test]
    fn rect_then_text_z_order_preserved() {
        let src = r##"zenith version=1 {
  project id="proj.tx2" name="TX2"
  tokens format="zenith-token-v1" {
    token id="color.bg"  type="color"      value="#ffffff"
    token id="color.ink" type="color"      value="#000000"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension"  value=(px)16
  }
  styles {}
  document id="doc.tx2" title="TX2" {
    page id="page.tx2" w=(px)400 h=(px)200 {
      rect id="bg.rect" x=(px)0 y=(px)0 w=(px)400 h=(px)200 fill=(token)"color.bg"
      text id="label.tx2" x=(px)10 y=(px)20 w=(px)380 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
        span "Hello"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let cmds = &result.scene.commands;
        // PushClip, FillRect, DrawGlyphRun, PopClip
        assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
        assert!(
            matches!(cmds[1], SceneCommand::FillRect { .. }),
            "second command must be FillRect (rect comes first)"
        );
        assert!(
            matches!(cmds[2], SceneCommand::DrawGlyphRun { .. }),
            "third command must be DrawGlyphRun (text comes after rect)"
        );
    }

    // ── Scene JSON of text contains DrawGlyphRun op + font_id, no byte arrays ─

    #[test]
    fn scene_json_draw_glyph_run_op_and_font_id_no_bytes() {
        let src = r##"zenith version=1 {
  project id="proj.tx3" name="TX3"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color"      value="#333333"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension"  value=(px)18
  }
  styles {}
  document id="doc.tx3" title="TX3" {
    page id="page.tx3" w=(px)300 h=(px)100 {
      text id="label.tx3" x=(px)0 y=(px)0 w=(px)300 h=(px)50 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
        span "Hi"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let j1 = result.scene.to_json().expect("serialize 1");
        let j2 = result.scene.to_json().expect("serialize 2");

        // Must contain the op tag.
        assert!(
            j1.contains(r#""op": "DrawGlyphRun""#),
            "JSON must contain DrawGlyphRun op; snippet: {}",
            &j1[..j1.len().min(500)]
        );
        // Must contain the font_id string.
        assert!(
            j1.contains("noto-sans-400-normal"),
            "JSON must contain font_id; snippet: {}",
            &j1[..j1.len().min(500)]
        );
        // Must NOT contain a large byte array (no font bytes in IR).
        // Large byte arrays appear as `[1, 2, 3, ...]` with > ~50 numbers.
        // A simple heuristic: no run of more than 10 consecutive numbers separated by ", ".
        // We check that the JSON does not contain "bytes" as a key.
        assert!(
            !j1.contains(r#""bytes""#),
            "JSON must not contain a 'bytes' field; font bytes must not appear in the IR"
        );
        // Determinism: two serializations must be identical.
        assert_eq!(j1, j2, "two serializations must be identical (determinism)");
    }

    // ── Group: children emitted in source order ───────────────────────────

    #[test]
    fn group_children_emitted_in_order() {
        // A page with a bg rect and a group containing a rect then an ellipse.
        // After PushClip + bg FillRect, the group produces: FillRect, FillEllipse.
        let src = r##"zenith version=1 {
  project id="proj.gc" name="GC"
  tokens format="zenith-token-v1" {
    token id="color.bg"   type="color" value="#ffffff"
    token id="color.r"    type="color" value="#ff0000"
    token id="color.e"    type="color" value="#0000ff"
  }
  styles {}
  document id="doc.gc" title="GC" {
    page id="page.gc" w=(px)320 h=(px)200 background=(token)"color.bg" {
      group id="group.gc" {
        rect id="rect.gc" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.r"
        ellipse id="ellipse.gc" x=(px)70 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.e"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect(bg), FillRect(rect.gc), FillEllipse(ellipse.gc), PopClip
        assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(
            matches!(cmds[1], SceneCommand::FillRect { .. }),
            "cmd[1] must be bg FillRect"
        );
        assert!(
            matches!(cmds[2], SceneCommand::FillRect { .. }),
            "cmd[2] must be group-child FillRect"
        );
        assert!(
            matches!(cmds[3], SceneCommand::FillEllipse { .. }),
            "cmd[3] must be group-child FillEllipse"
        );
        assert!(matches!(cmds[4], SceneCommand::PopClip));
    }

    // ── Group: visible=false → entire subtree excluded ────────────────────

    #[test]
    fn invisible_group_subtree_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.gv" name="GV"
  tokens format="zenith-token-v1" {
    token id="color.r" type="color" value="#ff0000"
    token id="color.b" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.gv" title="GV" {
    page id="page.gv" w=(px)100 h=(px)100 {
      group id="group.gv" visible=#false {
        rect id="rect.gv1" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.r"
        rect id="rect.gv2" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.b"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Only PushClip + PopClip; both children excluded because group is invisible.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── Group: opacity cascades to child alpha ────────────────────────────

    #[test]
    fn group_opacity_cascades_to_child() {
        // Group opacity=0.5, child rect fill is fully opaque #ffffff (a=255).
        // Expected child FillRect alpha ≈ 128 (255 * 1.0 * 0.5 = 127.5 → 128).
        let src = r##"zenith version=1 {
  project id="proj.go" name="GO"
  tokens format="zenith-token-v1" {
    token id="color.w" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.go" title="GO" {
    page id="page.go" w=(px)100 h=(px)100 {
      group id="group.go" opacity=0.5 {
        rect id="rect.go" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.w"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => {
                // 255 * 1.0 (node opacity) * 0.5 (group opacity) = 127.5 → 128.
                assert_eq!(
                    color.a, 128,
                    "cascaded opacity 0.5 must give a=128; got {}",
                    color.a
                );
            }
            other => panic!("expected FillRect, got {other:?}"),
        }
    }

    // ── Group: x/y translates child geometry ─────────────────────────────

    #[test]
    fn group_xy_translates_child() {
        // Group x=(px)10 y=(px)20; child rect at x=(px)5 y=(px)5.
        // Expected FillRect at x=15.0 y=25.0.
        let src = r##"zenith version=1 {
  project id="proj.gt" name="GT"
  tokens format="zenith-token-v1" {
    token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.gt" title="GT" {
    page id="page.gt" w=(px)200 h=(px)200 {
      group id="group.gt" x=(px)10 y=(px)20 {
        rect id="rect.gt" x=(px)5 y=(px)5 w=(px)50 h=(px)50 fill=(token)"color.k"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillRect, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillRect { x, y, .. } => {
                assert_eq!(
                    *x, 15.0,
                    "child x must be group.x(10) + rect.x(5) = 15; got {x}"
                );
                assert_eq!(
                    *y, 25.0,
                    "child y must be group.y(20) + rect.y(5) = 25; got {y}"
                );
            }
            other => panic!("expected FillRect, got {other:?}"),
        }
    }

    // ── Unresolvable font → text_unshaped advisory, no DrawGlyphRun ──────

    #[test]
    fn unresolvable_font_family_produces_text_unshaped_advisory() {
        let src = r##"zenith version=1 {
  project id="proj.tx4" name="TX4"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.tx4" title="TX4" {
    page id="page.tx4" w=(px)200 h=(px)100 {
      text id="label.tx4" x=(px)0 y=(px)0 w=(px)200 h=(px)50 fill="#000000" font-family="Nonexistent" {
        span "test"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        // Must have exactly one advisory with code "scene.text_unshaped".
        let unshaped: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == "scene.text_unshaped")
            .collect();
        assert_eq!(
            unshaped.len(),
            1,
            "expected 1 text_unshaped advisory; got: {:?}",
            result.diagnostics
        );

        // No DrawGlyphRun emitted.
        let glyph_cmds: Vec<_> = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
            .collect();
        assert!(
            glyph_cmds.is_empty(),
            "no DrawGlyphRun expected when font is unresolvable; got: {:?}",
            glyph_cmds
        );
    }

    // ── Ellipse: token fill compiles to FillEllipse ───────────────────────

    #[test]
    fn single_ellipse_token_fill_compiles_correctly() {
        let src = r##"zenith version=1 {
  project id="proj.e1" name="E1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.e1" title="E1" {
    page id="page.e1" w=(px)640 h=(px)360 {
      ellipse id="ellipse.e1" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, FillEllipse, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

        assert!(
            matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 640.0 && h == 360.0),
            "first command must be PushClip covering the page"
        );

        match &cmds[1] {
            SceneCommand::FillEllipse { x, y, w, h, color } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*w, 640.0);
                assert_eq!(*h, 360.0);
                // #f8fafc → r=0xf8=248, g=0xfa=250, b=0xfc=252, a=255
                assert_eq!(color.r, 0xf8);
                assert_eq!(color.g, 0xfa);
                assert_eq!(color.b, 0xfc);
                assert_eq!(color.a, 255);
            }
            other => panic!("expected FillEllipse, got {other:?}"),
        }

        assert!(
            matches!(cmds[2], SceneCommand::PopClip),
            "last command must be PopClip"
        );
    }

    // ── Ellipse: visible=false not emitted ────────────────────────────────

    #[test]
    fn invisible_ellipse_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.e2" name="E2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#abcdef"
  }
  styles {}
  document id="doc.e2" title="E2" {
    page id="page.e2" w=(px)100 h=(px)100 {
      ellipse id="ellipse.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" visible=#false
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Only PushClip + PopClip; no FillEllipse.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── Line: token stroke compiles to StrokeLine ─────────────────────────

    #[test]
    fn single_line_token_stroke_compiles_correctly() {
        let src = r##"zenith version=1 {
  project id="proj.l1" name="L1"
  tokens format="zenith-token-v1" {
    token id="color.rule" type="color" value="#94a3b8"
    token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.l1" title="L1" {
    page id="page.l1" w=(px)320 h=(px)200 {
      line id="line.divider" x1=(px)40 y1=(px)100 x2=(px)280 y2=(px)100 stroke=(token)"color.rule" stroke-width=(token)"size.stroke"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip, StrokeLine, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

        assert!(
            matches!(cmds[0], SceneCommand::PushClip { .. }),
            "first command must be PushClip"
        );

        match &cmds[1] {
            SceneCommand::StrokeLine {
                x1,
                y1,
                x2,
                y2,
                color,
                stroke_width,
            } => {
                assert_eq!(*x1, 40.0);
                assert_eq!(*y1, 100.0);
                assert_eq!(*x2, 280.0);
                assert_eq!(*y2, 100.0);
                // #94a3b8 → r=0x94=148, g=0xa3=163, b=0xb8=184
                assert_eq!(color.r, 0x94);
                assert_eq!(color.g, 0xa3);
                assert_eq!(color.b, 0xb8);
                assert_eq!(color.a, 255);
                // size.stroke = (px)2
                assert_eq!(*stroke_width, 2.0);
            }
            other => panic!("expected StrokeLine, got {other:?}"),
        }

        assert!(
            matches!(cmds[2], SceneCommand::PopClip),
            "last command must be PopClip"
        );
    }

    // ── Line: visible=false not emitted ──────────────────────────────────

    #[test]
    fn invisible_line_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.l2" name="L2"
  tokens format="zenith-token-v1" {
    token id="color.rule" type="color" value="#94a3b8"
  }
  styles {}
  document id="doc.l2" title="L2" {
    page id="page.l2" w=(px)100 h=(px)100 {
      line id="line.hidden" x1=(px)0 y1=(px)50 x2=(px)100 y2=(px)50 stroke=(token)"color.rule" visible=#false
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Only PushClip + PopClip; no StrokeLine.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── Frame: PushClip → FillRect(child) → PopClip sequence ─────────────

    #[test]
    fn frame_emits_pushclip_children_popclip() {
        let src = r##"zenith version=1 {
  project id="proj.f1" name="F1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.f1" title="F1" {
    page id="page.f1" w=(px)320 h=(px)200 {
      frame id="frame.clip" x=(px)40 y=(px)40 w=(px)120 h=(px)100 {
        rect id="rect.inner" x=(px)50 y=(px)50 w=(px)60 h=(px)60 fill=(token)"color.fill"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Page PushClip, Frame PushClip, FillRect(child), Frame PopClip, Page PopClip
        assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

        // Page clip
        assert!(
            matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 320.0 && h == 200.0),
            "cmd[0] must be page PushClip"
        );
        // Frame clip — the frame's own bbox
        assert!(
            matches!(cmds[1], SceneCommand::PushClip { x, y, w, h } if x == 40.0 && y == 40.0 && w == 120.0 && h == 100.0),
            "cmd[1] must be frame PushClip at (40,40,120,100); got: {:?}",
            cmds[1]
        );
        // Child FillRect
        assert!(
            matches!(cmds[2], SceneCommand::FillRect { .. }),
            "cmd[2] must be child FillRect"
        );
        // Frame PopClip
        assert!(
            matches!(cmds[3], SceneCommand::PopClip),
            "cmd[3] must be frame PopClip"
        );
        // Page PopClip
        assert!(
            matches!(cmds[4], SceneCommand::PopClip),
            "cmd[4] must be page PopClip"
        );
    }

    // ── Frame: child overflow still emitted (renderer clips, not compiler) ─

    #[test]
    fn frame_child_overflow_still_emitted() {
        // Child rect extends well beyond the frame bounds — compiler must emit
        // its full FillRect unchanged; clipping is the renderer's job.
        let src = r##"zenith version=1 {
  project id="proj.f2" name="F2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#f97316"
  }
  styles {}
  document id="doc.f2" title="F2" {
    page id="page.f2" w=(px)320 h=(px)200 {
      frame id="frame.clip" x=(px)40 y=(px)40 w=(px)120 h=(px)100 {
        rect id="rect.overflow" x=(px)100 y=(px)30 w=(px)100 h=(px)120 fill=(token)"color.fill"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Ensure child FillRect is present with its full (unclipped) geometry.
        let fill_rects: Vec<_> = cmds
            .iter()
            .filter_map(|c| {
                if let SceneCommand::FillRect { x, y, w, h, .. } = c {
                    Some((*x, *y, *w, *h))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(fill_rects.len(), 1, "expected exactly one FillRect");
        let (rx, ry, rw, rh) = fill_rects[0];
        assert_eq!(
            rx, 100.0,
            "child FillRect x must be 100 (absolute, unclipped)"
        );
        assert_eq!(ry, 30.0, "child FillRect y must be 30");
        assert_eq!(rw, 100.0, "child FillRect w must be 100");
        assert_eq!(rh, 120.0, "child FillRect h must be 120");
    }

    // ── Frame: missing geometry → advisory, no PushClip ───────────────────

    #[test]
    fn frame_missing_geometry_skipped() {
        // Frame with x=None; compile must push a scene.missing_geometry advisory
        // and emit NO PushClip (so push/pop balance is preserved).
        let src = r##"zenith version=1 {
  project id="proj.f3" name="F3"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.f3" title="F3" {
    page id="page.f3" w=(px)100 h=(px)100 {
      frame id="frame.nogeo" y=(px)0 w=(px)100 h=(px)100 {
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let missing: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == "scene.missing_geometry")
            .collect();
        assert_eq!(
            missing.len(),
            1,
            "expected 1 scene.missing_geometry advisory; got: {:?}",
            result.diagnostics
        );

        // Push/pop must still be balanced: only page PushClip + PopClip.
        let push_count = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::PushClip { .. }))
            .count();
        let pop_count = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::PopClip))
            .count();
        assert_eq!(push_count, pop_count, "PushClip/PopClip must be balanced");
        assert_eq!(push_count, 1, "only the page PushClip must be present");
    }

    // ── Frame: visible=false → entire subtree excluded ────────────────────

    #[test]
    fn invisible_frame_subtree_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.f4" name="F4"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.f4" title="F4" {
    page id="page.f4" w=(px)100 h=(px)100 {
      frame id="frame.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 visible=#false {
        rect id="rect.inner" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.fill"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // Only page PushClip + PopClip; no frame PushClip, no FillRect.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── Frame: opacity cascades to child alpha ─────────────────────────────

    #[test]
    fn frame_opacity_cascades_to_child() {
        // Frame opacity=0.5, child rect fill fully opaque #ffffff (a=255).
        // Expected child FillRect alpha ≈ 128 (255 * 1.0 * 0.5 = 127.5 → 128).
        let src = r##"zenith version=1 {
  project id="proj.f5" name="F5"
  tokens format="zenith-token-v1" {
    token id="color.w" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.f5" title="F5" {
    page id="page.f5" w=(px)100 h=(px)100 {
      frame id="frame.opaque" x=(px)0 y=(px)0 w=(px)100 h=(px)100 opacity=0.5 {
        rect id="rect.inner" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.w"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let fill_rect = result
            .scene
            .commands
            .iter()
            .find(|c| matches!(c, SceneCommand::FillRect { .. }));
        match fill_rect {
            Some(SceneCommand::FillRect { color, .. }) => {
                // 255 * 1.0 (node opacity) * 0.5 (frame opacity) = 127.5 → 128.
                assert_eq!(
                    color.a, 128,
                    "cascaded opacity 0.5 must give a=128; got {}",
                    color.a
                );
            }
            _ => panic!("expected a FillRect command"),
        }
    }

    // ── Frame: does NOT translate children (clip-only) ─────────────────────

    #[test]
    fn frame_does_not_translate_child() {
        // Frame at x=(px)40 y=(px)40; child rect at x=(px)50 y=(px)50.
        // Because frame is clip-only (no translation), the child FillRect must
        // be at x=50.0 y=50.0, NOT 90.0/90.0.
        let src = r##"zenith version=1 {
  project id="proj.f6" name="F6"
  tokens format="zenith-token-v1" {
    token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.f6" title="F6" {
    page id="page.f6" w=(px)200 h=(px)200 {
      frame id="frame.noxlate" x=(px)40 y=(px)40 w=(px)120 h=(px)120 {
        rect id="rect.abs" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.k"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let fill_rect = result
            .scene
            .commands
            .iter()
            .find(|c| matches!(c, SceneCommand::FillRect { .. }));
        match fill_rect {
            Some(SceneCommand::FillRect { x, y, .. }) => {
                assert_eq!(
                    *x, 50.0,
                    "child x must be 50 (absolute, frame does not translate); got {x}"
                );
                assert_eq!(
                    *y, 50.0,
                    "child y must be 50 (absolute, frame does not translate); got {y}"
                );
            }
            _ => panic!("expected a FillRect command"),
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Image node compile tests
    // ══════════════════════════════════════════════════════════════════════

    use crate::ir::FitMode;

    // ── image → PushClip, DrawImage, PopClip with default fields ──────────

    #[test]
    fn image_emits_pushclip_drawimage_popclip() {
        let src = r##"zenith version=1 {
  project id="proj.i1" name="I1"
  assets {
    asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i1" title="I1" {
    page id="page.i1" w=(px)320 h=(px)200 {
      image id="img.i1" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 fit="stretch"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        // PushClip(page), PushClip(box), DrawImage, PopClip(box), PopClip(page)
        assert_eq!(cmds.len(), 5, "expected 5 commands, got: {:?}", cmds);
        assert!(
            matches!(cmds[1], SceneCommand::PushClip { x, y, w, h } if x == 40.0 && y == 40.0 && w == 160.0 && h == 120.0),
            "cmd[1] must be the image box PushClip"
        );
        match &cmds[2] {
            SceneCommand::DrawImage {
                x,
                y,
                w,
                h,
                asset_id,
                fit,
                pos_x,
                pos_y,
                opacity,
            } => {
                assert_eq!(*x, 40.0);
                assert_eq!(*y, 40.0);
                assert_eq!(*w, 160.0);
                assert_eq!(*h, 120.0);
                assert_eq!(asset_id, "asset.swatch");
                assert_eq!(*fit, FitMode::Stretch);
                assert_eq!(*pos_x, 50.0, "default object-position-x must be 50");
                assert_eq!(*pos_y, 50.0, "default object-position-y must be 50");
                assert_eq!(*opacity, 1.0);
            }
            other => panic!("expected DrawImage, got {other:?}"),
        }
        assert!(matches!(cmds[3], SceneCommand::PopClip));
    }

    // ── image fit="cover" + object-position-x=(pct)25 → mapped fields ─────

    #[test]
    fn image_fit_and_object_position_mapped() {
        let src = r##"zenith version=1 {
  project id="proj.i2" name="I2"
  assets {
    asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i2" title="I2" {
    page id="page.i2" w=(px)320 h=(px)200 {
      image id="img.i2" asset="asset.swatch" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" object-position-x=(pct)25 object-position-y="start"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let draw = result
            .scene
            .commands
            .iter()
            .find_map(|c| match c {
                SceneCommand::DrawImage {
                    fit, pos_x, pos_y, ..
                } => Some((*fit, *pos_x, *pos_y)),
                _ => None,
            })
            .expect("must emit a DrawImage");
        assert_eq!(draw.0, FitMode::Cover);
        assert_eq!(draw.1, 25.0, "object-position-x (pct)25 → 25.0");
        assert_eq!(draw.2, 0.0, "object-position-y start → 0.0");
    }

    // ── invisible image is not emitted ────────────────────────────────────

    #[test]
    fn invisible_image_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.i3" name="I3"
  assets {
    asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i3" title="I3" {
    page id="page.i3" w=(px)320 h=(px)200 {
      image id="img.i3" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 visible=#false
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let cmds = &result.scene.commands;
        // Only the page PushClip + PopClip; no image commands.
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {cmds:?}"
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SceneCommand::DrawImage { .. })),
            "no DrawImage expected for invisible image"
        );
    }

    // ── image opacity cascades under a group opacity ──────────────────────

    #[test]
    fn image_opacity_cascades() {
        // Group opacity 0.5 × image opacity 0.5 = 0.25.
        let src = r##"zenith version=1 {
  project id="proj.i4" name="I4"
  assets {
    asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i4" title="I4" {
    page id="page.i4" w=(px)320 h=(px)200 {
      group id="group.i4" opacity=0.5 {
        image id="img.i4" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 opacity=0.5
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let opacity = result
            .scene
            .commands
            .iter()
            .find_map(|c| match c {
                SceneCommand::DrawImage { opacity, .. } => Some(*opacity),
                _ => None,
            })
            .expect("must emit a DrawImage");
        assert!(
            (opacity - 0.25).abs() < 1e-9,
            "cascaded opacity must be 0.25; got {opacity}"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Polygon / Polyline compile tests
    // ══════════════════════════════════════════════════════════════════════

    // ── polygon: fill + stroke emits FillPolygon then StrokePolyline(closed) ─

    #[test]
    fn polygon_emits_fill_and_stroke() {
        let src = r##"zenith version=1 {
  project id="proj.p1" name="P1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ff0000"
    token id="color.stroke" type="color" value="#000000"
    token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.p1" title="P1" {
    page id="page.p1" w=(px)320 h=(px)200 {
      polygon id="poly.tri" fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.stroke" {
        point x=(px)160 y=(px)40
        point x=(px)260 y=(px)170
        point x=(px)60 y=(px)170
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        // PushClip, FillPolygon, StrokePolyline, PopClip
        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 4, "expected 4 commands, got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillPolygon {
                points,
                color,
                even_odd,
            } => {
                // 3 points × 2 = 6 coordinates
                assert_eq!(points.len(), 6, "must have 6 flat coords");
                assert_eq!(points[0], 160.0, "x0 must be 160");
                assert_eq!(points[1], 40.0, "y0 must be 40");
                assert_eq!(color.r, 255, "fill color must be red");
                assert!(!even_odd, "even_odd must be false by default");
            }
            other => panic!("cmd[1] must be FillPolygon, got {other:?}"),
        }

        match &cmds[2] {
            SceneCommand::StrokePolyline {
                points,
                closed,
                color,
                stroke_width,
            } => {
                assert_eq!(points.len(), 6);
                assert!(closed, "polygon stroke must be closed");
                assert_eq!(color.r, 0, "stroke color must be black");
                assert!((stroke_width - 2.0).abs() < 1e-9);
            }
            other => panic!("cmd[2] must be StrokePolyline, got {other:?}"),
        }
    }

    // ── polygon: fill-rule="evenodd" → FillPolygon.even_odd == true ───────

    #[test]
    fn polygon_evenodd_fill_rule() {
        let src = r##"zenith version=1 {
  project id="proj.p2" name="P2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.p2" title="P2" {
    page id="page.p2" w=(px)200 h=(px)200 {
      polygon id="poly.star" fill=(token)"color.fill" fill-rule="evenodd" {
        point x=(px)100 y=(px)10
        point x=(px)40 y=(px)180
        point x=(px)190 y=(px)60
        point x=(px)10 y=(px)60
        point x=(px)160 y=(px)180
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let fp = result.scene.commands.iter().find_map(|c| match c {
            SceneCommand::FillPolygon { even_odd, .. } => Some(*even_odd),
            _ => None,
        });
        assert_eq!(fp, Some(true), "fill-rule=evenodd must set even_odd=true");
    }

    // ── polyline: stroke-only → one StrokePolyline(closed:false), no FillPolygon ─

    #[test]
    fn polyline_emits_open_stroke() {
        let src = r##"zenith version=1 {
  project id="proj.pl1" name="PL1"
  tokens format="zenith-token-v1" {
    token id="color.stroke" type="color" value="#334155"
    token id="size.stroke" type="dimension" value=(px)3
  }
  styles {}
  document id="doc.pl1" title="PL1" {
    page id="page.pl1" w=(px)320 h=(px)200 {
      polyline id="line.conn" stroke=(token)"color.stroke" stroke-width=(token)"size.stroke" {
        point x=(px)40 y=(px)100
        point x=(px)120 y=(px)60
        point x=(px)200 y=(px)140
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        // PushClip, StrokePolyline, PopClip — no FillPolygon
        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SceneCommand::FillPolygon { .. })),
            "stroke-only polyline must not emit FillPolygon"
        );

        match &cmds[1] {
            SceneCommand::StrokePolyline { points, closed, .. } => {
                assert_eq!(points.len(), 6, "3 points × 2 = 6 flat coords");
                assert!(!closed, "polyline stroke must NOT be closed");
            }
            other => panic!("cmd[1] must be StrokePolyline, got {other:?}"),
        }
    }

    // ── polygon: visible=false → not emitted ──────────────────────────────

    #[test]
    fn invisible_polygon_not_emitted() {
        let src = r##"zenith version=1 {
  project id="proj.p3" name="P3"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.p3" title="P3" {
    page id="page.p3" w=(px)100 h=(px)100 {
      polygon id="poly.hidden" fill=(token)"color.fill" visible=#false {
        point x=(px)10 y=(px)10
        point x=(px)90 y=(px)10
        point x=(px)50 y=(px)90
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        let cmds = &result.scene.commands;
        assert_eq!(
            cmds.len(),
            2,
            "expected PushClip + PopClip only; got: {:?}",
            cmds
        );
        assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
        assert!(matches!(cmds[1], SceneCommand::PopClip));
    }

    // ── polygon: group opacity 0.5 cascades into fill color.a ─────────────

    #[test]
    fn polygon_opacity_cascades() {
        let src = r##"zenith version=1 {
  project id="proj.p4" name="P4"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.p4" title="P4" {
    page id="page.p4" w=(px)200 h=(px)200 {
      group id="grp.p4" opacity=0.5 {
        polygon id="poly.p4" fill=(token)"color.fill" {
          point x=(px)10 y=(px)10
          point x=(px)100 y=(px)10
          point x=(px)55 y=(px)100
        }
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let fill_a = result.scene.commands.iter().find_map(|c| match c {
            SceneCommand::FillPolygon { color, .. } => Some(color.a),
            _ => None,
        });
        // #ffffff α=255, node opacity=1.0, ctx opacity=0.5 → 255*0.5 ≈ 128
        assert!(
            fill_a.map(|a| (a as i32 - 128).abs() <= 1).unwrap_or(false),
            "cascaded opacity 0.5 must halve fill alpha to ≈128; got {fill_a:?}"
        );
    }

    // ── Style cascade tests ───────────────────────────────────────────────

    /// A rect with no local fill but a style that provides fill → FillRect emitted.
    #[test]
    fn rect_inherits_fill_from_style() {
        let src = r##"zenith version=1 {
  project id="proj.sc1" name="SC1"
  tokens format="zenith-token-v1" {
    token id="color.panel" type="color" value="#3b82f6"
  }
  styles {
    style id="style.panel" {
      fill (token)"color.panel"
    }
  }
  document id="doc.sc1" title="SC1" {
    page id="page.sc1" w=(px)320 h=(px)200 {
      rect id="rect.sc1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 style="style.panel"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        // PushClip, FillRect (from style fill), PopClip
        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => {
                // #3b82f6 → r=0x3b=59, g=0x82=130, b=0xf6=246
                assert_eq!(color.r, 0x3b, "r must be 0x3b from style fill");
                assert_eq!(color.g, 0x82, "g must be 0x82 from style fill");
                assert_eq!(color.b, 0xf6, "b must be 0xf6 from style fill");
            }
            other => panic!("expected FillRect from style cascade, got {other:?}"),
        }
    }

    /// A rect with BOTH local fill AND a style fill → local fill wins.
    #[test]
    fn node_local_fill_overrides_style() {
        let src = r##"zenith version=1 {
  project id="proj.sc2" name="SC2"
  tokens format="zenith-token-v1" {
    token id="color.style" type="color" value="#ff0000"
    token id="color.local" type="color" value="#00ff00"
  }
  styles {
    style id="style.red" {
      fill (token)"color.style"
    }
  }
  document id="doc.sc2" title="SC2" {
    page id="page.sc2" w=(px)320 h=(px)200 {
      rect id="rect.sc2" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.local" style="style.red"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => {
                // Must be local (green #00ff00), NOT the style (red #ff0000).
                assert_eq!(color.r, 0x00, "local fill r=0 must override style r=255");
                assert_eq!(color.g, 0xff, "local fill g=255 must override style g=0");
                assert_eq!(color.b, 0x00, "local fill b=0 must override style b=0");
            }
            other => panic!("expected FillRect with local color, got {other:?}"),
        }
    }

    /// A text node with style providing font-size → DrawGlyphRun uses the style size.
    #[test]
    fn text_inherits_font_from_style() {
        let src = r##"zenith version=1 {
  project id="proj.sc3" name="SC3"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#111827"
    token id="size.title" type="dimension" value=(px)32
  }
  styles {
    style id="style.title" {
      fill (token)"color.ink"
      font-size (token)"size.title"
    }
  }
  document id="doc.sc3" title="SC3" {
    page id="page.sc3" w=(px)640 h=(px)360 {
      text id="text.sc3" x=(px)10 y=(px)20 w=(px)400 h=(px)50 style="style.title" {
        span "Hello"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        let unshaped: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == "scene.text_unshaped")
            .collect();
        assert!(
            unshaped.is_empty(),
            "no text_unshaped diagnostics expected; got: {:?}",
            result.diagnostics
        );

        let cmds = &result.scene.commands;
        match cmds
            .iter()
            .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        {
            Some(SceneCommand::DrawGlyphRun {
                font_size, color, ..
            }) => {
                assert_eq!(*font_size, 32.0, "font_size must be 32px from style");
                assert_eq!(
                    color.r, 0x11,
                    "fill must come from style (color.ink r=0x11)"
                );
            }
            _ => panic!("expected DrawGlyphRun from style cascade"),
        }
    }

    /// A polygon with no local fill/stroke but a style providing both → both emitted.
    #[test]
    fn polygon_inherits_stroke_from_style() {
        let src = r##"zenith version=1 {
  project id="proj.sc4" name="SC4"
  tokens format="zenith-token-v1" {
    token id="color.stroke" type="color" value="#ef4444"
    token id="size.sw" type="dimension" value=(px)2
  }
  styles {
    style id="style.outlined" {
      stroke (token)"color.stroke"
      stroke-width (token)"size.sw"
    }
  }
  document id="doc.sc4" title="SC4" {
    page id="page.sc4" w=(px)320 h=(px)200 {
      polygon id="poly.sc4" style="style.outlined" {
        point x=(px)50 y=(px)10
        point x=(px)90 y=(px)90
        point x=(px)10 y=(px)90
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );

        // PushClip, StrokePolyline (no fill), PopClip
        let cmds = &result.scene.commands;
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

        match &cmds[1] {
            SceneCommand::StrokePolyline {
                color,
                stroke_width,
                closed,
                ..
            } => {
                // #ef4444 → r=0xef=239
                assert_eq!(color.r, 0xef, "stroke r must be 0xef from style");
                assert!(
                    (*stroke_width - 2.0).abs() < 0.01,
                    "stroke-width must be 2px from style"
                );
                assert!(closed, "polygon stroke must be closed");
            }
            other => panic!("expected StrokePolyline from style cascade, got {other:?}"),
        }
    }
}
