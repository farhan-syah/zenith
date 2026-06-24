//! Obstacle-avoiding orthogonal connector routing.
//!
//! [`route_orthogonal_avoiding`] finds an axis-aligned (right-angle) path from a
//! start point to an end point that never enters the interior of any obstacle
//! box. It builds an orthogonal-visibility (Hanan) grid from the obstacle edges
//! and the endpoints, then runs a deterministic A* whose state carries the
//! arrival orientation so the search minimizes both length and the number of
//! bends. When no obstacle-free path exists the caller falls back to a simple
//! elbow.
//!
//! Everything here is pure: integer/`f64` geometry only, ordered `BTreeMap`
//! state, a `BinaryHeap` frontier with a total deterministic ordering, no
//! hashing, no time, no randomness — same inputs always produce the same bytes.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

/// Extra cost charged whenever a step changes orientation, so the search prefers
/// straighter paths with fewer corners over equal-length zig-zags.
const BEND_PENALTY: f64 = 24.0;

/// Coordinate-comparison tolerance for dedup and strict-interior tests.
const EPS: f64 = 1e-6;

/// A node in the A* search: a grid vertex `(xi, yi)` plus the orientation used
/// to ARRIVE at it (`0` = a horizontal move, `1` = a vertical move). Carrying
/// the arrival orientation lets the cost charge a bend only on a real turn.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Node {
    xi: usize,
    yi: usize,
    dir: u8,
}

/// A frontier entry ordered as a MIN-heap on `f = g + h`, with deterministic
/// tie-breaks on `(xi, yi, dir)`. `BinaryHeap` is a max-heap, so every
/// comparison is reversed to pop the smallest `f` (and smallest node on ties).
struct Frontier {
    f: f64,
    node: Node,
}

impl PartialEq for Frontier {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Frontier {}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Frontier {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse so the BinaryHeap (max-heap) yields the minimum `f` first;
        // costs are always finite, so `total_cmp` is a strict total order.
        // Tie-break by node identity (also reversed) for full determinism.
        other
            .f
            .total_cmp(&self.f)
            .then_with(|| other.node.cmp(&self.node))
    }
}

/// An obstacle inflated by `margin`, stored as its four edges
/// `(left, top, right, bottom)`.
#[derive(Clone, Copy)]
struct Rect {
    l: f64,
    t: f64,
    r: f64,
    b: f64,
}

/// `true` when `v` lies strictly inside the open interval `(lo, hi)` by more
/// than `EPS`. Running exactly along an edge is therefore NOT inside.
fn inside(v: f64, lo: f64, hi: f64) -> bool {
    v > lo + EPS && v < hi - EPS
}

/// A grid vertex is blocked iff it sits strictly inside some obstacle.
fn vertex_blocked(x: f64, y: f64, obstacles: &[Rect]) -> bool {
    obstacles
        .iter()
        .any(|o| inside(x, o.l, o.r) && inside(y, o.t, o.b))
}

/// A horizontal edge at height `y` spanning `x ∈ [x0, x1]` (with `x0 < x1`) is
/// blocked iff it passes strictly through some obstacle's interior — i.e. the
/// obstacle straddles `y` and the edge's x-overlap with the obstacle has
/// positive length after shrinking by `EPS`.
fn h_edge_blocked(y: f64, x0: f64, x1: f64, obstacles: &[Rect]) -> bool {
    obstacles
        .iter()
        .any(|o| inside(y, o.t, o.b) && (x0.max(o.l) + EPS) < (x1.min(o.r) - EPS))
}

/// Vertical counterpart of [`h_edge_blocked`]: an edge at `x` spanning
/// `y ∈ [y0, y1]` (with `y0 < y1`).
fn v_edge_blocked(x: f64, y0: f64, y1: f64, obstacles: &[Rect]) -> bool {
    obstacles
        .iter()
        .any(|o| inside(x, o.l, o.r) && (y0.max(o.t) + EPS) < (y1.min(o.b) - EPS))
}

/// Sort with `total_cmp` and drop near-duplicates within `EPS`, keeping the
/// first of each cluster. The input is consumed and a deduped vector returned.
fn sort_unique(mut vals: Vec<f64>) -> Vec<f64> {
    vals.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::with_capacity(vals.len());
    for v in vals {
        match out.last() {
            Some(&last) if (v - last).abs() <= EPS => {}
            _ => out.push(v),
        }
    }
    out
}

