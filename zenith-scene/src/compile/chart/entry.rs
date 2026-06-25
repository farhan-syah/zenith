//! `compile_chart` entry point.
//!
//! Resolves geometry, computes the scale and plot area, and emits the axis
//! frame, series geometry, and labels for axis-bearing chart kinds (`bar`,
//! `line`, `area`). Sparklines render directly into their bbox with a small
//! inset and no axes. Pie and donut charts tessellate wedge polygons into the
//! bbox via `pie::emit_pie` (no axes, no Y scale).
//!
//! Returns `0.0`: charts are absolute-positioned and do not participate in
//! flow layout (same contract as `compile_pattern`).

use zenith_core::{ChartNode, Diagnostic, FontStyle, dim_to_px};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine};

use crate::ir::{Color, SceneCommand};

use super::super::NodeCtx;
use super::super::RenderCtx;
use super::super::paint::resolve_property_color;
use super::super::text::run_to_scene_glyphs;
use super::super::util::{missing_geometry_diag, resolve_anchored_axis, unsupported_unit_diag};
use super::axis::{AxisColors, emit_axis_lines, emit_gridlines_and_labels};
use super::bar::{BarMode, CatLabels, emit_bars, emit_category_labels, stacked_max};
use super::frame::{PlotArea, plot_area};
use super::legend::{LegendArea, emit_legend, measure_legend_width};
use super::line::{emit_area_fill, emit_line_series, line_points};
use super::palette::series_color;
use super::pie::{emit_pie, slice_color};
use super::scale::{LinearScale, data_range, nice_ticks};

// ── Default colors ─────────────────────────────────────────────────────────────

/// Default axis line color (medium gray).
const DEFAULT_AXIS_COLOR: Color = Color::srgb(120, 120, 120, 255);
/// Default gridline color (light gray).
const DEFAULT_GRID_COLOR: Color = Color::srgb(225, 225, 225, 255);
/// Default tick label color (dark gray).
const DEFAULT_LABEL_COLOR: Color = Color::srgb(90, 90, 90, 255);
/// Default title color (near-black).
pub(super) const DEFAULT_TITLE_COLOR: Color = Color::srgb(40, 40, 40, 255);

// ── compile_chart ─────────────────────────────────────────────────────────────

