//! Scene compilation: `Document` → `CompileResult`.
//!
//! Entry point: [`compile`].
//!
//! Rect, ellipse, line, text, code, and group nodes are compiled; the page
//! background is emitted first; unknown nodes produce an advisory diagnostic
//! and are skipped.
//!
//! [`compile`] renders page 0; [`compile_page`] renders a chosen page by index.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Document, FontProvider, FontStyle, FrameNode, GroupNode, ImageNode, Node,
    ObjectPosition, Point, PolygonNode, PolylineNode, PropertyValue, ResolvedToken, ResolvedValue,
    Span, Style, TokenKind, Unit, builtin_color, is_supported, resolve_tokens, scan,
    token_id_for_kind,
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
/// [`compile_page`] renders a chosen page; this wrapper renders page 0.  If the
/// document has no pages an empty scene is returned with an advisory diagnostic.
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
    compile_page(doc, fonts, 0)
}

/// Compile the page at `page_index` (0-based) of `doc` into a [`CompileResult`],
/// using `fonts` to shape text nodes.
///
/// If the document has no pages an empty scene is returned with a
/// `scene.no_pages` advisory; if `page_index` is out of range (but pages exist)
/// an empty scene is returned with a `scene.page_out_of_range` advisory.
///
/// Pass `&zenith_core::default_provider()` to use the bundled Noto Sans
/// font, which is sufficient for basic text rendering.
///
/// # No-panic guarantee
///
/// This function never calls `unwrap`, `expect`, `panic!`, `todo!`,
/// `unimplemented!`, or performs unchecked indexing (page lookup uses `.get()`).
/// All failure paths push a diagnostic and continue.
pub fn compile_page(doc: &Document, fonts: &dyn FontProvider, page_index: usize) -> CompileResult {
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

    // ── Step 2: select the requested page ────────────────────────────────
    let Some(page) = doc.body.pages.get(page_index) else {
        if doc.body.pages.is_empty() {
            diagnostics.push(Diagnostic::advisory(
                "scene.no_pages",
                "document has no pages; an empty scene is returned",
                None,
                Some(doc.body.id.clone()),
            ));
        } else {
            diagnostics.push(Diagnostic::advisory(
                "scene.page_out_of_range",
                format!(
                    "page index {} is out of range; document has {} page(s)",
                    page_index,
                    doc.body.pages.len()
                ),
                None,
                Some(doc.body.id.clone()),
            ));
        }
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

            // Apply node opacity then cascade ctx.opacity on top.
            let node_opacity = rect.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

            // Resolve corner radius (optional; 0.0 when absent). Node-local
            // overrides style.
            let radius_prop = rect
                .radius
                .clone()
                .or_else(|| style_prop(&rect.style, style_map, "radius").cloned());
            let radius = resolve_property_dimension_px(&radius_prop, resolved, 0.0);

            // FILL (emitted first, under the stroke) — node-local prop overrides
            // style cascade.
            let fill_prop = rect
                .fill
                .as_ref()
                .or_else(|| style_prop(&rect.style, style_map, "fill"));
            if let Some(fill_prop) = fill_prop
                && let Some(mut color) =
                    resolve_property_color(fill_prop, resolved, diagnostics, &rect.id)
            {
                color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
                if radius > 0.0 {
                    commands.push(SceneCommand::FillRoundedRect {
                        x,
                        y,
                        w,
                        h,
                        radius,
                        color,
                    });
                } else {
                    commands.push(SceneCommand::FillRect { x, y, w, h, color });
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
                color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
                let sw = rect
                    .stroke_width
                    .clone()
                    .or_else(|| style_prop(&rect.style, style_map, "stroke-width").cloned());
                let stroke_width = resolve_property_dimension_px(&sw, resolved, 1.0);

                // Stroke alignment offsets the stroke path relative to the box
                // edge by half the stroke width. `center` (default) straddles the
                // edge; `inside`/`outside` shift the whole stroked rectangle in or
                // out. The fill geometry above is unaffected.
                let half = stroke_width / 2.0;
                let (sx, sy, sw_geom, sh_geom, sradius) = match rect.stroke_alignment.as_deref() {
                    // The corner radius only shifts for an already-rounded rect;
                    // a sharp rect (radius 0) must stay sharp.
                    Some("inside") => (
                        x + half,
                        y + half,
                        w - stroke_width,
                        h - stroke_width,
                        if radius > 0.0 {
                            (radius - half).max(0.0)
                        } else {
                            0.0
                        },
                    ),
                    Some("outside") => (
                        x - half,
                        y - half,
                        w + stroke_width,
                        h + stroke_width,
                        if radius > 0.0 { radius + half } else { 0.0 },
                    ),
                    // "center" (default) and any unrecognized value.
                    _ => (x, y, w, h, radius),
                };

                // An inside-aligned stroke can shrink the box to nothing; skip
                // rather than emit a degenerate rectangle.
                if sw_geom > 0.0 && sh_geom > 0.0 {
                    if sradius > 0.0 {
                        commands.push(SceneCommand::StrokeRoundedRect {
                            x: sx,
                            y: sy,
                            w: sw_geom,
                            h: sh_geom,
                            radius: sradius,
                            color,
                            stroke_width,
                        });
                    } else {
                        commands.push(SceneCommand::StrokeRect {
                            x: sx,
                            y: sy,
                            w: sw_geom,
                            h: sh_geom,
                            color,
                            stroke_width,
                        });
                    }
                }
            }
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

            // Skip silently if every span is empty (nothing to draw).
            if text.spans.iter().all(|s| s.text.is_empty()) {
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
                // A dimension is not a family name → fall back to the default.
                Some(PropertyValue::Dimension(_)) => "Noto Sans".to_owned(),
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

            // Node opacity, applied once and cascaded with ctx.opacity onto
            // every span's alpha below.
            let node_opacity = text.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

            // Node-level fill/weight props with style cascade — these are the
            // per-span fallbacks (span override → node → style → default).
            let node_fill_prop: Option<&PropertyValue> = text
                .fill
                .as_ref()
                .or_else(|| style_prop(&text.style, style_map, "fill"));
            let node_weight_prop: Option<&PropertyValue> = text
                .font_weight
                .as_ref()
                .or_else(|| style_prop(&text.style, style_map, "font-weight"));

            // Shape EACH span as its own run, positioning runs left-to-right.
            // Per-span fill and font-weight are honored; family and size are
            // shared (v0 has no per-span family/size override). Cross-span
            // kerning is lost relative to a single concatenated run — accepted
            // for v0. Style is hardcoded to Normal: italic/underline/strike are
            // a future unit (need an italic face / decoration drawing).
            let mut x_cursor = text_x;
            for span in &text.spans {
                if span.text.is_empty() {
                    continue;
                }

                // Per-span fill: span.fill overrides node fill; default black.
                let fill_prop = span.fill.as_ref().or(node_fill_prop);
                let mut color = fill_prop
                    .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
                    .unwrap_or(Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    });
                color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

                // Per-span weight: span.font_weight overrides node weight; 400.
                let weight_prop = span.font_weight.as_ref().or(node_weight_prop);
                let weight = resolve_font_weight(weight_prop, resolved, 400);

                // Per-span italic selects the italic face; otherwise upright.
                let style = if span.italic == Some(true) {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };

                let req = ShapeRequest {
                    text: &span.text,
                    families: &families,
                    weight,
                    style,
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
                        let run_advance = run.advance_width as f64;
                        let glyphs: Vec<SceneGlyph> = run
                            .glyphs
                            .iter()
                            .map(|g| SceneGlyph {
                                glyph_id: g.glyph_id,
                                dx: g.x,
                                dy: g.y,
                            })
                            .collect();

                        // Per-span decorations: a thin filled rule in the span's
                        // own color, spanning the run's advance. Position/thickness
                        // are derived from the font size (the shaped run does not
                        // expose the font's underline metrics) — a deterministic v0
                        // approximation. Emitted before the glyphs so the text sits
                        // on top of any overlap.
                        let deco_thickness = (font_size as f64 / 14.0).max(1.0);
                        if span.underline == Some(true) {
                            commands.push(SceneCommand::FillRect {
                                x: x_cursor,
                                y: baseline_y + font_size as f64 * 0.12,
                                w: run_advance,
                                h: deco_thickness,
                                color,
                            });
                        }
                        if span.strikethrough == Some(true) {
                            commands.push(SceneCommand::FillRect {
                                x: x_cursor,
                                y: baseline_y - font_size as f64 * 0.30,
                                w: run_advance,
                                h: deco_thickness,
                                color,
                            });
                        }

                        commands.push(SceneCommand::DrawGlyphRun {
                            x: x_cursor,
                            y: baseline_y,
                            font_id: run.font_id,
                            font_size: run.font_size,
                            color,
                            glyphs,
                        });

                        // Advance the cursor past this run for the next span.
                        x_cursor += run_advance;
                    }
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

        Node::Code(code) => {
            // Skip invisible code nodes.
            if code.visible == Some(false) {
                return;
            }

            // Resolve geometry — x and y are required; skip if absent or bad unit.
            let (Some(x_dim), Some(y_dim)) = (&code.x, &code.y) else {
                diagnostics.push(Diagnostic::advisory(
                    "scene.missing_geometry",
                    format!(
                        "code node '{}' is missing x or y geometry; skipped",
                        code.id
                    ),
                    code.source_span,
                    Some(code.id.clone()),
                ));
                return;
            };

            let Some(code_x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "code node",
                    &code.id,
                    "x",
                    code.source_span,
                ));
                return;
            };
            let Some(code_y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "code node",
                    &code.id,
                    "y",
                    code.source_span,
                ));
                return;
            };

            // Width/height are OPTIONAL; they bound the clip rectangle when
            // present. A bad unit yields None (no clip), not a hard skip.
            let code_w: Option<f64> = code.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
            let code_h: Option<f64> = code.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));

            // Apply group translation offset.
            let code_x = code_x_raw + ctx.dx;
            let code_y = code_y_raw + ctx.dy;

            // Resolve font family with style cascade.
            // Priority: node-local font_family → style font-family → default
            // "Noto Sans Mono" (the monospace default for code).
            let font_family_prop = code
                .font_family
                .as_ref()
                .or_else(|| style_prop(&code.style, style_map, "font-family"));
            let family_name: String = match font_family_prop {
                Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
                    Some(rt) => match &rt.value {
                        ResolvedValue::FontFamily(name) => name.clone(),
                        _ => "Noto Sans Mono".to_owned(),
                    },
                    None => "Noto Sans Mono".to_owned(),
                },
                Some(PropertyValue::Literal(name)) => name.clone(),
                // A dimension is not a family name → fall back to the default.
                Some(PropertyValue::Dimension(_)) => "Noto Sans Mono".to_owned(),
                None => "Noto Sans Mono".to_owned(),
            };
            let families = vec![family_name];

            // Resolve font size in pixels with style cascade; default to 14.0.
            let font_size_prop = code
                .font_size
                .clone()
                .or_else(|| style_prop(&code.style, style_map, "font-size").cloned());
            let font_size: f32 =
                resolve_property_dimension_px(&font_size_prop, resolved, 14.0) as f32;

            // Resolve fill color with style cascade; default to opaque black.
            let fill_prop = code
                .fill
                .as_ref()
                .or_else(|| style_prop(&code.style, style_map, "fill"));
            let mut color = fill_prop
                .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &code.id))
                .unwrap_or(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                });

            // Apply node opacity then cascade ctx.opacity on top.
            let node_opacity = code.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
            color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

            // Tab expansion: replace each literal tab with `tab_width` spaces
            // (default 4). A `tab_width` of 0 makes tabs vanish — acceptable.
            let tab_width = code.tab_width.unwrap_or(4) as usize;
            let expanded = code.content.replace('\t', &" ".repeat(tab_width));

            // Overflow clip: clipping is the default; only `overflow="visible"`
            // disables it. The clip is applied only when enabled AND both w and
            // h resolved (the clip rectangle is fully determined). Resolve the
            // decision BEFORE the PushClip so push/pop stay balanced across the
            // emission loop (mirrors compile_frame/compile_image discipline).
            let clip_enabled = code.overflow.as_deref() != Some("visible");
            let clip_box = match (clip_enabled, code_w, code_h) {
                (true, Some(w), Some(h)) => Some((w, h)),
                _ => None,
            };
            if let Some((w, h)) = clip_box {
                commands.push(SceneCommand::PushClip {
                    x: code_x,
                    y: code_y,
                    w,
                    h,
                });
            }

            // Resolve syntax-highlighting settings before the per-line loop.
            // `theme` drives builtin fallback colors; `hl_lang` is `Some` only
            // when the node declares a language that oxidoc-highlight supports.
            // When `hl_lang` is `None` the existing single-run path is used
            // unchanged, guaranteeing byte-identical output for all non-highlighted
            // documents.
            let theme = code.syntax_theme.unwrap_or_default();
            let hl_lang: Option<&str> = code.language.as_deref().filter(|l| is_supported(l));

            // Helper: resolve a TokenKind to a Color, consulting doc tokens first
            // and falling back to the builtin palette. Opacity is baked in.
            let syntax_color = |kind: TokenKind| -> Color {
                let hex: &str = resolved
                    .get(token_id_for_kind(kind))
                    .and_then(|rt| match &rt.value {
                        ResolvedValue::Color(h) => Some(h.as_str()),
                        _ => None,
                    })
                    .unwrap_or_else(|| builtin_color(theme, kind));
                let mut c = parse_srgb_hex(hex).unwrap_or(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                });
                c.a = (c.a as f64 * node_opacity * ctx.opacity).round() as u8;
                c
            };

            // Multi-line emission: each physical line becomes its own glyph run,
            // stacked by `line_height`. Blank lines emit no run but the index `i`
            // still advances, preserving their vertical space. All non-blank
            // lines share identical ascent/line_height (same font + size), so the
            // per-line metrics give consistent stacking.
            for (i, line) in expanded.split('\n').enumerate() {
                if line.is_empty() {
                    continue;
                }

                if let Some(lang) = hl_lang {
                    // ── Highlighted path: per-token colored segments ──────────
                    // Tokenise the line, walk gaps between tokens, collect
                    // (text_slice, color) pairs, shape each, then emit.
                    let plain_color = syntax_color(TokenKind::Plain);
                    let tokens = scan(line, lang);

                    // Build segment list: (slice, color)
                    let mut segments: Vec<(&str, Color)> = Vec::new();
                    let mut pos: usize = 0;
                    for tok in &tokens {
                        // Gap before this token → plain color.
                        if tok.start > pos
                            && let Some(gap) = line.get(pos..tok.start)
                            && !gap.is_empty()
                        {
                            segments.push((gap, plain_color));
                        }
                        if let Some(slice) = line.get(tok.start..tok.end)
                            && !slice.is_empty()
                        {
                            segments.push((slice, syntax_color(tok.kind)));
                        }
                        pos = tok.end;
                    }
                    // Trailing gap after last token.
                    if pos < line.len()
                        && let Some(tail) = line.get(pos..)
                        && !tail.is_empty()
                    {
                        segments.push((tail, plain_color));
                    }

                    // Shape all segments; collect (run, color) pairs so metrics
                    // can be read from the first successful run before emitting.
                    let mut shaped = Vec::new();
                    for (seg_text, seg_color) in segments {
                        let req = ShapeRequest {
                            text: seg_text,
                            families: &families,
                            weight: 400,
                            style: FontStyle::Normal,
                            font_size,
                        };
                        match engine.shape(&req, fonts) {
                            Err(e) => {
                                diagnostics.push(Diagnostic::advisory(
                                    "scene.text_unshaped",
                                    format!(
                                        "code node '{}' could not be shaped: {}",
                                        code.id, e.message
                                    ),
                                    code.source_span,
                                    Some(code.id.clone()),
                                ));
                                // Skip this segment; cursor does not advance.
                            }
                            Ok(run) => {
                                shaped.push((run, seg_color));
                            }
                        }
                    }

                    // Emit: metrics are font-constant, read from first shaped run.
                    if let Some((first_run, _)) = shaped.first() {
                        let baseline_y = code_y
                            + first_run.ascent as f64
                            + (i as f64) * first_run.line_height as f64;
                        let mut x_cursor = code_x;
                        for (run, seg_color) in shaped {
                            let advance = run.advance_width as f64;
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
                                x: x_cursor,
                                y: baseline_y,
                                font_id: run.font_id,
                                font_size: run.font_size,
                                color: seg_color,
                                glyphs,
                            });
                            x_cursor += advance;
                        }
                    }
                } else {
                    // ── Plain path (no highlighting): single run per line ──────
                    // Kept byte-identical to the original implementation.
                    let req = ShapeRequest {
                        text: line,
                        families: &families,
                        weight: 400,
                        style: FontStyle::Normal,
                        font_size,
                    };

                    match engine.shape(&req, fonts) {
                        Err(e) => {
                            diagnostics.push(Diagnostic::advisory(
                                "scene.text_unshaped",
                                format!(
                                    "code node '{}' could not be shaped: {}",
                                    code.id, e.message
                                ),
                                code.source_span,
                                Some(code.id.clone()),
                            ));
                            continue;
                        }
                        Ok(run) => {
                            let baseline_y =
                                code_y + run.ascent as f64 + (i as f64) * run.line_height as f64;
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
                                x: code_x,
                                y: baseline_y,
                                font_id: run.font_id,
                                font_size: run.font_size,
                                color,
                                glyphs,
                            });
                        }
                    }
                }
            }

            if clip_box.is_some() {
                commands.push(SceneCommand::PopClip);
            }
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
        // A literal dimension (e.g. `font-size=(px)24`) resolves directly,
        // bringing literal visual dimensions to parity with token-backed ones.
        Some(PropertyValue::Dimension(dim)) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
        _ => default,
    }
}

