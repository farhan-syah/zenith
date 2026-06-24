//! Opt-in connector "line jumps": deterministic hops drawn where two top-level
//! connector polylines cross each other.
//!
//! This is page furniture, run by [`compile_page`](super::compile_page) as a
//! post-pass over the already-emitted scene commands when the page declares
//! `line-jumps="arc"` or `line-jumps="gap"`. With no such property the pass
//! never runs and the scene is byte-identical.
//!
//! Two styles are supported:
//! - `arc`: replace the crossing segment with a small semicircular bump centred
//!   on the crossing point, on a fixed side, so the hopping line reads as
//!   stepping over the line it crosses.
//! - `gap`: break the crossing segment, leaving a small gap centred on the
//!   crossing point, so the hopping line reads as passing under.
//!
//! WHO HOPS (deterministic): at each crossing the HORIZONTAL segment hops over a
//! VERTICAL one. When the two segments are not a clean horizontal/vertical pair
//! (e.g. two diagonals), the connector that appears LATER in document order
//! hops. Crossings are PROPER only — a shared endpoint or a mere touch does not
//! count.

use crate::ir::SceneCommand;

/// Bump radius / gap half-length, in pixels.
const JUMP_R: f64 = 5.0;

/// Number of straight segments used to approximate a semicircular arc bump.
const ARC_SEGMENTS: usize = 8;

/// Float epsilon for "strictly interior" crossing tests and axis-aligned
/// classification.
const EPS: f64 = 1e-9;

/// Record the index of the single `StrokePolyline` emitted by a connector whose
/// commands occupy `commands[start..]`.
///
/// A connector that emitted any transform/clip/layer bracket in that range (a
/// rotated or otherwise bracketed connector) is skipped entirely — its stroke is
/// NOT recorded, so the line-jump pass leaves it alone. Otherwise the one
/// `StrokePolyline` in the range is recorded by its absolute index. If the range
/// has no `StrokePolyline`, nothing is recorded.
pub(in crate::compile) fn record_connector_stroke(
    commands: &[SceneCommand],
    start: usize,
    out: &mut Vec<usize>,
) {
    let range = match commands.get(start..) {
        Some(r) => r,
        None => return,
    };
    let bracketed = range.iter().any(|c| {
        matches!(
            c,
            SceneCommand::PushTransform { .. }
                | SceneCommand::PushClip { .. }
                | SceneCommand::PushLayer { .. }
        )
    });
    if bracketed {
        return;
    }
    for (offset, cmd) in range.iter().enumerate() {
        if matches!(cmd, SceneCommand::StrokePolyline { .. }) {
            out.push(start + offset);
            return;
        }
    }
}

/// A crossing of the hopping connector's segment by another connector's segment.
#[derive(Clone, Copy)]
struct Hop {
    /// Index of the hopping segment in the hopping polyline (segment `s` spans
    /// points `s` and `s + 1`).
    seg: usize,
    /// Crossing point.
    px: f64,
    py: f64,
    /// Distance of the crossing from the segment's START point (used to order
    /// multiple hops along one segment).
    dist_from_start: f64,
}

