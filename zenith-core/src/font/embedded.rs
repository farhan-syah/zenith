//! Compile-time bytes of the bundled Noto fonts.
//!
//! These are the deterministic default faces used by [`super::default_provider`].
//! They live under `zenith-core/assets/fonts/` so the crate is self-contained
//! when published, and are exposed publicly so other workspace crates (e.g.
//! `zenith-layout`) can reuse the exact same bytes without embedding a copy.

/// Noto Sans Regular — `"Noto Sans"`, weight 400, normal.
pub const NOTO_SANS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
/// Noto Sans Bold — `"Noto Sans"`, weight 700, normal.
pub const NOTO_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");
/// Noto Sans Italic — `"Noto Sans"`, weight 400, italic.
pub const NOTO_SANS_ITALIC: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Italic.ttf");
/// Noto Sans Bold Italic — `"Noto Sans"`, weight 700, italic.
pub const NOTO_SANS_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/NotoSans-BoldItalic.ttf");
/// Noto Sans Mono Regular — `"Noto Sans Mono"`, weight 400, normal.
pub const NOTO_SANS_MONO_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/NotoSansMono-Regular.ttf");
/// Noto Sans Mono Bold — `"Noto Sans Mono"`, weight 700, normal.
pub const NOTO_SANS_MONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSansMono-Bold.ttf");
