//! Scene-command → PDF content-operator translation.
//!
//! [`translate`] walks the scene display list once and emits a single page
//! content stream, accumulating the page resources it references (alpha
//! ExtGStates, axial-gradient shadings, image XObjects) into [`PageResources`]
//! for the document writer to materialize.
//!
//! Every [`SceneCommand`] variant is handled explicitly — no wildcard arm
//! silently drops a primitive. The one honest v0 limitation matched explicitly
//! at its arm is color-bitmap (emoji) glyphs (omitted; the print scenarios use
//! none).
//!
//! Non-vector effect brackets — blur, drop-shadow, per-pixel color filter, and
//! mask — have no vector PDF equivalent, so [`translate`] buffers each bracket
//! INCLUSIVE (the `Begin*`, its body, and the matching `End*`), renders it as a
//! standalone sub-scene via the raster backend (which self-applies the effect),
//! crops to the opaque bounding box, and embeds the result as an image XObject
//! (see [`embed_rasterized_region`]). All four are honored, not no-ops.

use pdf_writer::Content;
use zenith_core::{AssetProvider, FontProvider};
use zenith_scene::{
    Color, FitMode, ImageClip, Paint as ScenePaint, Scene, SceneCommand, StrokeAlign,
};

use super::color;
use super::geometry::{GlyphPen, ellipse_path, poly_path, rounded_rect_path};
use super::gradient::{AxialGradient, resolve as resolve_gradient};
use super::image::{DecodedImage, decode_for_pdf};

/// Page-level resources accumulated during [`translate`], keyed for
/// deduplication and emitted in a deterministic order by the document writer.
#[derive(Default)]
pub(super) struct PageResources {
    /// Distinct fill/stroke alpha values (< 255) seen, each becoming one
    /// `/ExtGState` with `ca` + `CA`. Sorted, deduped → stable resource names.
    pub(super) alphas: Vec<u8>,
    /// Axial gradient shadings, in first-seen (draw) order. Index = resource id.
    pub(super) gradients: Vec<AxialGradient>,
    /// Decoded image XObjects, in first-seen order. Index = resource id.
    pub(super) images: Vec<DecodedImage>,
}

impl PageResources {
    /// Intern an alpha byte, returning its stable `ExtGState` resource index.
    pub(super) fn intern_alpha(&mut self, a: u8) -> usize {
        match self.alphas.binary_search(&a) {
            Ok(i) => i,
            Err(i) => {
                self.alphas.insert(i, a);
                i
            }
        }
    }
}

/// The resource-name prefixes. Names are `<prefix><index>`, e.g. `ga2`, `sh0`,
/// `im1` — ASCII only, deterministic.
pub(super) const ALPHA_PREFIX: &str = "ga";
pub(super) const SHADING_PREFIX: &str = "sh";
pub(super) const IMAGE_PREFIX: &str = "im";

