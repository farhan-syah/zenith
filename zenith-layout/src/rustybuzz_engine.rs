//! `rustybuzz`-backed shaping engine for Zenith.
//!
//! This is the ONLY module in the crate that imports `rustybuzz` or
//! `rustybuzz::ttf_parser`. No third-party type escapes to a public signature.

use std::collections::BTreeSet;

use zenith_core::FontProvider;

use crate::engine::{
    FallbackResult, PositionedGlyph, ShapeRequest, TextDirection, TextLayoutEngine, ZenithGlyphRun,
};
use crate::error::LayoutError;

/// Code points that legitimately have no standalone glyph (consumed during
/// shaping) and must NOT be reported as missing: control/whitespace and the
/// Unicode default-ignorable ranges (joiners, bidi marks, variation selectors,
/// BOM, soft hyphen, word joiner, etc.).
fn is_ignorable_for_coverage(ch: char) -> bool {
    ch.is_control()
        || ch.is_whitespace()
        || matches!(
            ch as u32,
            0x00AD            // soft hyphen
            | 0x200B..=0x200F // ZWSP, ZWNJ, ZWJ, LRM, RLM
            | 0x202A..=0x202E // bidi embeddings/overrides
            | 0x2060..=0x206F // word joiner, invisible operators, deprecated format
            | 0xFEFF          // BOM / ZWNBSP
            | 0xFE00..=0xFE0F // variation selectors
            | 0xE0100..=0xE01EF // variation selectors supplement
        )
}

/// HarfBuzz-port shaping engine backed by `rustybuzz` and `rustybuzz::ttf_parser`.
///
/// Construct once and reuse across many `shape` calls; the engine is stateless.
#[derive(Debug, Clone)]
pub struct RustybuzzEngine;

impl RustybuzzEngine {
    /// Create a new `RustybuzzEngine`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustybuzzEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RustybuzzEngine {
    /// Shape `text` with an already-parsed `face` and produce a single
    /// [`ZenithGlyphRun`] tagged with `font_id`.
    ///
    /// This is the one place shaping, scaling, and metric derivation live, so
    /// `shape` (single-face) and `shape_with_fallback` (per-glyph fallback)
    /// cannot diverge: both route every run through here. Glyphs are positioned
    /// from `x = 0` within the run.
    ///
    /// # Errors
    ///
    /// Returns `LayoutError` if the face reports `units_per_em <= 0`.
    fn shape_run_with_face(
        face: &rustybuzz::Face<'_>,
        text: &str,
        font_id: String,
        font_size: f32,
        direction: TextDirection,
    ) -> Result<ZenithGlyphRun, LayoutError> {
        // ── Compute pixel scale ───────────────────────────────────────────────
        // `units_per_em` comes from the `ttf_parser::Face` trait exposed by
        // `rustybuzz::Face` via Deref.
        let units_per_em = face.units_per_em();
        if units_per_em <= 0 {
            return Err(LayoutError::new(format!(
                "font '{font_id}' reports units_per_em = {units_per_em}"
            )));
        }
        // `units_per_em` is a positive `i32` (guarded above); the OTF spec
        // range (16–16384) is exactly representable as `f32`.
        let scale = font_size / units_per_em as f32;

        // ── Derive line metrics ───────────────────────────────────────────────
        // `ascender` and `descender` are in font units; descender is negative.
        let ascent = f32::from(face.ascender()) * scale;
        let descent = -(f32::from(face.descender()) * scale); // store positive magnitude
        let line_gap = f32::from(face.line_gap()) * scale;
        let line_height = ascent + descent + line_gap;

        // ── Shape the text ────────────────────────────────────────────────────
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        // RTL sets the buffer direction so rustybuzz reorders glyphs to visual
        // order and applies RTL-correct joining (Arabic, Hebrew). The run's
        // advance + glyph pen positions stay left-to-right, so a word emitted
        // at its left x renders correctly; LTR is the default (unchanged).
        buffer.set_direction(match direction {
            TextDirection::Ltr => rustybuzz::Direction::LeftToRight,
            TextDirection::Rtl => rustybuzz::Direction::RightToLeft,
        });

        // Shape with no extra features; deterministic across machines.
        let glyph_buffer = rustybuzz::shape(face, &[], buffer);

        let infos = glyph_buffer.glyph_infos();
        let positions = glyph_buffer.glyph_positions();

        // ── Build glyph list ──────────────────────────────────────────────────
        let mut glyphs: Vec<PositionedGlyph> = Vec::with_capacity(infos.len());
        let mut pen_x: f32 = 0.0;
        let mut pen_y: f32 = 0.0;

        for (info, pos) in infos.iter().zip(positions.iter()) {
            // glyph_id is u32 in rustybuzz; OTF glyph IDs fit in u16 (max 65535).
            // A value above u16::MAX indicates a malformed font — map it to the
            // .notdef glyph (0) rather than silently truncating.
            let glyph_id = u16::try_from(info.glyph_id).unwrap_or(0);

            let x = pen_x + pos.x_offset as f32 * scale;
            // y_offset is in font units; positive = up in font coords → negative screen y.
            let y = pen_y - pos.y_offset as f32 * scale;

            glyphs.push(PositionedGlyph { glyph_id, x, y });

            pen_x += pos.x_advance as f32 * scale;
            pen_y += pos.y_advance as f32 * scale;
        }

        let advance_width = pen_x;

        Ok(ZenithGlyphRun {
            font_id,
            font_size,
            ascent,
            descent,
            line_height,
            advance_width,
            glyphs,
        })
    }
}

