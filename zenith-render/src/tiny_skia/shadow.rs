//! Drop-shadow / outer-glow compositing for shadowed leaf nodes.
//!
//! The node's ink is captured into an offscreen `Pixmap` (premultiplied RGBA8).
//! At EndShadow, each shadow layer is derived from the ink's coverage (alpha),
//! tinted with the layer color, blurred, and composited behind the crisp ink.
//!
//! Blur uses a deterministic three-box approximation of a Gaussian
//! (Ivan Kuchin / "Fastest Gaussian Blur" 3-box method,
//! <http://blog.ivank.net/fastest-gaussian-blur.html>). All arithmetic uses
//! fixed integer/float evaluation order with consistent rounding, so output is
//! byte-identical across runs (no time, randomness, or hashing).

use tiny_skia::{Pixmap, PixmapPaint, Transform};
use zenith_scene::ShadowSpec;

/// Local alias for the scene `Color` carried inside a `ShadowSpec`, so this
/// helper does not need to import the scene `Color` name (which would collide
/// with tiny-skia's `Color`). Resolved at call sites via `spec.color`.
type SceneColor = zenith_scene::Color;

/// Paint all shadow layers of one capture onto `canvas`, then the crisp ink.
///
/// Layers are painted in REVERSE declared order so the first-declared layer ends
/// up on top of later layers (all behind the ink). `width`/`height` match both
/// `canvas` and `ink`.
pub(super) fn composite_shadows(
    canvas: &mut Pixmap,
    ink: &Pixmap,
    shadows: &[ShadowSpec],
    width: u32,
    height: u32,
) {
    let paint = PixmapPaint::default(); // source-over, opacity 1.0
    for spec in shadows.iter().rev() {
        // Build the tinted shadow coverage from the ink's straight alpha.
        let Some(mut shadow) = Pixmap::new(width, height) else {
            continue;
        };
        tint_coverage(&mut shadow, ink, spec.color);

        // Blur in premultiplied space (correct for source-over compositing).
        gaussian_blur_premul(&mut shadow, spec.blur);

        // Composite at the rounded integer offset. Rounding is deterministic.
        let dx = round_offset(spec.dx);
        let dy = round_offset(spec.dy);
        canvas.draw_pixmap(dx, dy, shadow.as_ref(), &paint, Transform::identity(), None);
    }

    // Crisp ink on top of every shadow.
    canvas.draw_pixmap(0, 0, ink.as_ref(), &paint, Transform::identity(), None);
}

/// Round a shadow offset to the nearest integer pixel, deterministically and
/// without panicking. `f64::round` ties away from zero; non-finite collapses to 0.
fn round_offset(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let r = v.round();
    if r >= i32::MAX as f64 {
        i32::MAX
    } else if r <= i32::MIN as f64 {
        i32::MIN
    } else {
        r as i32
    }
}

/// Fill `shadow` (premultiplied RGBA8) with the shadow color, modulated by the
/// `ink`'s per-pixel alpha.
///
/// For each pixel: straight alpha = `ink_alpha * (color.a / 255)`, color =
/// `color.rgb`; written PREMULTIPLIED. Iterates in lockstep via `chunks_exact(4)`,
/// which guarantees exactly 4 bytes per chunk; direct indexing is panic-free.
/// `shadow` and `ink` share dimensions.
fn tint_coverage(shadow: &mut Pixmap, ink: &Pixmap, color: SceneColor) {
    let ca = u32::from(color.a);
    let cr = u32::from(color.r);
    let cg = u32::from(color.g);
    let cb = u32::from(color.b);
    let dst = shadow.data_mut();
    let src = ink.data();
    for (out, inp) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        // tiny-skia premultiplied RGBA: byte 3 is alpha (coverage). The ink's
        // premultiplied alpha equals its straight alpha (alpha is never scaled).
        // chunks_exact(4) guarantees exactly 4 bytes; direct indexing is safe.
        let ink_a = u32::from(inp[3]);
        // straight shadow alpha = ink_a * ca / 255, rounded.
        let a = ((ink_a * ca) + 127) / 255;
        // Premultiply the (constant) color by this alpha.
        let pr = ((cr * a) + 127) / 255;
        let pg = ((cg * a) + 127) / 255;
        let pb = ((cb * a) + 127) / 255;
        out[0] = pr.min(255) as u8;
        out[1] = pg.min(255) as u8;
        out[2] = pb.min(255) as u8;
        out[3] = a.min(255) as u8;
    }
}