/// Translate `scene` into a single content stream plus the [`PageResources`] it
/// references. `fonts` resolves glyph outlines; `assets` resolves image bytes.
pub(super) fn translate(
    scene: &Scene,
    fonts: &dyn FontProvider,
    assets: &dyn AssetProvider,
) -> (Content, PageResources) {
    let mut content = Content::new();
    let mut res = PageResources::default();

    // Initial CTM: flip the y axis so scene (top-left, y-down) maps to PDF
    // user space (bottom-left, y-up). 1 scene px == 1 PDF unit.
    content.transform([1.0, 0.0, 0.0, -1.0, 0.0, scene.height as f32]);

    let page = (scene.width, scene.height);

    // Inclusive effect buffer. The four non-vector effect brackets — blur,
    // drop-shadow, per-pixel color filter, and mask — have no vector PDF
    // primitive, so while one is open we BUFFER the WHOLE bracket INCLUSIVE (the
    // `Begin*`, its body, and the matching `End*`) instead of emitting, then
    // render it as a standalone sub-scene whose raster backend self-applies every
    // effect (see `embed_rasterized_region`). `depth` counts nested effect
    // brackets so the buffer closes only at the matching outermost `End*`; a
    // mask>blur nesting closes correctly because every inner `Begin*` bumps the
    // count and every `End*` lowers it. A bracket is intercepted only at top
    // level; once a buffer is open, any nested effect is just more buffered
    // content handled by that one sub-scene render. `None` means draws emit
    // directly.
    let mut effect_buf: Option<(u32, Vec<SceneCommand>)> = None;

    for cmd in &scene.commands {
        let is_open = is_effect_open(cmd);
        let is_close = is_effect_close(cmd);

        // While a bracket is open, buffer everything, tracking nesting depth, and
        // render the region when the outermost bracket closes.
        if let Some((depth, buffered)) = effect_buf.as_mut() {
            buffered.push(cmd.clone());
            if is_open {
                *depth += 1;
            } else if is_close {
                *depth = depth.saturating_sub(1);
                if *depth == 0
                    && let Some((_, region)) = effect_buf.take()
                {
                    super::raster_embed::embed_rasterized_region(
                        &mut content,
                        &mut res,
                        &region,
                        page,
                        fonts,
                        assets,
                    );
                }
            }
            continue;
        }

        // No buffer open: a top-level effect-open starts one (the Begin is
        // buffered too so the sub-scene self-applies the effect). An empty
        // `BeginFilter` is a no-op — it stays vector and falls through to
        // `emit_command` (which no-ops it); blur/shadow/mask always open.
        if is_open && !is_empty_filter(cmd) {
            effect_buf = Some((1, vec![cmd.clone()]));
            continue;
        }

        emit_command(&mut content, &mut res, cmd, page, fonts, assets);
    }

    (content, res)
}

/// A scene command's role in non-vector effect bracketing.
enum EffectBracket {
    /// Opens an offscreen capture (blur, shadow, filter, or mask).
    Open,
    /// Closes the innermost offscreen capture.
    Close,
    /// Not an effect bracket — a plain draw, clip, layer, or transform command.
    None,
}

/// Exhaustively classify a command's effect-bracket role. Every `SceneCommand`
/// variant is listed explicitly (no wildcard arm), so adding a new variant
/// forces a compile error here and can never be silently treated as a plain draw.
fn effect_bracket(cmd: &SceneCommand) -> EffectBracket {
    match cmd {
        // ── Effect openers ────────────────────────────────────────────────
        SceneCommand::BeginShadow { .. }
        | SceneCommand::BeginBlur { .. }
        | SceneCommand::BeginFilter { .. }
        | SceneCommand::BeginMask { .. } => EffectBracket::Open,

        // ── Effect closers ────────────────────────────────────────────────
        SceneCommand::EndShadow
        | SceneCommand::EndBlur
        | SceneCommand::EndFilter
        | SceneCommand::EndMask => EffectBracket::Close,

        // ── Plain draw / clip / layer / transform commands ─────────────────
        SceneCommand::FillRect { .. }
        | SceneCommand::StrokeRect { .. }
        | SceneCommand::FillRoundedRect { .. }
        | SceneCommand::StrokeRoundedRect { .. }
        | SceneCommand::FillEllipse { .. }
        | SceneCommand::StrokeEllipse { .. }
        | SceneCommand::StrokeLine { .. }
        | SceneCommand::FillPolygon { .. }
        | SceneCommand::StrokePolyline { .. }
        | SceneCommand::DrawImage { .. }
        | SceneCommand::DrawSvgAsset { .. }
        | SceneCommand::DrawGlyphRun { .. }
        | SceneCommand::PushClip { .. }
        | SceneCommand::PopClip
        | SceneCommand::PushLayer { .. }
        | SceneCommand::PopLayer
        | SceneCommand::PushTransform { .. }
        | SceneCommand::PopTransform => EffectBracket::None,
    }
}

/// True for a command that opens a non-vector effect bracket (shadow, blur,
/// filter, or mask).
fn is_effect_open(cmd: &SceneCommand) -> bool {
    matches!(effect_bracket(cmd), EffectBracket::Open)
}

