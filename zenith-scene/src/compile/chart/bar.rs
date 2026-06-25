//! Bar chart geometry and emission for `kind="bar"`.
//!
//! `bar_rects` is a pure geometry function (no engine, no I/O) that computes
//! pixel rectangles for grouped or stacked bars. `emit_bars` resolves series
//! colors and pushes `FillRect` commands and value labels. `emit_category_labels`
//! pushes X-axis category labels below the plot area.

use zenith_core::{ChartNode, Diagnostic, FontStyle};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine};

use crate::ir::{Color, Paint, SceneCommand};

use super::super::NodeCtx;
use super::super::text::run_to_scene_glyphs;
use super::axis::format_tick_label;
use super::frame::PlotArea;
use super::palette::series_color;
use super::scale::LinearScale;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Fraction of a category slot that is padding (split equally on each side).
const CATEGORY_PAD_FRAC: f64 = 0.20;
/// Gap between adjacent bars within a grouped category, as a fraction of
/// `bar_w` (applied once between each pair).
const BAR_GAP_FRAC: f64 = 0.15;

/// Color for value labels drawn above (or inside) each bar.
const VALUE_LABEL_COLOR: Color = Color::srgb(60, 60, 60, 255);

// ── BarMode ───────────────────────────────────────────────────────────────────

/// Layout mode for a bar chart.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum BarMode {
    /// Series bars are placed side-by-side within each category slot.
    Grouped,
    /// Series bars are stacked vertically within each category slot.
    Stacked,
}

impl BarMode {
    /// Derive the bar mode from the optional `bar_mode` string property.
    ///
    /// `Some("stacked")` → `Stacked`; everything else (including `None`,
    /// `Some("grouped")`, or any unrecognised string) → `Grouped`.
    pub(super) fn from_opt(s: Option<&str>) -> BarMode {
        match s {
            Some("stacked") => BarMode::Stacked,
            _ => BarMode::Grouped,
        }
    }
}

// ── BarRect ───────────────────────────────────────────────────────────────────

/// Pixel rectangle for a single bar.
///
/// A `w == 0.0` or `h < 0.5` sentinel means "nothing to draw here" — the
/// emitter skips these.
#[derive(Clone, Copy)]
pub(super) struct BarRect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
}

// ── bar_rects ─────────────────────────────────────────────────────────────────

/// Compute pixel rectangles for every bar.
///
/// Returns `rects[series_idx][category_idx]`. The outer `Vec` has one entry
/// per series; the inner `Vec` has one entry per category (padded to
/// `n_categories` with zero-size sentinels when a series has fewer values).
///
/// Returns an empty `Vec` when `n_categories == 0` (no data) or when the
/// plot area is degenerate (`w <= 0`).
///
/// This function is PURE — no engine, no I/O, no side effects beyond the
/// returned `Vec`.
pub(super) fn bar_rects(
    plot: &PlotArea,
    y_scale: &LinearScale,
    series_values: &[&[f64]],
    mode: BarMode,
) -> Vec<Vec<BarRect>> {
    let n_categories = series_values.iter().map(|s| s.len()).max().unwrap_or(0);
    if n_categories == 0 || plot.w <= 0.0 {
        return Vec::new();
    }

    let n_series = series_values.len();
    if n_series == 0 {
        return Vec::new();
    }

    // Snap the value baseline to a whole device pixel. Bar segment edges are
    // rounded to integers (below) so abutting stacked segments share an exact
    // pixel boundary — without this, two fills meeting at a fractional y leave a
    // 1px anti-aliased seam (background hairline) between them.
    let baseline_px = y_scale.map(0.0).round();
    let slot_w = plot.w / n_categories as f64;
    let usable_w = slot_w * (1.0 - CATEGORY_PAD_FRAC);
    let left_pad = (slot_w - usable_w) / 2.0;

    match mode {
        BarMode::Grouped => {
            // bar_w accounts for gaps between bars within a slot.
            // Formula: n_series * bar_w + (n_series - 1) * bar_w * BAR_GAP_FRAC = usable_w
            //   => bar_w * (n_series + (n_series-1)*BAR_GAP_FRAC) = usable_w
            //   => bar_w * (n_series * (1+BAR_GAP_FRAC) - BAR_GAP_FRAC) = usable_w
            let bar_w = usable_w / (n_series as f64 * (1.0 + BAR_GAP_FRAC) - BAR_GAP_FRAC).max(1.0);

            if bar_w <= 0.0 {
                return Vec::new();
            }

            let step = bar_w * (1.0 + BAR_GAP_FRAC);

            series_values
                .iter()
                .enumerate()
                .map(|(s, sv)| {
                    (0..n_categories)
                        .map(|c| match sv.get(c) {
                            None => BarRect {
                                x: 0.0,
                                y: 0.0,
                                w: 0.0,
                                h: 0.0,
                            },
                            Some(&value) => {
                                let bar_x = plot.x + c as f64 * slot_w + left_pad + s as f64 * step;
                                let top = y_scale.map(value).round();
                                let h = (baseline_px - top).abs();
                                let y = top.min(baseline_px);
                                BarRect {
                                    x: bar_x,
                                    y,
                                    w: bar_w,
                                    h,
                                }
                            }
                        })
                        .collect()
                })
                .collect()
        }

        BarMode::Stacked => {
            let bar_w = usable_w;
            // Per-category running cumulative (one accumulator per category).
            let mut cumulative = vec![0.0f64; n_categories];

            series_values
                .iter()
                .map(|sv| {
                    (0..n_categories)
                        .map(|c| match sv.get(c) {
                            None => BarRect {
                                x: 0.0,
                                y: 0.0,
                                w: 0.0,
                                h: 0.0,
                            },
                            Some(&value) => {
                                let bar_x = plot.x + c as f64 * slot_w + left_pad;
                                let lower = cumulative.get(c).copied().unwrap_or(0.0);
                                let upper = lower + value;
                                if let Some(slot) = cumulative.get_mut(c) {
                                    *slot = upper;
                                }
                                let top = y_scale.map(upper).round();
                                let bottom = y_scale.map(lower).round();
                                let y = top.min(bottom);
                                let h = (bottom - top).abs();
                                BarRect {
                                    x: bar_x,
                                    y,
                                    w: bar_w,
                                    h,
                                }
                            }
                        })
                        .collect()
                })
                .collect()
        }
    }
}

