//! Legend strip rendering for chart nodes.
//!
//! `measure_legend_width` shapes each label at 10 px to compute the strip
//! width; `emit_legend` pushes colored swatches and label glyph runs into the
//! command buffer. Both functions are no-ops when no entries are supplied.

use zenith_core::{Diagnostic, FontStyle};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine};

use crate::ir::{Color, Paint, SceneCommand};

use super::super::NodeCtx;
use super::super::text::run_to_scene_glyphs;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Left padding inside the legend strip, before the swatch.
const PAD_L: f64 = 10.0;
/// Right padding inside the legend strip, after the label.
const PAD_R: f64 = 10.0;
/// Swatch square edge length (px).
const SWATCH: f64 = 11.0;
/// Gap between the right edge of the swatch and the start of the label.
const GAP: f64 = 6.0;
/// Vertical slot height per legend entry.
const LINE_H: f64 = 18.0;
/// Font size for legend labels (px).
const FONT: f32 = 10.0;
/// Default legend label text color (dark gray).
const LEGEND_TEXT_COLOR: Color = Color::srgb(60, 60, 60, 255);

// ── LegendArea ────────────────────────────────────────────────────────────────

/// The right-side strip reserved for the legend.
#[derive(Clone, Copy)]
pub(super) struct LegendArea {
    /// Left edge of the legend strip in device-space pixels.
    pub(super) x: f64,
    /// Top edge of the legend strip in device-space pixels.
    pub(super) y: f64,
    /// Width of the legend strip in device-space pixels.
    pub(super) w: f64,
    /// Height of the legend strip in device-space pixels.
    pub(super) h: f64,
}

// ── Pure width arithmetic ─────────────────────────────────────────────────────

/// Compute the total legend strip width from the widest label advance.
///
/// `width = PAD_L + SWATCH + GAP + max_advance + PAD_R`
///
/// This pure helper is separated from the shaping loop so it can be unit-tested
/// without constructing a `NodeCtx`.
pub(super) fn legend_width_from_advance(max_advance: f64) -> f64 {
    PAD_L + SWATCH + GAP + max_advance + PAD_R
}

// ── measure_legend_width ──────────────────────────────────────────────────────

/// Measure the pixel width needed to render a legend strip for `entries`.
///
/// Returns `0.0` when `entries` is empty. For each entry the label is shaped at
/// [`FONT`] px (weight 400, normal style, LTR) to obtain its advance width; the
/// maximum across all entries determines the strip width via
/// [`legend_width_from_advance`]. On a shaping error the entry's advance is
/// treated as `0.0` (the entry is still drawn, using a zero-advance fallback).
pub(super) fn measure_legend_width(entries: &[(String, Color)], cx: NodeCtx<'_>) -> f64 {
    if entries.is_empty() {
        return 0.0;
    }

    let families = [String::from("Noto Sans")];
    let mut max_advance: f64 = 0.0;

    for (label, _) in entries {
        let req = ShapeRequest {
            text: label,
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: FONT,
            direction: TextDirection::Ltr,
        };
        let advance: f64 = match cx.engine.shape_with_fallback(&req, cx.fonts) {
            Ok(result) => result.runs.iter().map(|r| r.advance_width as f64).sum(),
            Err(_) => 0.0,
        };
        if advance > max_advance {
            max_advance = advance;
        }
    }

    legend_width_from_advance(max_advance)
}

// ── emit_legend ───────────────────────────────────────────────────────────────

/// Emit swatches and labels for `entries` into `area`, vertically centered.
///
/// The entries are laid out top-to-bottom with `LINE_H` px per row, centered
/// within `area.h`. Entries whose top edge would exceed the bottom of the area
/// are silently skipped (overflow guard).
///
/// No-op when `area.w <= 0.0` or `entries` is empty.
pub(super) fn emit_legend(
    entries: &[(String, Color)],
    area: LegendArea,
    cx: NodeCtx<'_>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if area.w <= 0.0 || entries.is_empty() {
        return;
    }

    let n = entries.len() as f64;
    let total_h = n * LINE_H;
    let start_y = (area.y + (area.h - total_h) / 2.0).max(area.y);
    let area_bottom = area.y + area.h;

    let families = [String::from("Noto Sans")];

    for (i, (label, color)) in entries.iter().enumerate() {
        let line_top = start_y + i as f64 * LINE_H;

        // Overflow guard: stop when this entry starts below the area bottom.
        if line_top >= area_bottom {
            break;
        }

        // Colored swatch square.
        let swatch_x = area.x + PAD_L;
        let swatch_y = line_top + (LINE_H - SWATCH) / 2.0;
        commands.push(SceneCommand::FillRect {
            x: swatch_x,
            y: swatch_y,
            w: SWATCH,
            h: SWATCH,
            paint: Paint::solid(*color),
        });

        // Label glyph run.
        let req = ShapeRequest {
            text: label,
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: FONT,
            direction: TextDirection::Ltr,
        };

        match cx.engine.shape_with_fallback(&req, cx.fonts) {
            Err(e) => {
                diagnostics.push(Diagnostic::advisory(
                    "scene.text_unshaped",
                    format!(
                        "chart legend label '{}' could not be shaped: {}",
                        label, e.message
                    ),
                    None,
                    None,
                ));
            }
            Ok(result) => {
                let ascent: f64 = result.runs.first().map(|r| r.ascent as f64).unwrap_or(8.0);
                let baseline_y = line_top + LINE_H / 2.0 + ascent * 0.35;
                let mut text_x = area.x + PAD_L + SWATCH + GAP;

                for run in result.runs {
                    let advance = run.advance_width as f64;
                    let glyphs = run_to_scene_glyphs(&run);
                    commands.push(SceneCommand::DrawGlyphRun {
                        x: text_x,
                        y: baseline_y,
                        font_id: run.font_id.clone(),
                        font_size: run.font_size,
                        color: LEGEND_TEXT_COLOR,
                        stroke_color: None,
                        stroke_width: None,
                        glyphs,
                    });
                    text_x += advance;
                }
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── legend_width_from_advance ─────────────────────────────────────────────

    #[test]
    fn width_from_advance_zero() {
        // Zero advance → PAD_L + SWATCH + GAP + 0 + PAD_R
        let expected = PAD_L + SWATCH + GAP + PAD_R;
        let got = legend_width_from_advance(0.0);
        assert!(
            (got - expected).abs() < 1e-9,
            "expected {expected}, got {got}"
        );
    }

    #[test]
    fn width_from_advance_nonzero() {
        let advance = 42.5;
        let expected = PAD_L + SWATCH + GAP + advance + PAD_R;
        let got = legend_width_from_advance(advance);
        assert!(
            (got - expected).abs() < 1e-9,
            "expected {expected}, got {got}"
        );
    }

    // ── measure_legend_width (engine-free cases) ──────────────────────────────

    // Engine-dependent shaping tests are omitted: building a NodeCtx requires
    // a RustybuzzEngine and FontProvider which are not available in a unit-test
    // context. The pure arithmetic is exercised above; integration/conformance
    // tests cover the full shaping path.

    // ── emit_legend no-op guard ───────────────────────────────────────────────
    // The engine-free no-op cases (area.w<=0 / empty entries) are
    // structural guarantees enforced by the early-return branches and do not
    // require a command buffer test here (no engine needed).
}