/// Compile a `chart` node.
///
/// Axis-bearing kinds (`bar`, `line`, `area`) emit:
/// - The Y axis and X axis lines (the frame).
/// - Horizontal gridlines at each Y tick.
/// - Numeric Y tick labels (shaped text, right-aligned).
/// - Series geometry (bars / line strokes / area fills).
/// - Category labels (X axis).
/// - The title (if present) above the plot area.
///
/// Sparklines render directly into their bbox (no axes, no labels, no title).
/// Pie and donut charts tessellate wedge polygons into the bbox with percentage
/// labels and an optional title — no axes, no Y scale. Any unknown kind string
/// emits nothing — see the gate comment for the reasoning.
///
/// Returns `0.0`: charts are absolute-positioned and do not participate in
/// flow layout.
pub(in crate::compile) fn compile_chart(
    chart: &ChartNode,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    // Exclude invisible charts.
    if chart.visible == Some(false) {
        return 0.0;
    }

    // Axis-bearing kinds proceed past this gate; non-renderable kinds exit
    // early. Sparkline is handled via its own branch below (after geometry
    // resolution) because it needs x/y/w/h but NOT the full axis machinery.
    match chart.kind.as_str() {
        "bar" | "line" | "area" | "sparkline" | "pie" | "donut" => {}
        // Forward-compat: unknown kind strings emit nothing. We enumerate the
        // known non-renderable kinds explicitly rather than using a wildcard so
        // that a newly added known kind causes a compile error at every match
        // site. (String matches don't trigger exhaustive warnings, but keeping
        // the set explicit makes the coverage intent clear.)
        _ => return 0.0,
    }

    // ── Geometry resolution ──────────────────────────────────────────────────
    // Mirrors compile_shape (leaf/shape.rs:110-162): require w+h, resolve x/y
    // with anchor fallback, apply ctx.dx/ctx.dy.
    let (Some(w_dim), Some(h_dim)) = (&chart.w, &chart.h) else {
        diagnostics.push(missing_geometry_diag("chart", &chart.id, chart.source_span));
        return 0.0;
    };
    let Some(w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "chart",
            &chart.id,
            "w",
            chart.source_span,
        ));
        return 0.0;
    };
    let Some(h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "chart",
            &chart.id,
            "h",
            chart.source_span,
        ));
        return 0.0;
    };

    let anchor_xy = cx.anchors.get(&chart.id).copied();

    let Some(x_raw) = resolve_anchored_axis(
        "chart",
        &chart.id,
        "x",
        chart.x.as_ref(),
        anchor_xy.map(|(ax, _)| ax),
        chart.source_span,
        diagnostics,
    ) else {
        return 0.0;
    };
    let Some(y_raw) = resolve_anchored_axis(
        "chart",
        &chart.id,
        "y",
        chart.y.as_ref(),
        anchor_xy.map(|(_, ay)| ay),
        chart.source_span,
        diagnostics,
    ) else {
        return 0.0;
    };

    let x = x_raw + ctx.dx;
    let y = y_raw + ctx.dy;

    // ── Sparkline early branch ───────────────────────────────────────────────
    // Sparklines render directly into their bbox (small inset, no axes, no
    // labels, no title). Handled here, after geometry resolution, so x/y/w/h
    // are available. Returns immediately after emitting.
    if chart.kind.as_str() == "sparkline" {
        return emit_sparkline(chart, (x, y, w, h), cx, commands, diagnostics);
    }

    // ── Pie / donut early branch ─────────────────────────────────────────────
    // Pie and donut charts need no axes, no plot area, and no Y scale. They
    // tessellate wedge polygons directly from the chart bbox. Handled here,
    // after geometry resolution, so x/y/w/h are available. Returns immediately.
    if matches!(chart.kind.as_str(), "pie" | "donut") {
        let is_donut = chart.kind.as_str() == "donut";
        let legend_on = chart.legend == Some(true);

        let legend_w = if legend_on {
            // One entry per category, colored by the slice palette.
            let n = chart.series.first().map(|s| s.values.len()).unwrap_or(0);
            let entries = pie_legend_entries(chart, n);
            measure_legend_width(&entries, cx).min(w * 0.4)
        } else {
            0.0
        };

        // Render the pie into the left portion of the bbox (reduced by legend strip).
        emit_pie(
            chart,
            (x, y, w - legend_w, h),
            is_donut,
            cx,
            commands,
            diagnostics,
        );

        // Legend strip to the right of the pie.
        if legend_on && legend_w > 0.0 {
            let n = chart.series.first().map(|s| s.values.len()).unwrap_or(0);
            let entries = pie_legend_entries(chart, n);
            emit_legend(
                &entries,
                LegendArea {
                    x: x + w - legend_w,
                    y,
                    w: legend_w,
                    h,
                },
                cx,
                commands,
                diagnostics,
            );
        }

        return 0.0;
    }

    // ── Axis style "hidden" ──────────────────────────────────────────────────
    // When axis_style="hidden" the caller explicitly opts out of axes.
    if chart.axis_style.as_deref() == Some("hidden") {
        return 0.0;
    }

    // ── Legend (axis-bearing kinds) ──────────────────────────────────────────
    // Build the series entries and measure the legend strip width before
    // computing the plot area so the plot width is already reduced by the time
    // `plot_area` is called. When `legend_on` is false `legend_w` is exactly
    // `0.0` and `series_entries` is empty — the existing render path is
    // byte-identical (same commands, same diagnostics).
    let legend_on = chart.legend == Some(true);
    let series_entries: Vec<(String, Color)> = if legend_on {
        chart
            .series
            .iter()
            .enumerate()
            .map(|(s, sr)| {
                let label = sr
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Series {}", s + 1));
                let color = series_color(sr, s, cx.resolved, diagnostics, &chart.id);
                (label, color)
            })
            .collect()
    } else {
        Vec::new()
    };
    let legend_w = if legend_on {
        measure_legend_width(&series_entries, cx).min(w * 0.4)
    } else {
        0.0
    };

    // ── Plot area ────────────────────────────────────────────────────────────
    let has_title = chart.title.is_some();
    let has_caption = chart.caption.is_some();
    let plot = plot_area(x, y, w - legend_w, h, has_title, has_caption);

    // ── Axis colors ──────────────────────────────────────────────────────────
    let axis_color = chart
        .stroke
        .as_ref()
        .and_then(|p| resolve_property_color(p, cx.resolved, diagnostics, &chart.id))
        .unwrap_or(DEFAULT_AXIS_COLOR);

    let colors = AxisColors {
        axis: axis_color,
        grid: DEFAULT_GRID_COLOR,
        label: DEFAULT_LABEL_COLOR,
    };

    // ── Y scale ──────────────────────────────────────────────────────────────
    // Build the scale even when there is no data so the empty frame is drawn.
    let (mut data_lo, mut data_hi) =
        data_range(&chart.series, chart.axis_min, chart.axis_max).unwrap_or((0.0, 1.0)); // fallback: (0,1) keeps the frame visible

    // Bar charts grow from a zero baseline, so the domain must include 0 — a
    // bar drawn from a non-zero floor misrepresents magnitude. Honor an explicit
    // axis_min if the author pinned one; line charts keep their auto-fit range.
    if chart.kind.as_str() == "bar" && chart.axis_min.is_none() {
        data_lo = data_lo.min(0.0);
    }

    // Stacked bars reach the per-category SUM, not the max single value, so the
    // value axis must be sized to the tallest column or the stack overflows the
    // plot. Honor an explicit axis_max if the author pinned one.
    if chart.kind.as_str() == "bar"
        && BarMode::from_opt(chart.bar_mode.as_deref()) == BarMode::Stacked
        && chart.axis_max.is_none()
    {
        data_hi = data_hi.max(stacked_max(chart));
    }

    // Inverted Y: data_min → pixel bottom, data_max → pixel top.
    let y_scale = LinearScale {
        data_min: data_lo,
        data_max: data_hi,
        pixel_min: plot.y + plot.h, // bottom of plot area
        pixel_max: plot.y,          // top of plot area
    };

    let y_ticks = nice_ticks(&y_scale, 5);

    // ── Emit chart content (kind-specific z-order) ───────────────────────────
    //
    // Bar: gridlines → bars → category labels → axis lines.
    // Line/area: gridlines → (area fills) → line strokes → axis lines → cat labels.
    match chart.kind.as_str() {
        "bar" => {
            let n_categories = chart
                .series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0);

            emit_gridlines_and_labels(
                &plot,
                &y_ticks,
                colors,
                &chart.id,
                cx,
                commands,
                diagnostics,
            );
            emit_bars(chart, &plot, &y_scale, cx, commands, diagnostics);
            emit_category_labels(
                &chart.categories,
                n_categories,
                CatLabels {
                    plot: &plot,
                    color: colors.label,
                    slot_center: true,
                },
                cx,
                commands,
                diagnostics,
            );
            emit_axis_lines(&plot, colors.axis, commands);
        }
        "line" | "area" => {
            let is_area = chart.kind.as_str() == "area";
            // Default edge-to-edge (first point on the value axis, last at the
            // right edge); point-placement="center" insets onto category bands.
            let slot_center = chart.point_placement.as_deref() == Some("center");
            let n_categories = chart
                .series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0);

            emit_gridlines_and_labels(
                &plot,
                &y_ticks,
                colors,
                &chart.id,
                cx,
                commands,
                diagnostics,
            );

            // Resolve color + points once per series (reused for fill + stroke).
            let mut series_geom: Vec<(Vec<(f64, f64)>, Color)> =
                Vec::with_capacity(chart.series.len());
            for (idx, series) in chart.series.iter().enumerate() {
                let c = series_color(series, idx, cx.resolved, diagnostics, &chart.id);
                let pts = line_points(&series.values, &plot, &y_scale, slot_center);
                series_geom.push((pts, c));
            }

            // Area fills first (drawn below the line strokes).
            if is_area {
                for (pts, c) in &series_geom {
                    // Area fill: series color at ~25% alpha.
                    let area_color = Color::srgb(c.r, c.g, c.b, 64);
                    emit_area_fill(pts, &plot, area_color, commands);
                }
            }

            // Line strokes on top of fills.
            for (pts, c) in &series_geom {
                emit_line_series(pts, *c, 2.0, commands);
            }

            emit_axis_lines(&plot, colors.axis, commands);

            if !chart.categories.is_empty() {
                emit_category_labels(
                    &chart.categories,
                    n_categories,
                    CatLabels {
                        plot: &plot,
                        color: colors.label,
                        slot_center,
                    },
                    cx,
                    commands,
                    diagnostics,
                );
            }
        }
        // sparkline is handled in its own early branch above and never reaches here.
        // pie/donut/unknown are filtered by the gate match and never reach here.
        _ => {}
    }

    // ── Legend ───────────────────────────────────────────────────────────────
    // The legend strip sits to the right of the plot area.  When `legend_on`
    // is false `legend_w` is `0.0` and this block is skipped — no commands are
    // emitted and the existing render path is byte-identical.
    if legend_on && legend_w > 0.0 {
        emit_legend(
            &series_entries,
            LegendArea {
                x: x + w - legend_w,
                y,
                w: legend_w,
                h,
            },
            cx,
            commands,
            diagnostics,
        );
    }

    // ── Title ────────────────────────────────────────────────────────────────
    if let Some(title) = &chart.title {
        emit_title(
            title,
            (x, y),
            DEFAULT_TITLE_COLOR,
            &chart.id,
            cx,
            commands,
            diagnostics,
        );
    }

    0.0
}

