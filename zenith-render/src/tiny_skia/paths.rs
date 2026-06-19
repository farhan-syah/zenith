//! Geometry / path helpers for the tiny-skia backend: rect intersection, clip
//! mask construction, polygon and rounded-rect path builders, and the glyph
//! outline pen.

use tiny_skia::{FillRule, Mask, Path, PathBuilder, Rect, Transform};

/// Build a clip `Mask` from the current effective clip rectangle.
///
/// Returns:
/// - `None` — the effective clip is empty or fully off-canvas; the caller
///   should skip the draw entirely (`continue`).
/// - `Some(None)` — the clip covers the whole pixmap; no masking needed,
///   draw with `mask = None` (the common, no-frame case — avoids allocating
///   a full-size mask on every top-level draw).
/// - `Some(Some(mask))` — a real sub-page clip; draw with `mask = Some(&mask)`.
pub(super) fn clip_mask(
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
pub(super) fn intersect_rects(
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
pub(super) fn build_poly_path(points: &[f64], closed: bool) -> Option<Path> {
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
pub(super) fn build_rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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
pub(super) struct GlyphOutlinePen {
    pub(super) builder: PathBuilder,
    origin_x: f32,
    baseline_y: f32,
    scale: f32,
}

impl GlyphOutlinePen {
    pub(super) fn new(origin_x: f32, baseline_y: f32, scale: f32) -> Self {
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