/// Locate the grid index of `target` in a sorted-unique coordinate axis,
/// matching within `EPS`. Returns `None` if no axis value matches.
fn index_of(axis: &[f64], target: f64) -> Option<usize> {
    axis.iter().position(|&v| (v - target).abs() <= EPS)
}

/// Route an orthogonal path from `start` to `end` that avoids every obstacle's
/// interior. `start_out`/`end_out` are unit outward directions (axis-aligned:
/// (±1,0) or (0,±1)) perpendicular to each box face, so the path leaves/enters
/// the box cleanly. Returns the flat `[x0,y0,x1,y1,…]` polyline, or `None` when
/// no obstacle-free path exists (caller falls back to a simple elbow).
pub(in crate::compile) fn route_orthogonal_avoiding(
    start: (f64, f64),
    start_out: (f64, f64),
    end: (f64, f64),
    end_out: (f64, f64),
    obstacles: &[(f64, f64, f64, f64)],
    margin: f64,
) -> Option<Vec<f64>> {
    // Inflate every obstacle by `margin` to its four edges.
    let inflated: Vec<Rect> = obstacles
        .iter()
        .map(|&(x, y, w, h)| Rect {
            l: x - margin,
            t: y - margin,
            r: x + w + margin,
            b: y + h + margin,
        })
        .collect();

    // Stub the endpoints out by one margin along their outward normals; the grid
    // search runs between the two stub points.
    let start_stub = (
        start.0 + start_out.0 * margin,
        start.1 + start_out.1 * margin,
    );
    let end_stub = (end.0 + end_out.0 * margin, end.1 + end_out.1 * margin);

    // Build the orthogonal-visibility grid axes from every inflated edge plus the
    // two stub coordinates.
    let mut raw_xs: Vec<f64> = Vec::with_capacity(inflated.len() * 2 + 2);
    let mut raw_ys: Vec<f64> = Vec::with_capacity(inflated.len() * 2 + 2);
    for rct in &inflated {
        raw_xs.push(rct.l);
        raw_xs.push(rct.r);
        raw_ys.push(rct.t);
        raw_ys.push(rct.b);
    }
    raw_xs.push(start_stub.0);
    raw_xs.push(end_stub.0);
    raw_ys.push(start_stub.1);
    raw_ys.push(end_stub.1);
    let xs = sort_unique(raw_xs);
    let ys = sort_unique(raw_ys);

    // Resolve the stub vertices into grid indices.
    let start_xi = index_of(&xs, start_stub.0)?;
    let start_yi = index_of(&ys, start_stub.1)?;
    let end_xi = index_of(&xs, end_stub.0)?;
    let end_yi = index_of(&ys, end_stub.1)?;

    // If both stubs land on the same vertex there is no grid path to search; the
    // route is just the two stubs (collapsed by simplification below).
    if start_xi == end_xi && start_yi == end_yi {
        let raw = vec![start.0, start.1, start_stub.0, start_stub.1, end.0, end.1];
        return Some(simplify_collinear(raw));
    }

    // A blocked stub vertex means the endpoint is buried in an obstacle: no path.
    let start_pt = (*xs.get(start_xi)?, *ys.get(start_yi)?);
    if vertex_blocked(start_pt.0, start_pt.1, &inflated) {
        return None;
    }

    // Seed orientation: a horizontal outward normal arrives horizontally (dir 0).
    let seed_dir: u8 = if start_out.0 != 0.0 { 0 } else { 1 };
    let goal_h = end_stub.0;
    let goal_v = end_stub.1;

    let start_node = Node {
        xi: start_xi,
        yi: start_yi,
        dir: seed_dir,
    };

    let mut g_score: BTreeMap<Node, f64> = BTreeMap::new();
    let mut came_from: BTreeMap<Node, Node> = BTreeMap::new();
    g_score.insert(start_node, 0.0);

    let mut frontier: BinaryHeap<Frontier> = BinaryHeap::new();
    let h0 = (start_pt.0 - goal_h).abs() + (start_pt.1 - goal_v).abs();
    frontier.push(Frontier {
        f: h0,
        node: start_node,
    });

    let mut goal_node: Option<Node> = None;

    while let Some(Frontier { f, node }) = frontier.pop() {
        let Some(&g) = g_score.get(&node) else {
            continue;
        };
        let Some(&px) = xs.get(node.xi) else {
            continue;
        };
        let Some(&py) = ys.get(node.yi) else {
            continue;
        };
        // Skip stale heap entries whose `f` no longer matches the best `g + h`.
        let h = (px - goal_h).abs() + (py - goal_v).abs();
        if (f - (g + h)).abs() > EPS {
            continue;
        }

        // Goal reached: any orientation arriving at the end stub vertex wins.
        if node.xi == end_xi && node.yi == end_yi {
            goal_node = Some(node);
            break;
        }

        // Expand the four axis-aligned neighbors.
        let mut consider = |nxi: usize, nyi: usize, move_dir: u8| {
            let (Some(&nx), Some(&ny)) = (xs.get(nxi), ys.get(nyi)) else {
                return;
            };
            if vertex_blocked(nx, ny, &inflated) {
                return;
            }
            let edge_clear = if move_dir == 0 {
                let (x0, x1) = if px < nx { (px, nx) } else { (nx, px) };
                !h_edge_blocked(py, x0, x1, &inflated)
            } else {
                let (y0, y1) = if py < ny { (py, ny) } else { (ny, py) };
                !v_edge_blocked(px, y0, y1, &inflated)
            };
            if !edge_clear {
                return;
            }
            let seg_len = if move_dir == 0 {
                (nx - px).abs()
            } else {
                (ny - py).abs()
            };
            let bend = if move_dir != node.dir {
                BEND_PENALTY
            } else {
                0.0
            };
            let tentative = g + seg_len + bend;
            let neighbor = Node {
                xi: nxi,
                yi: nyi,
                dir: move_dir,
            };
            let improved = match g_score.get(&neighbor) {
                Some(&existing) => tentative + EPS < existing,
                None => true,
            };
            if improved {
                g_score.insert(neighbor, tentative);
                came_from.insert(neighbor, node);
                let nh = (nx - goal_h).abs() + (ny - goal_v).abs();
                frontier.push(Frontier {
                    f: tentative + nh,
                    node: neighbor,
                });
            }
        };

        if node.xi + 1 < xs.len() {
            consider(node.xi + 1, node.yi, 0);
        }
        if node.xi > 0 {
            consider(node.xi - 1, node.yi, 0);
        }
        if node.yi + 1 < ys.len() {
            consider(node.xi, node.yi + 1, 1);
        }
        if node.yi > 0 {
            consider(node.xi, node.yi - 1, 1);
        }
    }

    let goal = goal_node?;

    // Reconstruct the grid path from the goal back to the start stub.
    let mut grid_rev: Vec<(f64, f64)> = Vec::new();
    let mut cur = goal;
    loop {
        let (Some(&gx), Some(&gy)) = (xs.get(cur.xi), ys.get(cur.yi)) else {
            return None;
        };
        grid_rev.push((gx, gy));
        if cur.xi == start_node.xi && cur.yi == start_node.yi {
            break;
        }
        cur = *came_from.get(&cur)?;
    }
    grid_rev.reverse();

    // Assemble `start → [grid path] → end` as a flat coordinate list, then drop
    // collinear interior points.
    let mut raw: Vec<f64> = Vec::with_capacity((grid_rev.len() + 2) * 2);
    raw.push(start.0);
    raw.push(start.1);
    for (gx, gy) in grid_rev {
        raw.push(gx);
        raw.push(gy);
    }
    raw.push(end.0);
    raw.push(end.1);

    Some(simplify_collinear(raw))
}