/// Apply line-jumps to the top-level connector strokes named by
/// `connector_strokes` (absolute indices into `commands`, in document order).
///
/// `mode` is `"arc"` (rewrite each hopping polyline's points in place) or
/// `"gap"` (split each hopping polyline into pieces, which changes the command
/// count). Any other value is a no-op. Deterministic: connectors are visited in
/// the given order and crossings are ordered by total-ordered float compares.
pub(in crate::compile) fn apply_line_jumps(
    commands: &mut Vec<SceneCommand>,
    connector_strokes: &[usize],
    mode: &str,
) {
    if mode != "arc" && mode != "gap" {
        return;
    }

    // Snapshot every connector's points up front so all crossings are computed
    // against the ORIGINAL routes, independent of how earlier connectors are
    // mutated.
    let mut snapshots: Vec<(usize, Vec<f64>)> = Vec::with_capacity(connector_strokes.len());
    for &idx in connector_strokes {
        if let Some(SceneCommand::StrokePolyline { points, .. }) = commands.get(idx) {
            snapshots.push((idx, points.clone()));
        }
    }

    // For each connector (by its position in `connector_strokes`), collect the
    // hops it must draw. Map order = position; the value is the list of hops.
    let mut hops_per_connector: Vec<Vec<Hop>> = vec![Vec::new(); snapshots.len()];

    // Pair connectors by document order i < j.
    let mut i = 0;
    while i < snapshots.len() {
        let mut j = i + 1;
        while j < snapshots.len() {
            let (_, ref a_pts) = snapshots[i];
            let (_, ref b_pts) = snapshots[j];
            collect_pair_hops(a_pts, b_pts, i, j, &mut hops_per_connector);
            j += 1;
        }
        i += 1;
    }

    if mode == "arc" {
        apply_arc(commands, &snapshots, &hops_per_connector);
    } else {
        apply_gap(commands, &snapshots, &hops_per_connector);
    }
}

/// Find every proper crossing between connector `i`'s polyline (`a_pts`) and
/// connector `j`'s polyline (`b_pts`), decide which connector hops at each, and
/// append a [`Hop`] to that connector's list.
fn collect_pair_hops(
    a_pts: &[f64],
    b_pts: &[f64],
    i: usize,
    j: usize,
    hops_per_connector: &mut [Vec<Hop>],
) {
    let a_segs = a_pts.len() / 2;
    let b_segs = b_pts.len() / 2;
    let mut sa = 0;
    while sa + 1 < a_segs {
        let Some(a) = segment(a_pts, sa) else {
            sa += 1;
            continue;
        };
        let mut sb = 0;
        while sb + 1 < b_segs {
            let Some(b) = segment(b_pts, sb) else {
                sb += 1;
                continue;
            };
            if let Some((px, py)) = proper_intersection(a, b) {
                // Decide who hops. The HORIZONTAL segment hops over a VERTICAL
                // one. Connector `a` hops only when it is horizontal and `b` is
                // vertical; in every other case (including `b` horizontal over
                // `a` vertical, and any non-axis-aligned pair) the connector
                // later in document order — which is `b`, since i < j — hops.
                let hop_is_a = is_horizontal(a) && is_vertical(b);

                if hop_is_a {
                    if let Some(list) = hops_per_connector.get_mut(i) {
                        let d = dist_from_start(a, px, py);
                        list.push(Hop {
                            seg: sa,
                            px,
                            py,
                            dist_from_start: d,
                        });
                    }
                } else if let Some(list) = hops_per_connector.get_mut(j) {
                    let d = dist_from_start(b, px, py);
                    list.push(Hop {
                        seg: sb,
                        px,
                        py,
                        dist_from_start: d,
                    });
                }
            }
            sb += 1;
        }
        sa += 1;
    }
}

/// A line segment as its two endpoints.
type Seg = ((f64, f64), (f64, f64));

/// Read segment `s` (points `s` and `s + 1`) from a flat point list.
fn segment(pts: &[f64], s: usize) -> Option<Seg> {
    let x0 = *pts.get(2 * s)?;
    let y0 = *pts.get(2 * s + 1)?;
    let x1 = *pts.get(2 * s + 2)?;
    let y1 = *pts.get(2 * s + 3)?;
    Some(((x0, y0), (x1, y1)))
}

fn is_horizontal(seg: Seg) -> bool {
    let ((_, y0), (_, y1)) = seg;
    (y1 - y0).abs() < EPS
}

fn is_vertical(seg: Seg) -> bool {
    let ((x0, _), (x1, _)) = seg;
    (x1 - x0).abs() < EPS
}

/// Distance of point `(px, py)` from a segment's start endpoint.
fn dist_from_start(seg: Seg, px: f64, py: f64) -> f64 {
    let ((x0, y0), _) = seg;
    let dx = px - x0;
    let dy = py - y0;
    (dx * dx + dy * dy).sqrt()
}