// ── Sparkline emitter ─────────────────────────────────────────────────────────

/// Emit a sparkline into `[x, y, w, h]` with a small inset on all four sides.
///
/// Sparklines are compact, axis-free series previews — no gridlines, no tick
/// labels, no title. One 1.5 px stroke per series, colored by the shared
/// series-color resolver.
///
/// Returns `0.0` (charts are absolute-positioned and do not participate in
/// flow layout).
fn emit_sparkline(
    chart: &ChartNode,
    bbox: (f64, f64, f64, f64),
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    const INSET: f64 = 4.0;
    let (x, y, w, h) = bbox;

    let spark_plot = PlotArea {
        x: x + INSET,
        y: y + INSET,
        w: (w - 2.0 * INSET).max(0.0),
        h: (h - 2.0 * INSET).max(0.0),
    };

    // Auto-fit data range (no zero-fold, no stacked expansion).
    let (data_lo, data_hi) =
        data_range(&chart.series, chart.axis_min, chart.axis_max).unwrap_or((0.0, 1.0));

    let y_scale = LinearScale {
        data_min: data_lo,
        data_max: data_hi,
        pixel_min: spark_plot.y + spark_plot.h,
        pixel_max: spark_plot.y,
    };

    for (idx, series) in chart.series.iter().enumerate() {
        let color = series_color(series, idx, cx.resolved, diagnostics, &chart.id);
        let pts = line_points(&series.values, &spark_plot, &y_scale, false);
        emit_line_series(&pts, color, 1.5, commands);
    }

    0.0
}

