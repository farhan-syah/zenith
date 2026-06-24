//! `connector` leaf-node compilation — a semantic arrow whose endpoints are
//! derived from the resolved boxes of its `from`/`to` targets — plus the
//! orthogonal-routing and arrowhead geometry helpers it relies on.

use std::collections::BTreeMap;

use zenith_core::{ConnectorNode, Diagnostic, ResolvedToken, Style};

use crate::ir::{Paint, SceneCommand, StrokeAlign};

use super::super::RenderCtx;
use super::super::paint::resolve_property_color;
use super::super::style_prop;
use super::super::util::{resolve_property_dimension_px, rotation_degrees};
use super::poly::flat_points_centroid_center;
use super::routing;

/// Obstacle-clearance margin (px) used by `route="avoid"`: obstacles inflate by
/// this amount and the path stubs out of each box face by the same distance, so
/// the routed line keeps a small gap from every box it skirts.
const ROUTE_MARGIN: f64 = 8.0;

/// Read-only borrow + scalar context for [`compile_connector`].
///
/// Bundles the maps and the per-subtree [`RenderCtx`] so the connector compiler
/// stays under the argument-count lint without an `#[allow]`. All fields are
/// borrows/`Copy` scalars held for the duration of a single compile call.
#[derive(Clone, Copy)]
pub(in crate::compile) struct ConnectorEnv<'a> {
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    pub(in crate::compile) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    pub(in crate::compile) ctx: RenderCtx,
}

/// Which edge of a box an anchor sits on, expressed as the orientation the path
/// must leave/enter through. `Horizontal` = a left/right edge → the path leaves
/// horizontally; `Vertical` = a top/bottom edge → the path leaves vertically.
///
/// Used by orthogonal routing (Unit 3) to guarantee the first/last segment is
/// perpendicular to the box edge, so arrowheads land axis-aligned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AnchorSide {
    Horizontal,
    Vertical,
}

/// Compute the page-absolute anchor point on a `(x, y, w, h)` box, AND the
/// orientation used to leave/enter it ([`AnchorSide`]).
///
/// Anchors are a **nine-point grid**: a horizontal band (`left` / `center` /
/// `right`) optionally combined with a vertical band (`top` / `center` /
/// `bottom`) via a hyphen — e.g. `top-left`, `bottom-center`, `center-right`.
/// A bare single token names the corresponding edge mid-point (`top` =
/// `top-center`, `left` = `center-left`); `center` is the box center. `mid` and
/// `middle` are accepted synonyms for `center`. The five pre-grid names
/// (`top`/`bottom`/`left`/`right`/`center`) resolve identically, so existing
/// output is unchanged.
///
/// `"auto"` (the default for an absent / unrecognized anchor) chooses the edge
/// by the dominant axis toward `toward` (the OTHER box's center): a larger
/// horizontal delta picks left/right (`Horizontal`), otherwise top/bottom
/// (`Vertical`).
fn resolve_anchor(
    boxr: (f64, f64, f64, f64),
    anchor: &str,
    toward: (f64, f64),
) -> ((f64, f64), AnchorSide) {
    let (x, y, w, h) = boxr;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    if let Some(resolved) = grid_anchor(anchor, boxr) {
        return resolved;
    }

    // "auto" and any unrecognized value: dominant-axis edge toward `toward`.
    let dx = toward.0 - cx;
    let dy = toward.1 - cy;
    if dx.abs() >= dy.abs() {
        let pt = if dx >= 0.0 { (x + w, cy) } else { (x, cy) };
        (pt, AnchorSide::Horizontal)
    } else if dy >= 0.0 {
        ((cx, y + h), AnchorSide::Vertical)
    } else {
        ((cx, y), AnchorSide::Vertical)
    }
}