/// True for a command that closes a non-vector effect bracket — the matching
/// `End*` for each [`is_effect_open`] case.
fn is_effect_close(cmd: &SceneCommand) -> bool {
    matches!(effect_bracket(cmd), EffectBracket::Close)
}

/// True for a `BeginFilter` carrying no filters — a no-op bracket that must not
/// open a buffer (it stays vector). The compiler never emits empty filters; the
/// guard is preserved defensively. All other commands return false.
fn is_empty_filter(cmd: &SceneCommand) -> bool {
    matches!(cmd, SceneCommand::BeginFilter { filters } if filters.is_empty())
}

/// Apply the fill-alpha ExtGState for `color` if it is non-opaque (interning the
/// alpha into `res`). Returns nothing; emits `/ga<i> gs` when needed.
fn apply_alpha(content: &mut Content, res: &mut PageResources, color: &Color) {
    if color.a == 255 {
        return;
    }
    let idx = res.intern_alpha(color.a);
    content.set_parameters(name(ALPHA_PREFIX, idx).as_name());
}

/// Fill a region with a scene [`ScenePaint`] (solid or gradient), where
/// `build_path` emits the path operators for the geometry and returns whether a
/// path was produced. `bbox` is the geometry's bounding box, used to resolve a
/// gradient's axial line. `even_odd` selects the fill / clip rule.
///
/// - **Solid** → set the fill color and fill the path.
/// - **Linear gradient** → clip to the path and paint an axial shading.
/// - **Radial gradient** → PDF v0 has no axial-shading equivalent, so it degrades
///   to a solid fill of the first stop color (consistent with the other v0 PDF
///   degradations: blur, drop-shadow, SVG assets).
fn fill_region<F: Fn(&mut Content) -> bool>(
    content: &mut Content,
    res: &mut PageResources,
    paint: &ScenePaint,
    bbox: (f64, f64, f64, f64),
    even_odd: bool,
    build_path: F,
) {
    let fill = |content: &mut Content, produced: bool| {
        if produced {
            if even_odd {
                content.fill_even_odd();
            } else {
                content.fill_nonzero();
            }
        } else {
            content.end_path();
        }
    };

    match paint {
        ScenePaint::Solid { color } => {
            content.save_state();
            apply_alpha(content, res, color);
            color::set_fill(content, color);
            let produced = build_path(content);
            fill(content, produced);
            content.restore_state();
        }
        ScenePaint::Gradient(gradient) if gradient.radial => {
            // Radial PDF degrade: solid fill with the first stop color.
            if let Some(first) = gradient.stops.first() {
                content.save_state();
                apply_alpha(content, res, &first.color);
                color::set_fill(content, &first.color);
                let produced = build_path(content);
                fill(content, produced);
                content.restore_state();
            }
        }
        ScenePaint::Gradient(gradient) => {
            let (x, y, w, h) = bbox;
            if let Some(g) = resolve_gradient(x, y, w, h, gradient) {
                let id = push_gradient(res, g);
                content.save_state();
                if build_path(content) {
                    if even_odd {
                        content.clip_even_odd();
                    } else {
                        content.clip_nonzero();
                    }
                    content.end_path();
                    content.shading(name(SHADING_PREFIX, id).as_name());
                } else {
                    content.end_path();
                }
                content.restore_state();
            }
        }
    }
}