// ── Title emitter ─────────────────────────────────────────────────────────────

/// Shape and emit a chart title above the plot area.
///
/// The title is placed at the top-left of the chart bbox, vertically just
/// inside the top margin, using Noto Sans 13 px.
pub(super) fn emit_title(
    title: &str,
    origin: (f64, f64),
    color: Color,
    chart_id: &str,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (chart_x, chart_y) = origin;
    let families = [String::from("Noto Sans")];
    let req = ShapeRequest {
        text: title,
        families: &families,
        weight: 600,
        style: FontStyle::Normal,
        font_size: 13.0,
        direction: TextDirection::Ltr,
    };

    match cx.engine.shape_with_fallback(&req, cx.fonts) {
        Err(e) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.text_unshaped",
                format!(
                    "chart '{}' title could not be shaped: {}",
                    chart_id, e.message
                ),
                None,
                Some(chart_id.to_owned()),
            ));
        }
        Ok(result) => {
            // Ascent from first run for baseline offset from top edge.
            let ascent: f64 = result.runs.first().map(|r| r.ascent as f64).unwrap_or(10.0);

            // Baseline sits `ascent` px below the chart's top edge, with 2 px
            // of breathing room so it does not clip at the top.
            let baseline_y = chart_y + ascent + 2.0;
            let mut label_x = chart_x + 4.0; // left-aligned with a small indent

            for run in result.runs {
                let advance = run.advance_width as f64;
                let glyphs = run_to_scene_glyphs(&run);
                commands.push(SceneCommand::DrawGlyphRun {
                    x: label_x,
                    y: baseline_y,
                    font_id: run.font_id.clone(),
                    font_size: run.font_size,
                    color,
                    stroke_color: None,
                    stroke_width: None,
                    glyphs,
                });
                label_x += advance;
            }
        }
    }
}

// ── Legend entry builders ─────────────────────────────────────────────────────

/// Build legend entries for a pie or donut chart.
///
/// One entry per category (indexed into `series[0].values`): the label comes
/// from `chart.categories[i]` when available, falling back to the 1-based
/// ordinal string `"1"`, `"2"`, … Slice colors follow the same deterministic
/// palette as `emit_pie`.
pub(super) fn pie_legend_entries(chart: &ChartNode, n: usize) -> Vec<(String, Color)> {
    (0..n)
        .map(|i| {
            let label = chart
                .categories
                .get(i)
                .cloned()
                .unwrap_or_else(|| (i + 1).to_string());
            let color = slice_color(i);
            (label, color)
        })
        .collect()
}
