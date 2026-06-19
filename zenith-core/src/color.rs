//! WCAG 2.2 color math — hex parsing, relative luminance, contrast ratio.
//!
//! All functions are pure and panic-free.  Alpha is ignored for luminance
//! (WCAG 2.2 §1.4.3 measures opaque foreground on opaque background).

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a `#rrggbb` or `#rrggbbaa` hex string into `(r, g, b)` as raw u8 bytes.
///
/// The leading `#` is required.  Only 6-digit and 8-digit forms are accepted;
/// 3-digit short-form is NOT (Zenith tokens always store canonical long form).
/// Alpha is silently discarded — only the RGB channels are returned.
///
/// Returns `None` on any parse error or unexpected length.
pub fn parse_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#')?;
    // Accept exactly 6 (rrggbb) or 8 (rrggbbaa) hex digits.
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

/// WCAG 2.2 relative luminance of an sRGB color, in the range 0.0..=1.0.
///
/// Formula: <https://www.w3.org/TR/WCAG22/#dfn-relative-luminance>
pub fn relative_luminance(rgb: (u8, u8, u8)) -> f64 {
    let linearize = |channel: u8| -> f64 {
        let c = channel as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = linearize(rgb.0);
    let g = linearize(rgb.1);
    let b = linearize(rgb.2);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2.2 contrast ratio between two colors, in the range 1.0..=21.0.
///
/// The order of the arguments does not matter — the brighter color is always
/// placed in the numerator.
///
/// Formula: `(L1 + 0.05) / (L2 + 0.05)` where `L1 >= L2`.
pub fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // parse_rgb ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_rgb_6_digits() {
        assert_eq!(parse_rgb("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_rgb("#000000"), Some((0, 0, 0)));
        assert_eq!(parse_rgb("#aabbcc"), Some((0xaa, 0xbb, 0xcc)));
    }

    #[test]
    fn parse_rgb_8_digits_ignores_alpha() {
        // Alpha byte (ff) is dropped; RGB is preserved.
        assert_eq!(parse_rgb("#aabbccff"), Some((0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_rgb("#ffffff80"), Some((255, 255, 255)));
    }

    #[test]
    fn parse_rgb_rejects_bad_input() {
        assert_eq!(parse_rgb("ffffff"), None); // missing #
        assert_eq!(parse_rgb("#fff"), None); // 3-digit short form
        assert_eq!(parse_rgb("#fffffg"), None); // invalid hex digit
        assert_eq!(parse_rgb(""), None);
        assert_eq!(parse_rgb("#"), None);
    }

    // relative_luminance ────────────────────────────────────────────────────

    #[test]
    fn luminance_black_is_zero() {
        let l = relative_luminance((0, 0, 0));
        assert!(l.abs() < 1e-10, "black luminance should be 0, got {l}");
    }

    #[test]
    fn luminance_white_is_one() {
        let l = relative_luminance((255, 255, 255));
        assert!(
            (l - 1.0).abs() < 1e-6,
            "white luminance should be ~1, got {l}"
        );
    }

    // contrast_ratio ────────────────────────────────────────────────────────

    #[test]
    fn contrast_white_vs_black_approx_21() {
        let ratio = contrast_ratio((255, 255, 255), (0, 0, 0));
        assert!(
            (ratio - 21.0).abs() < 0.1,
            "white/black ratio should be ~21, got {ratio}"
        );
    }

    #[test]
    fn contrast_white_vs_white_is_1() {
        let ratio = contrast_ratio((255, 255, 255), (255, 255, 255));
        assert!(
            (ratio - 1.0).abs() < 1e-6,
            "same color ratio should be 1, got {ratio}"
        );
    }

    #[test]
    fn contrast_gray_777_on_white_approx_4_48() {
        // #777777 on #ffffff — well-known WCAG reference pair ≈ 4.48:1.
        let ratio = contrast_ratio((0x77, 0x77, 0x77), (255, 255, 255));
        assert!(
            (ratio - 4.48).abs() < 0.1,
            "#777777 on white should be ~4.48, got {ratio}"
        );
    }

    #[test]
    fn contrast_ratio_is_symmetric() {
        let ab = contrast_ratio((0x77, 0x77, 0x77), (255, 255, 255));
        let ba = contrast_ratio((255, 255, 255), (0x77, 0x77, 0x77));
        assert!(
            (ab - ba).abs() < 1e-10,
            "contrast_ratio must be symmetric: {ab} vs {ba}"
        );
    }
}