/// Axis-aligned bounding box `(x, y, w, h)` of a flat `[x0,y0,x1,y1,…]` point
/// list, used to resolve a gradient line for polygon fills. The caller has
/// already verified at least three finite points.
fn poly_bbox(points: &[f64]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for pair in points.chunks_exact(2) {
        min_x = min_x.min(pair[0]);
        max_x = max_x.max(pair[0]);
        min_y = min_y.min(pair[1]);
        max_y = max_y.max(pair[1]);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

pub(super) fn emit_command(
    content: &mut Content,
    res: &mut PageResources,
    cmd: &SceneCommand,
    page: (f64, f64),
    fonts: &dyn FontProvider,
    assets: &dyn AssetProvider,
) {
    match cmd {
        // ── Filled shapes ─────────────────────────────────────────────────
        SceneCommand::FillRect { x, y, w, h, paint } => {
            if !rect_ok(*x, *y, *w, *h) {
                return;
            }
            fill_region(content, res, paint, (*x, *y, *w, *h), false, |c| {
                c.rect(*x as f32, *y as f32, *w as f32, *h as f32);
                true
            });
        }

        SceneCommand::StrokeRect {
            x,
            y,
            w,
            h,
            color,
            stroke_width,
            // PDF v0 renders solid strokes only; dash params are intentionally ignored here.
            ..
        } => {
            if !rect_ok(*x, *y, *w, *h) || !finite(*stroke_width) {
                return;
            }
            content.save_state();
            apply_alpha(content, res, color);
            color::set_stroke(content, color);
            content.set_line_width(*stroke_width as f32);
            content.rect(*x as f32, *y as f32, *w as f32, *h as f32);
            content.stroke();
            content.restore_state();
        }

        SceneCommand::FillRoundedRect {
            x,
            y,
            w,
            h,
            radius,
            radii,
            paint,
        } => {
            if !rect_ok(*x, *y, *w, *h) || !finite(*radius) {
                return;
            }
            let corner_radii = radii.unwrap_or([*radius; 4]);
            fill_region(content, res, paint, (*x, *y, *w, *h), false, |c| {
                rounded_rect_path(c, *x, *y, *w, *h, corner_radii);
                true
            });
        }

        SceneCommand::StrokeRoundedRect {
            x,
            y,
            w,
            h,
            radius,
            radii,
            color,
            stroke_width,
            // PDF v0 renders solid strokes only; dash params are intentionally ignored here.
            ..
        } => {
            if !rect_ok(*x, *y, *w, *h) || !finite(*radius) || !finite(*stroke_width) {
                return;
            }
            let corner_radii = radii.unwrap_or([*radius; 4]);
            content.save_state();
            apply_alpha(content, res, color);
            color::set_stroke(content, color);
            content.set_line_width(*stroke_width as f32);
            rounded_rect_path(content, *x, *y, *w, *h, corner_radii);
            content.stroke();
            content.restore_state();
        }

        SceneCommand::FillEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            paint,
        } => {
            if !rect_ok(*x, *y, *w, *h) {
                return;
            }
            fill_region(content, res, paint, (*x, *y, *w, *h), false, |c| {
                ellipse_path(c, *x, *y, *w, *h, *rx, *ry);
                true
            });
        }

        SceneCommand::StrokeEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            color,
            stroke_width,
            // PDF v0 renders solid strokes only; dash params are intentionally ignored here.
            ..
        } => {
            if !rect_ok(*x, *y, *w, *h) || !finite(*stroke_width) {
                return;
            }
            content.save_state();
            apply_alpha(content, res, color);
            color::set_stroke(content, color);
            content.set_line_width(*stroke_width as f32);
            ellipse_path(content, *x, *y, *w, *h, *rx, *ry);
            content.stroke();
            content.restore_state();
        }

        SceneCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
            // PDF v0 renders solid strokes only; dash params are intentionally ignored here.
            ..
        } => {
            if !finite(*x1)
                || !finite(*y1)
                || !finite(*x2)
                || !finite(*y2)
                || !finite(*stroke_width)
            {
                return;
            }
            content.save_state();
            apply_alpha(content, res, color);
            color::set_stroke(content, color);
            content.set_line_width(*stroke_width as f32);
            content.move_to(*x1 as f32, *y1 as f32);
            content.line_to(*x2 as f32, *y2 as f32);
            content.stroke();
            content.restore_state();
        }

        SceneCommand::FillPolygon {
            points,
            paint,
            even_odd,
        } => {
            if points.len() < 6 || points.iter().any(|v| !v.is_finite()) {
                return;
            }
            let bbox = poly_bbox(points);
            fill_region(content, res, paint, bbox, *even_odd, |c| {
                poly_path(c, points, true)
            });
        }

        SceneCommand::StrokePolyline {
            points,
            color,
            stroke_width,
            closed,
            align,
            fill_even_odd,
        } => {
            if points.len() < 4 || points.iter().any(|v| !v.is_finite()) || !finite(*stroke_width) {
                return;
            }

            // Aligned stroke (Inside/Outside on a CLOSED polygon): draw at 2× width
            // and clip to the fill region (Inside) or its complement (Outside) so a
            // full-width stroke sits flush against the boundary. Center / open paths
            // are unchanged.
            let aligned = *closed && !matches!(align, StrokeAlign::Center);

            content.save_state();
            apply_alpha(content, res, color);
            color::set_stroke(content, color);

            if aligned {
                // 1. Install the alignment clip from the polygon fill path.
                match align {
                    StrokeAlign::Inside => {
                        // Clip = polygon interior (per fill rule).
                        if !poly_path(content, points, true) {
                            content.end_path();
                            content.restore_state();
                            return;
                        }
                        if *fill_even_odd {
                            content.clip_even_odd();
                        } else {
                            content.clip_nonzero();
                        }
                        content.end_path();
                    }
                    StrokeAlign::Outside => {
                        // Clip = (generous outer rect) minus polygon interior, via the
                        // even-odd rule on the combined subpaths → the exterior region.
                        let (pw, ph) = page;
                        let m = pw.max(ph).max(1.0); // generous margin past the page
                        content.move_to(-m as f32, -m as f32);
                        content.line_to((pw + m) as f32, -m as f32);
                        content.line_to((pw + m) as f32, (ph + m) as f32);
                        content.line_to(-m as f32, (ph + m) as f32);
                        content.close_path();
                        if !poly_path(content, points, true) {
                            content.end_path();
                            content.restore_state();
                            return;
                        }
                        content.clip_even_odd();
                        content.end_path();
                    }
                    // `aligned` is only true when align != Center, so this arm is dead;
                    // kept (no wildcard) for exhaustiveness. A no-op is the safe fallback
                    // — it simply leaves the clip unchanged.
                    StrokeAlign::Center => {}
                }
                // 2. Stroke the path at 2× width inside the clip.
                content.set_line_width((*stroke_width * 2.0) as f32);
                if poly_path(content, points, true) {
                    content.stroke();
                } else {
                    content.end_path();
                }
            } else {
                content.set_line_width(*stroke_width as f32);
                if poly_path(content, points, *closed) {
                    content.stroke();
                } else {
                    content.end_path();
                }
            }
            content.restore_state();
        }

        // ── Text ──────────────────────────────────────────────────────────
        SceneCommand::DrawGlyphRun {
            x,
            y,
            font_id,
            font_size,
            color,
            // v0: glyph stroke is fill-only in PDF output; stroke_color/stroke_width
            // are intentionally ignored here.
            stroke_color: _,
            stroke_width: _,
            glyphs,
        } => {
            emit_glyph_run(
                content,
                res,
                fonts,
                GlyphRun {
                    x: *x,
                    y: *y,
                    font_id,
                    font_size: *font_size,
                    color,
                    glyphs,
                },
            );
        }

        // ── Images ────────────────────────────────────────────────────────
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
            clip_shape,
            src_rect: _,
        } => {
            emit_image(
                content,
                res,
                fonts,
                assets,
                ImageDraw {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    asset_id,
                    fit: *fit,
                    pos_x: *pos_x,
                    pos_y: *pos_y,
                    opacity: *opacity,
                    clip_shape,
                },
            );
        }

        // SVG assets are pre-resolved to a raster in the raster backend; the
        // scene IR for the print scenarios never emits this variant. It is
        // matched explicitly (no silent wildcard) and deferred for PDF v0: a
        // faithful vector embedding would require an SVG→PDF path translator,
        // out of scope here. Documented limitation.
        SceneCommand::DrawSvgAsset { .. } => {}

        // ── Clip stack ────────────────────────────────────────────────────
        // PushClip → save the graphics state, install the rect clip, and clear
        // the path; the matching PopClip restores. This nests one q/Q level per
        // clip exactly like the raster backend's clip stack.
        SceneCommand::PushClip { x, y, w, h } => {
            content.save_state();
            content.rect(*x as f32, *y as f32, *w as f32, *h as f32);
            content.clip_nonzero();
            content.end_path();
        }
        SceneCommand::PopClip => {
            content.restore_state();
        }

        // ── Transform stack ───────────────────────────────────────────────
        // Rotation about a pivot: save, translate to pivot, rotate, translate
        // back; the matching PopTransform restores.
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            content.save_state();
            let theta = (*angle_deg).to_radians();
            let (s, c) = (theta.sin() as f32, theta.cos() as f32);
            let (cx, cy) = (*cx as f32, *cy as f32);
            // Translate(cx,cy) · Rotate(θ) · Translate(-cx,-cy), as one matrix.
            content.transform([c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]);
        }
        SceneCommand::PopTransform => {
            content.restore_state();
        }

        // ── Compositing layers ────────────────────────────────────────────
        // Layer opacity is applied per-draw via the color alpha cascade already
        // resolved into each command's color in the scene IR, so a layer
        // bracket needs only a save/restore to scope any state it sets. (No
        // group transparency object in v0; matched explicitly, not dropped.)
        //
        // v0 limitation: the `blend_mode` field is ignored — the PDF backend has
        // no ExtGState soft-mask / blend-mode group, so blended content renders
        // source-over. Documented honest limitation (the PNG backend honors it).
        SceneCommand::PushLayer { .. } => {
            content.save_state();
        }
        SceneCommand::PopLayer => {
            content.restore_state();
        }

        // ── Non-vector effect brackets ────────────────────────────────────
        // `translate` intercepts every BALANCED effect bracket (blur, shadow,
        // filter, mask) before dispatch, buffering it inclusive and rendering it
        // as a self-applying sub-scene (see `embed_rasterized_region`). These
        // arms are therefore unreachable in normal flow; kept (no wildcard) for
        // exhaustiveness, and a no-op is the safe fallback so a malformed or
        // standalone End* / empty Begin* reaching here can never panic.
        SceneCommand::BeginShadow { .. } => {}
        SceneCommand::EndShadow => {}
        SceneCommand::BeginBlur { .. } => {}
        SceneCommand::EndBlur => {}
        SceneCommand::BeginFilter { .. } => {}
        SceneCommand::EndFilter => {}
        SceneCommand::BeginMask { .. } => {}
        SceneCommand::EndMask => {}
    }
}