/// Largest per-category cumulative total across all series — the height a
/// stacked column reaches. Used to size the value axis so stacked bars fit
/// inside the plot area (a grouped chart sizes to the max single value instead).
///
/// Returns `0.0` when there are no series/values. Negative values are summed
/// too, matching the stacking model.
pub(super) fn stacked_max(chart: &ChartNode) -> f64 {
    let n_categories = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0);
    let mut max = 0.0_f64;
    for c in 0..n_categories {
        let sum: f64 = chart.series.iter().filter_map(|s| s.values.get(c)).sum();
        if sum > max {
            max = sum;
        }
    }
    max
}

// ── emit_bars ─────────────────────────────────────────────────────────────────

/// Resolve series colors and emit `FillRect` commands for every bar, followed
/// by a value label floated just above (or inside) each bar.
///
/// Bars with `w <= 0` or `h < 0.5` are skipped. An empty series list or an
/// empty series produces no commands.
pub(super) fn emit_bars(
    chart: &ChartNode,
    plot: &PlotArea,
    y_scale: &LinearScale,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let series_values: Vec<&[f64]> = chart.series.iter().map(|s| s.values.as_slice()).collect();
    let mode = BarMode::from_opt(chart.bar_mode.as_deref());
    let rects = bar_rects(plot, y_scale, &series_values, mode);

    if rects.is_empty() {
        return;
    }

    // Hoisted outside the per-series/per-bar loops: one allocation for all
    // value-label shape requests in this chart.
    let value_label_families = [String::from("Noto Sans")];

    for s in 0..chart.series.len() {
        // Resolve color: explicit series color → palette fallback.
        // Shared resolver lives in palette::series_color so all chart kinds
        // (bar, line, area, sparkline) produce identical color semantics.
        let color = match chart.series.get(s) {
            Some(series) => series_color(series, s, cx.resolved, diagnostics, &chart.id),
            None => continue,
        };

        let paint = Paint::solid(color);

        if let Some(series_rects) = rects.get(s) {
            for (c, rect) in series_rects.iter().enumerate() {
                if rect.w <= 0.0 || rect.h < 0.5 {
                    continue;
                }

                commands.push(SceneCommand::FillRect {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: rect.h,
                    paint: paint.clone(),
                });

                // Value label: the raw data value for this bar.
                let value = match chart.series.get(s).and_then(|sr| sr.values.get(c)) {
                    Some(v) => *v,
                    None => continue,
                };

                emit_value_label(
                    value,
                    *rect,
                    LabelCtx {
                        plot,
                        families: &value_label_families,
                        chart_id: &chart.id,
                    },
                    cx,
                    commands,
                    diagnostics,
                );
            }
        }
    }
}

// ── emit_value_label ──────────────────────────────────────────────────────────

/// Per-chart context shared by every bar's value label — bundled so the
/// font-family allocation is hoisted once and the emitter's argument list stays
/// within bounds.
#[derive(Clone, Copy)]
struct LabelCtx<'a> {
    plot: &'a PlotArea,
    families: &'a [String],
    chart_id: &'a str,
}