/// Compute the three box-blur sizes that approximate a Gaussian of the given
/// `sigma` with `n == 3` passes (Kuchin's method).
///
/// Returns three odd box widths; the box radius for a width `w` is `(w - 1) / 2`.
fn boxes_for_gauss(sigma: f64) -> [u32; 3] {
    const N: f64 = 3.0;
    if sigma <= 0.0 {
        return [1, 1, 1];
    }
    let w_ideal = ((12.0 * sigma * sigma / N) + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    if wl < 1 {
        wl = 1;
    }
    let wu = wl + 2;
    let wl_f = wl as f64;
    let m_ideal =
        (12.0 * sigma * sigma - N * wl_f * wl_f - 4.0 * N * wl_f - 3.0 * N) / (-4.0 * wl_f - 4.0);
    let m = m_ideal.round() as i64;
    let mut out = [0u32; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let w = if (i as i64) < m { wl } else { wu };
        *slot = w.max(1) as u32;
    }
    out
}

/// Apply a deterministic separable Gaussian-approximation blur (three box
/// passes) to a premultiplied RGBA8 `Pixmap`, in place.
///
/// Each pass is a horizontal box blur followed by a vertical box blur, computed
/// with running sums over each of the four premultiplied channels independently
/// (premultiplied blur is correct for source-over compositing). All indexing is
/// bounds-guarded; arithmetic uses `u32` running sums with fixed rounding, so
/// the result is byte-identical across runs.
fn gaussian_blur_premul(pm: &mut Pixmap, sigma: f64) {
    // Non-positive or non-finite sigma → no blur (NaN-safe: the `> 0.0` test is
    // false for NaN, so we return early).
    if !(sigma.is_finite() && sigma > 0.0) {
        return;
    }
    let width = pm.width() as usize;
    let height = pm.height() as usize;
    if width == 0 || height == 0 {
        return;
    }
    let boxes = boxes_for_gauss(sigma);
    let data = pm.data_mut();
    let expected = width * height * 4;
    if data.len() != expected {
        return; // defensive: unexpected layout → leave untouched
    }
    let mut scratch = vec![0u8; expected];
    for w in boxes {
        let radius = ((w.max(1) - 1) / 2) as usize;
        if radius == 0 {
            continue; // a 1-wide box is identity
        }
        // Horizontal: data → scratch.
        box_blur_h(data, &mut scratch, width, height, radius);
        // Vertical: scratch → data.
        box_blur_v(&scratch, data, width, height, radius);
    }
}

/// Horizontal box blur of radius `radius` over premultiplied RGBA8, writing the
/// result into `dst`. Uses a running sum per channel; no panic indexing.
fn box_blur_h(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: usize) {
    let window = (2 * radius + 1) as u32;
    let last = width.saturating_sub(1);
    for y in 0..height {
        let row = y * width * 4;
        for c in 0..4 {
            // Initialize the running sum for the window CENTERED at x=0, i.e.
            // positions [-radius, radius] each clamped to [0, width-1] (edge
            // extension). Positions < 0 collapse onto column 0.
            let mut sum: u32 = 0;
            for i in 0..=(2 * radius) {
                // Map loop index i∈[0,2r] to signed offset (i - radius), clamped.
                let xx = (i.saturating_sub(radius)).min(last);
                let v = src.get(row + xx * 4 + c).copied().unwrap_or(0);
                sum += u32::from(v);
            }
            for x in 0..width {
                if let Some(o) = dst.get_mut(row + x * 4 + c) {
                    *o = ((sum + window / 2) / window).min(255) as u8;
                }
                // Slide one column right: add pixel at x+radius+1 (clamped),
                // drop the leftmost at x-radius (clamped to 0 via saturating_sub).
                let add_x = (x + radius + 1).min(last);
                let sub_x = x.saturating_sub(radius);
                let add = src.get(row + add_x * 4 + c).copied().unwrap_or(0);
                let sub = src.get(row + sub_x * 4 + c).copied().unwrap_or(0);
                sum = sum + u32::from(add) - u32::from(sub);
            }
        }
    }
}

/// Vertical box blur of radius `radius` over premultiplied RGBA8, writing the
/// result into `dst`. Uses a running sum per channel; no panic indexing.
fn box_blur_v(src: &[u8], dst: &mut [u8], width: usize, height: usize, radius: usize) {
    let window = (2 * radius + 1) as u32;
    let stride = width * 4;
    let last = height.saturating_sub(1);
    for x in 0..width {
        let col = x * 4;
        for c in 0..4 {
            // Window centered at y=0 with edge extension (see box_blur_h).
            let mut sum: u32 = 0;
            for i in 0..=(2 * radius) {
                let yy = (i.saturating_sub(radius)).min(last);
                let v = src.get(col + yy * stride + c).copied().unwrap_or(0);
                sum += u32::from(v);
            }
            for y in 0..height {
                if let Some(o) = dst.get_mut(col + y * stride + c) {
                    *o = ((sum + window / 2) / window).min(255) as u8;
                }
                let add_y = (y + radius + 1).min(last);
                let sub_y = y.saturating_sub(radius);
                let add = src.get(col + add_y * stride + c).copied().unwrap_or(0);
                let sub = src.get(col + sub_y * stride + c).copied().unwrap_or(0);
                sum = sum + u32::from(add) - u32::from(sub);
            }
        }
    }
}