/// Resolve an optional font-weight property to a numeric weight (100–900).
///
/// Returns `default` when the property is absent, references a non-fontWeight
/// (or unresolved) token, or carries a dimension. The idiomatic path is a token
/// ref resolving to a `FontWeight`. A bare numeric literal (e.g. `font-weight=700`)
/// is parsed directly; an unparsable literal falls back to `default`. Mirrors
/// `resolve_property_dimension_px`.
fn resolve_font_weight(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: u16,
) -> u16 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontWeight(w) => *w as u16,
                _ => default,
            },
            None => default,
        },
        Some(PropertyValue::Literal(s)) => s.parse::<u16>().unwrap_or(default),
        // A dimension is not a weight → fall back to the default.
        Some(PropertyValue::Dimension(_)) => default,
        None => default,
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
        // A dimension is not a color; advise and skip (mirrors wrong-type tokens).
        PropertyValue::Dimension(_) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.wrong_token_type",
                format!(
                    "node '{}' has a dimension value where a color is expected; skipped",
                    subject_id
                ),
                None,
                Some(subject_id.to_owned()),
            ));
            None
        }
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

    // ── rect: fill only → FillRect (regression) ──────────────────────────

    #[test]
    fn rect_fill_only_emits_fill_rect() {
        let src = r##"zenith version=1 {
  project id="proj.rf" name="RF"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.rf" title="RF" {
    page id="page.rf" w=(px)100 h=(px)100 {
      rect id="rect.rf" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill"
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
        assert!(
            matches!(cmds[1], SceneCommand::FillRect { .. }),
            "expected a single FillRect; got {:?}",
            cmds[1]
        );
    }

    // ── rect: fill + stroke → FillRect then StrokeRect ───────────────────

    #[test]
    fn rect_fill_and_stroke_emits_fill_then_stroke() {
        let src = r##"zenith version=1 {
  project id="proj.rfs" name="RFS"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
    token id="color.stroke" type="color" value="#445566"
    token id="size.sw" type="dimension" value=(px)4
  }
  styles {}
  document id="doc.rfs" title="RFS" {
    page id="page.rfs" w=(px)100 h=(px)100 {
      rect id="rect.rfs" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
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
        // PushClip, FillRect, StrokeRect, PopClip
        assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
        match &cmds[1] {
            SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x11),
            other => panic!("expected FillRect first, got {other:?}"),
        }
        match &cmds[2] {
            SceneCommand::StrokeRect {
                color,
                stroke_width,
                ..
            } => {
                assert_eq!(color.r, 0x44, "stroke color r must be 0x44");
                assert!(
                    (*stroke_width - 4.0).abs() < 0.01,
                    "stroke-width must be 4px"
                );
            }
            other => panic!("expected StrokeRect on top, got {other:?}"),
        }
    }

    // ── rect: fill + radius → FillRoundedRect ────────────────────────────

    #[test]
    fn rect_fill_with_radius_emits_fill_rounded_rect() {
        let src = r##"zenith version=1 {
  project id="proj.rfr" name="RFR"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
    token id="size.r" type="dimension" value=(px)8
  }
  styles {}
  document id="doc.rfr" title="RFR" {
    page id="page.rfr" w=(px)100 h=(px)100 {
      rect id="rect.rfr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" radius=(token)"size.r"
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
        // PushClip, FillRoundedRect, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
        match &cmds[1] {
            SceneCommand::FillRoundedRect { radius, color, .. } => {
                assert_eq!(color.r, 0x11);
                assert!((*radius - 8.0).abs() < 0.01, "radius must be 8px");
            }
            other => panic!("expected FillRoundedRect, got {other:?}"),
        }
    }

    // ── rect: fill + stroke + radius → FillRoundedRect then StrokeRoundedRect

    #[test]
    fn rect_fill_stroke_radius_emits_rounded_fill_then_rounded_stroke() {
        let src = r##"zenith version=1 {
  project id="proj.rfsr" name="RFSR"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
    token id="color.stroke" type="color" value="#445566"
    token id="size.sw" type="dimension" value=(px)4
    token id="size.r" type="dimension" value=(px)8
  }
  styles {}
  document id="doc.rfsr" title="RFSR" {
    page id="page.rfsr" w=(px)100 h=(px)100 {
      rect id="rect.rfsr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.sw" radius=(token)"size.r"
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
        // PushClip, FillRoundedRect, StrokeRoundedRect, PopClip
        assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
        match &cmds[1] {
            SceneCommand::FillRoundedRect { radius, .. } => {
                assert!((*radius - 8.0).abs() < 0.01, "fill radius must be 8px");
            }
            other => panic!("expected FillRoundedRect first, got {other:?}"),
        }
        match &cmds[2] {
            SceneCommand::StrokeRoundedRect {
                radius,
                stroke_width,
                color,
                ..
            } => {
                assert_eq!(color.r, 0x44);
                assert!((*radius - 8.0).abs() < 0.01, "stroke radius must be 8px");
                assert!(
                    (*stroke_width - 4.0).abs() < 0.01,
                    "stroke-width must be 4px"
                );
            }
            other => panic!("expected StrokeRoundedRect on top, got {other:?}"),
        }
    }

    // ── rect: stroke only (no fill) → StrokeRect only ────────────────────

    #[test]
    fn rect_stroke_only_emits_stroke_rect() {
        let src = r##"zenith version=1 {
  project id="proj.rso" name="RSO"
  tokens format="zenith-token-v1" {
    token id="color.stroke" type="color" value="#445566"
    token id="size.sw" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.rso" title="RSO" {
    page id="page.rso" w=(px)100 h=(px)100 {
      rect id="rect.rso" x=(px)10 y=(px)10 w=(px)40 h=(px)40 stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
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
        // PushClip, StrokeRect, PopClip
        assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
        match &cmds[1] {
            SceneCommand::StrokeRect {
                color,
                stroke_width,
                ..
            } => {
                assert_eq!(color.r, 0x44);
                assert!(
                    (*stroke_width - 2.0).abs() < 0.01,
                    "stroke-width must be 2px"
                );
            }
            other => panic!("expected a single StrokeRect, got {other:?}"),
        }
    }

    #[test]
    fn rect_stroke_alignment_inside_and_outside_shift_geometry() {
        // sw = 4 → inside shifts in by 2 (x+2, w-4); outside shifts out by 2.
        let doc_for = |align: &str| {
            let src = format!(
                r##"zenith version=1 {{
  project id="proj.sa" name="SA"
  tokens format="zenith-token-v1" {{
    token id="color.stroke" type="color" value="#445566"
    token id="size.sw" type="dimension" value=(px)4
  }}
  styles {{}}
  document id="doc.sa" title="SA" {{
    page id="page.sa" w=(px)200 h=(px)200 {{
      rect id="rect.sa" x=(px)20 y=(px)20 w=(px)100 h=(px)100 stroke=(token)"color.stroke" stroke-width=(token)"size.sw" stroke-alignment="{align}"
    }}
  }}
}}
"##
            );
            let doc = parse(&src);
            compile(&doc, &default_provider())
        };

        let stroke_xywh = |result: &CompileResult| -> (f64, f64, f64, f64) {
            for c in &result.scene.commands {
                if let SceneCommand::StrokeRect { x, y, w, h, .. } = c {
                    return (*x, *y, *w, *h);
                }
            }
            panic!("no StrokeRect emitted");
        };

        assert_eq!(
            stroke_xywh(&doc_for("inside")),
            (22.0, 22.0, 96.0, 96.0),
            "inside must inset the box by sw/2 on each side (w - sw)"
        );
        assert_eq!(
            stroke_xywh(&doc_for("outside")),
            (18.0, 18.0, 104.0, 104.0),
            "outside must outset by sw/2"
        );
        assert_eq!(
            stroke_xywh(&doc_for("center")),
            (20.0, 20.0, 100.0, 100.0),
            "center must be unchanged"
        );
    }

    // ── Code node: multi-line stacks DrawGlyphRun by line_height ─────────────

    #[test]
    fn code_node_multi_line_stacks_by_line_height() {
        // A 3-line code node (no w/h → no clip) emits 3 DrawGlyphRun commands
        // whose baselines increase by a constant delta equal to line_height.
        let src = r##"zenith version=1 {
  project id="proj.cd1" name="CD1"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd1" title="CD1" {
    page id="page.cd1" w=(px)400 h=(px)200 {
      code id="code.cd1" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
        content "line one\nline two\nline three"
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

        let runs: Vec<f64> = result
            .scene
            .commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 3, "expected 3 DrawGlyphRun; got {}", runs.len());

        let d0 = runs[1] - runs[0];
        let d1 = runs[2] - runs[1];
        assert!(d0 > 0.0, "baselines must increase; got {runs:?}");
        assert!(
            (d0 - d1).abs() < 0.001,
            "inter-line delta must be constant (line_height); got {d0} vs {d1}"
        );
    }

    // ── Code node: overflow clip wraps the runs; "visible" omits the clip ────

    #[test]
    fn code_node_overflow_clip_wraps_runs() {
        // Default overflow + w/h present → PushClip, runs…, PopClip.
        let src = r##"zenith version=1 {
  project id="proj.cd2" name="CD2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd2" title="CD2" {
    page id="page.cd2" w=(px)400 h=(px)200 {
      code id="code.cd2" x=(px)10 y=(px)20 w=(px)300 h=(px)80 {
        content "alpha\nbeta"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let cmds = &result.scene.commands;

        // First command after the page background is PushClip; last is PopClip.
        let first_run = cmds
            .iter()
            .position(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
            .expect("a DrawGlyphRun must exist");
        assert!(
            matches!(cmds[first_run - 1], SceneCommand::PushClip { .. }),
            "PushClip must immediately precede the first run; got {:?}",
            cmds[first_run - 1]
        );
        assert!(
            matches!(cmds.last(), Some(SceneCommand::PopClip)),
            "PopClip must be the final command; got {:?}",
            cmds.last()
        );

        // overflow="visible" → no clip at all.
        let src_vis = r##"zenith version=1 {
  project id="proj.cd2v" name="CD2V"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd2v" title="CD2V" {
    page id="page.cd2v" w=(px)400 h=(px)200 {
      code id="code.cd2v" x=(px)10 y=(px)20 w=(px)300 h=(px)80 overflow="visible" {
        content "alpha\nbeta"
      }
    }
  }
}
"##;
        let doc_vis = parse(src_vis);
        let result_vis = compile(&doc_vis, &default_provider());
        // The page itself always wraps content in one PushClip/PopClip. With
        // overflow=visible the code node must add NO clip of its own, so exactly
        // one PushClip (the page) should be present — not two.
        let push_clips = result_vis
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::PushClip { .. }))
            .count();
        assert_eq!(
            push_clips, 1,
            "overflow=visible must add no clip beyond the page's; got {:?}",
            result_vis.scene.commands
        );
    }

    // ── Code node: blank middle line preserves vertical space ────────────────

    #[test]
    fn code_node_blank_line_preserves_spacing() {
        // "a\n\nb" → 2 runs (blank skipped), but "b" sits at i=2 spacing:
        // baseline_b == code_y + ascent + 2*line_height.
        let src = r##"zenith version=1 {
  project id="proj.cd3" name="CD3"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd3" title="CD3" {
    page id="page.cd3" w=(px)400 h=(px)200 {
      code id="code.cd3" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
        content "a\n\nb"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let runs: Vec<f64> = result
            .scene
            .commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(
            runs.len(),
            2,
            "blank middle line must be skipped → 2 runs; got {}",
            runs.len()
        );

        // The delta between "a" (i=0) and "b" (i=2) must equal 2*line_height,
        // i.e. exactly twice a single-step delta. Derive a single step from a
        // separate two-line node sharing the same font/size.
        let src2 = r##"zenith version=1 {
  project id="proj.cd3b" name="CD3B"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd3b" title="CD3B" {
    page id="page.cd3b" w=(px)400 h=(px)200 {
      code id="code.cd3b" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
        content "a\nb"
      }
    }
  }
}
"##;
        let doc2 = parse(src2);
        let result2 = compile(&doc2, &default_provider());
        let runs2: Vec<f64> = result2
            .scene
            .commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(runs2.len(), 2);
        let single_step = runs2[1] - runs2[0];
        let blank_gap = runs[1] - runs[0];
        assert!(
            (blank_gap - 2.0 * single_step).abs() < 0.001,
            "blank line must reserve one line: expected 2*{single_step}, got {blank_gap}"
        );
    }

    // ── Code node: leading tab expands and the node compiles cleanly ─────────

    #[test]
    fn code_node_tab_expansion_compiles() {
        // A line with a leading tab and tab-width=2 expands to 2 leading spaces.
        // Exact glyph counts are brittle, so assert the node compiles to a run
        // without a shaping error.
        let src = r##"zenith version=1 {
  project id="proj.cd4" name="CD4"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd4" title="CD4" {
    page id="page.cd4" w=(px)400 h=(px)200 {
      code id="code.cd4" x=(px)10 y=(px)20 tab-width=2 overflow="visible" {
        content "\tindented"
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
            "no shaping error expected: {unshaped:?}"
        );
        let run_count = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
            .count();
        assert_eq!(run_count, 1, "expected one DrawGlyphRun");
    }

    // ── Code node: default font family is the mono face ──────────────────────

    #[test]
    fn code_node_default_font_is_mono() {
        // No font-family → the run's font_id resolves to the mono face.
        let src = r##"zenith version=1 {
  project id="proj.cd5" name="CD5"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd5" title="CD5" {
    page id="page.cd5" w=(px)400 h=(px)200 {
      code id="code.cd5" x=(px)10 y=(px)20 overflow="visible" {
        content "fn main() {}"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let font_id = result
            .scene
            .commands
            .iter()
            .find_map(|c| match c {
                SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
                _ => None,
            })
            .expect("a DrawGlyphRun must exist");
        assert!(
            font_id.contains("noto-sans-mono"),
            "default code font must be mono; got font_id {font_id}"
        );
    }

    // ── Code node: syntax highlighting splits into per-token runs ────────────

    /// A code node with `language="rust"` and a Rust snippet must produce MORE
    /// DrawGlyphRun commands than there are lines (per-token splitting) and at
    /// least two distinct colors.
    #[test]
    fn code_node_highlighted_rust_emits_per_token_runs() {
        let src = r##"zenith version=1 {
  project id="proj.hl1" name="HL1"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.hl1" title="HL1" {
    page id="page.hl1" w=(px)800 h=(px)400 {
      code id="code.hl1" x=(px)10 y=(px)10 language="rust" overflow="visible" {
        content "let x = 42;"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let runs: Vec<&SceneCommand> = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
            .collect();
        // "let x = 42;" tokenises into multiple tokens → more than 1 run per line.
        assert!(
            runs.len() > 1,
            "highlighted line must emit multiple runs; got {}",
            runs.len()
        );
        // At least two distinct colors must appear (keyword vs number vs operator…).
        let mut colors: Vec<(u8, u8, u8, u8)> = runs
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { color, .. } => {
                    Some((color.r, color.g, color.b, color.a))
                }
                _ => None,
            })
            .collect();
        colors.dedup();
        assert!(
            colors.len() >= 2,
            "at least two distinct colors expected; got {:?}",
            colors
        );
    }

    /// A code node with NO language (or an unsupported one) must emit exactly
    /// ONE DrawGlyphRun per non-empty line — byte-identical to the pre-highlight
    /// behavior.
    #[test]
    fn code_node_no_language_single_run_per_line() {
        let src = r##"zenith version=1 {
  project id="proj.hl2" name="HL2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.hl2" title="HL2" {
    page id="page.hl2" w=(px)800 h=(px)400 {
      code id="code.hl2" x=(px)10 y=(px)10 language="zzz" overflow="visible" {
        content "let x = 42;\nlet y = 1;"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let run_count = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
            .count();
        // 2 non-empty lines → exactly 2 runs (single-run plain path).
        assert_eq!(
            run_count, 2,
            "unsupported language must yield 1 run/line (2 total); got {run_count}"
        );
    }

    /// A code node with `language="rust"` and a doc-declared `syntax.keyword`
    /// token (red) must use that color for keyword runs, overriding the builtin.
    #[test]
    fn code_node_highlighted_doc_token_overrides_builtin_color() {
        // `let` is a Rust keyword. With syntax.keyword=#ff0000 the keyword run
        // must be red (r=255, g=0, b=0).
        let src = r##"zenith version=1 {
  project id="proj.hl3" name="HL3"
  tokens format="zenith-token-v1" {
    token id="syntax.keyword" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.hl3" title="HL3" {
    page id="page.hl3" w=(px)800 h=(px)400 {
      code id="code.hl3" x=(px)10 y=(px)10 language="rust" overflow="visible" {
        content "let x = 1;"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let keyword_run = result.scene.commands.iter().find_map(|c| match c {
            SceneCommand::DrawGlyphRun { color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0 =>
            {
                Some(*color)
            }
            _ => None,
        });
        assert!(
            keyword_run.is_some(),
            "expected a red (r=255,g=0,b=0) run for the overridden keyword token; \
             commands: {:?}",
            result.scene.commands
        );
    }

    // ── Text node: font-weight token selects the bold face ───────────────────

    /// A text node with a `font-weight` token resolving to 700 must emit a
    /// `DrawGlyphRun` whose `font_id` is the BOLD Noto Sans face; a text node
    /// with NO font-weight must resolve to the regular (400) face.
    #[test]
    fn text_node_font_weight_selects_bold_face() {
        // Helper: extract the first DrawGlyphRun's font_id from a compiled doc.
        fn first_run_font_id(src: &str) -> String {
            let doc = parse(src);
            let result = compile(&doc, &default_provider());
            result
                .scene
                .commands
                .iter()
                .find_map(|c| match c {
                    SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
                    _ => None,
                })
                .expect("a DrawGlyphRun must exist")
        }

        // Bold: font-weight=(token)"weight.bold" → fontWeight 700 → bold face.
        let bold_src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {
    token id="weight.bold" type="fontWeight" value=700
  }
  styles {}
  document id="doc.fw" title="FW" {
    page id="page.fw" w=(px)400 h=(px)200 {
      text id="text.bold" x=(px)10 y=(px)20 w=(px)380 h=(px)40 font-weight=(token)"weight.bold" { span "Bold" }
    }
  }
}
"##;
        let bold_font_id = first_run_font_id(bold_src);
        assert!(
            bold_font_id.contains("noto-sans-700"),
            "font-weight 700 must select the bold face; got font_id {bold_font_id}"
        );

        // Regular: no font-weight → the default (400) face.
        let regular_src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fw" title="FW" {
    page id="page.fw" w=(px)400 h=(px)200 {
      text id="text.reg" x=(px)10 y=(px)20 w=(px)380 h=(px)40 { span "Regular" }
    }
  }
}
"##;
        let regular_font_id = first_run_font_id(regular_src);
        assert!(
            regular_font_id.contains("noto-sans-400") && !regular_font_id.contains("700"),
            "absent font-weight must select the regular (400) face; got font_id {regular_font_id}"
        );
    }

    // ── Per-span text rendering ───────────────────────────────────────────

    /// Collect every `DrawGlyphRun` (x, color, font_id) in source order.
    fn glyph_runs(src: &str) -> Vec<(f64, Color, String)> {
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        result
            .scene
            .commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun {
                    x, color, font_id, ..
                } => Some((*x, *color, font_id.clone())),
                _ => None,
            })
            .collect()
    }

    /// Two spans with different fill tokens → two runs, distinct colors, the
    /// second positioned to the right of the first.
    #[test]
    fn text_spans_render_with_per_span_fill_and_order() {
        let src = r##"zenith version=1 {
  project id="proj.ps" name="PS"
  tokens format="zenith-token-v1" {
    token id="color.red" type="color" value="#ff0000"
    token id="color.blue" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.ps" title="PS" {
    page id="page.ps" w=(px)400 h=(px)200 {
      text id="text.ps" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
        span "Red" fill=(token)"color.red"
        span "Blue" fill=(token)"color.blue"
      }
    }
  }
}
"##;
        let runs = glyph_runs(src);
        assert_eq!(
            runs.len(),
            2,
            "expected two DrawGlyphRun; got {}",
            runs.len()
        );

        let (x0, c0, _) = &runs[0];
        let (x1, c1, _) = &runs[1];
        assert_eq!((c0.r, c0.g, c0.b), (0xff, 0x00, 0x00), "first span red");
        assert_eq!((c1.r, c1.g, c1.b), (0x00, 0x00, 0xff), "second span blue");
        assert!(
            x1 > x0,
            "second run x ({x1}) must be greater than first ({x0})"
        );
    }

    /// A bold second span → its run resolves to the 700 face while the first
    /// (regular) span resolves to the 400 face.
    #[test]
    fn text_spans_render_with_per_span_weight() {
        let src = r##"zenith version=1 {
  project id="proj.pw" name="PW"
  tokens format="zenith-token-v1" {
    token id="weight.bold" type="fontWeight" value=700
  }
  styles {}
  document id="doc.pw" title="PW" {
    page id="page.pw" w=(px)400 h=(px)200 {
      text id="text.pw" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
        span "Reg"
        span "Bold" font-weight=(token)"weight.bold"
      }
    }
  }
}
"##;
        let runs = glyph_runs(src);
        assert_eq!(
            runs.len(),
            2,
            "expected two DrawGlyphRun; got {}",
            runs.len()
        );
        assert!(
            runs[0].2.contains("noto-sans-400"),
            "first span must use the regular (400) face; got {}",
            runs[0].2
        );
        assert!(
            runs[1].2.contains("noto-sans-700"),
            "second span must use the bold (700) face; got {}",
            runs[1].2
        );
    }

    /// An italic span selects the italic face; a plain span stays upright.
    #[test]
    fn text_italic_span_selects_italic_face() {
        let src = r##"zenith version=1 {
  project id="proj.it" name="IT"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.it" title="IT" {
    page id="page.it" w=(px)400 h=(px)200 {
      text id="text.it" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
        span "Up"
        span "Italic" italic=#true
      }
    }
  }
}
"##;
        let runs = glyph_runs(src);
        assert_eq!(runs.len(), 2, "expected two runs; got {}", runs.len());
        assert!(
            !runs[0].2.contains("italic"),
            "first span must be upright; got {}",
            runs[0].2
        );
        assert!(
            runs[1].2.contains("italic"),
            "second span must use the italic face; got {}",
            runs[1].2
        );
    }

    /// Underline/strikethrough spans each emit one decoration `FillRect`; a
    /// plain span emits none.
    #[test]
    fn text_span_decorations_emit_fill_rects() {
        let src = r##"zenith version=1 {
  project id="proj.dec" name="DEC"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.dec" title="DEC" {
    page id="page.dec" w=(px)400 h=(px)200 {
      text id="text.dec" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
        span "plain"
        span "under" underline=#true
        span "strike" strikethrough=#true
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let fill_rects = result
            .scene
            .commands
            .iter()
            .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
            .count();
        assert_eq!(
            fill_rects, 2,
            "one underline + one strikethrough → 2 decoration rects; got {fill_rects}"
        );
    }

    /// A single-span node emits exactly one run (non-breaking regression).
    #[test]
    fn text_single_span_emits_one_run() {
        let src = r##"zenith version=1 {
  project id="proj.ss" name="SS"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ss" title="SS" {
    page id="page.ss" w=(px)400 h=(px)200 {
      text id="text.ss" x=(px)10 y=(px)20 w=(px)380 h=(px)40 { span "Solo" }
    }
  }
}
"##;
        let runs = glyph_runs(src);
        assert_eq!(runs.len(), 1, "single span must emit exactly one run");
    }

    /// An empty span between two non-empty spans is skipped (no run emitted),
    /// yet positioning of the following span still accounts for the previous
    /// span's advance — i.e. empty spans don't emit but don't break order.
    #[test]
    fn text_empty_span_is_skipped_without_breaking_order() {
        let src = r##"zenith version=1 {
  project id="proj.es" name="ES"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.es" title="ES" {
    page id="page.es" w=(px)400 h=(px)200 {
      text id="text.es" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
        span "AAAA"
        span ""
        span "BBBB"
      }
    }
  }
}
"##;
        let runs = glyph_runs(src);
        assert_eq!(
            runs.len(),
            2,
            "empty span must be skipped → two runs; got {}",
            runs.len()
        );
        let (x0, _, _) = &runs[0];
        let (x1, _, _) = &runs[1];
        assert!(
            x1 > x0,
            "third span x ({x1}) must follow the first span's advance ({x0})"
        );
    }

    // ── Multi-page page selection ─────────────────────────────────────────

    /// A two-page document. Page 1 has a full-bleed rect filled `#252525`
    /// (r=0x25); page 2 a full-bleed rect filled `#dcdcdc` (r=0xdc). The page
    /// fill color uniquely identifies which page was compiled.
    const TWO_PAGE_DOC: &str = r##"zenith version=1 {
  project id="proj.mp" name="MP"
  tokens format="zenith-token-v1" {
    token id="color.p1" type="color" value="#252525"
    token id="color.p2" type="color" value="#dcdcdc"
  }
  styles {}
  document id="doc.mp" title="MP" {
    page id="page.p1" w=(px)100 h=(px)100 {
      rect id="rect.p1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.p1"
    }
    page id="page.p2" w=(px)200 h=(px)200 {
      rect id="rect.p2" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.p2"
    }
  }
}
"##;

    /// The `FillRect` color red-channel values present in a scene.
    fn fill_reds(result: &CompileResult) -> Vec<u8> {
        result
            .scene
            .commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::FillRect { color, .. } => Some(color.r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn compile_page_selects_second_page() {
        let doc = parse(TWO_PAGE_DOC);
        let result = compile_page(&doc, &default_provider(), 1);

        // Page 2 size is 200×200 (page 1 is 100×100).
        assert_eq!(result.scene.width, 200.0, "must be page 2's width");
        assert_eq!(result.scene.height, 200.0, "must be page 2's height");

        let reds = fill_reds(&result);
        assert!(
            reds.contains(&0xdc),
            "page-2 fill (#dc...) must be present; got {reds:?}"
        );
        assert!(
            !reds.contains(&0x25),
            "page-1-only fill (#25...) must be absent; got {reds:?}"
        );
    }

    #[test]
    fn compile_page_out_of_range_is_empty_with_advisory() {
        let doc = parse(TWO_PAGE_DOC);
        let result = compile_page(&doc, &default_provider(), 9);

        assert_eq!(result.scene.width, 0.0, "out-of-range scene must be 0 wide");
        assert_eq!(
            result.scene.height, 0.0,
            "out-of-range scene must be 0 tall"
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "scene.page_out_of_range"),
            "out-of-range page must emit scene.page_out_of_range; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn compile_no_pages_still_yields_no_pages_advisory() {
        let src = r##"zenith version=1 {
  project id="proj.np" name="NP"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.np" title="NP" {}
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());

        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "scene.no_pages"),
            "empty document must emit scene.no_pages"
        );
    }

    #[test]
    fn compile_equals_compile_page_zero() {
        let doc = parse(TWO_PAGE_DOC);
        let via_compile = compile(&doc, &default_provider());
        let via_page0 = compile_page(&doc, &default_provider(), 0);

        // compile renders page 1 (index 0): same dimensions and fills.
        assert_eq!(via_compile.scene.width, via_page0.scene.width);
        assert_eq!(via_compile.scene.height, via_page0.scene.height);
        assert_eq!(fill_reds(&via_compile), fill_reds(&via_page0));
        // And it is page 1, not page 2.
        assert!(fill_reds(&via_compile).contains(&0x25));
    }

    // ── Literal visual dimensions (no token) resolve at compile time ──────────

    /// A rect with a LITERAL `radius=(px)16` (no token) must emit a
    /// `FillRoundedRect` whose radius is 16.0 — previously the literal was
    /// dropped and the radius defaulted to 0.0 (a plain FillRect).
    #[test]
    fn rect_literal_radius_emits_fill_rounded_rect() {
        let src = r##"zenith version=1 {
  project id="proj.lr" name="LR"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.lr" title="LR" {
    page id="page.lr" w=(px)100 h=(px)100 {
      rect id="rect.lr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 radius=(px)16 fill=(token)"color.fill"
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        let cmds = &result.scene.commands;
        match cmds
            .iter()
            .find(|c| matches!(c, SceneCommand::FillRoundedRect { .. }))
        {
            Some(SceneCommand::FillRoundedRect { radius, .. }) => {
                assert!(
                    (*radius - 16.0).abs() < 0.01,
                    "literal radius must resolve to 16px, got {radius}"
                );
            }
            other => panic!("expected FillRoundedRect, got {other:?}"),
        }
    }

    /// A text node with a LITERAL `font-size=(px)20` must produce a
    /// `DrawGlyphRun` whose `font_size` is 20.0.
    #[test]
    fn text_literal_font_size_resolves() {
        let src = r##"zenith version=1 {
  project id="proj.lfs" name="LFS"
  tokens format="zenith-token-v1" {
    token id="color.text" type="color" value="#111827"
  }
  styles {}
  document id="doc.lfs" title="LFS" {
    page id="page.lfs" w=(px)320 h=(px)200 {
      text id="text.lfs" x=(px)10 y=(px)10 w=(px)200 h=(px)50 fill=(token)"color.text" font-size=(px)20 {
        span "Hi"
      }
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        match result
            .scene
            .commands
            .iter()
            .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        {
            Some(SceneCommand::DrawGlyphRun { font_size, .. }) => {
                assert_eq!(*font_size, 20.0, "literal font-size must resolve to 20px");
            }
            other => panic!("expected DrawGlyphRun, got {other:?}"),
        }
    }

    /// A line with a LITERAL `stroke-width=(px)3` must produce a `StrokeLine`
    /// whose `stroke_width` is 3.0.
    #[test]
    fn line_literal_stroke_width_resolves() {
        let src = r##"zenith version=1 {
  project id="proj.lsw" name="LSW"
  tokens format="zenith-token-v1" {
    token id="color.rule" type="color" value="#94a3b8"
  }
  styles {}
  document id="doc.lsw" title="LSW" {
    page id="page.lsw" w=(px)320 h=(px)200 {
      line id="line.lsw" x1=(px)40 y1=(px)100 x2=(px)280 y2=(px)100 stroke=(token)"color.rule" stroke-width=(px)3
    }
  }
}
"##;
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        match result
            .scene
            .commands
            .iter()
            .find(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        {
            Some(SceneCommand::StrokeLine { stroke_width, .. }) => {
                assert_eq!(
                    *stroke_width, 3.0,
                    "literal stroke-width must resolve to 3px"
                );
            }
            other => panic!("expected StrokeLine, got {other:?}"),
        }
    }
}
