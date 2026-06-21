//! Per-pixel color filters for filtered leaf nodes.
//!
//! The node's ink is captured into an offscreen `Pixmap` (premultiplied RGBA8).
//! At EndFilter, each [`FilterSpec`] is applied, in declared order, to the
//! straight-alpha (un-pre-multiplied) RGB of every pixel, then the result is
//! re-premultiplied and composited onto the target.
//!
//! All arithmetic is pure `f64` with deterministic rounding (no time, no
//! randomness, no hashing), so output is byte-identical across runs on the same
//! machine — matching the shadow/blur anti-aliasing policy.
//!
//! Color-filter semantics follow the standard CSS/SVG `filter` functions. Alpha
//! is never modified by a color filter; fully-transparent pixels are skipped.

use tiny_skia::Pixmap;
use zenith_scene::FilterSpec;

use super::pixels::premultiplied_to_straight;

/// Apply `filters` (in order) to every pixel of `pm`, in place.
///
/// Each pixel is un-pre-multiplied to straight `[0,1]` RGB, transformed by each
/// filter in turn (clamped after every op), then re-premultiplied. Iteration is
/// over `chunks_exact_mut(4)`, which guarantees exactly 4 bytes per chunk; no
/// manual indexing, no panics.
pub(super) fn apply_filters(pm: &mut Pixmap, filters: &[FilterSpec]) {
    if filters.is_empty() {
        return;
    }
    for px in pm.data_mut().chunks_exact_mut(4) {
        // tiny-skia premultiplied RGBA byte order: [r, g, b, a].
        let a = px[3];
        if a == 0 {
            // Alpha 0 → fully transparent; color filters leave it untouched.
            continue;
        }
        let (sr, sg, sb, _) = premultiplied_to_straight(px[0], px[1], px[2], a);
        let mut r = f64::from(sr) / 255.0;
        let mut g = f64::from(sg) / 255.0;
        let mut b = f64::from(sb) / 255.0;

        for spec in filters {
            let (nr, ng, nb) = apply_one(spec, r, g, b);
            r = nr.clamp(0.0, 1.0);
            g = ng.clamp(0.0, 1.0);
            b = nb.clamp(0.0, 1.0);
        }

        // Straight [0,1] → straight [0,255] with deterministic rounding
        // (`floor(x * 255 + 0.5)`), matching the shadow premultiply path.
        let sr = to_u8(r);
        let sg = to_u8(g);
        let sb = to_u8(b);

        // Re-premultiply by the (unchanged) alpha, mirroring shadow.rs:
        // premultiplied = (channel * alpha + 127) / 255.
        let af = u32::from(a);
        px[0] = premul(u32::from(sr), af);
        px[1] = premul(u32::from(sg), af);
        px[2] = premul(u32::from(sb), af);
        // px[3] (alpha) is intentionally left unchanged.
    }
}

/// Apply `filters` (in order) to a STRAIGHT-alpha RGBA8 buffer, in place.
///
/// Unlike [`apply_filters`], this operates directly on straight (un-premultiplied)
/// RGBA8 — the exact format of [`crate::backend::RasterImage::rgba`] — so there is
/// no premultiply round-trip. The per-kind color math is the SAME (`apply_one`,
/// clamped after every op, rounded via `to_u8`). Alpha bytes are never modified;
/// fully-transparent pixels (`a == 0`) are skipped. Iteration is over
/// `chunks_exact_mut(4)`, so there is no manual indexing and no panic.
///
/// Used by the PDF backend, which has no per-pixel filter primitive: a filtered
/// leaf is rasterized to straight RGBA, transformed here, then embedded as an
/// image XObject. Pure `f64` math with deterministic rounding ⇒ byte-identical
/// across runs.
pub(crate) fn apply_filters_straight(rgba: &mut [u8], filters: &[FilterSpec]) {
    if filters.is_empty() {
        return;
    }
    for px in rgba.chunks_exact_mut(4) {
        // Straight RGBA byte order: [r, g, b, a].
        let a = px[3];
        if a == 0 {
            // Alpha 0 → fully transparent; color filters leave it untouched.
            continue;
        }
        let mut r = f64::from(px[0]) / 255.0;
        let mut g = f64::from(px[1]) / 255.0;
        let mut b = f64::from(px[2]) / 255.0;

        for spec in filters {
            let (nr, ng, nb) = apply_one(spec, r, g, b);
            r = nr.clamp(0.0, 1.0);
            g = ng.clamp(0.0, 1.0);
            b = nb.clamp(0.0, 1.0);
        }

        // Straight [0,1] → straight [0,255] with the same deterministic rounding
        // (`floor(x * 255 + 0.5)`) used by `apply_filters`.
        px[0] = to_u8(r);
        px[1] = to_u8(g);
        px[2] = to_u8(b);
        // px[3] (alpha) is intentionally left unchanged.
    }
}