/// Resolve a nine-point grid anchor string (e.g. `top-left`, `bottom-center`,
/// `center`) to its point and leave/enter orientation. Returns `None` when the
/// string names no grid position (e.g. `auto`), so the caller falls back to
/// dominant-axis resolution. `mid`/`middle` are synonyms for `center`.
fn grid_anchor(anchor: &str, boxr: (f64, f64, f64, f64)) -> Option<((f64, f64), AnchorSide)> {
    let (x, y, w, h) = boxr;
    let (mut top, mut bottom, mut left, mut right, mut center, mut recognized) =
        (false, false, false, false, false, false);
    for part in anchor.split('-') {
        match part {
            "top" => top = true,
            "bottom" => bottom = true,
            "left" => left = true,
            "right" => right = true,
            "center" | "centre" | "mid" | "middle" => center = true,
            _ => continue,
        }
        recognized = true;
    }
    if !recognized {
        return None;
    }
    let px = if left {
        x
    } else if right {
        x + w
    } else {
        x + w / 2.0
    };
    let py = if top {
        y
    } else if bottom {
        y + h
    } else {
        y + h / 2.0
    };
    // Orientation: a pure top/bottom anchor leaves vertically; a pure left/right
    // anchor leaves horizontally; the center and the four corners default to
    // horizontal (matching the pre-grid `center` behavior).
    let vertical_only = (top || bottom) && !(left || right);
    let side = if vertical_only {
        AnchorSide::Vertical
    } else {
        AnchorSide::Horizontal
    };
    let _ = center; // `center` only affects which band is unspecified.
    Some(((px, py), side))
}

/// Build a flat right-angle (elbow) point list between two anchors, with the
/// first segment perpendicular to `f`'s edge (`fs`) and the last perpendicular to
/// `t`'s edge (`ts`). Returns 8 coords (4-point Z-route) when both anchors share
/// an orientation, or 6 coords (3-point L-corner) when they differ.
///
/// Collinear/degenerate elbows (e.g. `mx == f.0`) are left as-is — zero-length
/// sub-segments render harmlessly and are never special-cased.
fn orthogonal_route(f: (f64, f64), fs: AnchorSide, t: (f64, f64), ts: AnchorSide) -> Vec<f64> {
    match (fs, ts) {
        // Both side edges → H–V–H Z-route, elbow at the mid x.
        (AnchorSide::Horizontal, AnchorSide::Horizontal) => {
            let mx = (f.0 + t.0) / 2.0;
            vec![f.0, f.1, mx, f.1, mx, t.1, t.0, t.1]
        }
        // Both top/bottom edges → V–H–V Z-route, elbow at the mid y.
        (AnchorSide::Vertical, AnchorSide::Vertical) => {
            let my = (f.1 + t.1) / 2.0;
            vec![f.0, f.1, f.0, my, t.0, my, t.0, t.1]
        }
        // Leaves F horizontally, enters T vertically → corner at (t.0, f.1).
        (AnchorSide::Horizontal, AnchorSide::Vertical) => {
            vec![f.0, f.1, t.0, f.1, t.0, t.1]
        }
        // Leaves F vertically, enters T horizontally → corner at (f.0, t.1).
        (AnchorSide::Vertical, AnchorSide::Horizontal) => {
            vec![f.0, f.1, f.0, t.1, t.0, t.1]
        }
    }
}

/// The unit outward normal of an anchor point on its box face, used to stub the
/// `route="avoid"` path cleanly out of (and into) each box. A `Horizontal` edge
/// (left/right) leaves along ±x toward the side the anchor sits on; a `Vertical`
/// edge (top/bottom) leaves along ±y.
fn outward_dir(side: AnchorSide, pt: (f64, f64), boxr: (f64, f64, f64, f64)) -> (f64, f64) {
    let cx = boxr.0 + boxr.2 / 2.0;
    let cy = boxr.1 + boxr.3 / 2.0;
    match side {
        AnchorSide::Horizontal => {
            let sign = if pt.0 - cx >= 0.0 { 1.0 } else { -1.0 };
            (sign, 0.0)
        }
        AnchorSide::Vertical => {
            let sign = if pt.1 - cy >= 0.0 { 1.0 } else { -1.0 };
            (0.0, sign)
        }
    }
}

/// How far a self-loop bulges out from the box edge, in pixels.
const LOOP_DEPTH: f64 = 28.0;
/// The largest half-width of a self-loop's two feet on the box edge, in pixels;
/// the actual half-width is also capped at 30% of the spanning box dimension so
/// the loop stays within the edge on small boxes.
const LOOP_HALF_MAX: f64 = 25.0;