/// Proper (strictly interior to BOTH segments) intersection of two segments, or
/// `None`. Shared endpoints / touching at an endpoint do NOT count.
fn proper_intersection(a: Seg, b: Seg) -> Option<(f64, f64)> {
    let ((ax0, ay0), (ax1, ay1)) = a;
    let ((bx0, by0), (bx1, by1)) = b;

    let rx = ax1 - ax0;
    let ry = ay1 - ay0;
    let sx = bx1 - bx0;
    let sy = by1 - by0;

    let denom = rx * sy - ry * sx;
    if denom.abs() < EPS {
        // Parallel or collinear: no single proper crossing.
        return None;
    }

    let qpx = bx0 - ax0;
    let qpy = by0 - ay0;

    let t = (qpx * sy - qpy * sx) / denom;
    let u = (qpx * ry - qpy * rx) / denom;

    // STRICTLY interior on both: excludes shared endpoints / touches.
    if t > EPS && t < 1.0 - EPS && u > EPS && u < 1.0 - EPS {
        Some((ax0 + t * rx, ay0 + t * ry))
    } else {
        None
    }
}

/// Order a connector's hops along its polyline: by segment index, then by
/// distance from that segment's start (total-ordered).
fn sort_hops(hops: &mut [Hop]) {
    hops.sort_by(|a, b| {
        a.seg
            .cmp(&b.seg)
            .then_with(|| a.dist_from_start.total_cmp(&b.dist_from_start))
    });
}

/// ARC mode: rewrite each hopping connector's `StrokePolyline.points` in place,
/// inserting a small semicircular bump at every crossing.
fn apply_arc(
    commands: &mut [SceneCommand],
    snapshots: &[(usize, Vec<f64>)],
    hops_per_connector: &[Vec<Hop>],
) {
    for (pos, (idx, base_pts)) in snapshots.iter().enumerate() {
        let Some(hops) = hops_per_connector.get(pos) else {
            continue;
        };
        if hops.is_empty() {
            continue;
        }
        let mut ordered = hops.to_vec();
        sort_hops(&mut ordered);
        let new_pts = rebuild_points_with_bumps(base_pts, &ordered);
        if let Some(SceneCommand::StrokePolyline { points, .. }) = commands.get_mut(*idx) {
            *points = new_pts;
        }
    }
}

/// Build a fresh flat point list from `base_pts`, inserting each hop's bump on
/// its segment in order. Untouched segments are copied verbatim.
fn rebuild_points_with_bumps(base_pts: &[f64], ordered_hops: &[Hop]) -> Vec<f64> {
    let n = base_pts.len() / 2;
    let mut out: Vec<f64> = Vec::with_capacity(base_pts.len());
    if n == 0 {
        return out;
    }
    // Always start at point 0.
    if let (Some(&x0), Some(&y0)) = (base_pts.first(), base_pts.get(1)) {
        out.push(x0);
        out.push(y0);
    }
    let mut s = 0;
    while s + 1 < n {
        // Hops on this segment, already globally ordered, so the subset for `s`
        // is also ordered along the segment.
        if let Some(seg) = segment(base_pts, s) {
            for hop in ordered_hops.iter().filter(|h| h.seg == s) {
                push_bump(&mut out, seg, hop.px, hop.py);
            }
        }
        // End of this segment = point s+1.
        if let (Some(&x), Some(&y)) = (base_pts.get(2 * (s + 1)), base_pts.get(2 * (s + 1) + 1)) {
            out.push(x);
            out.push(y);
        }
        s += 1;
    }
    out
}