/// Push a gradient and return its resource index.
pub(super) fn push_gradient(res: &mut PageResources, g: AxialGradient) -> usize {
    let id = res.gradients.len();
    res.gradients.push(g);
    id
}

/// Borrow/scalar context for one [`SceneCommand::DrawGlyphRun`] emission,
/// bundled into a `Copy` struct so [`emit_glyph_run`] stays within the
/// argument-count budget without an `#[allow]`.
#[derive(Clone, Copy)]
struct GlyphRun<'a> {
    /// Text-box origin x in pixels.
    x: f64,
    /// Baseline y in pixels.
    y: f64,
    /// Stable font-face identifier; resolved via `FontProvider::by_id`.
    font_id: &'a str,
    /// Font size at which glyphs were shaped, in pixels.
    font_size: f32,
    /// Fill color of the glyph run.
    color: &'a Color,
    /// Positioned glyphs, baseline-relative.
    glyphs: &'a [zenith_scene::SceneGlyph],
}

fn emit_glyph_run(
    content: &mut Content,
    res: &mut PageResources,
    fonts: &dyn FontProvider,
    run: GlyphRun<'_>,
) {
    let GlyphRun {
        x,
        y,
        font_id,
        font_size,
        color,
        glyphs,
    } = run;
    let Some(font_data) = fonts.by_id(font_id) else {
        return;
    };
    let Ok(face) = ttf_parser::Face::parse(&font_data.bytes, font_data.index) else {
        return;
    };
    let units_per_em = face.units_per_em();
    if units_per_em == 0 {
        return;
    }
    let scale = font_size / f32::from(units_per_em);

    content.save_state();
    apply_alpha(content, res, color);
    color::set_fill(content, color);

    // Build one combined path of all glyph outlines, then a single fill. Color
    // bitmap (emoji) glyphs would return Some from `glyph_raster_image`; for PDF
    // v0 they are skipped (documented). Outline fonts never hit that branch.
    let mut any = false;
    for glyph in glyphs {
        if face
            .glyph_raster_image(ttf_parser::GlyphId(glyph.glyph_id), font_size as u16)
            .is_some()
        {
            // Color-bitmap emoji: omitted in PDF v0 (no scenario uses emoji).
            continue;
        }
        let origin_x = x as f32 + glyph.dx;
        let baseline_y = y as f32 + glyph.dy;
        let mut pen = GlyphPen::new(content, origin_x, baseline_y, scale);
        if face
            .outline_glyph(ttf_parser::GlyphId(glyph.glyph_id), &mut pen)
            .is_some()
        {
            any = true;
        }
    }
    if any {
        content.fill_nonzero();
    } else {
        content.end_path();
    }
    content.restore_state();
}