/// Shape and emit a numeric value label above (or inside) a bar.
///
/// The label is centered horizontally over the bar. Normally placed 3 px above
/// the bar top; if that would clip above the plot area the label is drawn 12 px
/// below the bar top (inside the bar).
///
/// `lc` bundles the per-chart context shared across every bar (plot rect, the
/// hoisted font-family list, and the chart id) so the allocation stays out of
/// the per-bar loop and the argument list stays within bounds.
fn emit_value_label(
    value: f64,
    rect: BarRect,
    lc: LabelCtx,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let plot = lc.plot;
    let label = format_tick_label(value);
    let req = ShapeRequest {
        text: &label,
        families: lc.families,
        weight: 400,
        style: FontStyle::Normal,
        font_size: 9.0,
        direction: TextDirection::Ltr,
    };

    match cx.engine.shape_with_fallback(&req, cx.fonts) {
        Err(e) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.text_unshaped",
                format!(
                    "chart '{}' bar value label '{}' could not be shaped: {}",
                    lc.chart_id, label, e.message
                ),
                None,
                Some(lc.chart_id.to_owned()),
            ));
        }
        Ok(result) => {
            let total_advance: f64 = result.runs.iter().map(|r| r.advance_width as f64).sum();
            let ascent: f64 = result.runs.first().map(|r| r.ascent as f64).unwrap_or(7.0);

            // Prefer label above bar; fall back to inside if it clips the plot top.
            let baseline_y = if rect.y - 3.0 - ascent >= plot.y {
                rect.y - 3.0
            } else {
                rect.y + 12.0
            };

            // Centered horizontally over the bar.
            let mut label_x = rect.x + rect.w / 2.0 - total_advance / 2.0;

            for run in result.runs {
                let advance = run.advance_width as f64;
                let glyphs = run_to_scene_glyphs(&run);
                commands.push(SceneCommand::DrawGlyphRun {
                    x: label_x,
                    y: baseline_y,
                    font_id: run.font_id.clone(),
                    font_size: run.font_size,
                    color: VALUE_LABEL_COLOR,
                    stroke_color: None,
                    stroke_width: None,
                    glyphs,
                });
                label_x += advance;
            }
        }
    }
}

// ── emit_category_labels ──────────────────────────────────────────────────────

