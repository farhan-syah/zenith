//! Pie and donut chart geometry and emission for `kind="pie"` and `kind="donut"`.
//!
//! `slice_angles` and `wedge_polygon` are pure geometry functions (no engine,
//! no I/O) suitable for unit testing. `emit_pie` composes them with text
//! shaping to produce `FillPolygon` and `DrawGlyphRun` commands.

use std::f64::consts::PI;

use zenith_core::{ChartNode, Diagnostic, FontStyle};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine};

use crate::ir::{Color, Paint, SceneCommand};

use super::super::NodeCtx;
use super::super::paint::resolve_property_color;
use super::super::text::run_to_scene_glyphs;
use super::bar::ON_FILL_LABEL_COLOR;
use super::entry::{DEFAULT_TITLE_COLOR, emit_title};
use super::palette::SERIES_PALETTE;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Tessellation step: one arc vertex per ~3 degrees.
const STEP_RAD: f64 = PI / 60.0;
/// Donut inner-radius fraction of the outer radius.
const DONUT_HOLE_FRAC: f64 = 0.58;

// ── PieGeom ───────────────────────────────────────────────────────────────────

/// Geometry parameters shared by all wedge computations for one pie/donut chart.
#[derive(Clone, Copy)]
pub(super) struct PieGeom {
    pub cx: f64,
    pub cy: f64,
    pub r_outer: f64,
    /// `0.0` for pie; `r_outer * DONUT_HOLE_FRAC` for donut.
    pub r_inner: f64,
}

// ── slice_angles ──────────────────────────────────────────────────────────────

/// Compute `(start_angle, end_angle)` in radians for each value, clockwise
/// from 12 o'clock (−π/2).
///
/// Non-positive and non-finite values are skipped: their pair is returned as
/// `(running_angle, running_angle)` (zero-width), so slice index `i` always
/// corresponds to `values[i]`.
///
/// Returns an empty `Vec` when the positive total is `<= 0.0`.
pub(super) fn slice_angles(values: &[f64]) -> Vec<(f64, f64)> {
    let total: f64 = values.iter().filter(|v| v.is_finite() && **v > 0.0).sum();
    if total <= 0.0 {
        return Vec::new();
    }

    let mut angles = Vec::with_capacity(values.len());
    let mut acc = -PI / 2.0; // start at 12 o'clock

    for &v in values {
        let sweep = if v.is_finite() && v > 0.0 {
            v / total * 2.0 * PI
        } else {
            0.0
        };
        angles.push((acc, acc + sweep));
        acc += sweep;
    }

    angles
}

// ── wedge_polygon ─────────────────────────────────────────────────────────────

/// Build a flat vertex list (`x0, y0, x1, y1, …`) for one wedge.
///
/// - `r_inner == 0.0` → pie fan: `[center, outer_p0, …, outer_pN]`.
/// - `r_inner > 0.0` → annulus ring: `[outer_p0, …, outer_pN, inner_pN, …, inner_p0]`.
///
/// Arc points are sampled every `STEP_RAD` radians, with at least 2 interior
/// arc points so every wedge forms a valid polygon. Y-down convention: positive
/// angle is clockwise.
pub(super) fn wedge_polygon(geom: PieGeom, a_start: f64, a_end: f64) -> Vec<f64> {
    let sweep = a_end - a_start;
    // Number of arc steps: at least 2, capped at 2 048 to guard against
    // non-finite or pathologically large sweep values.
    let n_steps = if sweep.is_finite() && sweep > 0.0 {
        ((sweep / STEP_RAD).ceil() as usize).clamp(2, 2048)
    } else {
        2
    };
    let arc_pts = n_steps + 1; // inclusive of both endpoints

    if geom.r_inner <= 0.0 {
        // Pie fan: center point first, then outer arc (forward).
        let mut pts = Vec::with_capacity(2 + arc_pts * 2);
        pts.push(geom.cx);
        pts.push(geom.cy);
        for i in 0..=n_steps {
            let a = a_start + sweep * (i as f64 / n_steps as f64);
            pts.push(geom.cx + geom.r_outer * a.cos());
            pts.push(geom.cy + geom.r_outer * a.sin());
        }
        pts
    } else {
        // Donut annulus: outer arc forward, inner arc reversed.
        let mut inner: Vec<(f64, f64)> = Vec::with_capacity(arc_pts);
        let mut pts = Vec::with_capacity(arc_pts * 4);
        for i in 0..=n_steps {
            let a = a_start + sweep * (i as f64 / n_steps as f64);
            pts.push(geom.cx + geom.r_outer * a.cos());
            pts.push(geom.cy + geom.r_outer * a.sin());
            inner.push((
                geom.cx + geom.r_inner * a.cos(),
                geom.cy + geom.r_inner * a.sin(),
            ));
        }
        for (px, py) in inner.iter().rev() {
            pts.push(*px);
            pts.push(*py);
        }
        pts
    }
}