/// Borrow/scalar context for one [`SceneCommand::DrawImage`] emission, bundled
/// into a `Copy` struct so [`emit_image`] stays within the argument-count
/// budget without an `#[allow]`.
#[derive(Clone, Copy)]
struct ImageDraw<'a> {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// Stable asset id; resolved via `AssetProvider::by_id`.
    asset_id: &'a str,
    /// How the image scales to fill the box.
    fit: FitMode,
    /// Horizontal object-position anchor in `0.0..=100.0`.
    pos_x: f64,
    /// Vertical object-position anchor in `0.0..=100.0`.
    pos_y: f64,
    /// Effective opacity, `0.0..=1.0`.
    opacity: f64,
    /// Optional non-rectangular clip shape inscribed in the box.
    clip_shape: &'a Option<ImageClip>,
}

fn emit_image(
    content: &mut Content,
    res: &mut PageResources,
    fonts: &dyn FontProvider,
    assets: &dyn AssetProvider,
    draw: ImageDraw<'_>,
) {
    let ImageDraw {
        x,
        y,
        w,
        h,
        asset_id,
        fit,
        pos_x,
        pos_y,
        opacity,
        clip_shape,
    } = draw;
    if !rect_ok(x, y, w, h) {
        return;
    }
    let Some(asset) = assets.by_id(asset_id) else {
        return;
    };
    // Dispatch on asset kind. Raster images embed as a Flate XObject below; SVG
    // assets translate to native PDF vector operators (paths + shadings) via the
    // `svg` module — true vector output, not a rasterized embed. Font/Unknown
    // kinds are not drawable images.
    match asset.kind {
        zenith_core::AssetKind::Image => {}
        zenith_core::AssetKind::Svg => {
            super::svg::emit_svg(
                content,
                res,
                fonts,
                &asset.bytes,
                super::svg::SvgPlacement {
                    x,
                    y,
                    w,
                    h,
                    fit,
                    pos_x,
                    pos_y,
                    opacity,
                    clip_shape,
                },
            );
            return;
        }
        zenith_core::AssetKind::Font | zenith_core::AssetKind::Unknown(_) => return,
    }
    let Some(decoded) = decode_for_pdf(&asset.bytes) else {
        return;
    };
    let (sw, sh) = (f64::from(decoded.width), f64::from(decoded.height));
    if !(sw > 0.0 && sh > 0.0) {
        return;
    }

    // Fit transform (sx, sy, tx, ty) in scene space — identical math to the
    // raster backend's DrawImage arm.
    let (sx, sy, tx, ty) = match fit {
        FitMode::Stretch => (w / sw, h / sh, x, y),
        FitMode::Contain => {
            let s = (w / sw).min(h / sh);
            let (rw, rh) = (sw * s, sh * s);
            (
                s,
                s,
                x + (w - rw) * pos_x / 100.0,
                y + (h - rh) * pos_y / 100.0,
            )
        }
        FitMode::Cover => {
            let s = (w / sw).max(h / sh);
            let (rw, rh) = (sw * s, sh * s);
            (
                s,
                s,
                x - (rw - w) * pos_x / 100.0,
                y - (rh - h) * pos_y / 100.0,
            )
        }
        FitMode::None => (
            1.0,
            1.0,
            x - (sw - w) * pos_x / 100.0,
            y - (sh - h) * pos_y / 100.0,
        ),
    };
    if !finite(sx) || !finite(sy) || !finite(tx) || !finite(ty) || sx <= 0.0 || sy <= 0.0 {
        return;
    }

    let id = res.images.len();
    res.images.push(decoded);

    content.save_state();

    // Opacity via an ExtGState (image opacity is a separate factor from any
    // color alpha). 1.0 needs no state.
    let op = (opacity as f32).clamp(0.0, 1.0);
    if op < 1.0 {
        let a = (op * 255.0).round().clamp(0.0, 255.0) as u8;
        let aidx = res.intern_alpha(a);
        content.set_parameters(name(ALPHA_PREFIX, aidx).as_name());
    }

    // Box clip (rect or inscribed shape). The compiler also pushes a PushClip
    // box around images, but re-asserting the box here is harmless and makes
    // the non-rectangular shape clip self-contained.
    match clip_shape {
        None => {
            content.rect(x as f32, y as f32, w as f32, h as f32);
            content.clip_nonzero();
            content.end_path();
        }
        Some(ImageClip::Ellipse) => {
            ellipse_path(content, x, y, w, h, None, None);
            content.clip_nonzero();
            content.end_path();
        }
        Some(ImageClip::RoundedRect { radius }) => {
            rounded_rect_path(content, x, y, w, h, [*radius; 4]);
            content.clip_nonzero();
            content.end_path();
        }
    }

    // An image XObject is a 1×1 unit square in its own space; place it by
    // mapping that unit square onto the fitted box. PDF images are y-up, so we
    // flip within the placement matrix: image row 0 (top) must land at the box
    // top (smaller scene-y). The CTM below maps unit (u, v) → scene point
    // (tx + u*sw*sx, ty + (1-v)*sh*sy), i.e. scale_y is negative with a +height
    // translate, all composed with the page's outer flip.
    let iw = (sw * sx) as f32;
    let ih = (sh * sy) as f32;
    content.transform([iw, 0.0, 0.0, -ih, tx as f32, ty as f32 + ih]);
    content.x_object(name(IMAGE_PREFIX, id).as_name());

    content.restore_state();
}

