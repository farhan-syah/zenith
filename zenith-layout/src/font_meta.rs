//! Font face metadata extraction via `rustybuzz::ttf_parser`.
//!
//! Reads the family name, weight, and style from a raw TTF/OTF byte slice
//! without loading a full shaping engine. Used at project-load time to
//! register asset-declared fonts in the [`BytesFontProvider`].

use rustybuzz::ttf_parser;
use rustybuzz::ttf_parser::name_id;
use zenith_core::FontStyle;

use crate::error::LayoutError;

/// Metadata extracted from a font face's name and OS/2 tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceMetadata {
    /// The typographic family name (e.g. `"Noto Sans"`).
    pub family: String,
    /// Numeric weight (e.g. 400, 700).
    pub weight: u16,
    /// Normal or italic style.
    pub style: FontStyle,
}

/// Extract [`FaceMetadata`] from raw font bytes at the given face `index`.
///
/// Family name resolution prefers name ID 16 (Typographic Family) over name
/// ID 1 (Family), and prefers unicode-encoded entries within each ID group.
///
/// # Errors
///
/// Returns [`LayoutError`] when:
/// - The bytes cannot be parsed as a valid font face.
/// - The name table contains no usable family name entry.
pub fn face_metadata(bytes: &[u8], index: u32) -> Result<FaceMetadata, LayoutError> {
    let face = ttf_parser::Face::parse(bytes, index)
        .map_err(|e| LayoutError::new(format!("font parse failed: {e:?}")))?;

    let family =
        best_family_name(&face).ok_or_else(|| LayoutError::new("font has no family name"))?;

    let weight = face.weight().to_number();
    let style = if face.is_italic() {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };

    Ok(FaceMetadata {
        family,
        weight,
        style,
    })
}

/// Walk the name table and return the best available family name.
///
/// Strategy:
/// 1. Collect the best unicode string for name ID 16 (Typographic Family).
/// 2. Collect the best unicode string for name ID 1 (Family).
/// 3. Return whichever is found first in that order; prefer unicode encoding.
fn best_family_name(face: &ttf_parser::Face<'_>) -> Option<String> {
    let mut typo_family: Option<String> = None;
    let mut family: Option<String> = None;

    for name in face.names() {
        if name.name_id == name_id::TYPOGRAPHIC_FAMILY
            && typo_family.is_none()
            && let Some(s) = name.to_string()
        {
            typo_family = Some(s);
        } else if name.name_id == name_id::FAMILY
            && family.is_none()
            && let Some(s) = name.to_string()
        {
            family = Some(s);
        }
    }

    typo_family.or(family)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Paths are relative to this source file: zenith-layout/src/font_meta.rs
    // The assets/fonts directory lives at the workspace root (two dirs up from src/).
    const REGULAR: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    const BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");
    const ITALIC: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Italic.ttf");
    const BOLD_ITALIC: &[u8] = include_bytes!("../../assets/fonts/NotoSans-BoldItalic.ttf");
    const MONO: &[u8] = include_bytes!("../../assets/fonts/NotoSansMono-Regular.ttf");

    #[test]
    fn noto_sans_regular_family_weight_style() {
        let m = face_metadata(REGULAR, 0).expect("regular must parse");
        assert!(
            m.family.contains("Noto Sans"),
            "family should contain 'Noto Sans', got '{}'",
            m.family
        );
        assert_eq!(m.weight, 400, "Regular weight must be 400");
        assert_eq!(m.style, FontStyle::Normal, "Regular must be Normal");
    }

    #[test]
    fn noto_sans_bold_weight() {
        let m = face_metadata(BOLD, 0).expect("bold must parse");
        assert!(
            m.family.contains("Noto Sans"),
            "family should contain 'Noto Sans', got '{}'",
            m.family
        );
        assert_eq!(m.weight, 700, "Bold weight must be 700");
        assert_eq!(m.style, FontStyle::Normal, "Bold (upright) must be Normal");
    }

    #[test]
    fn noto_sans_italic_style() {
        let m = face_metadata(ITALIC, 0).expect("italic must parse");
        assert!(
            m.family.contains("Noto Sans"),
            "family should contain 'Noto Sans', got '{}'",
            m.family
        );
        assert_eq!(m.style, FontStyle::Italic, "Italic must be Italic");
    }

    #[test]
    fn noto_sans_bold_italic_weight_and_style() {
        let m = face_metadata(BOLD_ITALIC, 0).expect("bold-italic must parse");
        assert!(
            m.family.contains("Noto Sans"),
            "family should contain 'Noto Sans', got '{}'",
            m.family
        );
        assert_eq!(m.weight, 700, "Bold-Italic weight must be 700");
        assert_eq!(m.style, FontStyle::Italic, "Bold-Italic must be Italic");
    }

    #[test]
    fn noto_sans_mono_family() {
        let m = face_metadata(MONO, 0).expect("mono must parse");
        assert!(
            m.family.contains("Noto Sans Mono") || m.family.contains("Noto Sans"),
            "mono family should contain 'Noto Sans', got '{}'",
            m.family
        );
        assert_eq!(m.weight, 400, "Mono Regular weight must be 400");
        assert_eq!(m.style, FontStyle::Normal, "Mono Regular must be Normal");
    }

    #[test]
    fn invalid_bytes_return_err() {
        let result = face_metadata(b"not a font", 0);
        assert!(result.is_err(), "invalid bytes must return Err");
        let msg = result.unwrap_err().message;
        assert!(
            msg.contains("font parse failed"),
            "error should mention 'font parse failed', got: {msg}"
        );
    }
}
