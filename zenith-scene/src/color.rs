//! sRGB hex color parsing.
//!
//! Accepts `#rrggbb` (alpha = 255) and `#rrggbbaa`.
//! All hex digits may be upper- or lower-case.

use crate::ir::Color;

/// Parse an sRGB hex color string into a [`Color`].
///
/// Accepts `#rrggbb` (opaque, alpha = 255) and `#rrggbbaa`.
/// Returns `None` on any malformed input; never panics.
pub fn parse_srgb_hex(s: &str) -> Option<Color> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'#') {
        return None;
    }
    let hex = &bytes[1..];
    match hex.len() {
        6 => {
            let r = from_hex2(hex[0], hex[1])?;
            let g = from_hex2(hex[2], hex[3])?;
            let b = from_hex2(hex[4], hex[5])?;
            Some(Color::srgb(r, g, b, 255))
        }
        8 => {
            let r = from_hex2(hex[0], hex[1])?;
            let g = from_hex2(hex[2], hex[3])?;
            let b = from_hex2(hex[4], hex[5])?;
            let a = from_hex2(hex[6], hex[7])?;
            Some(Color::srgb(r, g, b, a))
        }
        _ => None,
    }
}

/// Parse any color literal — sRGB hex (`#rrggbb`/`#rrggbbaa`) or CMYK
/// (`cmyk(c,m,y,k)`) — into an [`ir::Color`](Color).
///
/// A CMYK literal yields a color whose `r`/`g`/`b` are the naive device sRGB
/// conversion and whose `cmyk` tag carries the original channels. A hex literal
/// yields an sRGB-origin color (`cmyk: None`). Returns `None` on any malformed
/// input; never panics.
pub fn parse_color(s: &str) -> Option<Color> {
    if s.starts_with("cmyk(") {
        let cmyk = zenith_core::parse_cmyk(s)?;
        let (r, g, b) = zenith_core::cmyk_to_srgb(cmyk);
        Some(Color::cmyk(cmyk.c, cmyk.m, cmyk.y, cmyk.k, r, g, b))
    } else {
        parse_srgb_hex(s)
    }
}

/// Decode two ASCII hex digits into a `u8`.  Returns `None` if either byte is
/// not a valid ASCII hex digit.
fn from_hex2(hi: u8, lo: u8) -> Option<u8> {
    let h = hex_nibble(hi)?;
    let l = hex_nibble(lo)?;
    Some((h << 4) | l)
}

/// Decode a single ASCII hex digit into its nibble value (0–15).
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lowercase_rrggbb() {
        let c = parse_srgb_hex("#f8fafc").expect("#f8fafc must parse");
        assert_eq!(c.r, 0xf8);
        assert_eq!(c.g, 0xfa);
        assert_eq!(c.b, 0xfc);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn parses_uppercase_rrggbb() {
        let c = parse_srgb_hex("#AABBCC").expect("#AABBCC must parse");
        assert_eq!(c.r, 0xAA);
        assert_eq!(c.g, 0xBB);
        assert_eq!(c.b, 0xCC);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn parses_rrggbbaa() {
        let c = parse_srgb_hex("#11223344").expect("#11223344 must parse");
        assert_eq!(c.r, 0x11);
        assert_eq!(c.g, 0x22);
        assert_eq!(c.b, 0x33);
        assert_eq!(c.a, 0x44);
    }

    #[test]
    fn hex_color_has_no_cmyk_tag() {
        let c = parse_srgb_hex("#f8fafc").expect("must parse");
        assert_eq!(c.cmyk, None);
    }

    #[test]
    fn parse_color_handles_cmyk() {
        let c = parse_color("cmyk(59,85,0,7)").expect("cmyk must parse");
        assert_eq!((c.r, c.g, c.b, c.a), (97, 36, 237, 255));
        assert_eq!(c.cmyk, Some([59.0, 85.0, 0.0, 7.0]));
    }

    #[test]
    fn parse_color_handles_hex() {
        let c = parse_color("#112233").expect("hex must parse");
        assert_eq!((c.r, c.g, c.b, c.a), (0x11, 0x22, 0x33, 255));
        assert_eq!(c.cmyk, None);
    }

    #[test]
    fn parse_color_rejects_malformed_cmyk() {
        assert!(parse_color("cmyk(0,0,0,200)").is_none());
    }

    #[test]
    fn rejects_invalid_hex() {
        assert!(parse_srgb_hex("#xyz").is_none());
    }

    #[test]
    fn rejects_missing_hash() {
        assert!(parse_srgb_hex("aabbcc").is_none());
    }

    #[test]
    fn rejects_too_short() {
        assert!(parse_srgb_hex("#aabb").is_none());
    }

    #[test]
    fn rejects_too_long() {
        assert!(parse_srgb_hex("#aabbccddee").is_none());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_srgb_hex("").is_none());
    }
}
