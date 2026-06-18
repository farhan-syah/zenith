//! Concrete rasterization backend powered by `tiny-skia`.
//!
//! This is the **only** module in the crate that names `tiny_skia` types or
//! `ttf_parser` types.  All other modules see only the backend-neutral types
//! from `backend.rs`.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};
use zenith_core::FontProvider;
use zenith_scene::{Scene, SceneCommand};

use crate::backend::{RasterBackend, RasterImage};
use crate::error::RenderError;

/// Maximum allowed dimension in either axis (width or height).
///
/// Prevents gigantic allocations from malformed or adversarial scenes.
const MAX_DIMENSION: u32 = 16_384;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Convert scene `f64` dimensions to `u32` pixels, enforcing sanity rules.
///
/// Returns `Err` when:
/// - The value is non-finite (`NaN`, `±inf`).
/// - `value.round()` is `<= 0` (page must have positive extent).
/// - The rounded value exceeds [`MAX_DIMENSION`].
fn f64_to_px(value: f64, axis: &str) -> Result<u32, RenderError> {
    if !value.is_finite() {
        return Err(RenderError::new(format!(
            "scene {axis} is non-finite ({value})"
        )));
    }
    let px = value.round();
    if px <= 0.0 {
        return Err(RenderError::new(format!(
            "scene {axis} rounds to a non-positive value ({px})"
        )));
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let px_u32 = px as u32;
    if px_u32 > MAX_DIMENSION {
        return Err(RenderError::new(format!(
            "scene {axis} ({px_u32}) exceeds maximum allowed dimension ({MAX_DIMENSION})"
        )));
    }
    Ok(px_u32)
}

/// Intersect two axis-aligned rectangles expressed as `(x, y, x2, y2)`.
///
/// Returns `None` when the intersection is empty.
fn intersect_rects(
    (ax, ay, ax2, ay2): (f64, f64, f64, f64),
    (bx, by, bx2, by2): (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let ix = ax.max(bx);
    let iy = ay.max(by);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);
    if ix < ix2 && iy < iy2 {
        Some((ix, iy, ix2, iy2))
    } else {
        None
    }
}

/// Convert premultiplied RGBA8 (tiny-skia's internal storage) to straight-alpha RGBA8.
fn premultiplied_to_straight(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8, u8) {
    if a == 0 {
        return (0, 0, 0, 0);
    }
    let a_u16 = u16::from(a);
    // Round via (v * 255 + a/2) / a
    let un = |v: u8| -> u8 {
        let v_u16 = u16::from(v);
        // (v * 255 + a/2) / a, clamped to 255
        let result = (v_u16 * 255 + a_u16 / 2) / a_u16;
        result.min(255) as u8
    };
    (un(r), un(g), un(b), a)
}

// ── Glyph outline pen ─────────────────────────────────────────────────────────

/// An `OutlineBuilder` that feeds ttf-parser outline commands into a
/// `tiny_skia::PathBuilder`, applying the Y-flip and scale transform needed to
/// map from font-units (Y-up) to pixmap coordinates (Y-down).
///
/// Font coordinate system: Y increases upward, origin at glyph origin.
/// Pixmap coordinate system: Y increases downward, origin at top-left.
///
/// Transform applied per point: `px = origin_x + fx * scale`,
///                               `py = baseline_y - fy * scale`.
struct GlyphOutlinePen {
    builder: PathBuilder,
    origin_x: f32,
    baseline_y: f32,
    scale: f32,
}

impl GlyphOutlinePen {
    fn new(origin_x: f32, baseline_y: f32, scale: f32) -> Self {
        Self {
            builder: PathBuilder::new(),
            origin_x,
            baseline_y,
            scale,
        }
    }

    /// Map a font-unit point `(fx, fy)` to pixmap coordinates.
    #[inline]
    fn to_px(&self, fx: f32, fy: f32) -> (f32, f32) {
        let px = self.origin_x + fx * self.scale;
        let py = self.baseline_y - fy * self.scale;
        (px, py)
    }
}

impl ttf_parser::OutlineBuilder for GlyphOutlinePen {
    fn move_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.to_px(x, y);
        self.builder.move_to(px, py);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.to_px(x, y);
        self.builder.line_to(px, py);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (px1, py1) = self.to_px(x1, y1);
        let (px, py) = self.to_px(x, y);
        self.builder.quad_to(px1, py1, px, py);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (px1, py1) = self.to_px(x1, y1);
        let (px2, py2) = self.to_px(x2, y2);
        let (px, py) = self.to_px(x, y);
        self.builder.cubic_to(px1, py1, px2, py2, px, py);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

// ── TinySkiaBackend ───────────────────────────────────────────────────────────

/// CPU rasterization backend backed by the `tiny-skia` library.
///
/// Determinism guarantees:
/// - Anti-aliasing is disabled for rect fills → integer-aligned rects produce
///   exact, reproducible pixels with no sub-pixel variance.
/// - Anti-aliasing is enabled for glyph fills — glyph edges are curved and
///   require AA for legible output. tiny-skia AA is pure-software and
///   deterministic on the same machine (no GPU, no random numbers).
/// - No `HashMap`, no random numbers, no timestamps.
/// - PNG encoding via `tiny_skia::Pixmap::encode_png` writes no timestamps.
pub struct TinySkiaBackend;

impl RasterBackend for TinySkiaBackend {
    fn rasterize(
        &self,
        scene: &Scene,
        fonts: &dyn FontProvider,
    ) -> Result<RasterImage, RenderError> {
        let width = f64_to_px(scene.width, "width")?;
        let height = f64_to_px(scene.height, "height")?;

        let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
            RenderError::new(format!("failed to allocate pixmap ({width}×{height})"))
        })?;
        // Background starts fully transparent (0,0,0,0) — the deterministic default.

        // Clip stack: each entry is (x, y, x2, y2) in scene coordinates.
        // The outermost clip is the page rectangle.
        let page_clip = (0.0_f64, 0.0_f64, scene.width, scene.height);
        let mut clip_stack: Vec<(f64, f64, f64, f64)> = vec![page_clip];

        for cmd in &scene.commands {
            match cmd {
                SceneCommand::PushClip { x, y, w, h } => {
                    let new_rect = (*x, *y, x + w, y + h);
                    let current = *clip_stack.last().unwrap_or(&page_clip);
                    // Push the intersection so the stack always represents the
                    // effective clip at the current nesting depth.
                    let intersected =
                        intersect_rects(current, new_rect).unwrap_or((0.0, 0.0, 0.0, 0.0)); // empty → degenerate
                    clip_stack.push(intersected);
                }

                // Never pop below the page clip (index 0).
                SceneCommand::PopClip if clip_stack.len() > 1 => {
                    clip_stack.pop();
                }

                SceneCommand::FillRect { x, y, w, h, color } => {
                    let fill_rect = (*x, *y, x + w, y + h);
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Intersect the fill rect with the current effective clip.
                    let (ix, iy, ix2, iy2) = match intersect_rects(fill_rect, effective_clip) {
                        Some(r) => r,
                        None => continue, // nothing to draw
                    };

                    let iw = ix2 - ix;
                    let ih = iy2 - iy;

                    // tiny-skia requires positive, finite values for Rect::from_xywh.
                    if iw <= 0.0
                        || ih <= 0.0
                        || !ix.is_finite()
                        || !iy.is_finite()
                        || !iw.is_finite()
                        || !ih.is_finite()
                    {
                        continue;
                    }

                    let rect = match Rect::from_xywh(ix as f32, iy as f32, iw as f32, ih as f32) {
                        Some(r) => r,
                        None => continue,
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = false; // deterministic: no edge AA variance

                    // Drawing outside the pixmap simply touches no pixels; not an error.
                    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                }

                SceneCommand::DrawGlyphRun {
                    x,
                    y,
                    font_id,
                    font_size,
                    color,
                    glyphs,
                } => {
                    // ── 1. Resolve font bytes ─────────────────────────────────
                    let font_data = match fonts.by_id(font_id) {
                        Some(fd) => fd,
                        None => {
                            // Unknown font id: skip the run silently. The page
                            // renders correctly for all other commands.
                            continue;
                        }
                    };

                    // ── 2. Parse the font face ────────────────────────────────
                    let face = match ttf_parser::Face::parse(&font_data.bytes, font_data.index) {
                        Ok(f) => f,
                        Err(_) => continue, // malformed font bytes: skip run
                    };

                    // ── 3. Compute scale from font units to pixels ────────────
                    let units_per_em = face.units_per_em();
                    if units_per_em == 0 {
                        continue; // degenerate font: skip
                    }
                    let scale = font_size / f32::from(units_per_em);

                    // ── 4. Build the paint for the glyph color ────────────────
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA is on for glyphs: curved outlines need sub-pixel coverage.
                    // tiny-skia AA is pure-software; output is deterministic on
                    // the same machine (no GPU, no random state).
                    paint.anti_alias = true;

                    // ── 5. Rasterize each glyph ───────────────────────────────
                    for glyph in glyphs {
                        let origin_x = *x as f32 + glyph.dx;
                        let baseline_y = *y as f32 + glyph.dy;

                        // Build path via outline pen.
                        let mut pen = GlyphOutlinePen::new(origin_x, baseline_y, scale);

                        // outline_glyph returns None for glyphs with no outlines
                        // (e.g. space, .notdef in some fonts). Skip those.
                        if face
                            .outline_glyph(ttf_parser::GlyphId(glyph.glyph_id), &mut pen)
                            .is_none()
                        {
                            continue;
                        }

                        // Finalise the path; None means an empty or degenerate path.
                        let path = match pen.builder.finish() {
                            Some(p) => p,
                            None => continue,
                        };

                        // Note: glyph pixels that fall outside the pixmap bounds
                        // are automatically discarded by tiny-skia — no explicit
                        // clip intersection needed. Applying the clip stack for
                        // per-glyph clipping (beyond the page edge) is deferred;
                        // the page-edge clip equals the pixmap for the current
                        // single-page skeleton, so this is correct for that case.
                        pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }

                // All other variants are not emitted yet; skip them deterministically.
                _ => {}
            }
        }

        // Convert tiny-skia's premultiplied RGBA8 to straight-alpha RGBA8.
        let raw = pixmap.data(); // &[u8], len = width*height*4, premul RGBA
        let mut rgba = Vec::with_capacity(raw.len());
        for chunk in raw.chunks_exact(4) {
            let (sr, sg, sb, sa) =
                premultiplied_to_straight(chunk[0], chunk[1], chunk[2], chunk[3]);
            rgba.push(sr);
            rgba.push(sg);
            rgba.push(sb);
            rgba.push(sa);
        }

        Ok(RasterImage {
            width,
            height,
            rgba,
        })
    }

    fn encode_png(&self, image: &RasterImage) -> Result<Vec<u8>, RenderError> {
        // Re-premultiply straight-alpha back to premultiplied for tiny-skia.
        let mut premul = Vec::with_capacity(image.rgba.len());
        for chunk in image.rgba.chunks_exact(4) {
            let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
            if a == 0 {
                premul.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let a_u16 = u16::from(a);
                let mul = |v: u8| -> u8 {
                    let result = (u16::from(v) * a_u16 + 127) / 255;
                    result.min(255) as u8
                };
                premul.push(mul(r));
                premul.push(mul(g));
                premul.push(mul(b));
                premul.push(a);
            }
        }

        let mut pixmap = Pixmap::new(image.width, image.height).ok_or_else(|| {
            RenderError::new(format!(
                "failed to allocate pixmap for encoding ({}×{})",
                image.width, image.height
            ))
        })?;

        let dst = pixmap.data_mut();
        if dst.len() != premul.len() {
            return Err(RenderError::new(
                "pixel buffer length mismatch during PNG encoding",
            ));
        }
        dst.copy_from_slice(&premul);

        pixmap
            .encode_png()
            .map_err(|e| RenderError::new(format!("PNG encoding failed: {e}")))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use zenith_core::{FontStyle, default_provider};
    use zenith_layout::{RustybuzzEngine, ShapeRequest, TextLayoutEngine};
    use zenith_scene::{Color, Scene, SceneCommand, SceneGlyph};

    use crate::backend::RasterBackend;
    use crate::render::{render_image, render_png};

    use super::TinySkiaBackend;

    fn red() -> Color {
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    fn make_solid_red_scene(page: f64) -> Scene {
        let mut s = Scene::new(page, page);
        s.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: page,
            h: page,
        });
        s.commands.push(SceneCommand::FillRect {
            x: 0.0,
            y: 0.0,
            w: page,
            h: page,
            color: red(),
        });
        s.commands.push(SceneCommand::PopClip);
        s
    }

    /// Index into a straight-alpha RGBA8 buffer for pixel (px, py) in an image
    /// of the given `width`.
    fn pixel(rgba: &[u8], width: u32, px: u32, py: u32) -> (u8, u8, u8, u8) {
        let base = ((py * width + px) * 4) as usize;
        (rgba[base], rgba[base + 1], rgba[base + 2], rgba[base + 3])
    }

    // ── pixel correctness ─────────────────────────────────────────────────

    #[test]
    fn pixel_correctness_solid_red() {
        let scene = make_solid_red_scene(4.0);
        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider)
            .expect("rasterize must succeed");
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        // center pixel
        assert_eq!(pixel(&img.rgba, img.width, 2, 2), (255, 0, 0, 255));
        // corner pixel
        assert_eq!(pixel(&img.rgba, img.width, 0, 0), (255, 0, 0, 255));
    }

    // ── determinism ───────────────────────────────────────────────────────

    #[test]
    fn determinism_identical_png_bytes() {
        let scene = make_solid_red_scene(4.0);
        let backend = TinySkiaBackend;
        let provider = default_provider();
        let png1 = backend
            .rasterize(&scene, &provider)
            .and_then(|img| backend.encode_png(&img))
            .expect("first render");
        let png2 = backend
            .rasterize(&scene, &provider)
            .and_then(|img| backend.encode_png(&img))
            .expect("second render");
        assert_eq!(
            png1, png2,
            "PNG output must be byte-identical for the same scene"
        );
    }

    // ── PNG validity ──────────────────────────────────────────────────────

    #[test]
    fn png_magic_bytes() {
        let scene = make_solid_red_scene(4.0);
        let backend = TinySkiaBackend;
        let provider = default_provider();
        let png = backend
            .rasterize(&scene, &provider)
            .and_then(|img| backend.encode_png(&img))
            .expect("render");
        assert_eq!(
            &png[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "output must start with PNG magic bytes"
        );
    }

    // ── clip enforced ─────────────────────────────────────────────────────

    #[test]
    fn clip_clamps_fill_to_page() {
        // 4×4 page; FillRect extends well beyond the page edge.
        let mut scene = Scene::new(4.0, 4.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        });
        scene.commands.push(SceneCommand::FillRect {
            x: 2.0,
            y: 2.0,
            w: 10.0,
            h: 10.0,
            color: red(),
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider)
            .expect("must not panic or error");
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        // Pixel inside the overlap region (3,3) should be red.
        assert_eq!(pixel(&img.rgba, img.width, 3, 3), (255, 0, 0, 255));
        // Pixel outside the fill (0,0) should be transparent.
        assert_eq!(pixel(&img.rgba, img.width, 0, 0), (0, 0, 0, 0));
    }

    // ── transparent default ───────────────────────────────────────────────

    #[test]
    fn transparent_default_no_fill() {
        let mut scene = Scene::new(4.0, 4.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend.rasterize(&scene, &provider).expect("must succeed");
        // All pixels must be fully transparent.
        for i in 0..(img.width * img.height) {
            let base = (i * 4) as usize;
            assert_eq!(
                &img.rgba[base..base + 4],
                &[0, 0, 0, 0],
                "pixel {i} must be transparent"
            );
        }
    }

    // ── invalid size ──────────────────────────────────────────────────────

    #[test]
    fn invalid_zero_size_returns_error() {
        let scene = Scene::new(0.0, 0.0);
        let backend = TinySkiaBackend;
        let provider = default_provider();
        assert!(
            backend.rasterize(&scene, &provider).is_err(),
            "zero-size scene must return RenderError"
        );
    }

    // ── glyph: draws pixels ───────────────────────────────────────────────

    /// Build a DrawGlyphRun scene for the letter "A" using the bundled Noto Sans
    /// font, then verify that at least one pixel in the output matches the run
    /// color (i.e. text was actually rasterized).
    #[test]
    fn glyph_run_draws_pixels() {
        let provider = default_provider();
        let families = vec!["Noto Sans".to_string()];
        let font_size = 32.0_f32;

        // Shape "A" to get a real glyph id from the bundled font.
        let req = ShapeRequest {
            text: "A",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size,
        };
        let run = RustybuzzEngine::new()
            .shape(&req, &provider)
            .expect("shaping must succeed");

        // Page: 80×40.  Baseline at y=32 (leaves room for the glyph above).
        let page_w = 80.0_f64;
        let page_h = 40.0_f64;
        let baseline_y = 34.0_f64;
        let origin_x = 4.0_f64;

        let ink_color = Color {
            r: 0,
            g: 0,
            b: 200,
            a: 255,
        };

        // Map the shaped glyphs into SceneGlyph instances.
        let glyphs: Vec<SceneGlyph> = run
            .glyphs
            .iter()
            .map(|g| SceneGlyph {
                glyph_id: g.glyph_id,
                dx: g.x,
                dy: g.y,
            })
            .collect();

        let mut scene = Scene::new(page_w, page_h);
        scene.commands.push(SceneCommand::DrawGlyphRun {
            x: origin_x,
            y: baseline_y,
            font_id: run.font_id.clone(),
            font_size,
            color: ink_color.clone(),
            glyphs,
        });

        let img = render_image(&scene, &provider).expect("render must succeed");

        // At least one pixel must have non-zero blue (the ink color).
        let any_ink = (0..img.height).any(|py| {
            (0..img.width).any(|px| {
                let (r, g, b, a) = pixel(&img.rgba, img.width, px, py);
                // Anti-aliased: the pixel need not be exactly (0,0,200,255);
                // just check that the blue channel is dominant and alpha > 0.
                a > 0 && b > r && b > g
            })
        });

        assert!(
            any_ink,
            "DrawGlyphRun must rasterize at least one ink pixel for 'A' at 32px"
        );
    }

    // ── glyph: determinism ────────────────────────────────────────────────

    #[test]
    fn glyph_run_deterministic_png() {
        let provider = default_provider();
        let families = vec!["Noto Sans".to_string()];
        let font_size = 24.0_f32;

        let req = ShapeRequest {
            text: "Zenith",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size,
        };
        let run = RustybuzzEngine::new()
            .shape(&req, &provider)
            .expect("shaping must succeed");

        let glyphs: Vec<SceneGlyph> = run
            .glyphs
            .iter()
            .map(|g| SceneGlyph {
                glyph_id: g.glyph_id,
                dx: g.x,
                dy: g.y,
            })
            .collect();

        let mut scene = Scene::new(200.0, 40.0);
        scene.commands.push(SceneCommand::DrawGlyphRun {
            x: 4.0,
            y: 30.0,
            font_id: run.font_id.clone(),
            font_size,
            color: Color {
                r: 10,
                g: 10,
                b: 10,
                a: 255,
            },
            glyphs,
        });

        let png1 = render_png(&scene, &provider).expect("first render");
        let png2 = render_png(&scene, &provider).expect("second render");
        assert_eq!(
            png1, png2,
            "glyph run PNG must be byte-identical across two renders"
        );
    }

    // ── glyph: missing font id ────────────────────────────────────────────

    #[test]
    fn glyph_run_missing_font_id_succeeds_silently() {
        let provider = default_provider();

        let mut scene = Scene::new(40.0, 40.0);
        scene.commands.push(SceneCommand::DrawGlyphRun {
            x: 0.0,
            y: 20.0,
            font_id: "nonexistent-font-000-normal".to_string(),
            font_size: 16.0,
            color: Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            glyphs: vec![SceneGlyph {
                glyph_id: 36,
                dx: 0.0,
                dy: 0.0,
            }],
        });

        // Must succeed (Ok) — the run is skipped, no panic, no error.
        let img =
            render_image(&scene, &provider).expect("render must succeed even with unknown font");

        // All pixels should be transparent (nothing was drawn).
        let any_opaque = (0..img.height).any(|py| {
            (0..img.width).any(|px| {
                let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
                a > 0
            })
        });
        assert!(
            !any_opaque,
            "no pixels should be drawn when the font id is unknown"
        );
    }
}
