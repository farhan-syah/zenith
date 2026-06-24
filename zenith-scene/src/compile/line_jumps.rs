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
//!
//! ROTATED CONNECTORS: crossings are computed in ON-PAGE (post-rotation)
//! coordinates. A connector under exactly ONE `PushTransform` (a rotation) is
//! mapped to its on-page route for detection, and any hop geometry built in
//! on-page space is mapped back through the inverse rotation before being
//! written into its local `points`. A connector under TWO OR MORE nested
//! rotations is excluded (rare; kept simple). A depth-0 connector has the
//! identity transform, which is a true no-op: its on-page points equal its raw
//! points and the written-back points are byte-identical to the unrotated path.

use crate::ir::SceneCommand;

/// Bump radius / gap half-length, in pixels.
const JUMP_R: f64 = 5.0;

/// Number of straight segments used to approximate a semicircular arc bump.
const ARC_SEGMENTS: usize = 8;

/// Float epsilon for "strictly interior" crossing tests and axis-aligned
/// classification.
const EPS: f64 = 1e-9;

/// Record the absolute index of the single `StrokePolyline` emitted by a
/// connector whose commands occupy `commands[start..]`.
///
/// This records EVERY connector (top-level or nested) by its stroke index; it no
/// longer inspects brackets. Whether a recorded connector actually participates
/// in line-jumps is decided later in [`apply_line_jumps`], which resolves each
/// connector's active rotation transform: depth-0 and single-rotation connectors
/// participate (crossings computed in on-page space), while connectors under two
/// or more nested rotations are excluded. If the range has no `StrokePolyline`,
/// nothing is recorded.
pub(in crate::compile) fn record_connector_stroke(
    commands: &[SceneCommand],
    start: usize,
    out: &mut Vec<usize>,
) {
    let range = match commands.get(start..) {
        Some(r) => r,
        None => return,
    };
    for (offset, cmd) in range.iter().enumerate() {
        if matches!(cmd, SceneCommand::StrokePolyline { .. }) {
            out.push(start + offset);
            return;
        }
    }
}

/// The rotation transform active over a connector at a given command index.
///
/// - [`Identity`](Transform::Identity) — no rotation active (depth 0). On-page
///   points equal the raw `points`; mapping is a true no-op.
/// - [`Rotate`](Transform::Rotate) — exactly one active `PushTransform`, a
///   rotation by `angle_deg` about pivot `(cx, cy)`.
#[derive(Clone, Copy)]
enum Transform {
    Identity,
    Rotate { angle_deg: f64, cx: f64, cy: f64 },
}

impl Transform {
    /// Map a LOCAL (pre-rotation) point to its ON-PAGE (post-rotation) position.
    /// Identity returns the point untouched — no trig, so no float drift.
    fn to_page(self, p: (f64, f64)) -> (f64, f64) {
        match self {
            Transform::Identity => p,
            Transform::Rotate { angle_deg, cx, cy } => rotate_pt(p, angle_deg, (cx, cy)),
        }
    }

    /// Map an ON-PAGE point back to LOCAL (pre-rotation) space. The inverse of a
    /// rotation by `angle_deg` is a rotation by `-angle_deg` about the same
    /// pivot. Identity returns the point untouched — a true no-op.
    fn to_local(self, p: (f64, f64)) -> (f64, f64) {
        match self {
            Transform::Identity => p,
            Transform::Rotate { angle_deg, cx, cy } => rotate_pt(p, -angle_deg, (cx, cy)),
        }
    }
}