/// Apply a single filter op to straight-alpha RGB in `[0,1]` (unclamped output;
/// the caller clamps each channel after every op).
fn apply_one(spec: &FilterSpec, r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    // Luma weights (Rec. 709), shared by grayscale, saturate, and duotone.
    const RW: f64 = 0.2126;
    const GW: f64 = 0.7152;
    const BW: f64 = 0.0722;
    match *spec {
        FilterSpec::Grayscale(amount) => {
            let a = amount.clamp(0.0, 1.0);
            let luma = RW * r + GW * g + BW * b;
            (lerp(r, luma, a), lerp(g, luma, a), lerp(b, luma, a))
        }
        FilterSpec::Invert(amount) => {
            let a = amount.clamp(0.0, 1.0);
            (
                lerp(r, 1.0 - r, a),
                lerp(g, 1.0 - g, a),
                lerp(b, 1.0 - b, a),
            )
        }
        FilterSpec::Sepia(amount) => {
            let a = amount.clamp(0.0, 1.0);
            let sr = 0.393 * r + 0.769 * g + 0.189 * b;
            let sg = 0.349 * r + 0.686 * g + 0.168 * b;
            let sb = 0.272 * r + 0.534 * g + 0.131 * b;
            (lerp(r, sr, a), lerp(g, sg, a), lerp(b, sb, a))
        }
        FilterSpec::Saturate(amount) => {
            // SVG feColorMatrix saturate matrix with s = amount.
            let s = amount;
            let out_r = (RW + (1.0 - RW) * s) * r + (GW - GW * s) * g + (BW - BW * s) * b;
            let out_g = (RW - RW * s) * r + (GW + (1.0 - GW) * s) * g + (BW - BW * s) * b;
            let out_b = (RW - RW * s) * r + (GW - GW * s) * g + (BW + (1.0 - BW) * s) * b;
            (out_r, out_g, out_b)
        }
        FilterSpec::Brightness(amount) => {
            let a = amount;
            (r * a, g * a, b * a)
        }
        FilterSpec::Contrast(amount) => {
            let a = amount;
            (
                (r - 0.5) * a + 0.5,
                (g - 0.5) * a + 0.5,
                (b - 0.5) * a + 0.5,
            )
        }
        FilterSpec::Duotone {
            amount,
            shadow,
            highlight,
        } => {
            // Straight [0,1] endpoints from the two colors.
            let sh = (
                f64::from(shadow.r) / 255.0,
                f64::from(shadow.g) / 255.0,
                f64::from(shadow.b) / 255.0,
            );
            let hi = (
                f64::from(highlight.r) / 255.0,
                f64::from(highlight.g) / 255.0,
                f64::from(highlight.b) / 255.0,
            );
            let luma = RW * r + GW * g + BW * b;
            // Map luma → shadow..highlight (dark→shadow, light→highlight).
            let d_r = lerp(sh.0, hi.0, luma);
            let d_g = lerp(sh.1, hi.1, luma);
            let d_b = lerp(sh.2, hi.2, luma);
            // Mix the duotone color with the original by `amount`.
            let t = amount.clamp(0.0, 1.0);
            (lerp(r, d_r, t), lerp(g, d_g, t), lerp(b, d_b, t))
        }
        FilterSpec::HueRotate(amount) => {
            // SVG feColorMatrix hueRotate matrix; amount is in DEGREES.
            let rad = amount.to_radians();
            let cos = rad.cos();
            let sin = rad.sin();
            let m00 = 0.213 + cos * 0.787 - sin * 0.213;
            let m01 = 0.715 - cos * 0.715 - sin * 0.715;
            let m02 = 0.072 - cos * 0.072 + sin * 0.928;
            let m10 = 0.213 - cos * 0.213 + sin * 0.143;
            let m11 = 0.715 + cos * 0.285 + sin * 0.140;
            let m12 = 0.072 - cos * 0.072 - sin * 0.283;
            let m20 = 0.213 - cos * 0.213 - sin * 0.787;
            let m21 = 0.715 - cos * 0.715 + sin * 0.715;
            let m22 = 0.072 + cos * 0.928 + sin * 0.072;
            (
                m00 * r + m01 * g + m02 * b,
                m10 * r + m11 * g + m12 * b,
                m20 * r + m21 * g + m22 * b,
            )
        }
    }
}

/// Linear interpolation `from → to` by `t` (caller guarantees `t ∈ [0,1]`).
fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

/// Convert a straight channel in `[0,1]` to `[0,255]`, rounding via
/// `floor(x * 255 + 0.5)`. Input is pre-clamped by the caller; the final
/// `min(255)` is a defensive guard against floating-point overshoot.
fn to_u8(x: f64) -> u8 {
    let v = (x * 255.0 + 0.5).floor();
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}

/// Premultiply a straight channel `c ∈ [0,255]` by alpha `a ∈ [0,255]`,
/// rounding via `(c * a + 127) / 255` — identical to the shadow path.
fn premul(c: u32, a: u32) -> u8 {
    (((c * a) + 127) / 255).min(255) as u8
}
