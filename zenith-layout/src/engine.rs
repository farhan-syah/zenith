//! Zenith-owned text-layout types and the `TextLayoutEngine` trait.
//!
//! No third-party shaping or font types appear here. All shaping engines
//! implement `TextLayoutEngine` and hide their dependencies behind it.

use zenith_core::{FontProvider, FontStyle};

use crate::error::LayoutError;

/// Base writing direction for a shaping request.
///
/// Controls the rustybuzz buffer direction so glyph advances and complex-script
/// joining (e.g. Arabic) are correct. The DEFAULT is [`TextDirection::Ltr`], so
/// a request that omits the field shapes exactly as before (byte-identical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Left-to-right (the default).
    #[default]
    Ltr,
    /// Right-to-left (Arabic, Hebrew, …). The shaper reorders glyphs to visual
    /// order and applies RTL-correct joining.
    Rtl,
}

/// A request to shape a run of text into positioned glyphs.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRequest<'a> {
    /// The text to shape.
    pub text: &'a str,
    /// Priority-ordered font family preferences.
    pub families: &'a [String],
    /// Font weight (e.g. 400 = regular, 700 = bold).
    pub weight: u16,
    /// Font style variant.
    pub style: FontStyle,
    /// Requested font size in pixels.
    pub font_size: f32,
    /// Base writing direction. Defaults to [`TextDirection::Ltr`].
    pub direction: TextDirection,
}

/// One positioned glyph, baseline-relative, measured from the run origin in pixels.
///
/// Positive x is rightward; positive y is downward (0 = on the baseline).
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    /// Glyph identifier within the resolved font face.
    pub glyph_id: u16,
    /// Horizontal offset from the run origin, in pixels.
    pub x: f32,
    /// Vertical offset from the baseline, in pixels (positive = below baseline).
    pub y: f32,
}

/// A shaped run of text in a single resolved font.
///
/// All values are in pixels. No third-party types appear in any field.
#[derive(Debug, Clone, PartialEq)]
pub struct ZenithGlyphRun {
    /// Stable id of the resolved font face (matches `FontData::id`).
    ///
    /// The renderer re-resolves font bytes via `FontProvider::by_id`.
    pub font_id: String,
    /// Font size at which the run was shaped, in pixels.
    pub font_size: f32,
    /// Ascent in pixels, positive above the baseline.
    ///
    /// Baseline placement: `box_top + ascent`.
    pub ascent: f32,
    /// Descent magnitude in pixels (positive value; baseline to bottom of descenders).
    pub descent: f32,
    /// Recommended line height in pixels: `ascent + descent + line_gap`.
    pub line_height: f32,
    /// Total pen advance across the run in pixels.
    pub advance_width: f32,
    /// Positioned glyphs, baseline-relative, in run order.
    pub glyphs: Vec<PositionedGlyph>,
}

/// Result of fallback shaping: the shaped runs plus any characters that NO
/// registered face (primary or fallback) could supply a glyph for.
pub struct FallbackResult {
    /// Shaped glyph runs, one per contiguous sub-run that resolved to a single
    /// face, in visual order.
    pub runs: Vec<ZenithGlyphRun>,
    /// Characters (deduped, sorted by codepoint) for which no registered face
    /// had a glyph. Excludes default-ignorable code points (joiners, variation
    /// selectors, control characters, whitespace, etc.).
    pub missing_chars: Vec<char>,
}

/// Trait implemented by every shaping engine.
///
/// Engines are free to resolve fonts, call native shapers, and accumulate any
/// internal state, but they must not expose third-party types through this trait.
pub trait TextLayoutEngine {
    /// Shape `req.text` into a `ZenithGlyphRun` using fonts from `provider`.
    ///
    /// # Errors
    ///
    /// Returns `LayoutError` if no font can be resolved, if the font bytes are
    /// malformed, if `units_per_em` is zero, or if any other shaping step fails.
    fn shape(
        &self,
        req: &ShapeRequest<'_>,
        provider: &dyn FontProvider,
    ) -> Result<ZenithGlyphRun, LayoutError>;

    /// Shape `req.text` with per-glyph font fallback, returning a
    /// [`FallbackResult`] with one [`ZenithGlyphRun`] per contiguous sub-run
    /// that resolved to a single face, plus any characters that no registered
    /// face could cover.
    ///
    /// The primary face (resolved from `req.families`/`weight`/`style`) shapes
    /// every character it covers; characters the primary lacks are itemized to
    /// the first face in `provider.all_faces()` (deterministic order) that
    /// covers them, falling back to the primary (rendering `.notdef`) when no
    /// registered face covers a character. Whitespace and punctuation the
    /// primary covers stay with the primary, so mixed-script runs do not
    /// fragment on shared characters.
    ///
    /// When every character is covered by the primary face this returns exactly
    /// one run, identical to [`Self::shape`], with an empty `missing_chars`.
    ///
    /// # Errors
    ///
    /// Returns `LayoutError` under the same conditions as [`Self::shape`]
    /// (no resolvable primary font, malformed bytes, zero `units_per_em`).
    fn shape_with_fallback(
        &self,
        req: &ShapeRequest<'_>,
        provider: &dyn FontProvider,
    ) -> Result<FallbackResult, LayoutError>;
}
