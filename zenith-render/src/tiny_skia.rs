//! Concrete rasterization backend powered by `tiny-skia`.
//!
//! This is the **only** module in the crate that names `tiny_skia` types or
//! `ttf_parser` types.  All other modules see only the backend-neutral types
//! from `backend.rs`.

use resvg::usvg;
use resvg::usvg::TreeParsing;
use resvg::usvg::TreeTextToPath;
use tiny_skia::{
    FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};
use zenith_core::{AssetKind, AssetProvider, FontProvider};
use zenith_scene::{FitMode, Scene, SceneCommand};

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
    let px_u32 = px as u32;
    if px_u32 > MAX_DIMENSION {
        return Err(RenderError::new(format!(
            "scene {axis} ({px_u32}) exceeds maximum allowed dimension ({MAX_DIMENSION})"
        )));
    }
    Ok(px_u32)
}

/// Build a clip `Mask` from the current effective clip rectangle.
///
/// Returns:
/// - `None` — the effective clip is empty or fully off-canvas; the caller
///   should skip the draw entirely (`continue`).
/// - `Some(None)` — the clip covers the whole pixmap; no masking needed,
///   draw with `mask = None` (the common, no-frame case — avoids allocating
///   a full-size mask on every top-level draw).
/// - `Some(Some(mask))` — a real sub-page clip; draw with `mask = Some(&mask)`.
fn clip_mask(
    effective_clip: (f64, f64, f64, f64),
    width: u32,
    height: u32,
) -> Option<Option<Mask>> {
    let pixmap_bounds = (0.0, 0.0, f64::from(width), f64::from(height));
    let (cx, cy, cx2, cy2) = intersect_rects(effective_clip, pixmap_bounds)?; // empty → None (skip)
    // If the clip covers the entire pixmap, no mask is needed.
    if cx <= 0.0 && cy <= 0.0 && cx2 >= f64::from(width) && cy2 >= f64::from(height) {
        return Some(None);
    }
    let mut mask = Mask::new(width, height)?;
    let rect = Rect::from_xywh(cx as f32, cy as f32, (cx2 - cx) as f32, (cy2 - cy) as f32)?;
    let clip_path = PathBuilder::from_rect(rect);
    // AA off: the clip is an axis-aligned rect and must be exact.
    mask.fill_path(&clip_path, FillRule::Winding, false, Transform::identity());
    Some(Some(mask))
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

/// Build a `tiny_skia::Path` from a flat `[x0, y0, x1, y1, …]` point list.
///
/// `closed` — when `true` the path is closed after the final vertex (polygon);
/// when `false` the path is left open (polyline stroke).
///
/// Returns `None` when the path is degenerate (e.g. zero-length). The caller
/// must have already verified that `points` contains at least 4 elements (2
/// vertices) and that all values are finite before calling this function.
fn build_poly_path(points: &[f64], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    // Safety: caller guarantees points.len() >= 4; first() / get(1) always Some.
    let (x0, y0) = match (points.first(), points.get(1)) {
        (Some(&x), Some(&y)) => (x as f32, y as f32),
        _ => return None,
    };
    pb.move_to(x0, y0);
    let mut i = 2;
    while i + 1 < points.len() {
        let (px, py) = match (points.get(i), points.get(i + 1)) {
            (Some(&x), Some(&y)) => (x as f32, y as f32),
            _ => break,
        };
        pb.line_to(px, py);
        i += 2;
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// Build a closed rounded-rectangle path with uniform corner radius `r`
/// (clamped by the caller to `min(w, h) / 2`). Corners are cubic Béziers using
/// the standard circle-approximation constant κ ≈ 0.5522848.
fn build_rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 || r < 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8_f32 * r; // κ·r control-point offset for a 90° cubic arc
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r); // top-right
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h); // bottom-right
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r); // bottom-left
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y); // top-left
    pb.close();
    pb.finish()
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
        assets: &dyn AssetProvider,
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

        // Lazily-built fontdb for SVG text→path conversion. Initialised at most
        // once per render, only when an SVG asset is actually drawn. Never loads
        // system fonts — only the registered faces from `fonts`.
        let mut svg_fontdb: Option<resvg::usvg::fontdb::Database> = None;

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

                SceneCommand::FillEllipse { x, y, w, h, color } => {
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Early-out: skip if the ellipse bbox is entirely outside the clip.
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    // Guard against non-finite or degenerate dimensions.
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    // Build the oval at its TRUE bounding box — NOT the intersected box.
                    // Intersecting the bbox before building the oval would reshape (squish)
                    // the ellipse under partial clip; instead we draw the full ellipse and
                    // let the clip mask truncate it.
                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let Some(path) = PathBuilder::from_oval(rect) else {
                        continue; // degenerate rect: skip
                    };

                    // Build clip mask from the effective clip (truncates, not reshapes).
                    // AA-on: curved fill, deterministic same-machine.
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::StrokeEllipse {
                    x,
                    y,
                    w,
                    h,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Ink-bbox early-out: the stroke extends half its width beyond
                    // the ellipse edge on all sides.
                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    // Build the oval path at its TRUE bounding box — NOT the
                    // intersected box. The clip mask truncates without reshaping.
                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let Some(path) = PathBuilder::from_oval(rect) else {
                        continue; // degenerate rect: skip
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA-on: curved stroke edge, deterministic same-machine.
                    paint.anti_alias = true;

                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::StrokeLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    stroke_width,
                } => {
                    // Guard against non-finite or out-of-f32-range values before
                    // building the path — tiny-skia requires finite f32 values,
                    // and a finite-but-huge f64 would overflow to f32::INFINITY.
                    if !x1.is_finite()
                        || !y1.is_finite()
                        || !x2.is_finite()
                        || !y2.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // A line is 1-D so we cannot reshape it to the clip; instead we
                    // compute the ink bounding box (endpoints expanded by half the
                    // stroke width) as a cheap early-out, then clip the stroke to the
                    // effective clip via a mask so sub-page (frame) clips truncate the
                    // line at the frame edge.
                    let half_sw = stroke_width / 2.0;
                    let ink_x = x1.min(*x2) - half_sw;
                    let ink_y = y1.min(*y2) - half_sw;
                    let ink_x2 = x1.max(*x2) + half_sw;
                    let ink_y2 = y1.max(*y2) + half_sw;
                    if intersect_rects((ink_x, ink_y, ink_x2, ink_y2), effective_clip).is_none() {
                        continue;
                    }

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    // Build path: a single open segment from (x1,y1) to (x2,y2).
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x1 as f32, *y1 as f32);
                    pb.line_to(*x2 as f32, *y2 as f32);
                    let path = match pb.finish() {
                        Some(p) => p,
                        None => continue, // degenerate (zero-length) line: skip
                    };

                    // Stroke defaults: Butt cap, Miter join, miter_limit 4.
                    // These are the normative v0 values (doc 09); we intentionally
                    // keep the defaults for cap/join and only set the width, so the
                    // defaults remain authoritative.
                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA on: diagonal lines need sub-pixel coverage; deterministic
                    // same-machine like ellipse/glyph fills.
                    paint.anti_alias = true;

                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        mask.as_ref(),
                    );
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

                    // ── 5. Build the clip mask (once per run) ─────────────────
                    // Glyph ink is clipped to the effective clip via the mask, so
                    // text inside a frame is truncated at the frame edge; deterministic
                    // same-machine (pure-software AA, no GPU).
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue, // entire run is off-canvas / clip is empty
                        Some(m) => m,
                    };

                    // ── 6. Rasterize each glyph ───────────────────────────────
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

                        pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            mask.as_ref(),
                        );
                    }
                }

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
                    // ── a. Resolve bytes; only raster images are drawn ────────
                    let Some(asset) = assets.by_id(asset_id) else {
                        continue; // unknown/missing asset: skip (no panic)
                    };
                    // ── b. Produce a raster Pixmap from Image (PNG) or Svg ────
                    let src: Pixmap = match asset.kind {
                        AssetKind::Image => {
                            let Ok(p) = Pixmap::decode_png(&asset.bytes) else {
                                continue; // malformed PNG: skip
                            };
                            p
                        }
                        AssetKind::Svg => {
                            // Build the fontdb at most once per render, only when
                            // an SVG is drawn. Loaded from the registered faces in
                            // deterministic BTreeMap (by_id) order — no system fonts.
                            let fontdb: &resvg::usvg::fontdb::Database = svg_fontdb
                                .get_or_insert_with(|| {
                                    let mut db = resvg::usvg::fontdb::Database::new();
                                    db.set_sans_serif_family("Noto Sans");
                                    db.set_serif_family("Noto Sans");
                                    db.set_monospace_family("Noto Sans Mono");
                                    for face in fonts.all_faces() {
                                        db.load_font_data(face.bytes.to_vec());
                                    }
                                    db
                                });
                            // Set default font-family so unstyled SVG <text> resolves
                            // to "Noto Sans" instead of the usvg default "Times New Roman".
                            let opts = usvg::Options {
                                font_family: "Noto Sans".to_owned(),
                                ..Default::default()
                            };
                            let Ok(mut usvg_tree) = usvg::Tree::from_data(&asset.bytes, &opts)
                            else {
                                continue; // malformed SVG: skip
                            };
                            usvg_tree.convert_text(fontdb);
                            let sz = usvg_tree.size;
                            let (svw, svh) = (f64::from(sz.width()), f64::from(sz.height()));
                            if !(svw > 0.0 && svh > 0.0) {
                                continue;
                            }
                            // Rasterize at destination resolution so the
                            // downstream bilinear scale is near 1:1 (crisp),
                            // preserving the SVG's own aspect ratio.
                            let raster_scale = ((*w / svw).max(*h / svh)).clamp(0.01, 16.0);
                            let pw = ((svw * raster_scale).ceil() as u32).max(1);
                            let ph = ((svh * raster_scale).ceil() as u32).max(1);
                            let Some(mut pm) = Pixmap::new(pw, ph) else {
                                continue;
                            };
                            let resvg_tree = resvg::Tree::from_usvg(&usvg_tree);
                            resvg_tree.render(
                                Transform::from_scale(raster_scale as f32, raster_scale as f32),
                                &mut pm.as_mut(),
                            );
                            pm
                        }
                        // Font or Unknown: not a drawable image; skip.
                        _ => continue,
                    };
                    let (sw, sh) = (f64::from(src.width()), f64::from(src.height()));
                    if !(sw > 0.0 && sh > 0.0) {
                        continue;
                    }

                    // ── c. Compute the fit transform (sx, sy, tx, ty) ─────────
                    // pos_x / pos_y are 0..=100 object-position anchors.
                    let (sx, sy, tx, ty) = match fit {
                        FitMode::Stretch => (w / sw, h / sh, *x, *y),
                        FitMode::Contain => {
                            let s = (w / sw).min(h / sh);
                            let (rw, rh) = (sw * s, sh * s);
                            let tx = x + (w - rw) * pos_x / 100.0;
                            let ty = y + (h - rh) * pos_y / 100.0;
                            (s, s, tx, ty)
                        }
                        FitMode::Cover => {
                            let s = (w / sw).max(h / sh);
                            let (rw, rh) = (sw * s, sh * s);
                            let tx = x - (rw - w) * pos_x / 100.0;
                            let ty = y - (rh - h) * pos_y / 100.0;
                            (s, s, tx, ty)
                        }
                        FitMode::None => {
                            let tx = x - (sw - w) * pos_x / 100.0;
                            let ty = y - (sh - h) * pos_y / 100.0;
                            (1.0, 1.0, tx, ty)
                        }
                    };
                    if !sx.is_finite()
                        || !sy.is_finite()
                        || !tx.is_finite()
                        || !ty.is_finite()
                        || sx <= 0.0
                        || sy <= 0.0
                    {
                        continue;
                    }

                    // ── d. Build the clip Mask from the effective clip ────────
                    // The compiler emits PushClip(box) before DrawImage, so
                    // clip_stack.last() already equals the image box ∩ enclosing
                    // clips (G-22 box-clip).  clip_mask() handles the full-pixmap
                    // fast path (returns Some(None) → no mask allocation) and the
                    // sub-page case (returns Some(Some(mask))).
                    let mask =
                        match clip_mask(*clip_stack.last().unwrap_or(&page_clip), width, height) {
                            None => continue, // clip fully off-canvas
                            Some(m) => m,
                        };

                    // ── e. Paint: opacity + bilinear filtering ────────────────
                    let paint = PixmapPaint {
                        opacity: (*opacity as f32).clamp(0.0, 1.0),
                        quality: FilterQuality::Bilinear,
                        ..Default::default()
                    };

                    // ── f. Scale + translate transform ────────────────────────
                    let transform =
                        Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, tx as f32, ty as f32);

                    // ── g. Composite. Box-clip (G-22) is enforced by the Mask;
                    // deterministic same-machine (pure-software bilinear). ─────
                    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, mask.as_ref());
                }

                SceneCommand::FillPolygon {
                    points,
                    color,
                    even_odd,
                } => {
                    // Guard: need at least 3 points (6 coordinates).
                    if points.len() < 6 {
                        continue;
                    }
                    // Guard: any non-finite coordinate.
                    if points.iter().any(|v| !v.is_finite()) {
                        continue;
                    }

                    let path = match build_poly_path(points, true) {
                        Some(p) => p,
                        None => continue,
                    };

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let fill_rule = if *even_odd {
                        FillRule::EvenOdd
                    } else {
                        FillRule::Winding
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.fill_path(
                        &path,
                        &paint,
                        fill_rule,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::StrokePolyline {
                    points,
                    color,
                    stroke_width,
                    closed,
                } => {
                    // Guard: need at least 2 points (4 coordinates).
                    if points.len() < 4 {
                        continue;
                    }
                    // Guard: any non-finite coordinate or invalid stroke_width.
                    if points.iter().any(|v| !v.is_finite())
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                    {
                        continue;
                    }

                    let path = match build_poly_path(points, *closed) {
                        Some(p) => p,
                        None => continue,
                    };

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    // Stroke defaults: Butt cap, Miter join, miter_limit 4 — normative v0.
                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::StrokeRect {
                    x,
                    y,
                    w,
                    h,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Ink-bbox early-out: the stroke extends half its width beyond
                    // the rect edge on all sides.
                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let path = PathBuilder::from_rect(rect);

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::FillRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    color,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !radius.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    let Some(path) = build_rounded_rect_path(
                        *x as f32,
                        *y as f32,
                        *w as f32,
                        *h as f32,
                        *radius as f32,
                    ) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                SceneCommand::StrokeRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !radius.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    let Some(path) = build_rounded_rect_path(
                        *x as f32,
                        *y as f32,
                        *w as f32,
                        *h as f32,
                        *radius as f32,
                    ) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        mask.as_ref(),
                    );
                }

                // PopClip when the stack is already at the page clip (depth 0),
                // and any future variants not yet handled: skip deterministically.
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
    use std::sync::Arc;

    use zenith_core::{AssetKind, BytesAssetProvider, FontStyle, default_provider};
    use zenith_layout::{RustybuzzEngine, ShapeRequest, TextLayoutEngine};
    use zenith_scene::{Color, FitMode, Scene, SceneCommand, SceneGlyph};

    use crate::backend::RasterBackend;
    use crate::render::{render_image, render_png};

    use super::TinySkiaBackend;

    /// A shared empty asset provider for tests that draw no images.
    fn no_assets() -> BytesAssetProvider {
        BytesAssetProvider::new()
    }

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
            .rasterize(&scene, &provider, &no_assets())
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
            .rasterize(&scene, &provider, &no_assets())
            .and_then(|img| backend.encode_png(&img))
            .expect("first render");
        let png2 = backend
            .rasterize(&scene, &provider, &no_assets())
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
            .rasterize(&scene, &provider, &no_assets())
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
            .rasterize(&scene, &provider, &no_assets())
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
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("must succeed");
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
            backend.rasterize(&scene, &provider, &no_assets()).is_err(),
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
            color: ink_color,
            glyphs,
        });

        let img = render_image(&scene, &provider, &no_assets()).expect("render must succeed");

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

        let png1 = render_png(&scene, &provider, &no_assets()).expect("first render");
        let png2 = render_png(&scene, &provider, &no_assets()).expect("second render");
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
        let img = render_image(&scene, &provider, &no_assets())
            .expect("render must succeed even with unknown font");

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

    // ── image: stretch renders + determinism ──────────────────────────────

    /// The committed 2×2 RGBA test PNG.
    const SWATCH_PNG: &[u8] = include_bytes!("../../examples/assets/swatch.png");

    fn swatch_provider() -> BytesAssetProvider {
        let mut p = BytesAssetProvider::new();
        p.register("asset.swatch", AssetKind::Image, Arc::from(SWATCH_PNG));
        p
    }

    /// Build a scene that draws the swatch stretched into a box, clipped to it.
    fn swatch_scene() -> Scene {
        let mut scene = Scene::new(40.0, 40.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        });
        scene.commands.push(SceneCommand::PushClip {
            x: 8.0,
            y: 8.0,
            w: 24.0,
            h: 24.0,
        });
        scene.commands.push(SceneCommand::DrawImage {
            x: 8.0,
            y: 8.0,
            w: 24.0,
            h: 24.0,
            asset_id: "asset.swatch".to_string(),
            fit: FitMode::Stretch,
            pos_x: 50.0,
            pos_y: 50.0,
            opacity: 1.0,
        });
        scene.commands.push(SceneCommand::PopClip);
        scene.commands.push(SceneCommand::PopClip);
        scene
    }

    #[test]
    fn draw_image_stretch_renders() {
        let backend = TinySkiaBackend;
        let fonts = default_provider();
        let assets = swatch_provider();
        let scene = swatch_scene();

        let img1 = backend
            .rasterize(&scene, &fonts, &assets)
            .expect("rasterize 1");
        let img2 = backend
            .rasterize(&scene, &fonts, &assets)
            .expect("rasterize 2");

        // (i) determinism: byte-identical pixels across two rasterizes.
        assert_eq!(
            img1.rgba, img2.rgba,
            "two rasterizes of the same image scene must be byte-identical"
        );

        // (ii) at least one pixel inside the box is non-transparent.
        let any_ink = (0..img1.height).any(|py| {
            (0..img1.width).any(|px| {
                let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
                a > 0
            })
        });
        assert!(
            any_ink,
            "DrawImage stretch must rasterize at least one non-transparent pixel"
        );
    }

    // ── FillPolygon: triangle renders + determinism ───────────────────────

    #[test]
    fn fill_polygon_renders() {
        // A simple triangle on a 100×100 page.
        let color = Color {
            r: 0,
            g: 200,
            b: 0,
            a: 255,
        };
        let mut scene = Scene::new(100.0, 100.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        scene.commands.push(SceneCommand::FillPolygon {
            // Triangle: top-center, bottom-right, bottom-left
            points: vec![50.0, 10.0, 90.0, 90.0, 10.0, 90.0],
            color,
            even_odd: false,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img1 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 1");

        // At least one pixel inside the triangle must be green.
        let any_ink = (0..img1.height).any(|py| {
            (0..img1.width).any(|px| {
                let (_, g, _, a) = pixel(&img1.rgba, img1.width, px, py);
                a > 0 && g > 0
            })
        });
        assert!(
            any_ink,
            "FillPolygon must rasterize at least one green pixel"
        );

        // Determinism: two renders must be byte-identical.
        let img2 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 2");
        assert_eq!(
            img1.rgba, img2.rgba,
            "two rasterizes of FillPolygon must be byte-identical"
        );
    }

    // ── StrokePolyline: open stroke renders + determinism ─────────────────

    #[test]
    fn stroke_polyline_renders() {
        let color = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let mut scene = Scene::new(100.0, 100.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        scene.commands.push(SceneCommand::StrokePolyline {
            points: vec![10.0, 50.0, 50.0, 10.0, 90.0, 50.0],
            color,
            stroke_width: 4.0,
            closed: false,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img1 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 1");

        // At least one pixel must be inked.
        let any_ink = (0..img1.height).any(|py| {
            (0..img1.width).any(|px| {
                let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
                a > 0
            })
        });
        assert!(
            any_ink,
            "StrokePolyline must rasterize at least one ink pixel"
        );

        // Determinism.
        let img2 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 2");
        assert_eq!(
            img1.rgba, img2.rgba,
            "two rasterizes of StrokePolyline must be byte-identical"
        );
    }

    #[test]
    fn stroke_ellipse_renders() {
        let color = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let mut scene = Scene::new(100.0, 100.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        scene.commands.push(SceneCommand::StrokeEllipse {
            x: 20.0,
            y: 30.0,
            w: 60.0,
            h: 40.0,
            color,
            stroke_width: 4.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img1 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 1");

        // At least one pixel must be inked.
        let any_ink = (0..img1.height).any(|py| {
            (0..img1.width).any(|px| {
                let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
                a > 0
            })
        });
        assert!(
            any_ink,
            "StrokeEllipse must rasterize at least one ink pixel"
        );

        // Determinism.
        let img2 = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize 2");
        assert_eq!(
            img1.rgba, img2.rgba,
            "two rasterizes of StrokeEllipse must be byte-identical"
        );
    }

    #[test]
    fn draw_image_missing_asset_is_skipped() {
        let backend = TinySkiaBackend;
        let fonts = default_provider();
        // Empty provider: the asset id is not registered.
        let assets = BytesAssetProvider::new();

        let mut scene = Scene::new(20.0, 20.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
        });
        scene.commands.push(SceneCommand::DrawImage {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            asset_id: "asset.missing".to_string(),
            fit: FitMode::Stretch,
            pos_x: 50.0,
            pos_y: 50.0,
            opacity: 1.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        // Must not panic; renders without any image pixels.
        let img = backend
            .rasterize(&scene, &fonts, &assets)
            .expect("rasterize must succeed even with a missing asset");
        let any_opaque = (0..img.height).any(|py| {
            (0..img.width).any(|px| {
                let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
                a > 0
            })
        });
        assert!(
            !any_opaque,
            "no pixels should be drawn when the asset is missing"
        );
    }

    // ── ellipse: partial clip truncates, does not reshape ─────────────────

    /// A 20×20 circle (FillEllipse x=0,y=0,w=20,h=20) is drawn inside a
    /// bottom-right quadrant clip [10,10,20,20].
    ///
    /// Correct behaviour (TRUNCATE): the ellipse is drawn at its TRUE bounds
    /// and the mask chops off the parts outside [10,10,20,20].
    ///
    /// Old wrong behaviour (RESHAPE): the ellipse bbox was intersected with the
    /// clip, yielding a tiny oval fitted to [10,10,10,10].  A corner pixel such
    /// as (18,18) — inside the clip box but outside the true circle — would
    /// have been filled because the reshaping made the oval touch it.
    ///
    /// We assert:
    /// - (18,18) alpha == 0  (outside the true circle; must stay transparent)
    /// - (12,12) alpha > 0   (inside both clip and true circle; must be filled)
    #[test]
    fn ellipse_partial_clip_truncates_not_reshapes() {
        let mut scene = Scene::new(20.0, 20.0);
        // Full-page outer clip.
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
        });
        // Bottom-right quadrant sub-page clip.
        scene.commands.push(SceneCommand::PushClip {
            x: 10.0,
            y: 10.0,
            w: 10.0,
            h: 10.0,
        });
        // A circle that exactly fits the full page (center (10,10), r=10).
        scene.commands.push(SceneCommand::FillEllipse {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            color: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        });
        scene.commands.push(SceneCommand::PopClip);
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize must succeed");

        // (18,18): inside clip [10,10,20,20] but outside the true circle
        // (dist from center (10,10) ≈ √(8²+8²) ≈ 11.3 > 10).
        // Must be transparent — the ellipse should be TRUNCATED here, not
        // reshaping the oval to fill the entire clip box.
        let (_, _, _, a_outside) = pixel(&img.rgba, img.width, 18, 18);
        assert_eq!(
            a_outside, 0,
            "pixel (18,18) is outside the true circle; must be transparent (truncate, not reshape)"
        );

        // (12,12): inside both the clip box and the true circle
        // (dist from center (10,10) ≈ √(2²+2²) ≈ 2.8 < 10).
        // Must have been drawn (alpha > 0).
        let (_, _, _, a_inside) = pixel(&img.rgba, img.width, 12, 12);
        assert!(
            a_inside > 0,
            "pixel (12,12) is inside both the clip and the circle; must be filled"
        );
    }

    // ── stroke line: sub-page clip mask is honored ────────────────────────

    /// A diagonal stroked line spanning the page is wrapped in a small top-left
    /// clip [0,0,5,5]. After wiring StrokeLine to `mask.as_ref()`, ink beyond the
    /// clip (e.g. (15,15), on the line but outside the box) must be suppressed,
    /// while ink inside the clip (near (2,2)) remains. Before the fix the line
    /// drew its full length (sub-page clip ignored) and (15,15) would be inked.
    #[test]
    fn stroke_line_clipped_to_subpage_clip() {
        let mut scene = Scene::new(20.0, 20.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
        });
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 5.0,
            h: 5.0,
        });
        scene.commands.push(SceneCommand::StrokeLine {
            x1: 0.0,
            y1: 0.0,
            x2: 20.0,
            y2: 20.0,
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            stroke_width: 4.0,
        });
        scene.commands.push(SceneCommand::PopClip);
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize must succeed");

        // (15,15): on the line but outside the [0,0,5,5] clip → must be clipped away.
        let (_, _, _, a_outside) = pixel(&img.rgba, img.width, 15, 15);
        assert_eq!(
            a_outside, 0,
            "pixel (15,15) is outside the sub-page clip; the stroked line must be truncated there"
        );

        // (2,2): on the line and inside the clip → must be inked.
        let (_, _, _, a_inside) = pixel(&img.rgba, img.width, 2, 2);
        assert!(
            a_inside > 0,
            "pixel (2,2) is on the line inside the clip; must be inked"
        );
    }

    // ── glyph run: sub-page clip mask is honored ──────────────────────────

    /// A glyph run for "A" at 32px is placed at x≈20, baseline≈34 on an
    /// 80×40 page, then wrapped in a tiny clip [0,0,4,4] that lies far from
    /// the glyph ink.  After fixing DrawGlyphRun to pass `mask.as_ref()`, the
    /// effective clip mask must suppress all ink → NO opaque pixel anywhere.
    ///
    /// Before the fix (mask=None) tiny-skia only clips to the pixmap edge, so
    /// the glyph would render normally and the test would fail.
    #[test]
    fn glyph_run_clipped_to_subpage_clip() {
        let provider = default_provider();
        let families = vec!["Noto Sans".to_string()];
        let font_size = 32.0_f32;

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

        let glyphs: Vec<SceneGlyph> = run
            .glyphs
            .iter()
            .map(|g| SceneGlyph {
                glyph_id: g.glyph_id,
                dx: g.x,
                dy: g.y,
            })
            .collect();

        let mut scene = Scene::new(80.0, 40.0);
        // Tiny clip box [0,0,4,4] — entirely disjoint from the glyph ink.
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        });
        // Glyph ink lands around x≥20, y up to baseline 34 — well outside the clip.
        scene.commands.push(SceneCommand::DrawGlyphRun {
            x: 20.0,
            y: 34.0,
            font_id: run.font_id.clone(),
            font_size,
            color: Color {
                r: 0,
                g: 0,
                b: 200,
                a: 255,
            },
            glyphs,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize must succeed");

        // The clip mask must suppress all glyph ink — no opaque pixel anywhere.
        let any_opaque = (0..img.height).any(|py| {
            (0..img.width).any(|px| {
                let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
                a > 0
            })
        });
        assert!(
            !any_opaque,
            "glyph ink must be fully clipped by the sub-page mask; found opaque pixels"
        );
    }

    // ── StrokeRect: border pixels are inked ───────────────────────────────

    #[test]
    fn stroke_rect_draws_border_pixels() {
        let mut scene = Scene::new(40.0, 40.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        });
        scene.commands.push(SceneCommand::StrokeRect {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            stroke_width: 4.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize must succeed");

        // A pixel on the top border (around y=10) must be inked.
        let (_, _, _, a_border) = pixel(&img.rgba, img.width, 20, 10);
        assert!(a_border > 0, "top border pixel (20,10) must be inked");

        // The interior center (20,20) must be EMPTY (stroke, not fill).
        let (_, _, _, a_center) = pixel(&img.rgba, img.width, 20, 20);
        assert_eq!(a_center, 0, "stroke-only interior (20,20) must be empty");
    }

    // ── FillRoundedRect: corner is cut, center is filled ──────────────────

    #[test]
    fn fill_rounded_rect_cuts_corner_fills_center() {
        // A rect [0,0,40,40] with a large radius (20 → fully circular) leaves the
        // extreme corner pixel (0,0) at background while the center is filled.
        let mut scene = Scene::new(40.0, 40.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        });
        scene.commands.push(SceneCommand::FillRoundedRect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
            radius: 20.0,
            color: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let provider = default_provider();
        let img = backend
            .rasterize(&scene, &provider, &no_assets())
            .expect("rasterize must succeed");

        // Corner (0,0) is outside the rounded shape → transparent.
        let (_, _, _, a_corner) = pixel(&img.rgba, img.width, 0, 0);
        assert_eq!(
            a_corner, 0,
            "corner pixel (0,0) must be cut by the radius (transparent)"
        );

        // Center (20,20) is inside → filled.
        let (_, _, _, a_center) = pixel(&img.rgba, img.width, 20, 20);
        assert!(a_center > 0, "center pixel (20,20) must be filled");
    }

    // ── determinism: StrokeRect + FillRoundedRect + StrokeRoundedRect ─────

    #[test]
    fn rounded_and_stroke_rects_deterministic_png() {
        let mut scene = Scene::new(80.0, 80.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        });
        scene.commands.push(SceneCommand::StrokeRect {
            x: 5.0,
            y: 5.0,
            w: 30.0,
            h: 30.0,
            color: Color {
                r: 200,
                g: 0,
                b: 0,
                a: 255,
            },
            stroke_width: 3.0,
        });
        scene.commands.push(SceneCommand::FillRoundedRect {
            x: 40.0,
            y: 5.0,
            w: 30.0,
            h: 30.0,
            radius: 10.0,
            color: Color {
                r: 0,
                g: 200,
                b: 0,
                a: 255,
            },
        });
        scene.commands.push(SceneCommand::StrokeRoundedRect {
            x: 20.0,
            y: 40.0,
            w: 40.0,
            h: 30.0,
            radius: 8.0,
            color: Color {
                r: 0,
                g: 0,
                b: 200,
                a: 255,
            },
            stroke_width: 3.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let provider = default_provider();
        let png1 = render_png(&scene, &provider, &no_assets()).expect("first render");
        let png2 = render_png(&scene, &provider, &no_assets()).expect("second render");
        assert_eq!(
            png1, png2,
            "StrokeRect + FillRoundedRect + StrokeRoundedRect scene must render byte-identically"
        );
    }

    // ── SVG asset: rasterizes and draws red pixels ────────────────────────

    /// An inline 10×10 SVG filled solid red is registered as `AssetKind::Svg`,
    /// drawn stretched into a 10×10 box on a 10×10 page, and the center pixel
    /// must be red (proving the SVG was rasterized and composited).
    #[test]
    fn draw_image_svg_asset_renders_red_pixels() {
        const RED_SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' \
            width='10' height='10'>\
            <rect width='10' height='10' fill='#ff0000'/>\
            </svg>";

        let mut assets = BytesAssetProvider::new();
        assets.register("asset.red", AssetKind::Svg, Arc::from(RED_SVG));

        let mut scene = Scene::new(10.0, 10.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        });
        scene.commands.push(SceneCommand::DrawImage {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            asset_id: "asset.red".to_string(),
            fit: FitMode::Stretch,
            pos_x: 50.0,
            pos_y: 50.0,
            opacity: 1.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let fonts = default_provider();
        let img = backend
            .rasterize(&scene, &fonts, &assets)
            .expect("SVG rasterize must succeed");

        // Center pixel must be red (r dominant, a > 0).
        let (r, g, b, a) = pixel(&img.rgba, img.width, 5, 5);
        assert!(a > 0, "center pixel must be opaque after SVG rasterize");
        assert!(
            r > g && r > b,
            "center pixel must be red-dominant; got r={r} g={g} b={b}"
        );
    }

    // ── SVG <text>: text element converts to paths and rasterizes ─────────

    /// An inline SVG containing a red `<text>` element is registered as
    /// `AssetKind::Svg`, drawn via DrawImage, and the output pixmap must contain
    /// at least one RED pixel — proving the text was converted to paths and
    /// rasterized (not silently dropped due to an empty fontdb).
    #[test]
    fn draw_image_svg_text_renders_red_pixels() {
        const TEXT_SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' \
            width='200' height='60'>\
            <text x='0' y='40' font-size='40' fill='#ff0000'>Hi</text>\
            </svg>";

        let mut assets = BytesAssetProvider::new();
        assets.register("asset.text_svg", AssetKind::Svg, Arc::from(TEXT_SVG));

        let mut scene = Scene::new(200.0, 60.0);
        scene.commands.push(SceneCommand::PushClip {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 60.0,
        });
        scene.commands.push(SceneCommand::DrawImage {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 60.0,
            asset_id: "asset.text_svg".to_string(),
            fit: FitMode::Stretch,
            pos_x: 50.0,
            pos_y: 50.0,
            opacity: 1.0,
        });
        scene.commands.push(SceneCommand::PopClip);

        let backend = TinySkiaBackend;
        let fonts = default_provider();
        let img = backend
            .rasterize(&scene, &fonts, &assets)
            .expect("SVG text rasterize must succeed");

        // At least one pixel must be red-dominant — text paths were rasterized.
        let any_red = (0..img.height).any(|py| {
            (0..img.width).any(|px| {
                let (r, g, b, a) = pixel(&img.rgba, img.width, px, py);
                a > 0 && r > g && r > b
            })
        });
        assert!(
            any_red,
            "SVG <text> must produce at least one red pixel after convert_text + rasterize"
        );
    }
}