/// Drop every interior point whose two neighbors share its x (all three x equal)
/// or share its y, in one deterministic forward pass. Endpoints are always kept.
fn simplify_collinear(pts: Vec<f64>) -> Vec<f64> {
    let n = pts.len() / 2;
    if n < 3 {
        return pts;
    }
    let mut out: Vec<f64> = Vec::with_capacity(pts.len());
    // Always keep the first point.
    if let (Some(&x0), Some(&y0)) = (pts.first(), pts.get(1)) {
        out.push(x0);
        out.push(y0);
    }
    for i in 1..n.saturating_sub(1) {
        let (Some(&px), Some(&py)) = (pts.get((i - 1) * 2), pts.get((i - 1) * 2 + 1)) else {
            continue;
        };
        let (Some(&cx), Some(&cy)) = (pts.get(i * 2), pts.get(i * 2 + 1)) else {
            continue;
        };
        let (Some(&nx), Some(&ny)) = (pts.get((i + 1) * 2), pts.get((i + 1) * 2 + 1)) else {
            continue;
        };
        let collinear_x = (px - cx).abs() <= EPS && (cx - nx).abs() <= EPS;
        let collinear_y = (py - cy).abs() <= EPS && (cy - ny).abs() <= EPS;
        if collinear_x || collinear_y {
            continue;
        }
        out.push(cx);
        out.push(cy);
    }
    // Always keep the last point.
    if let (Some(&xl), Some(&yl)) = (pts.get((n - 1) * 2), pts.get((n - 1) * 2 + 1)) {
        out.push(xl);
        out.push(yl);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original (un-inflated) interior of a `(x, y, w, h)` box.
    fn box_interior(b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        (b.0, b.1, b.0 + b.2, b.1 + b.3)
    }

    /// Assert no segment of the flat polyline passes through the strict interior
    /// of `b`. A segment crosses iff some point along it lies strictly inside.
    fn assert_no_crossing(pts: &[f64], b: (f64, f64, f64, f64)) {
        let (l, t, r, bot) = box_interior(b);
        let n = pts.len() / 2;
        for i in 0..n.saturating_sub(1) {
            let x0 = pts[i * 2];
            let y0 = pts[i * 2 + 1];
            let x1 = pts[(i + 1) * 2];
            let y1 = pts[(i + 1) * 2 + 1];
            if (y0 - y1).abs() <= EPS {
                // Horizontal segment at y0.
                if y0 > t + EPS && y0 < bot - EPS {
                    let (xa, xb) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
                    let ov_lo = xa.max(l);
                    let ov_hi = xb.min(r);
                    assert!(
                        ov_lo + EPS >= ov_hi - EPS,
                        "horizontal segment {:?}->{:?} crosses interior of {b:?}",
                        (x0, y0),
                        (x1, y1)
                    );
                }
            } else if (x0 - x1).abs() <= EPS {
                // Vertical segment at x0.
                if x0 > l + EPS && x0 < r - EPS {
                    let (ya, yb) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
                    let ov_lo = ya.max(t);
                    let ov_hi = yb.min(bot);
                    assert!(
                        ov_lo + EPS >= ov_hi - EPS,
                        "vertical segment {:?}->{:?} crosses interior of {b:?}",
                        (x0, y0),
                        (x1, y1)
                    );
                }
            }
        }
    }

    #[test]
    fn no_obstacles_returns_clean_path() {
        let start = (140.0, 80.0);
        let end = (300.0, 100.0);
        let route = route_orthogonal_avoiding(start, (1.0, 0.0), end, (-1.0, 0.0), &[], 8.0)
            .expect("empty obstacle set must route");
        assert!(route.len() >= 4, "need at least a start and end point");
        assert_eq!(route[0], start.0);
        assert_eq!(route[1], start.1);
        let n = route.len();
        assert_eq!(route[n - 2], end.0);
        assert_eq!(route[n - 1], end.1);
    }

    #[test]
    fn obstacle_between_endpoints_is_avoided() {
        let start = (140.0, 100.0);
        let end = (460.0, 100.0);
        // A box squarely on the straight line between the two endpoints.
        let obstacle = (260.0, 60.0, 80.0, 80.0);
        let route =
            route_orthogonal_avoiding(start, (1.0, 0.0), end, (-1.0, 0.0), &[obstacle], 8.0)
                .expect("a detour around a single box must exist");
        assert_no_crossing(&route, obstacle);
        assert_eq!(route[0], start.0);
        assert_eq!(route[1], start.1);
        let n = route.len();
        assert_eq!(route[n - 2], end.0);
        assert_eq!(route[n - 1], end.1);
    }

    #[test]
    fn deterministic_identical_calls() {
        let start = (140.0, 100.0);
        let end = (460.0, 100.0);
        let obstacle = (260.0, 60.0, 80.0, 80.0);
        let a = route_orthogonal_avoiding(start, (1.0, 0.0), end, (-1.0, 0.0), &[obstacle], 8.0);
        let b = route_orthogonal_avoiding(start, (1.0, 0.0), end, (-1.0, 0.0), &[obstacle], 8.0);
        assert_eq!(a, b);
    }

    #[test]
    fn fully_enclosed_start_returns_none() {
        let start = (100.0, 100.0);
        let end = (400.0, 100.0);
        // A ring of four boxes surrounding the start point so every exit edge
        // is blocked.
        let obstacles = [
            (60.0, 40.0, 80.0, 20.0),   // top wall
            (60.0, 140.0, 80.0, 20.0),  // bottom wall
            (40.0, 40.0, 20.0, 120.0),  // left wall
            (140.0, 40.0, 20.0, 120.0), // right wall
        ];
        let route =
            route_orthogonal_avoiding(start, (0.0, -1.0), end, (-1.0, 0.0), &obstacles, 8.0);
        assert!(
            route.is_none(),
            "an enclosed start must not route: {route:?}"
        );
    }
}
