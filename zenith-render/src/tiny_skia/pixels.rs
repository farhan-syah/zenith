//! Dimension and pixel-format conversion helpers for the tiny-skia backend.

use crate::error::RenderError;

/// Maximum allowed dimension in either axis (width or height).
///
/// Prevents gigantic allocations from malformed or adversarial scenes.
const MAX_DIMENSION: u32 = 16_384;

/// Convert scene `f64` dimensions to `u32` pixels, enforcing sanity rules.
///
/// Returns `Err` when:
/// - The value is non-finite (`NaN`, `±inf`).
/// - `value.round()` is `<= 0` (page must have positive extent).
/// - The rounded value exceeds [`MAX_DIMENSION`].
pub(super) fn f64_to_px(value: f64, axis: &str) -> Result<u32, RenderError> {
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

/// Convert premultiplied RGBA8 (tiny-skia's internal storage) to straight-alpha RGBA8.
pub(super) fn premultiplied_to_straight(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8, u8) {
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