/// Pick which edge a self-loop bulges from, parsed loosely from an anchor string
/// (`top`/`bottom`/`left`/`right`); anything else (including `auto`/absent)
/// defaults to the top edge.
fn loop_side(anchor: Option<&str>) -> &'static str {
    match anchor {
        Some(a) if a.contains("bottom") => "bottom",
        Some(a) if a.contains("left") => "left",
        Some(a) if a.contains("right") => "right",
        _ => "top",
    }
}

/// Build a small rectangular self-loop off one `side` of a `(x, y, w, h)` box:
/// two feet on that edge, bulging out by [`LOOP_DEPTH`]. The path runs foot →
/// out → across → back → foot, so the last segment arrives perpendicular to the
/// edge and an end marker points back into the box.
fn self_loop_path(boxr: (f64, f64, f64, f64), side: &str) -> Vec<f64> {
    let (x, y, w, h) = boxr;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let d = LOOP_DEPTH;
    match side {
        "bottom" => {
            let half = (w * 0.3).min(LOOP_HALF_MAX);
            let yb = y + h;
            vec![
                cx - half,
                yb,
                cx - half,
                yb + d,
                cx + half,
                yb + d,
                cx + half,
                yb,
            ]
        }
        "left" => {
            let half = (h * 0.3).min(LOOP_HALF_MAX);
            vec![
                x,
                cy - half,
                x - d,
                cy - half,
                x - d,
                cy + half,
                x,
                cy + half,
            ]
        }
        "right" => {
            let half = (h * 0.3).min(LOOP_HALF_MAX);
            let xr = x + w;
            vec![
                xr,
                cy - half,
                xr + d,
                cy - half,
                xr + d,
                cy + half,
                xr,
                cy + half,
            ]
        }
        _ => {
            let half = (w * 0.3).min(LOOP_HALF_MAX);
            vec![
                cx - half,
                y,
                cx - half,
                y - d,
                cx + half,
                y - d,
                cx + half,
                y,
            ]
        }
    }
}

/// Bounds-safe read of the `i`-th `(x, y)` point from a flat `[x0,y0,x1,y1,…]`
/// list. Returns `None` if the point is out of range (no panic, no indexing).
fn point_at(pts: &[f64], i: usize) -> Option<(f64, f64)> {
    let x = pts.get(i * 2)?;
    let y = pts.get(i * 2 + 1)?;
    Some((*x, *y))
}