// ── Small helpers ──────────────────────────────────────────────────────────

#[inline]
fn finite(v: f64) -> bool {
    v.is_finite()
}

#[inline]
fn rect_ok(x: f64, y: f64, w: f64, h: f64) -> bool {
    finite(x) && finite(y) && finite(w) && finite(h) && w > 0.0 && h > 0.0
}

/// A small owned resource-name buffer (`<prefix><index>`), kept on the stack to
/// avoid per-call heap churn while satisfying `pdf_writer::Name`'s borrow.
pub(super) struct ResName {
    buf: [u8; 24],
    len: usize,
}

impl ResName {
    pub(super) fn as_name(&self) -> pdf_writer::Name<'_> {
        pdf_writer::Name(&self.buf[..self.len])
    }
}

/// Build a deterministic ASCII resource name `<prefix><index>`.
pub(super) fn name(prefix: &str, index: usize) -> ResName {
    use std::io::Write;
    let mut buf = [0u8; 24];
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    // prefix is a short ASCII literal and index is a usize; the 24-byte buffer
    // is always large enough, so the writes cannot fail. If they ever did, the
    // name would be truncated to `cursor.position()` bytes — still valid ASCII.
    let _ = write!(cursor, "{prefix}{index}");
    let len = cursor.position() as usize;
    ResName { buf, len }
}