/// Append the intermediate points of a semicircular bump centred at `(px, py)`
/// on segment `seg`. The bump bulges toward a FIXED side: decreasing y for a
/// horizontal segment, decreasing x for a vertical segment, and decreasing y for
/// any other (diagonal) segment. The two base points sit at `±JUMP_R` along the
/// segment direction; only the interior arc points are emitted (the segment's
/// own endpoints are pushed by the caller).
fn push_bump(out: &mut Vec<f64>, seg: Seg, px: f64, py: f64) {
    let ((x0, y0), (x1, y1)) = seg;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < EPS {
        return;
    }
    // Unit direction along the segment (start → end).
    let ux = dx / len;
    let uy = dy / len;

    // Fixed-side outward normal. For a horizontal segment bump toward -y; for a
    // vertical segment bump toward -x; otherwise toward -y.
    let (nx, ny) = if is_horizontal(seg) {
        (0.0, -1.0)
    } else if is_vertical(seg) {
        (-1.0, 0.0)
    } else {
        (0.0, -1.0)
    };

    // Bump base entry/exit, ordered along the segment direction.
    // Parameter angle goes from PI (entry, -JUMP_R along dir) to 0 (exit,
    // +JUMP_R along dir), so the arc points are emitted start→end.
    let pi = std::f64::consts::PI;
    let steps = ARC_SEGMENTS;
    // Emit entry base, then arc interior, then exit base — all as interior
    // points between the segment endpoints.
    // Entry base point (start side of crossing).
    out.push(px - JUMP_R * ux);
    out.push(py - JUMP_R * uy);
    // Interior arc points (exclude the two base endpoints, which we already /
    // will push as bases).
    let mut k = 1;
    while k < steps {
        let frac = k as f64 / steps as f64;
        let theta = pi * (1.0 - frac); // PI → 0
        // Position along segment: cos(theta) maps PI→-1, 0→+1.
        let along = JUMP_R * theta.cos();
        let out_dist = JUMP_R * theta.sin();
        let ax = px + along * ux + out_dist * nx;
        let ay = py + along * uy + out_dist * ny;
        out.push(ax);
        out.push(ay);
        k += 1;
    }
    // Exit base point (end side of crossing).
    out.push(px + JUMP_R * ux);
    out.push(py + JUMP_R * uy);
}

/// GAP mode: rebuild the whole command vector, splitting each hopping
/// connector's single `StrokePolyline` into pieces with a small gap centred on
/// every crossing. All non-connector commands and untouched connectors are
/// copied verbatim in their original positions.
fn apply_gap(
    commands: &mut Vec<SceneCommand>,
    snapshots: &[(usize, Vec<f64>)],
    hops_per_connector: &[Vec<Hop>],
) {
    use std::collections::BTreeMap;

    // Map: command index → ordered hops, only for connectors that actually hop.
    let mut split_at: BTreeMap<usize, Vec<Hop>> = BTreeMap::new();
    for (pos, (idx, _)) in snapshots.iter().enumerate() {
        if let Some(hops) = hops_per_connector.get(pos)
            && !hops.is_empty()
        {
            let mut ordered = hops.to_vec();
            sort_hops(&mut ordered);
            split_at.insert(*idx, ordered);
        }
    }
    if split_at.is_empty() {
        // No-op: leave the commands exactly as they are.
        return;
    }

    let mut new_cmds: Vec<SceneCommand> = Vec::with_capacity(commands.len());
    for (idx, cmd) in commands.iter().enumerate() {
        match split_at.get(&idx) {
            Some(hops) => {
                if let SceneCommand::StrokePolyline {
                    points,
                    color,
                    stroke_width,
                    closed,
                    align,
                    fill_even_odd,
                } = cmd
                {
                    for piece in split_polyline(points, hops) {
                        new_cmds.push(SceneCommand::StrokePolyline {
                            points: piece,
                            color: *color,
                            stroke_width: *stroke_width,
                            closed: *closed,
                            align: *align,
                            fill_even_odd: *fill_even_odd,
                        });
                    }
                } else {
                    new_cmds.push(cmd.clone());
                }
            }
            None => new_cmds.push(cmd.clone()),
        }
    }
    *commands = new_cmds;
}