/// Compile a `connector` leaf node — a semantic arrow whose endpoints are
/// DERIVED at compile time from the resolved boxes of its `from`/`to` targets.
///
/// Unit 1 draws a STRAIGHT 2-point line between the resolved edge anchors. Unit 2
/// adds filled-triangle arrowheads at the `to` end (`marker-end="arrow"`) and/or
/// the `from` end (`marker-start="arrow"`), in the line's stroke color and inside
/// the same rotation bracket. Unit 3 adds `route="orthogonal"` — a right-angle
/// elbow path (4-point Z-route or 3-point L-corner) instead of the straight
/// diagonal — and orients arrowheads along the actual first/last routed segment
/// so they land axis-aligned. When `from`/`to` is absent, or a target box is
/// not in `node_boxes` (unresolved), nothing is emitted (graceful — validation
/// warned); markers follow the same guards, so a skipped line skips its heads.
pub(in crate::compile) fn compile_connector(
    connector: &ConnectorNode,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    env: ConnectorEnv,
) {
    let ConnectorEnv {
        resolved,
        style_map,
        node_boxes,
        ctx,
    } = env;

    if connector.visible == Some(false) {
        return;
    }

    // Both endpoints are required to route; absent → emit nothing (validation
    // already warned via `connector.missing_target`).
    let (Some(from_id), Some(to_id)) = (connector.from.as_deref(), connector.to.as_deref()) else {
        return;
    };

    // Look up the resolved page-absolute boxes of both targets. A missing box
    // (unresolved id, or a target with no authored geometry) → emit nothing.
    let (Some(from_box), Some(to_box)) = (node_boxes.get(from_id), node_boxes.get(to_id)) else {
        return;
    };
    let from_box = *from_box;
    let to_box = *to_box;

    let from_center = (from_box.0 + from_box.2 / 2.0, from_box.1 + from_box.3 / 2.0);
    let to_center = (to_box.0 + to_box.2 / 2.0, to_box.1 + to_box.3 / 2.0);

    // Resolve anchors: each end aims toward the OTHER box's center for "auto".
    let from_anchor = connector.from_anchor.as_deref().unwrap_or("auto");
    let to_anchor = connector.to_anchor.as_deref().unwrap_or("auto");
    let (f_pt, f_side) = resolve_anchor(from_box, from_anchor, to_center);
    let (t_pt, t_side) = resolve_anchor(to_box, to_anchor, from_center);

    // A self-loop (`from` and `to` name the same node) cannot be a line between
    // two distinct points — it routes as a small rectangular loop off one edge of
    // the box (the side picked from the `from`/`to` anchor, defaulting to the
    // top), with the marker landing back on that edge.
    //
    // Otherwise route selection applies: `orthogonal` builds a right-angle elbow
    // path; `avoid` runs an obstacle-avoiding orthogonal router around the other
    // boxes (and falls back to the plain elbow when no clear path exists);
    // everything else (None / "straight" / unknown — validation already warned)
    // is the straight 2-point line, byte-identical to Unit 1/2.
    let flat_points = if from_id == to_id {
        let side = loop_side(
            connector
                .from_anchor
                .as_deref()
                .or(connector.to_anchor.as_deref()),
        );
        self_loop_path(from_box, side)
    } else {
        match connector.route.as_deref() {
            Some("orthogonal") => orthogonal_route(f_pt, f_side, t_pt, t_side),
            Some("avoid") => {
                let obstacles: Vec<(f64, f64, f64, f64)> = node_boxes
                    .iter()
                    .filter(|(id, _)| id.as_str() != from_id && id.as_str() != to_id)
                    .map(|(_, b)| *b)
                    .collect();
                let f_out = outward_dir(f_side, f_pt, from_box);
                let t_out = outward_dir(t_side, t_pt, to_box);
                routing::route_orthogonal_avoiding(
                    f_pt,
                    f_out,
                    t_pt,
                    t_out,
                    &obstacles,
                    ROUTE_MARGIN,
                )
                .unwrap_or_else(|| orthogonal_route(f_pt, f_side, t_pt, t_side))
            }
            _ => vec![f_pt.0, f_pt.1, t_pt.0, t_pt.1],
        }
    };

    // STROKE — only emit when a stroke color is present (mirrors polyline: no
    // stroke token → nothing drawn). Style cascade for stroke + stroke-width.
    let node_opacity = connector.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let stroke_prop = connector
        .stroke
        .as_ref()
        .or_else(|| style_prop(&connector.style, style_map, "stroke"));
    let Some(stroke_prop) = stroke_prop else {
        return;
    };
    let Some(mut color) = resolve_property_color(stroke_prop, resolved, diagnostics, &connector.id)
    else {
        return;
    };
    color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

    let sw = connector
        .stroke_width
        .clone()
        .or_else(|| style_prop(&connector.style, style_map, "stroke-width").cloned());
    let stroke_width = resolve_property_dimension_px(sw.as_ref(), resolved, 1.0);

    // Rotation bracket: rotate about the line's bbox center, matching polyline.
    let rot = rotation_degrees(connector.rotate.as_ref());
    if let Some(angle) = rot {
        let (cx, cy) = flat_points_centroid_center(&flat_points);
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Derive marker endpoints from the ACTUAL routed path BEFORE the Vec is moved
    // into the stroke command, so orthogonal arrowheads orient along the real
    // last/first segment (axis-aligned), not the global anchor line. For a
    // 2-point straight line these reduce to today's (tx,ty)/(fx,fy) endpoints.
    let n = flat_points.len() / 2;
    let end_tip = point_at(&flat_points, n.saturating_sub(1));
    let end_from = point_at(&flat_points, n.saturating_sub(2));
    let start_tip = point_at(&flat_points, 0);
    let start_from = point_at(&flat_points, 1);

    commands.push(SceneCommand::StrokePolyline {
        points: flat_points,
        color,
        stroke_width,
        closed: false,
        align: StrokeAlign::Center,
        fill_even_odd: false,
    });

    // ARROWHEAD MARKERS (Unit 2/3) — filled triangles in the SAME stroke color,
    // INSIDE the rotation bracket so they rotate with the line. The tip sits
    // exactly on the path endpoint; the base extends back along the adjacent
    // segment. Fewer than 2 points → endpoints are `None` and markers are skipped.
    {
        let mut emit_head = |tip, from_pt| {
            if let Some(points) = arrowhead_points(tip, from_pt, stroke_width) {
                commands.push(SceneCommand::FillPolygon {
                    points,
                    paint: Paint::solid(color),
                    even_odd: false,
                });
            }
        };
        if connector.marker_end.as_deref() == Some("arrow")
            && let (Some(tip), Some(from_pt)) = (end_tip, end_from)
        {
            emit_head(tip, from_pt);
        }
        if connector.marker_start.as_deref() == Some("arrow")
            && let (Some(tip), Some(from_pt)) = (start_tip, start_from)
        {
            emit_head(tip, from_pt);
        }
    }

    if rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

/// Build a filled-triangle arrowhead whose tip sits at `tip`, arriving along the
/// segment from `from_pt` → `tip` (so the head points in the direction of travel
/// into `tip`). Returns a flat `[x0,y0, x1,y1, x2,y2]` (tip, left base, right
/// base), or `None` if the segment is degenerate (endpoints coincide) and the
/// head cannot be oriented. Size scales with `stroke_width`, clamped so thin
/// strokes still get a visible head.
fn arrowhead_points(tip: (f64, f64), from_pt: (f64, f64), stroke_width: f64) -> Option<Vec<f64>> {
    let vx = tip.0 - from_pt.0;
    let vy = tip.1 - from_pt.1;
    let len = (vx * vx + vy * vy).sqrt();
    if len < 1e-6 {
        return None;
    }
    let (ux, uy) = (vx / len, vy / len);
    let (px, py) = (-uy, ux);
    // head_len: 3.5× stroke; half_w: 2.0× stroke — clamped so hairline strokes
    // still produce a visible 7px × 8px head.
    let head_len = (stroke_width * 3.5).max(7.0);
    let half_w = (stroke_width * 2.0).max(4.0);
    let base_cx = tip.0 - ux * head_len;
    let base_cy = tip.1 - uy * head_len;
    let left_x = base_cx + px * half_w;
    let left_y = base_cy + py * half_w;
    let right_x = base_cx - px * half_w;
    let right_y = base_cy - py * half_w;
    Some(vec![tip.0, tip.1, left_x, left_y, right_x, right_y])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_side_parses_edges_and_defaults_to_top() {
        assert_eq!(loop_side(Some("bottom-center")), "bottom");
        assert_eq!(loop_side(Some("center-left")), "left");
        assert_eq!(loop_side(Some("right")), "right");
        assert_eq!(loop_side(Some("top")), "top");
        assert_eq!(loop_side(Some("auto")), "top");
        assert_eq!(loop_side(None), "top");
    }

    #[test]
    fn self_loop_top_bulges_above_the_box() {
        // 120×60 box at (60,110): top edge y=110, center x=120.
        let pts = self_loop_path((60.0, 110.0, 120.0, 60.0), "top");
        // Four points: foot, out, across, foot — all above the top edge.
        assert_eq!(pts.len(), 8);
        let half = (120.0_f64 * 0.3).min(LOOP_HALF_MAX);
        assert_eq!(
            pts,
            vec![
                120.0 - half,
                110.0,
                120.0 - half,
                110.0 - LOOP_DEPTH,
                120.0 + half,
                110.0 - LOOP_DEPTH,
                120.0 + half,
                110.0,
            ]
        );
        // The two feet sit on the top edge; the bulge is strictly above it.
        assert!(pts[3] < 110.0 && pts[5] < 110.0);
    }

    #[test]
    fn self_loop_right_bulges_past_the_right_edge() {
        // 120×60 box at (250,110): right edge x=370, center y=140.
        let pts = self_loop_path((250.0, 110.0, 120.0, 60.0), "right");
        assert_eq!(pts.len(), 8);
        // Feet on the right edge (x=370); bulge extends past it.
        assert_eq!(pts[0], 370.0);
        assert_eq!(pts[6], 370.0);
        assert!(pts[2] > 370.0 && pts[4] > 370.0);
    }

    #[test]
    fn self_loop_is_deterministic() {
        let a = self_loop_path((10.0, 20.0, 80.0, 40.0), "bottom");
        let b = self_loop_path((10.0, 20.0, 80.0, 40.0), "bottom");
        assert_eq!(a, b);
    }
}