// ── Slice color ───────────────────────────────────────────────────────────────

/// Return the palette color for slice `idx` (cycles through `SERIES_PALETTE`).
pub(super) fn slice_color(idx: usize) -> Color {
    SERIES_PALETTE
        .get(idx % SERIES_PALETTE.len())
        .copied()
        .unwrap_or(Color::srgb(66, 133, 244, 255))
}

// ── Slice-label context ───────────────────────────────────────────────────────

/// Per-slice label context bundled to keep `emit_slice_label` within 7 arguments.
#[derive(Clone, Copy)]
struct LabelCtx<'a> {
    geom: PieGeom,
    is_donut: bool,
    families: &'a [String],
    chart_id: &'a str,
}

/// Shape and emit a percentage label for one pie/donut slice in `label_color`.
///
/// The label is centered radially at `label_r` on the midpoint angle of the
/// slice.
fn emit_slice_label(
    label: &str,
    mid_angle: f64,
    label_color: Color,
    lc: LabelCtx,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let geom = lc.geom;
    let label_r = if lc.is_donut {
        (geom.r_outer + geom.r_inner) / 2.0
    } else {
        geom.r_outer * 0.60
    };

    let lx = geom.cx + label_r * mid_angle.cos();
    let ly = geom.cy + label_r * mid_angle.sin();

    let req = ShapeRequest {
        text: label,
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
                    "chart '{}' pie slice label '{}' could not be shaped: {}",
                    lc.chart_id, label, e.message
                ),
                None,
                Some(lc.chart_id.to_owned()),
            ));
        }
        Ok(result) => {
            let total_advance: f64 = result.runs.iter().map(|r| r.advance_width as f64).sum();
            let ascent: f64 = result.runs.first().map(|r| r.ascent as f64).unwrap_or(7.0);
            let baseline_y = ly + ascent * 0.35;
            let mut label_x = lx - total_advance / 2.0;
            let color = label_color;

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

// ── emit_pie ──────────────────────────────────────────────────────────────────

/// Emit all scene commands for a `kind="pie"` or `kind="donut"` chart.
///
/// Uses only `series[0].values` (single-series pie). Slice `i` maps to
/// `values[i]` and `categories[i]`. Non-positive/non-finite values are skipped.
/// Slices narrower than 0.15 rad (~8.6°) receive no percentage label.
///
/// Returns `0.0` (charts are absolute-positioned and do not participate in
/// flow layout).
pub(super) fn emit_pie(
    chart: &ChartNode,
    bbox: (f64, f64, f64, f64),
    is_donut: bool,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let (x, y, w, h) = bbox;

    // Reserve space for an optional title above the chart area.
    let title_h = if chart.title.is_some() { 24.0 } else { 10.0 };
    let pad = 12.0;
    let draw_x = x + pad;
    let draw_y = y + title_h;
    let draw_w = (w - 2.0 * pad).max(0.0);
    let draw_h = (h - title_h - pad).max(0.0);

    let cx_c = draw_x + draw_w / 2.0;
    let cy_c = draw_y + draw_h / 2.0;
    let r_outer = (draw_w.min(draw_h) / 2.0).max(0.0);
    let r_inner = if is_donut {
        r_outer * DONUT_HOLE_FRAC
    } else {
        0.0
    };

    // Always emit the title even when the drawing area is too small.
    let emit_chart_title = |commands: &mut Vec<SceneCommand>, diagnostics: &mut Vec<Diagnostic>| {
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
    };

    if r_outer <= 0.0 {
        emit_chart_title(commands, diagnostics);
        return 0.0;
    }

    let values = chart
        .series
        .first()
        .map(|s| s.values.as_slice())
        .unwrap_or(&[]);
    let angles = slice_angles(values);

    if angles.is_empty() {
        emit_chart_title(commands, diagnostics);
        return 0.0;
    }

    // Total of positive finite values — needed for percentage computation.
    let total: f64 = values.iter().filter(|v| v.is_finite() && **v > 0.0).sum();

    let geom = PieGeom {
        cx: cx_c,
        cy: cy_c,
        r_outer,
        r_inner,
    };
    let suppress_labels = chart.value_labels.as_deref() == Some("none");
    // Chart-level label-color override; falls back to the white on-fill default.
    let chart_label_color = chart
        .value_color
        .as_ref()
        .and_then(|p| resolve_property_color(p, cx.resolved, diagnostics, &chart.id))
        .unwrap_or(ON_FILL_LABEL_COLOR);
    let families = [String::from("Noto Sans")];

    for (i, (a_start, a_end)) in angles.iter().enumerate() {
        let sweep = a_end - a_start;
        if sweep <= 0.0 {
            continue;
        }

        let fill = slice_color(i);
        let poly = wedge_polygon(geom, *a_start, *a_end);
        commands.push(SceneCommand::FillPolygon {
            points: poly,
            paint: Paint::solid(fill),
            even_odd: false,
        });

        if !suppress_labels && sweep >= 0.15 {
            let value = values.get(i).copied().unwrap_or(0.0);
            let pct = (value / total * 100.0).round() as i64;
            let label = format!("{pct}%");
            let mid = a_start + sweep / 2.0;

            let lc = LabelCtx {
                geom,
                is_donut,
                families: &families,
                chart_id: &chart.id,
            };
            // Per-slice label color overrides the chart-level color when set.
            let slice_label_color = chart
                .label_colors
                .get(i)
                .and_then(|p| resolve_property_color(p, cx.resolved, diagnostics, &chart.id))
                .unwrap_or(chart_label_color);
            emit_slice_label(
                &label,
                mid,
                slice_label_color,
                lc,
                cx,
                commands,
                diagnostics,
            );
        }
    }

    emit_chart_title(commands, diagnostics);

    0.0
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    // ── slice_angles ──────────────────────────────────────────────────────────

    #[test]
    fn slice_angles_equal_values_four_slices() {
        let angles = slice_angles(&[1.0, 1.0, 1.0, 1.0]);
        assert_eq!(angles.len(), 4, "expected 4 slices");

        // Each slice should span PI/2 radians.
        for (start, end) in &angles {
            let sweep = end - start;
            assert!(
                (sweep - PI / 2.0).abs() < EPS,
                "sweep should be PI/2, got {sweep}"
            );
        }

        // First slice starts at -PI/2 (12 o'clock).
        assert!((angles[0].0 - (-PI / 2.0)).abs() < EPS);

        // Slices are sequential: each end == next start.
        for i in 0..angles.len() - 1 {
            assert!(
                (angles[i].1 - angles[i + 1].0).abs() < EPS,
                "slice {i} end ({}) != slice {} start ({})",
                angles[i].1,
                i + 1,
                angles[i + 1].0
            );
        }

        // Full circle: last end − first start ≈ 2π.
        let full = angles.last().unwrap().1 - angles[0].0;
        assert!(
            (full - 2.0 * PI).abs() < EPS,
            "full circle expected, got {full}"
        );
    }

    #[test]
    fn slice_angles_empty_returns_empty() {
        assert!(slice_angles(&[]).is_empty());
    }

    #[test]
    fn slice_angles_all_zero_returns_empty() {
        assert!(slice_angles(&[0.0, 0.0, 0.0]).is_empty());
    }

    #[test]
    fn slice_angles_negative_only_returns_empty() {
        assert!(slice_angles(&[-1.0, -5.0]).is_empty());
    }

    #[test]
    fn slice_angles_skips_zero_value_neighbors_correct() {
        // [1, 0, 1] → 3 entries; middle has zero sweep; neighbors each span PI.
        let angles = slice_angles(&[1.0, 0.0, 1.0]);
        assert_eq!(angles.len(), 3);

        let (s0, e0) = angles[0];
        let (s1, e1) = angles[1];
        let (s2, e2) = angles[2];

        // Slice 1 (zero value) has zero width.
        assert!(
            (e1 - s1).abs() < EPS,
            "zero-value slice should have 0 sweep"
        );

        // s1 == e0 and s2 == e1 (sequential).
        assert!((e0 - s1).abs() < EPS);
        assert!((e1 - s2).abs() < EPS);

        // Slices 0 and 2 span exactly PI each (equal share of 2π).
        assert!((e0 - s0 - PI).abs() < EPS, "slice 0 sweep: {}", e0 - s0);
        assert!((e2 - s2 - PI).abs() < EPS, "slice 2 sweep: {}", e2 - s2);
    }

    // ── wedge_polygon ─────────────────────────────────────────────────────────

    fn test_geom_pie() -> PieGeom {
        PieGeom {
            cx: 100.0,
            cy: 100.0,
            r_outer: 50.0,
            r_inner: 0.0,
        }
    }

    fn test_geom_donut() -> PieGeom {
        PieGeom {
            cx: 100.0,
            cy: 100.0,
            r_outer: 50.0,
            r_inner: 29.0,
        }
    }

    #[test]
    fn wedge_polygon_pie_center_first() {
        let geom = test_geom_pie();
        let pts = wedge_polygon(geom, -PI / 2.0, 0.0); // quarter-circle

        // At least 6 floats (center + 2 arc pts × 2 coords each).
        assert!(pts.len() >= 6, "too few points: {}", pts.len());
        // First two coords must be the center.
        assert!((pts[0] - geom.cx).abs() < EPS, "first x != cx");
        assert!((pts[1] - geom.cy).abs() < EPS, "first y != cy");
    }

    #[test]
    fn wedge_polygon_pie_outer_points_on_radius() {
        let geom = test_geom_pie();
        let pts = wedge_polygon(geom, -PI / 2.0, PI / 2.0);

        // Skip the center point (first 2 coords) and check all arc points.
        let mut i = 2;
        while i + 1 < pts.len() {
            let dx = pts[i] - geom.cx;
            let dy = pts[i + 1] - geom.cy;
            let r = (dx * dx + dy * dy).sqrt();
            assert!(
                (r - geom.r_outer).abs() < 1e-6,
                "arc point at idx {i} not on r_outer: r={r}"
            );
            i += 2;
        }
    }

    #[test]
    fn wedge_polygon_donut_no_center() {
        let geom = test_geom_donut();
        let pts = wedge_polygon(geom, -PI / 2.0, 0.0);

        // Donut: even number of coords; at least 4 arc points (2 rings × 2+ pts).
        assert!(pts.len() % 2 == 0);
        // Must not start with center point — first point should be on r_outer.
        let dx = pts[0] - geom.cx;
        let dy = pts[1] - geom.cy;
        let r_first = (dx * dx + dy * dy).sqrt();
        assert!(
            (r_first - geom.r_outer).abs() < 1e-6,
            "donut first point should be on r_outer, got r={r_first}"
        );
    }

    #[test]
    fn wedge_polygon_donut_two_rings() {
        let geom = test_geom_donut();
        // Half-circle — many arc points.
        let pts = wedge_polygon(geom, -PI / 2.0, PI / 2.0);

        // First half: r_outer points; second half: r_inner points.
        // There are (n_steps+1) points per ring.
        let sweep = PI; // PI/2 to PI/2 = PI
        let n_steps = ((sweep / STEP_RAD).ceil() as usize).max(2);
        let ring_len = n_steps + 1; // inclusive both ends

        assert_eq!(pts.len(), ring_len * 4, "expected {} floats", ring_len * 4);

        // Check outer ring (first ring_len points).
        for i in 0..ring_len {
            let dx = pts[i * 2] - geom.cx;
            let dy = pts[i * 2 + 1] - geom.cy;
            let r = (dx * dx + dy * dy).sqrt();
            assert!(
                (r - geom.r_outer).abs() < 1e-6,
                "outer ring point {i}: r={r} expected {}",
                geom.r_outer
            );
        }

        // Check inner ring (second ring_len points, reversed order in output).
        let base = ring_len * 2;
        for i in 0..ring_len {
            let dx = pts[base + i * 2] - geom.cx;
            let dy = pts[base + i * 2 + 1] - geom.cy;
            let r = (dx * dx + dy * dy).sqrt();
            assert!(
                (r - geom.r_inner).abs() < 1e-6,
                "inner ring point {i}: r={r} expected {}",
                geom.r_inner
            );
        }
    }
}