/// Split a flat polyline into pieces, opening a gap of half-length `JUMP_R` on
/// each side of every crossing (along the segment direction). Returns the list
/// of piece point-lists in order from the polyline start.
fn split_polyline(base_pts: &[f64], ordered_hops: &[Hop]) -> Vec<Vec<f64>> {
    let n = base_pts.len() / 2;
    let mut pieces: Vec<Vec<f64>> = Vec::new();
    if n == 0 {
        return pieces;
    }
    let mut current: Vec<f64> = Vec::new();
    if let (Some(&x0), Some(&y0)) = (base_pts.first(), base_pts.get(1)) {
        current.push(x0);
        current.push(y0);
    }
    let mut s = 0;
    while s + 1 < n {
        if let Some(seg) = segment(base_pts, s) {
            let ((x0, y0), (x1, y1)) = seg;
            let dx = x1 - x0;
            let dy = y1 - y0;
            let len = (dx * dx + dy * dy).sqrt();
            for hop in ordered_hops.iter().filter(|h| h.seg == s) {
                if len < EPS {
                    continue;
                }
                let ux = dx / len;
                let uy = dy / len;
                // End the current piece just BEFORE the crossing.
                current.push(hop.px - JUMP_R * ux);
                current.push(hop.py - JUMP_R * uy);
                pieces.push(std::mem::take(&mut current));
                // Start the next piece just AFTER the crossing.
                current.push(hop.px + JUMP_R * ux);
                current.push(hop.py + JUMP_R * uy);
            }
        }
        // Append the segment end point to the current piece.
        if let (Some(&x), Some(&y)) = (base_pts.get(2 * (s + 1)), base_pts.get(2 * (s + 1) + 1)) {
            current.push(x);
            current.push(y);
        }
        s += 1;
    }
    if current.len() >= 4 {
        pieces.push(current);
    } else if !current.is_empty() && pieces.is_empty() {
        // Degenerate: keep a single piece even if short, so nothing vanishes.
        pieces.push(current);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Color, StrokeAlign};

    fn stroke(points: Vec<f64>) -> SceneCommand {
        SceneCommand::StrokePolyline {
            points,
            color: Color::srgb(0, 0, 0, 255),
            stroke_width: 2.0,
            closed: false,
            align: StrokeAlign::Center,
            fill_even_odd: false,
        }
    }

    fn polyline_points(cmd: &SceneCommand) -> Vec<f64> {
        match cmd {
            SceneCommand::StrokePolyline { points, .. } => points.clone(),
            _ => panic!("expected StrokePolyline"),
        }
    }

    fn count_strokes(cmds: &[SceneCommand]) -> usize {
        cmds.iter()
            .filter(|c| matches!(c, SceneCommand::StrokePolyline { .. }))
            .count()
    }

    /// Horizontal connector (along x) and vertical connector (along y) crossing
    /// at (50, 50). The horizontal one must hop (arc): more points; the vertical
    /// stays unchanged.
    #[test]
    fn arc_horizontal_hops_over_vertical() {
        // index 0: horizontal y=50 from x=0..100
        // index 1: vertical x=50 from y=0..100
        let mut cmds = vec![
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
        ];
        let before_h = polyline_points(&cmds[0]);
        let before_v = polyline_points(&cmds[1]);

        apply_line_jumps(&mut cmds, &[0, 1], "arc");

        let after_h = polyline_points(&cmds[0]);
        let after_v = polyline_points(&cmds[1]);

        assert!(
            after_h.len() > before_h.len(),
            "horizontal connector should gain bump points: {after_h:?}"
        );
        assert_eq!(after_v, before_v, "vertical connector must be unchanged");
        // The bump should bulge toward -y (decreasing y) near x=50.
        let min_y = after_h
            .chunks_exact(2)
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min);
        assert!(min_y < 50.0, "bump must dip above the line (smaller y)");
    }

    /// Same crossing with `gap`: the horizontal connector becomes two strokes;
    /// the vertical stays one.
    #[test]
    fn gap_horizontal_splits() {
        let mut cmds = vec![
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
        ];
        apply_line_jumps(&mut cmds, &[0, 1], "gap");
        // Original 2 strokes; horizontal split into 2 → total 3.
        assert_eq!(count_strokes(&cmds), 3, "expected one split + one intact");
        // First piece ends before x=50, second starts after.
        let first = polyline_points(&cmds[0]);
        let second = polyline_points(&cmds[1]);
        let last_x_first = first[first.len() - 2];
        let first_x_second = second[0];
        assert!(last_x_first < 50.0, "first piece ends before crossing");
        assert!(first_x_second > 50.0, "second piece starts after crossing");
    }

    /// Two connectors that do not cross: nothing changes, count stable.
    #[test]
    fn no_crossing_no_change() {
        let mut cmds = vec![
            stroke(vec![0.0, 10.0, 100.0, 10.0]),
            stroke(vec![0.0, 90.0, 100.0, 90.0]),
        ];
        let before = cmds.clone();
        let before_count = count_strokes(&cmds);
        apply_line_jumps(&mut cmds, &[0, 1], "arc");
        assert_eq!(count_strokes(&cmds), before_count);
        assert_eq!(polyline_points(&cmds[0]), polyline_points(&before[0]));
        assert_eq!(polyline_points(&cmds[1]), polyline_points(&before[1]));

        let mut cmds_gap = before.clone();
        apply_line_jumps(&mut cmds_gap, &[0, 1], "gap");
        assert_eq!(count_strokes(&cmds_gap), before_count);
    }

    /// Running the pass twice on equal input yields equal commands.
    #[test]
    fn determinism_arc() {
        let base = vec![
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
        ];
        let mut a = base.clone();
        let mut b = base;
        apply_line_jumps(&mut a, &[0, 1], "arc");
        apply_line_jumps(&mut b, &[0, 1], "arc");
        assert_eq!(polyline_points(&a[0]), polyline_points(&b[0]));
        assert_eq!(polyline_points(&a[1]), polyline_points(&b[1]));
    }

    /// A shared endpoint (touching, not crossing) does not produce a hop.
    #[test]
    fn touching_endpoint_no_hop() {
        // Horizontal ends at (50,50); vertical starts at (50,50).
        let mut cmds = vec![
            stroke(vec![0.0, 50.0, 50.0, 50.0]),
            stroke(vec![50.0, 50.0, 50.0, 100.0]),
        ];
        let before = cmds.clone();
        apply_line_jumps(&mut cmds, &[0, 1], "arc");
        assert_eq!(polyline_points(&cmds[0]), polyline_points(&before[0]));
        assert_eq!(polyline_points(&cmds[1]), polyline_points(&before[1]));
    }

    /// Unknown / none mode is a no-op.
    #[test]
    fn none_mode_no_op() {
        let mut cmds = vec![
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
        ];
        let before = cmds.clone();
        apply_line_jumps(&mut cmds, &[0, 1], "none");
        assert_eq!(count_strokes(&cmds), count_strokes(&before));
        assert_eq!(polyline_points(&cmds[0]), polyline_points(&before[0]));
    }

    /// record_connector_stroke skips a bracketed (transform-wrapped) connector.
    #[test]
    fn record_skips_bracketed() {
        let cmds = vec![
            SceneCommand::PushTransform {
                angle_deg: 10.0,
                cx: 0.0,
                cy: 0.0,
            },
            stroke(vec![0.0, 0.0, 10.0, 10.0]),
            SceneCommand::PopTransform,
        ];
        let mut out = Vec::new();
        record_connector_stroke(&cmds, 0, &mut out);
        assert!(out.is_empty(), "bracketed connector must be skipped");
    }

    /// record_connector_stroke records the single stroke for a plain connector.
    #[test]
    fn record_plain_connector() {
        let cmds = vec![stroke(vec![0.0, 0.0, 10.0, 10.0])];
        let mut out = Vec::new();
        record_connector_stroke(&cmds, 0, &mut out);
        assert_eq!(out, vec![0]);
    }
}