impl TextLayoutEngine for RustybuzzEngine {
    fn shape(
        &self,
        req: &ShapeRequest<'_>,
        provider: &dyn FontProvider,
    ) -> Result<ZenithGlyphRun, LayoutError> {
        // ── 1. Resolve font bytes ─────────────────────────────────────────────
        let font_data = provider
            .resolve(req.families, req.weight, req.style)
            .ok_or_else(|| {
                LayoutError::new(format!("no font resolved for families {:?}", req.families))
            })?;

        // ── 2. Parse the font face ────────────────────────────────────────────
        let face =
            rustybuzz::Face::from_slice(&font_data.bytes, font_data.index).ok_or_else(|| {
                LayoutError::new(format!(
                    "failed to parse font face for '{}' (index {})",
                    font_data.id, font_data.index
                ))
            })?;

        // ── 3. Shape via the shared single-face helper ────────────────────────
        Self::shape_run_with_face(&face, req.text, font_data.id, req.font_size, req.direction)
    }

    fn shape_with_fallback(
        &self,
        req: &ShapeRequest<'_>,
        provider: &dyn FontProvider,
    ) -> Result<FallbackResult, LayoutError> {
        // ── 1. Resolve + parse the PRIMARY face ───────────────────────────────
        let primary_data = provider
            .resolve(req.families, req.weight, req.style)
            .ok_or_else(|| {
                LayoutError::new(format!("no font resolved for families {:?}", req.families))
            })?;
        let primary_face = rustybuzz::Face::from_slice(&primary_data.bytes, primary_data.index)
            .ok_or_else(|| {
                LayoutError::new(format!(
                    "failed to parse font face for '{}' (index {})",
                    primary_data.id, primary_data.index
                ))
            })?;

        // ── 2. Build an ordered, deduplicated face cache for coverage probing ─
        // The primary occupies index 0; remaining faces follow in the
        // deterministic order `provider.all_faces()` returns them, skipping any
        // face that shares the primary's id (so the primary is never duplicated).
        // Each entry parses once and is reused for every coverage check + shape.
        // Bind the owned face data for the whole function so the parsed
        // `Face`s below can borrow from it (they outlive a per-iteration temp).
        let all_faces_data = provider.all_faces();
        let mut faces: Vec<(String, rustybuzz::Face<'_>)> =
            vec![(primary_data.id.clone(), primary_face)];
        for fd in &all_faces_data {
            if fd.id == primary_data.id {
                continue;
            }
            // A face whose bytes fail to parse simply cannot cover any glyph;
            // skip it rather than failing the whole shape.
            if let Some(f) = rustybuzz::Face::from_slice(&fd.bytes, fd.index) {
                faces.push((fd.id.clone(), f));
            }
        }

        // Coverage test: does the face at `faces[idx]` have a glyph for `ch`?
        // Uses ttf-parser's `Face::glyph_index`, exposed on `rustybuzz::Face`
        // via Deref.
        let covers = |idx: usize, ch: char| -> bool {
            faces
                .get(idx)
                .is_some_and(|(_, f)| f.glyph_index(ch).is_some())
        };

        // ── 3. Itemize text into contiguous sub-runs by chosen face index ─────
        // For each char: prefer the primary (index 0) when it covers the char;
        // otherwise the FIRST non-primary face (lowest index ≥ 1) that covers
        // it; otherwise the primary (so it shapes as .notdef / tofu, matching
        // current behavior). Consecutive chars with the same chosen face merge.
        // Sub-run boundaries are recorded as byte ranges into `req.text` so the
        // exact substring is shaped (and reported as the run's source text).
        // Chars that fall back to the primary (index 0) because NO face covers
        // them are recorded in `missing` (unless ignorable).
        let mut missing: BTreeSet<char> = BTreeSet::new();

        // (face_idx, byte_start, byte_end) per sub-run, in text order.
        let mut segments: Vec<(usize, usize, usize)> = Vec::new();
        for (byte_off, ch) in req.text.char_indices() {
            let idx = if covers(0, ch) {
                0
            } else {
                let mut chosen = 0_usize;
                for idx in 1..faces.len() {
                    if covers(idx, ch) {
                        chosen = idx;
                        break;
                    }
                }
                // chosen == 0 means no face covered it; record as missing.
                if chosen == 0 && !is_ignorable_for_coverage(ch) {
                    missing.insert(ch);
                }
                chosen
            };
            let ch_end = byte_off + ch.len_utf8();
            match segments.last_mut() {
                Some((last_idx, _, last_end)) if *last_idx == idx => {
                    *last_end = ch_end;
                }
                _ => segments.push((idx, byte_off, ch_end)),
            }
        }

        // Empty text → no segments; shape the empty string with the primary so
        // a (degenerate but valid) run with primary metrics is still returned,
        // matching `shape("")`.
        if segments.is_empty() {
            let (font_id, face) = faces.first().ok_or_else(|| {
                LayoutError::new("internal: primary face missing from cache".to_owned())
            })?;
            return Ok(FallbackResult {
                runs: vec![Self::shape_run_with_face(
                    face,
                    req.text,
                    font_id.clone(),
                    req.font_size,
                    req.direction,
                )?],
                missing_chars: missing.into_iter().collect(),
            });
        }

        // ── 4. Shape each sub-run with its chosen face ────────────────────────
        // The all-primary case is exactly one segment at index 0 spanning the
        // whole text → a single run byte-identical to `shape`, because both
        // call `shape_run_with_face` with the same face, text, id, and size.
        //
        // Segments are itemized in LOGICAL (text) order. The returned runs are
        // concatenated left-to-right by the caller, so for RTL the FIRST logical
        // segment must sit rightmost: reverse the emission order. A single
        // segment (the common all-primary case) is unaffected, and LTR keeps
        // logical order — so both the LTR path and a single-run RTL word stay
        // byte-identical.
        if req.direction == TextDirection::Rtl {
            segments.reverse();
        }
        let mut runs: Vec<ZenithGlyphRun> = Vec::with_capacity(segments.len());
        for (idx, start, end) in segments {
            let (font_id, face) = faces.get(idx).ok_or_else(|| {
                LayoutError::new("internal: chosen face index out of range".to_owned())
            })?;
            let sub_text = req.text.get(start..end).ok_or_else(|| {
                LayoutError::new("internal: sub-run byte range out of bounds".to_owned())
            })?;
            runs.push(Self::shape_run_with_face(
                face,
                sub_text,
                font_id.clone(),
                req.font_size,
                req.direction,
            )?);
        }

        Ok(FallbackResult {
            runs,
            missing_chars: missing.into_iter().collect(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use zenith_core::{FontStyle, default_provider};

    use super::*;

    fn shape_at(font_size: f32) -> Result<ZenithGlyphRun, LayoutError> {
        let families = vec!["Noto Sans".to_string()];
        let req = ShapeRequest {
            text: "Hello Zenith",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        RustybuzzEngine::new().shape(&req, &provider)
    }

    #[test]
    fn shape_hello_zenith_at_24px() {
        let run = shape_at(24.0).expect("shaping should succeed");

        // font_id matches the registered stable id.
        assert_eq!(run.font_id, "noto-sans-400-normal");

        // Glyph count: "Hello Zenith" = 12 characters including the space.
        assert!(
            run.glyphs.len() >= 10,
            "expected >= 10 glyphs, got {}",
            run.glyphs.len()
        );

        // Metrics sanity.
        assert!(
            run.ascent > 0.0,
            "ascent must be positive, got {}",
            run.ascent
        );
        assert!(
            run.advance_width > 0.0,
            "advance_width must be positive, got {}",
            run.advance_width
        );

        // Glyph x positions must be non-decreasing (monotonic pen advance).
        let mut prev_x = f32::NEG_INFINITY;
        for g in &run.glyphs {
            assert!(
                g.x >= prev_x - 1e-4,
                "x positions must be non-decreasing: {} < {}",
                g.x,
                prev_x
            );
            prev_x = g.x;
        }
    }

    #[test]
    fn shaping_is_deterministic() {
        let run1 = shape_at(24.0).expect("first shape");
        let run2 = shape_at(24.0).expect("second shape");
        assert_eq!(run1, run2, "shaping must be deterministic");
    }

    #[test]
    fn unknown_family_returns_error() {
        let families = vec!["Nonexistent".to_string()];
        let req = ShapeRequest {
            text: "test",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 16.0,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        let result = RustybuzzEngine::new().shape(&req, &provider);
        assert!(result.is_err(), "unknown family must return Err");
        let msg = result.unwrap_err().message;
        assert!(
            msg.contains("no font resolved"),
            "error message should mention unresolved font, got: {msg}"
        );
    }

    #[test]
    fn fallback_all_primary_matches_single_shape() {
        // CRITICAL byte-identity guarantee: text fully covered by the primary
        // face must yield exactly ONE run identical to `shape()`.
        let families = vec!["Noto Sans".to_string()];
        let req = ShapeRequest {
            text: "Hello Zenith 123!",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 24.0,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        let engine = RustybuzzEngine::new();

        let single = engine.shape(&req, &provider).expect("single-run shape");
        let result = engine
            .shape_with_fallback(&req, &provider)
            .expect("fallback shape");

        assert_eq!(
            result.runs.len(),
            1,
            "all-primary text must produce exactly one run"
        );
        assert_eq!(
            result.runs.first().expect("one run"),
            &single,
            "all-primary fallback run must be byte-identical to shape()"
        );
        assert!(
            result.missing_chars.is_empty(),
            "fully-covered ASCII must have no missing chars"
        );
    }

    #[test]
    fn fallback_empty_text_matches_single_shape() {
        // Degenerate empty input must still match `shape("")` (one run).
        let families = vec!["Noto Sans".to_string()];
        let req = ShapeRequest {
            text: "",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 16.0,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        let engine = RustybuzzEngine::new();

        let single = engine.shape(&req, &provider).expect("single empty shape");
        let result = engine
            .shape_with_fallback(&req, &provider)
            .expect("fallback empty shape");
        assert_eq!(
            result.runs.len(),
            1,
            "empty text still yields one (degenerate) run"
        );
        assert_eq!(result.runs.first().expect("one run"), &single);
    }

    #[test]
    fn fallback_unknown_primary_returns_error() {
        // No resolvable primary → Err, exactly like `shape`.
        let families = vec!["Nonexistent".to_string()];
        let req = ShapeRequest {
            text: "test",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 16.0,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        let result = RustybuzzEngine::new().shape_with_fallback(&req, &provider);
        assert!(result.is_err(), "unknown primary family must return Err");
    }

    #[test]
    fn fallback_is_deterministic() {
        let families = vec!["Noto Sans".to_string()];
        let req = ShapeRequest {
            text: "Hi there",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 18.0,
            direction: TextDirection::Ltr,
        };
        let provider = default_provider();
        let engine = RustybuzzEngine::new();
        let a = engine.shape_with_fallback(&req, &provider).expect("a");
        let b = engine.shape_with_fallback(&req, &provider).expect("b");
        assert_eq!(a.runs, b.runs, "fallback shaping must be deterministic");
        assert_eq!(
            a.missing_chars, b.missing_chars,
            "missing_chars must be deterministic"
        );
    }

    #[test]
    fn rtl_reverses_visual_glyph_order() {
        // For a non-joining script (Latin), RTL shaping reorders glyphs to
        // visual (right-to-left) order: the RTL glyph_id sequence is the reverse
        // of the LTR one, while the total advance stays positive and equal.
        let families = vec!["Noto Sans".to_string()];
        let provider = default_provider();
        let engine = RustybuzzEngine::new();

        let ltr = engine
            .shape(
                &ShapeRequest {
                    text: "ABC",
                    families: &families,
                    weight: 400,
                    style: FontStyle::Normal,
                    font_size: 24.0,
                    direction: TextDirection::Ltr,
                },
                &provider,
            )
            .expect("ltr shape");
        let rtl = engine
            .shape(
                &ShapeRequest {
                    text: "ABC",
                    families: &families,
                    weight: 400,
                    style: FontStyle::Normal,
                    font_size: 24.0,
                    direction: TextDirection::Rtl,
                },
                &provider,
            )
            .expect("rtl shape");

        let ltr_ids: Vec<u16> = ltr.glyphs.iter().map(|g| g.glyph_id).collect();
        let mut rtl_ids: Vec<u16> = rtl.glyphs.iter().map(|g| g.glyph_id).collect();
        rtl_ids.reverse();
        assert_eq!(
            ltr_ids, rtl_ids,
            "RTL glyph order must be the visual reverse of LTR"
        );
        assert!(rtl.advance_width > 0.0, "RTL advance must be positive");
        assert!(
            (rtl.advance_width - ltr.advance_width).abs() < 1e-3,
            "RTL and LTR total advance must match"
        );
    }

    #[test]
    fn rtl_shaping_is_deterministic() {
        let families = vec!["Noto Sans".to_string()];
        let provider = default_provider();
        let engine = RustybuzzEngine::new();
        let req = ShapeRequest {
            text: "Shalom",
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 20.0,
            direction: TextDirection::Rtl,
        };
        let a = engine.shape(&req, &provider).expect("a");
        let b = engine.shape(&req, &provider).expect("b");
        assert_eq!(a, b, "RTL shaping must be deterministic");
    }

    #[test]
    fn font_size_scaling_proportional() {
        let run24 = shape_at(24.0).expect("24px");
        let run48 = shape_at(48.0).expect("48px");

        // Ascent should be ~2× when font_size doubles.
        let ratio_ascent = run48.ascent / run24.ascent;
        assert!(
            (ratio_ascent - 2.0).abs() < 0.01,
            "ascent ratio should be ~2.0, got {ratio_ascent}"
        );

        // advance_width should also be ~2×.
        let ratio_adv = run48.advance_width / run24.advance_width;
        assert!(
            (ratio_adv - 2.0).abs() < 0.01,
            "advance_width ratio should be ~2.0, got {ratio_adv}"
        );
    }
}