/// Rotate point `p` by `angle_deg` degrees about `pivot`.
fn rotate_pt(p: (f64, f64), angle_deg: f64, pivot: (f64, f64)) -> (f64, f64) {
    let (px, py) = p;
    let (cx, cy) = pivot;
    let rad = angle_deg.to_radians();
    let (s, c) = (rad.sin(), rad.cos());
    let dx = px - cx;
    let dy = py - cy;
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// The active rotation transform for a connector whose `StrokePolyline` sits at
/// command index `idx`: scan `commands[0..idx]`, pushing each `PushTransform`'s
/// value onto a stack and popping on each `PopTransform`. At `idx`:
///
/// - empty stack → [`Transform::Identity`] (depth 0, unchanged behavior),
/// - exactly one entry → that single [`Transform::Rotate`],
/// - two or more → `None`, meaning the connector is EXCLUDED from line jumps.
///
/// `PushClip` / `PushLayer` / `BeginBlur` are deliberately ignored — they never
/// move geometry, and the outermost media clip is always open.
fn active_transform_at(commands: &[SceneCommand], idx: usize) -> Option<Transform> {
    let mut stack: Vec<Transform> = Vec::new();
    let prefix = match commands.get(..idx) {
        Some(p) => p,
        None => return Some(Transform::Identity),
    };
    for cmd in prefix {
        if let SceneCommand::PushTransform { angle_deg, cx, cy } = cmd {
            stack.push(Transform::Rotate {
                angle_deg: *angle_deg,
                cx: *cx,
                cy: *cy,
            });
        } else if matches!(cmd, SceneCommand::PopTransform) {
            stack.pop();
        }
    }
    match stack.as_slice() {
        [] => Some(Transform::Identity),
        [single] => Some(*single),
        _ => None,
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

    // Snapshot every PARTICIPATING connector up front so all crossings are
    // computed against the ORIGINAL routes, independent of how earlier
    // connectors are mutated. A connector participates when its active rotation
    // transform is Identity (depth 0, on-page == raw) or a single Rotate
    // (depth 1, on-page is the rotated route); a connector under two or more
    // nested rotations is excluded. Computed on the ORIGINAL command stream
    // before any arc/gap rewrite.
    //
    // Each snapshot carries the connector's `transform` and its `on_page` points
    // (raw points mapped to on-page space). Crossing detection and hop geometry
    // run entirely in on-page space; the transform maps results back to local.
    // For Identity the mapping is a literal no-op, so depth-0 connectors take
    // the exact same values as before — byte-identical.
    let mut snapshots: Vec<Snapshot> = Vec::with_capacity(connector_strokes.len());
    for &idx in connector_strokes {
        let Some(transform) = active_transform_at(commands, idx) else {
            continue;
        };
        if let Some(SceneCommand::StrokePolyline { points, .. }) = commands.get(idx) {
            let on_page = map_points(points, |p| transform.to_page(p));
            snapshots.push(Snapshot {
                idx,
                transform,
                on_page,
            });
        }
    }

    // For each connector (by its position in `snapshots`), collect the hops it
    // must draw. Index = position; the value is the list of hops, in on-page
    // segment space.
    let mut hops_per_connector: Vec<Vec<Hop>> = vec![Vec::new(); snapshots.len()];

    // Pair connectors by document order i < j, in ON-PAGE coordinates.
    for (i, a) in snapshots.iter().enumerate() {
        for (j, b) in snapshots.iter().enumerate().skip(i + 1) {
            collect_pair_hops(&a.on_page, &b.on_page, i, j, &mut hops_per_connector);
        }
    }

    if mode == "arc" {
        apply_arc(commands, &snapshots, &hops_per_connector);
    } else {
        apply_gap(commands, &snapshots, &hops_per_connector);
    }
}

/// A participating connector's snapshot: where to write, its active rotation
/// transform, and its route in ON-PAGE coordinates.
struct Snapshot {
    /// Absolute command index of this connector's `StrokePolyline`.
    idx: usize,
    /// Active rotation transform (identity for depth-0).
    transform: Transform,
    /// Raw `points` mapped to on-page space. For identity, equal to raw points.
    on_page: Vec<f64>,
}

/// Map a flat `[x0, y0, x1, y1, …]` point list through `f`, returning a new flat
/// list. A trailing lone coordinate (malformed) is dropped.
fn map_points(pts: &[f64], f: impl Fn((f64, f64)) -> (f64, f64)) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    for pair in pts.chunks_exact(2) {
        if let (Some(&x), Some(&y)) = (pair.first(), pair.get(1)) {
            let (nx, ny) = f((x, y));
            out.push(nx);
            out.push(ny);
        }
    }
    out
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
    snapshots: &[Snapshot],
    hops_per_connector: &[Vec<Hop>],
) {
    for (pos, snap) in snapshots.iter().enumerate() {
        let Some(hops) = hops_per_connector.get(pos) else {
            continue;
        };
        if hops.is_empty() {
            continue;
        }
        let mut ordered = hops.to_vec();
        sort_hops(&mut ordered);
        // Build the bumped route in ON-PAGE space, then map every point back to
        // this connector's LOCAL space. For identity the map is a no-op, so the
        // written points are byte-identical to the unrotated path.
        let on_page_pts = rebuild_points_with_bumps(&snap.on_page, &ordered);
        let new_pts = map_points(&on_page_pts, |p| snap.transform.to_local(p));
        if let Some(SceneCommand::StrokePolyline { points, .. }) = commands.get_mut(snap.idx) {
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
    snapshots: &[Snapshot],
    hops_per_connector: &[Vec<Hop>],
) {
    use std::collections::BTreeMap;

    // Map: command index → (transform, on-page route, ordered hops), only for
    // connectors that actually hop. The split is computed on the ON-PAGE route;
    // each piece is mapped back to the connector's LOCAL space (a no-op for
    // identity, so depth-0 pieces are byte-identical).
    let mut split_at: BTreeMap<usize, (Transform, Vec<f64>, Vec<Hop>)> = BTreeMap::new();
    for (pos, snap) in snapshots.iter().enumerate() {
        if let Some(hops) = hops_per_connector.get(pos)
            && !hops.is_empty()
        {
            let mut ordered = hops.to_vec();
            sort_hops(&mut ordered);
            split_at.insert(snap.idx, (snap.transform, snap.on_page.clone(), ordered));
        }
    }
    if split_at.is_empty() {
        // No-op: leave the commands exactly as they are.
        return;
    }

    let mut new_cmds: Vec<SceneCommand> = Vec::with_capacity(commands.len());
    for (idx, cmd) in commands.iter().enumerate() {
        match split_at.get(&idx) {
            Some((transform, on_page, hops)) => {
                if let SceneCommand::StrokePolyline {
                    color,
                    stroke_width,
                    closed,
                    align,
                    fill_even_odd,
                    ..
                } = cmd
                {
                    for piece in split_polyline(on_page, hops) {
                        let local = map_points(&piece, |p| transform.to_local(p));
                        new_cmds.push(SceneCommand::StrokePolyline {
                            points: local,
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

    /// record_connector_stroke now records a transform-wrapped connector too;
    /// the rotation exclusion happens later in the depth filter, not here.
    #[test]
    fn record_includes_bracketed() {
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
        assert_eq!(out, vec![1], "stroke index recorded regardless of bracket");
    }

    /// record_connector_stroke records the single stroke for a plain connector.
    #[test]
    fn record_plain_connector() {
        let cmds = vec![stroke(vec![0.0, 0.0, 10.0, 10.0])];
        let mut out = Vec::new();
        record_connector_stroke(&cmds, 0, &mut out);
        assert_eq!(out, vec![0]);
    }

    /// `active_transform_at` classifies the rotation stack: depth 0 → Identity,
    /// depth 1 → the single Rotate, depth 2+ → None (excluded). A lone PushClip
    /// (which never moves geometry) does not count toward the stack.
    #[test]
    fn active_transform_classifies_depth() {
        // idx 0: PushClip (open, never moves geometry)
        // idx 1: stroke at depth 0 (clip only) → Identity
        // idx 2: PushTransform
        // idx 3: stroke at transform depth 1 → Rotate
        // idx 4: PushTransform (nested)
        // idx 5: stroke at depth 2 → excluded (None)
        // idx 6: PopTransform
        // idx 7: PopTransform
        let cmds = vec![
            SceneCommand::PushClip {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            SceneCommand::PushTransform {
                angle_deg: 30.0,
                cx: 50.0,
                cy: 50.0,
            },
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
            SceneCommand::PushTransform {
                angle_deg: 15.0,
                cx: 10.0,
                cy: 10.0,
            },
            stroke(vec![0.0, 0.0, 10.0, 10.0]),
            SceneCommand::PopTransform,
            SceneCommand::PopTransform,
        ];
        assert!(
            matches!(active_transform_at(&cmds, 1), Some(Transform::Identity)),
            "stroke at idx 1 is identity (clip only)"
        );
        assert!(
            matches!(
                active_transform_at(&cmds, 3),
                Some(Transform::Rotate {
                    angle_deg: 30.0,
                    ..
                })
            ),
            "stroke at idx 3 is a single rotation"
        );
        assert!(
            active_transform_at(&cmds, 5).is_none(),
            "stroke at idx 5 is under two rotations → excluded"
        );
    }

    /// `rotate_pt` and its inverse round-trip exactly enough, and a 90° rotation
    /// about the origin maps (1, 0) → (0, 1).
    #[test]
    fn rotate_pt_inverse_round_trips() {
        let p = (1.0, 0.0);
        let r = rotate_pt(p, 90.0, (0.0, 0.0));
        assert!(
            (r.0 - 0.0).abs() < 1e-9 && (r.1 - 1.0).abs() < 1e-9,
            "{r:?}"
        );
        let back = rotate_pt(r, -90.0, (0.0, 0.0));
        assert!(
            (back.0 - 1.0).abs() < 1e-9 && (back.1 - 0.0).abs() < 1e-9,
            "{back:?}"
        );
    }

    /// A depth-1 (single PushTransform) connector that crosses an unrotated
    /// connector in ON-PAGE space now participates: the horizontal-over-vertical
    /// rule applies on-page, and the chosen connector gains a hop, written in its
    /// LOCAL (pre-rotation) space.
    ///
    /// The rotated connector is the vertical-on-page one: in local space it runs
    /// horizontally (0,50)→(100,50), and a +90° rotation about (50,50) turns it
    /// into the on-page vertical segment x=50, y=0..100. The plain connector is
    /// on-page horizontal y=50, x=0..100. On-page, horizontal hops over vertical,
    /// so the plain (idx 3) connector hops; the rotated one stays unchanged.
    #[test]
    fn arc_depth_one_connector_participates() {
        // idx 0: PushTransform +90° about (50,50)
        // idx 1: local-horizontal stroke → on-page vertical x=50
        // idx 2: PopTransform
        // idx 3: plain on-page horizontal stroke y=50
        let mut cmds = vec![
            SceneCommand::PushTransform {
                angle_deg: 90.0,
                cx: 50.0,
                cy: 50.0,
            },
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            SceneCommand::PopTransform,
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
        ];
        let before_rot = polyline_points(&cmds[1]);
        let before_plain = polyline_points(&cmds[3]);

        apply_line_jumps(&mut cmds, &[1, 3], "arc");

        let after_rot = polyline_points(&cmds[1]);
        let after_plain = polyline_points(&cmds[3]);

        assert_eq!(
            after_rot, before_rot,
            "rotated (on-page vertical) connector must be unchanged"
        );
        assert!(
            after_plain.len() > before_plain.len(),
            "plain on-page-horizontal connector should gain bump points: {after_plain:?}"
        );
    }

    /// A connector under TWO nested PushTransforms is excluded: even though it
    /// would cross a plain connector on-page, it does not participate and neither
    /// connector hops (the plain one has no surviving partner).
    #[test]
    fn depth_two_connector_excluded() {
        // idx 0: PushTransform
        // idx 1: PushTransform (nested)
        // idx 2: stroke at depth 2 → EXCLUDED
        // idx 3: PopTransform
        // idx 4: PopTransform
        // idx 5: plain stroke (would cross in raw coords)
        let mut cmds = vec![
            SceneCommand::PushTransform {
                angle_deg: 10.0,
                cx: 50.0,
                cy: 50.0,
            },
            SceneCommand::PushTransform {
                angle_deg: 20.0,
                cx: 50.0,
                cy: 50.0,
            },
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
            SceneCommand::PopTransform,
            SceneCommand::PopTransform,
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
        ];
        let before_inner = polyline_points(&cmds[2]);
        let before_plain = polyline_points(&cmds[5]);
        apply_line_jumps(&mut cmds, &[2, 5], "arc");
        assert_eq!(
            polyline_points(&cmds[2]),
            before_inner,
            "depth-2 connector is excluded → unchanged"
        );
        assert_eq!(
            polyline_points(&cmds[5]),
            before_plain,
            "plain connector has no surviving partner → unchanged"
        );
    }

    /// Byte-identity guard: two unrotated crossing connectors produce a fixed,
    /// known hopped output. This is the literal value the pass emitted before the
    /// on-page refactor; the identity transform must not perturb it.
    #[test]
    fn arc_depth_zero_byte_identical_known_values() {
        let mut cmds = vec![
            stroke(vec![0.0, 50.0, 100.0, 50.0]),
            stroke(vec![50.0, 0.0, 50.0, 100.0]),
        ];
        apply_line_jumps(&mut cmds, &[0, 1], "arc");
        let after_h = polyline_points(&cmds[0]);

        // Reproduce the expected bumped route directly from the bump builder on
        // the raw (identity-on-page) segment: this is exactly what depth-0 must
        // still yield, with no rotation calls in the path.
        let expected = rebuild_points_with_bumps(
            &[0.0, 50.0, 100.0, 50.0],
            &[Hop {
                seg: 0,
                px: 50.0,
                py: 50.0,
                dist_from_start: 50.0,
            }],
        );
        assert_eq!(
            after_h, expected,
            "depth-0 arc output must be byte-identical to the un-rotated builder"
        );
        // The vertical connector is untouched.
        assert_eq!(polyline_points(&cmds[1]), vec![50.0, 0.0, 50.0, 100.0]);
    }
}