/// Emit X-axis category labels centered under each category slot.
///
/// When `categories` is shorter than `n_categories`, the remaining slots are
/// labelled by 1-based index (`"1"`, `"2"`, …). Empty label strings are
/// skipped.
pub(super) fn emit_category_labels(
    categories: &[String],
    n_categories: usize,
    plot: &PlotArea,
    label_color: Color,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if n_categories == 0 || plot.w <= 0.0 {
        return;
    }

    let slot_w = plot.w / n_categories as f64;
    let baseline_y = plot.y + plot.h + 14.0;
    let families = [String::from("Noto Sans")];

    for c in 0..n_categories {
        let label: String = categories
            .get(c)
            .cloned()
            .unwrap_or_else(|| (c + 1).to_string());

        if label.is_empty() {
            continue;
        }

        let req = ShapeRequest {
            text: &label,
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 9.0,
            direction: TextDirection::Ltr,
        };

        match cx.engine.shape_with_fallback(&req, cx.fonts) {
            Err(e) => {
                diagnostics.push(Diagnostic::advisory(
                    "scene.text_unshaped",
                    format!(
                        "chart category label '{}' could not be shaped: {}",
                        label, e.message
                    ),
                    None,
                    None,
                ));
            }
            Ok(result) => {
                let total_advance: f64 = result.runs.iter().map(|r| r.advance_width as f64).sum();
                let center_x = plot.x + c as f64 * slot_w + slot_w / 2.0;
                let mut label_x = center_x - total_advance / 2.0;

                for run in result.runs {
                    let advance = run.advance_width as f64;
                    let glyphs = run_to_scene_glyphs(&run);
                    commands.push(SceneCommand::DrawGlyphRun {
                        x: label_x,
                        y: baseline_y,
                        font_id: run.font_id.clone(),
                        font_size: run.font_size,
                        color: label_color,
                        stroke_color: None,
                        stroke_width: None,
                        glyphs,
                    });
                    label_x += advance;
                }
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a simple PlotArea for testing.
    fn test_plot() -> PlotArea {
        PlotArea {
            x: 44.0,
            y: 10.0,
            w: 300.0,
            h: 200.0,
        }
    }

    /// Build a y_scale mapping data [0,100] onto pixels [210, 10] (inverted).
    fn test_scale() -> LinearScale {
        LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 210.0, // plot.y + plot.h = 10+200
            pixel_max: 10.0,  // plot.y
        }
    }

    #[test]
    fn bar_mode_from_opt_stacked() {
        assert_eq!(BarMode::from_opt(Some("stacked")), BarMode::Stacked);
    }

    #[test]
    fn bar_mode_from_opt_grouped_variants() {
        assert_eq!(BarMode::from_opt(None), BarMode::Grouped);
        assert_eq!(BarMode::from_opt(Some("grouped")), BarMode::Grouped);
        assert_eq!(BarMode::from_opt(Some("x")), BarMode::Grouped);
    }

    #[test]
    fn bar_rects_empty_series_returns_empty() {
        let plot = test_plot();
        let scale = test_scale();
        let result = bar_rects(&plot, &scale, &[], BarMode::Grouped);
        assert!(result.is_empty());
    }

    #[test]
    fn bar_rects_zero_categories_returns_empty() {
        let plot = test_plot();
        let scale = test_scale();
        // Series with no values => n_categories == 0
        let empty: &[f64] = &[];
        let result = bar_rects(&plot, &scale, &[empty], BarMode::Grouped);
        assert!(result.is_empty());
    }

    #[test]
    fn bar_rects_single_series_grouped_geometry() {
        let plot = test_plot();
        let scale = test_scale();
        let values: &[f64] = &[25.0, 50.0, 75.0];
        let rects = bar_rects(&plot, &scale, &[values], BarMode::Grouped);

        assert_eq!(rects.len(), 1, "one series");
        assert_eq!(rects[0].len(), 3, "three categories");

        let eps = 0.5;

        for r in &rects[0] {
            // All bars within the horizontal extent of the plot.
            assert!(r.x >= plot.x - eps, "bar left of plot.x");
            assert!(
                r.x + r.w <= plot.x + plot.w + eps,
                "bar right of plot right"
            );
            // Bar bottom is at the baseline (pixel for value 0).
            let baseline = scale.map(0.0);
            assert!(
                (r.y + r.h - baseline).abs() < eps,
                "bar bottom not at baseline"
            );
        }

        // A larger data value → taller bar → smaller y (higher up on screen).
        let r0 = rects[0][0]; // value 25
        let r1 = rects[0][1]; // value 50
        let r2 = rects[0][2]; // value 75
        assert!(r0.h < r1.h, "25 bar shorter than 50 bar");
        assert!(r1.h < r2.h, "50 bar shorter than 75 bar");
        assert!(
            r2.y < r0.y,
            "75 bar top is higher (smaller y) than 25 bar top"
        );
    }

    #[test]
    fn bar_rects_grouped_two_series_no_overlap() {
        let plot = test_plot();
        let scale = test_scale();
        let s0: &[f64] = &[30.0, 60.0, 90.0];
        let s1: &[f64] = &[10.0, 20.0, 30.0];
        let rects = bar_rects(&plot, &scale, &[s0, s1], BarMode::Grouped);

        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].len(), 3);
        assert_eq!(rects[1].len(), 3);

        for (c, (r0, r1)) in rects[0].iter().zip(rects[1].iter()).enumerate() {
            // Series 0 is to the left of series 1 within each slot.
            assert!(
                r0.x < r1.x,
                "series 0 bar not left of series 1 bar at category {}",
                c
            );
            // No horizontal overlap: s0 right edge <= s1 left edge.
            assert!(
                r0.x + r0.w <= r1.x + 0.5,
                "bars overlap at category {}: s0 right={} s1 left={}",
                c,
                r0.x + r0.w,
                r1.x
            );
        }
    }

    #[test]
    fn bar_rects_stacked_same_x_stacked_heights() {
        let plot = test_plot();
        let scale = test_scale();
        let s0: &[f64] = &[20.0, 40.0];
        let s1: &[f64] = &[30.0, 10.0];
        let rects = bar_rects(&plot, &scale, &[s0, s1], BarMode::Stacked);

        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].len(), 2);
        assert_eq!(rects[1].len(), 2);

        let eps = 0.5;
        for c in 0..2 {
            let r0 = rects[0][c];
            let r1 = rects[1][c];

            // Same x and w (stacked, same slot).
            assert!(
                (r0.x - r1.x).abs() < eps,
                "stacked bars differ in x at cat {}",
                c
            );
            assert!(
                (r0.w - r1.w).abs() < eps,
                "stacked bars differ in w at cat {}",
                c
            );

            // Series 1 is stacked above series 0: its top pixel is smaller (higher).
            assert!(r1.y < r0.y, "series 1 not above series 0 at cat {}", c);

            // Combined pixel height equals height for the summed value.
            let combined_value = s0[c] + s1[c];
            let expected_h = (scale.map(0.0) - scale.map(combined_value)).abs();
            let actual_h = r0.h + r1.h;
            assert!(
                (actual_h - expected_h).abs() < eps,
                "stacked heights don't sum at cat {}: got {} expected {}",
                c,
                actual_h,
                expected_h
            );
        }
    }
}
