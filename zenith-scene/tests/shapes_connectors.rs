mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::{SceneCommand, StrokeAlign};

// ── connector node (U1): straight line between resolved edge anchors ──────────
//
// A `connector` declares `from`/`to` target ids and, at compile time, resolves
// those nodes' boxes to draw a STRAIGHT 2-point line between anchor points on
// their edges. U1 = straight line, no arrowhead markers, no orthogonal routing.

/// Collect the first `StrokePolyline`'s flat points, or panic.
fn first_stroke_polyline_points(cmds: &[SceneCommand]) -> Vec<f64> {
    cmds.iter()
        .find_map(|c| match c {
            SceneCommand::StrokePolyline { points, .. } => Some(points.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected a StrokePolyline; got: {cmds:?}"))
}

/// Two rects laid out horizontally with a connector between them, default
/// (auto) anchors. The from-box center is left of the to-box center, so auto
/// picks the right edge of `a` and the left edge of `b`.
#[test]
fn connector_auto_anchors_between_horizontal_boxes() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#dbeafe"
token id="color.line" type="color" value="#1e3a8a"
token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 fill=(token)"color.fill"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 fill=(token)"color.fill"
  connector id="c1" from="a" to="b" stroke=(token)"color.line" stroke-width=(token)"size.stroke"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // a: x=40 y=40 w=100 h=80  → center (90,80),  right-mid edge = (140, 80)
    // b: x=300 y=60 w=100 h=80 → center (350,100), left-mid edge = (300, 100)
    let pts = first_stroke_polyline_points(cmds);
    assert_eq!(
        pts,
        vec![140.0, 80.0, 300.0, 100.0],
        "auto anchors must be a's right-mid and b's left-mid; got {pts:?}"
    );

    // U1: straight 2-point open line, centered stroke.
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            SceneCommand::StrokePolyline { points, closed: false, align: StrokeAlign::Center, .. }
                if points.len() == 4
        )),
        "connector must emit a straight open StrokePolyline; got {cmds:?}"
    );
}

/// Explicit `from-anchor="right"` / `to-anchor="left"` anchors are honored
/// verbatim (no auto resolution).
#[test]
fn connector_explicit_anchors_are_honored() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="right" to-anchor="left" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // a right-mid = (140, 80); b left-mid = (300, 100).
    let pts = first_stroke_polyline_points(cmds);
    assert_eq!(pts, vec![140.0, 80.0, 300.0, 100.0]);
}

/// Nine-point grid anchors resolve to box corners: `from-anchor="bottom-right"`
/// / `to-anchor="top-left"` attach at those exact corners.
#[test]
fn connector_nine_point_corner_anchors() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="bottom-right" to-anchor="top-left" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    // a bottom-right = (140, 120); b top-left = (300, 60).
    let pts = first_stroke_polyline_points(cmds);
    assert_eq!(pts, vec![140.0, 120.0, 300.0, 60.0]);
}

/// `mid` is a synonym for `center`, and a bare edge name is that edge's
/// mid-point: `from-anchor="mid-right"` = right-mid, `to-anchor="top"` =
/// top-center.
#[test]
fn connector_anchor_synonyms_and_edge_midpoints() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="mid-right" to-anchor="top" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    // a mid-right = (140, 80); b top-center = (350, 60).
    let pts = first_stroke_polyline_points(cmds);
    assert_eq!(pts, vec![140.0, 80.0, 350.0, 60.0]);
}

/// A connector to a MISSING target emits no StrokePolyline (graceful skip).
#[test]
fn connector_missing_target_emits_nothing() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="ghost" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::StrokePolyline { .. })),
        "connector to a missing target must emit no StrokePolyline; got {cmds:?}"
    );
}

/// A connector reroutes when its target box changes: compiling two documents
/// that differ only in the `to` rect's position yields different endpoints.
#[test]
fn connector_reroutes_when_target_moves() {
    let doc_src = |to_x: u32| {
        format!(
            r##"zenith version=1 {{
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {{
token id="color.line" type="color" value="#1e3a8a"
  }}
  styles {{}}
  document id="doc.cn" title="CN" {{
page id="page.cn" w=(px)640 h=(px)360 {{
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px){to_x} y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="right" to-anchor="left" stroke=(token)"color.line"
}}
  }}
}}
"##
        )
    };

    let r1 = compile(&parse(&doc_src(300)), &default_provider());
    let r2 = compile(&parse(&doc_src(420)), &default_provider());

    let p1 = first_stroke_polyline_points(&r1.scene.commands);
    let p2 = first_stroke_polyline_points(&r2.scene.commands);

    // The `to` (left-edge) endpoint must move with the target rect.
    assert_eq!(p1[2], 300.0, "first layout: b left-mid x = 300");
    assert_eq!(p2[2], 420.0, "moved layout: b left-mid x = 420");
    assert_ne!(
        p1, p2,
        "connector endpoints must change when the target moves"
    );
}

// ── connector node (U2): arrowhead markers (marker-start / marker-end) ────────
//
// `marker-end="arrow"` adds a filled-triangle head whose tip sits exactly on the
// `to` anchor; `marker-start="arrow"` does the same at the `from` anchor. The head
// reuses the line's stroke color. Default (no marker) emits only the line.

/// Collect every `FillPolygon`'s flat points, in command order.
fn all_fill_polygon_points(cmds: &[SceneCommand]) -> Vec<Vec<f64>> {
    cmds.iter()
        .filter_map(|c| match c {
            SceneCommand::FillPolygon { points, .. } => Some(points.clone()),
            _ => None,
        })
        .collect()
}

/// `marker-end="arrow"` → one StrokePolyline (the line) PLUS one FillPolygon (the
/// arrowhead): 3 vertices (6 coords), with the tip sitting on the `to` anchor.
#[test]
fn connector_marker_end_emits_arrowhead_at_to_anchor() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="right" to-anchor="left" stroke=(token)"color.line" marker-end="arrow"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // The line is still emitted.
    let line = first_stroke_polyline_points(cmds);
    assert_eq!(line, vec![140.0, 80.0, 300.0, 100.0]);

    // Exactly one arrowhead, a 3-vertex triangle.
    let heads = all_fill_polygon_points(cmds);
    assert_eq!(
        heads.len(),
        1,
        "marker-end must emit exactly one FillPolygon; got {cmds:?}"
    );
    let head = &heads[0];
    assert_eq!(
        head.len(),
        6,
        "arrowhead must be a 3-point triangle (6 coords)"
    );

    // The tip sits on the `to` anchor (300, 100): one vertex must equal it.
    let to_anchor = (300.0, 100.0);
    let has_tip = head
        .chunks_exact(2)
        .any(|p| (p[0] - to_anchor.0).abs() < 1e-9 && (p[1] - to_anchor.1).abs() < 1e-9);
    assert!(
        has_tip,
        "an arrowhead vertex must equal the to anchor; got {head:?}"
    );

    // Horizontal left→right travel: the tip is the rightmost vertex.
    let max_x = head
        .chunks_exact(2)
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x - to_anchor.0).abs() < 1e-9,
        "head must point rightward: tip x is the max x; got {head:?}"
    );
}

/// `marker-start` AND `marker-end` both "arrow" → the line PLUS TWO FillPolygons,
/// one tip on the `from` anchor and one tip on the `to` anchor.
#[test]
fn connector_both_markers_emit_two_arrowheads() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" from-anchor="right" to-anchor="left" stroke=(token)"color.line" marker-start="arrow" marker-end="arrow"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    assert_eq!(
        first_stroke_polyline_points(cmds),
        vec![140.0, 80.0, 300.0, 100.0]
    );

    let heads = all_fill_polygon_points(cmds);
    assert_eq!(
        heads.len(),
        2,
        "both markers must emit two FillPolygons; got {cmds:?}"
    );

    let from_anchor = (140.0, 80.0);
    let to_anchor = (300.0, 100.0);
    let tip_on = |head: &Vec<f64>, t: (f64, f64)| {
        head.chunks_exact(2)
            .any(|p| (p[0] - t.0).abs() < 1e-9 && (p[1] - t.1).abs() < 1e-9)
    };
    assert!(
        heads.iter().any(|h| tip_on(h, from_anchor)),
        "one arrowhead tip must sit on the from anchor; got {heads:?}"
    );
    assert!(
        heads.iter().any(|h| tip_on(h, to_anchor)),
        "one arrowhead tip must sit on the to anchor; got {heads:?}"
    );
}

/// Default (no markers) → only the line, no FillPolygon (U1 regression).
#[test]
fn connector_without_markers_emits_no_arrowhead() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    assert!(
        cmds.iter()
            .any(|c| matches!(c, SceneCommand::StrokePolyline { .. })),
        "connector must still emit its line; got {cmds:?}"
    );
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::FillPolygon { .. })),
        "a connector with no markers must emit no FillPolygon; got {cmds:?}"
    );
}

// ── connector node (U3): orthogonal routing + multi-segment marker orientation ─
//
// `route="orthogonal"` replaces the straight diagonal with a right-angle elbow
// path: an H–V–H (or V–H–V) 4-point Z-route when both anchors share an
// orientation, or a 3-point L-corner when they differ. The first segment leaves
// `from` perpendicular to its edge and the last enters `to` perpendicular to its
// edge, so arrowheads land axis-aligned. Straight routing is unchanged.

/// Two boxes side-by-side, `route="orthogonal"` with auto anchors → an H–V–H
/// 4-point Z-route with the vertical riser at the mid x and right angles.
#[test]
fn connector_orthogonal_horizontal_boxes_makes_z_route() {
    let src = r##"zenith version=1 {
  project id="proj.co" name="CO"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.co" title="CO" {
page id="page.co" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="orthogonal" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let pts = first_stroke_polyline_points(&result.scene.commands);

    // a right-mid = (140,80) [Horizontal], b left-mid = (300,100) [Horizontal].
    // mid x = (140+300)/2 = 220 → [140,80, 220,80, 220,100, 300,100].
    assert_eq!(
        pts,
        vec![140.0, 80.0, 220.0, 80.0, 220.0, 100.0, 300.0, 100.0],
        "horizontal-anchored orthogonal route must be an H–V–H Z-route; got {pts:?}"
    );
    // Right-angle elbow: the two riser points share the mid x.
    assert_eq!(pts[2], pts[4], "elbow x's must be equal (right angles)");
    assert_eq!(pts[2], (140.0 + 300.0) / 2.0, "riser sits at the mid x");
    // First segment is horizontal (leaves a's right edge), last is horizontal
    // (enters b's left edge).
    assert_eq!(pts[1], 80.0, "first segment leaves horizontally at fy");
    assert_eq!(pts[7], 100.0, "last segment enters horizontally at ty");
}

/// Two boxes stacked vertically, `route="orthogonal"` with auto anchors → a
/// V–H–V 4-point Z-route with the horizontal run at the mid y.
#[test]
fn connector_orthogonal_stacked_boxes_makes_vertical_z() {
    let src = r##"zenith version=1 {
  project id="proj.co" name="CO"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.co" title="CO" {
page id="page.co" w=(px)640 h=(px)480 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)60 y=(px)300 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="orthogonal" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let pts = first_stroke_polyline_points(&result.scene.commands);

    // a center (90,80), b center (110,340): dy dominates → Vertical anchors.
    // a bottom-mid = (90,120), b top-mid = (110,300). mid y = (120+300)/2 = 210
    // → [90,120, 90,210, 110,210, 110,300].
    assert_eq!(
        pts,
        vec![90.0, 120.0, 90.0, 210.0, 110.0, 210.0, 110.0, 300.0],
        "vertical-anchored orthogonal route must be a V–H–V Z-route; got {pts:?}"
    );
    // Right-angle elbow: the two crossbar points share the mid y.
    assert_eq!(pts[3], pts[5], "elbow y's must be equal (right angles)");
    assert_eq!(pts[3], (120.0 + 300.0) / 2.0, "crossbar sits at the mid y");
    // First segment vertical (leaves a's bottom edge), last vertical (enters b's top).
    assert_eq!(pts[0], pts[2], "first segment leaves vertically at fx");
    assert_eq!(pts[4], pts[6], "last segment enters vertically at tx");
}

/// `route="orthogonal"` + `marker-end="arrow"`: the arrowhead tip sits on the
/// `to` anchor and is axis-aligned to the (horizontal) last segment — its two
/// base vertices share the same x.
#[test]
fn connector_orthogonal_with_marker_end_arrowhead_is_axis_aligned() {
    let src = r##"zenith version=1 {
  project id="proj.co" name="CO"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.co" title="CO" {
page id="page.co" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="orthogonal" stroke=(token)"color.line" marker-end="arrow"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Last orthogonal segment is (220,100)→(300,100): horizontal entry into b.
    let heads = all_fill_polygon_points(cmds);
    assert_eq!(
        heads.len(),
        1,
        "marker-end must emit one FillPolygon; got {cmds:?}"
    );
    let head = &heads[0];
    assert_eq!(head.len(), 6, "arrowhead must be a 3-point triangle");

    // Tip on the `to` anchor (300,100).
    let to_anchor = (300.0, 100.0);
    let has_tip = head
        .chunks_exact(2)
        .any(|p| (p[0] - to_anchor.0).abs() < 1e-9 && (p[1] - to_anchor.1).abs() < 1e-9);
    assert!(
        has_tip,
        "an arrowhead vertex must equal the to anchor; got {head:?}"
    );

    // Axis-aligned to a horizontal entry: the two base vertices (everything but
    // the tip) share the same x.
    let base: Vec<(f64, f64)> = head
        .chunks_exact(2)
        .map(|p| (p[0], p[1]))
        .filter(|p| (p.0 - to_anchor.0).abs() >= 1e-9 || (p.1 - to_anchor.1).abs() >= 1e-9)
        .collect();
    assert_eq!(base.len(), 2, "triangle must have two base vertices");
    assert!(
        (base[0].0 - base[1].0).abs() < 1e-9,
        "horizontal entry → base vertices share the same x; got {base:?}"
    );
}

/// `route="straight"` (and the omitted default) still emits a 2-point line whose
/// marker endpoints are the raw anchors — U1/U2 byte-for-byte regression guard.
#[test]
fn connector_straight_route_unchanged_regression() {
    let src = r##"zenith version=1 {
  project id="proj.co" name="CO"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.co" title="CO" {
page id="page.co" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)40 w=(px)100 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)300 y=(px)60 w=(px)100 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="straight" stroke=(token)"color.line" marker-end="arrow"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Straight 2-point line between the raw anchors.
    let line = first_stroke_polyline_points(cmds);
    assert_eq!(
        line,
        vec![140.0, 80.0, 300.0, 100.0],
        "straight route must remain a 2-point line; got {line:?}"
    );

    // Marker endpoints are the raw anchors: tip on the `to` anchor.
    let heads = all_fill_polygon_points(cmds);
    assert_eq!(
        heads.len(),
        1,
        "marker-end must emit one FillPolygon; got {cmds:?}"
    );
    let to_anchor = (300.0, 100.0);
    let has_tip = heads[0]
        .chunks_exact(2)
        .any(|p| (p[0] - to_anchor.0).abs() < 1e-9 && (p[1] - to_anchor.1).abs() < 1e-9);
    assert!(
        has_tip,
        "straight marker tip must sit on the to anchor; got {:?}",
        heads[0]
    );
}

// ── connector node: obstacle-avoiding routing (route="avoid") ─────────────────
//
// `route="avoid"` routes an orthogonal path that steers around every OTHER box
// in the document (boxes other than the connector's own from/to targets). The
// resulting polyline never passes through an obstacle's interior; when no clear
// path exists it degrades to the plain elbow. A connector with no obstacle in
// the way still produces a valid polyline from the from-anchor to the to-anchor.

/// Assert no segment of a flat `[x0,y0,x1,y1,…]` polyline passes through the
/// strict interior of the box `(x, y, w, h)`. Mirrors the router's own crossing
/// check but operates on the un-inflated box.
fn assert_polyline_misses_box(pts: &[f64], boxr: (f64, f64, f64, f64)) {
    const E: f64 = 1e-6;
    let (l, t, r, bot) = (boxr.0, boxr.1, boxr.0 + boxr.2, boxr.1 + boxr.3);
    let n = pts.len() / 2;
    for i in 0..n.saturating_sub(1) {
        let x0 = pts[i * 2];
        let y0 = pts[i * 2 + 1];
        let x1 = pts[(i + 1) * 2];
        let y1 = pts[(i + 1) * 2 + 1];
        if (y0 - y1).abs() <= E {
            if y0 > t + E && y0 < bot - E {
                let (xa, xb) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
                let ov_lo = xa.max(l);
                let ov_hi = xb.min(r);
                assert!(
                    ov_lo + E >= ov_hi - E,
                    "horizontal segment {:?}->{:?} crosses interior of {boxr:?}; pts {pts:?}",
                    (x0, y0),
                    (x1, y1)
                );
            }
        } else if (x0 - x1).abs() <= E && x0 > l + E && x0 < r - E {
            let (ya, yb) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            let ov_lo = ya.max(t);
            let ov_hi = yb.min(bot);
            assert!(
                ov_lo + E >= ov_hi - E,
                "vertical segment {:?}->{:?} crosses interior of {boxr:?}; pts {pts:?}",
                (x0, y0),
                (x1, y1)
            );
        }
    }
}

/// Two boxes separated horizontally with a THIRD box sitting on the straight
/// line between them; `route="avoid"` must produce a polyline that detours
/// around the middle box's interior while still starting/ending at the anchors.
#[test]
fn connector_avoid_routes_around_obstacle() {
    let src = r##"zenith version=1 {
  project id="proj.av" name="AV"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.av" title="AV" {
page id="page.av" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)80 w=(px)80 h=(px)80 stroke=(token)"color.line"
  rect id="obs" x=(px)260 y=(px)60 w=(px)80 h=(px)120 stroke=(token)"color.line"
  rect id="b" x=(px)480 y=(px)80 w=(px)80 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="avoid" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let pts = first_stroke_polyline_points(&result.scene.commands);

    // a center (80,120) left of b center (520,120): auto picks a's right edge
    // (120,120) and b's left edge (480,120). The obstacle box spans x∈[260,340],
    // y∈[60,180] — squarely on the y=120 straight line.
    assert_eq!(pts[0], 120.0, "path starts at a's right-edge x");
    assert_eq!(pts[1], 120.0, "path starts at a's right-edge y");
    let n = pts.len();
    assert_eq!(pts[n - 2], 480.0, "path ends at b's left-edge x");
    assert_eq!(pts[n - 1], 120.0, "path ends at b's left-edge y");

    assert_polyline_misses_box(&pts, (260.0, 60.0, 80.0, 120.0));
}

/// `route="avoid"` with no obstacle between the two boxes still yields a valid
/// polyline running from the from-anchor to the to-anchor.
#[test]
fn connector_avoid_without_obstacle_routes_cleanly() {
    let src = r##"zenith version=1 {
  project id="proj.av" name="AV"
  tokens format="zenith-token-v1" {
token id="color.line" type="color" value="#1e3a8a"
  }
  styles {}
  document id="doc.av" title="AV" {
page id="page.av" w=(px)640 h=(px)360 {
  rect id="a" x=(px)40 y=(px)80 w=(px)80 h=(px)80 stroke=(token)"color.line"
  rect id="b" x=(px)480 y=(px)80 w=(px)80 h=(px)80 stroke=(token)"color.line"
  connector id="c1" from="a" to="b" route="avoid" stroke=(token)"color.line"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let pts = first_stroke_polyline_points(&result.scene.commands);

    assert!(
        pts.len() >= 4,
        "must have at least start and end; got {pts:?}"
    );
    // a right-edge (120,120) → b left-edge (480,120).
    assert_eq!(pts[0], 120.0);
    assert_eq!(pts[1], 120.0);
    let n = pts.len();
    assert_eq!(pts[n - 2], 480.0);
    assert_eq!(pts[n - 1], 120.0);
}

// ── Mask ──────────────────────────────────────────────────────────────

/// A rect carrying `mask=(token)` with NO effect must emit a
/// `BeginMask { mask }` … `EndMask` bracket around its fill, with the
/// `MaskSpec` carrying the rect's x/y/w/h and the resolved shape/radius/feather.
#[test]
fn mask_emits_begin_end_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.mk" name="Mk"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
    token id="mask.m" type="mask" {
      rounded radius=12 feather=8
    }
  }
  styles {}
  document id="doc.mk" title="Mk" {
    page id="page.mk" w=(px)200 h=(px)200 {
      rect id="rect.mk" x=(px)10 y=(px)20 w=(px)80 h=(px)40 fill=(token)"color.fill" mask=(token)"mask.m"
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Locate the BeginMask and verify the resolved spec.
    let spec = cmds
        .iter()
        .find_map(|c| match c {
            SceneCommand::BeginMask { mask } => Some(*mask),
            _ => None,
        })
        .expect("a BeginMask must be emitted");
    assert_eq!(spec.shape, zenith_scene::MaskShape::RoundedRect);
    assert_eq!(spec.radius, 12.0);
    assert_eq!(spec.feather, 8.0);
    assert!(!spec.invert);
    assert_eq!((spec.x, spec.y, spec.w, spec.h), (10.0, 20.0, 80.0, 40.0));

    // Bracket is balanced, BeginMask precedes a fill which precedes EndMask.
    let begin_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginMask { .. }))
        .expect("begin index");
    let end_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndMask))
        .expect("end index");
    assert!(begin_idx < end_idx, "BeginMask must precede EndMask");
    let has_fill_between = cmds
        .get(begin_idx + 1..end_idx)
        .map(|window| {
            window.iter().any(|c| {
                matches!(
                    c,
                    SceneCommand::FillRect { .. } | SceneCommand::FillRoundedRect { .. }
                )
            })
        })
        .unwrap_or(false);
    assert!(
        has_fill_between,
        "the fill must sit inside the mask bracket: {cmds:?}"
    );

    // No effect was set, so no blur/shadow/filter bracket appears.
    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            SceneCommand::BeginBlur { .. }
                | SceneCommand::BeginShadow { .. }
                | SceneCommand::BeginFilter { .. }
        )),
        "no effect bracket should appear for a mask-only rect: {cmds:?}"
    );
}

/// A rect with BOTH `blur` and `mask` must emit the fill TWICE: once bare
/// (sharp base), then inside `BeginMask BeginBlur .. EndBlur EndMask`. The
/// command order is: bare fill, BeginMask, BeginBlur, fill, EndBlur, EndMask.
#[test]
fn mask_and_blur_emits_sharp_base_then_masked_effect() {
    let src = r##"zenith version=1 {
  project id="proj.mb" name="Mb"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
    token id="mask.m" type="mask" {
      ellipse feather=6
    }
  }
  styles {}
  document id="doc.mb" title="Mb" {
    page id="page.mb" w=(px)200 h=(px)200 {
      rect id="rect.mb" x=(px)10 y=(px)10 w=(px)80 h=(px)60 fill=(token)"color.fill" mask=(token)"mask.m" blur=(px)6
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Collect the indices of the relevant commands in order.
    let is_fill = |c: &SceneCommand| matches!(c, SceneCommand::FillRect { .. });
    let fills: Vec<usize> = cmds
        .iter()
        .enumerate()
        .filter(|(_, c)| is_fill(c))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        fills.len(),
        2,
        "fill must be emitted twice (sharp base + masked): {cmds:?}"
    );

    let begin_mask = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginMask { .. }))
        .expect("BeginMask");
    let begin_blur = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .expect("BeginBlur");
    let end_blur = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndBlur))
        .expect("EndBlur");
    let end_mask = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndMask))
        .expect("EndMask");

    // Order: bare fill < BeginMask < BeginBlur < masked fill < EndBlur < EndMask.
    assert!(fills[0] < begin_mask, "sharp base fill precedes BeginMask");
    assert!(begin_mask < begin_blur, "BeginMask precedes BeginBlur");
    assert!(begin_blur < fills[1], "BeginBlur precedes the masked fill");
    assert!(fills[1] < end_blur, "masked fill precedes EndBlur");
    assert!(end_blur < end_mask, "EndBlur precedes EndMask");
}

/// A rect with `blur` and NO mask must be byte-identical to the pre-mask
/// stream: BeginBlur, fill, EndBlur — and NO BeginMask anywhere.
#[test]
fn blur_without_mask_emits_no_mask_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.bn" name="Bn"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
  }
  styles {}
  document id="doc.bn" title="Bn" {
    page id="page.bn" w=(px)200 h=(px)200 {
      rect id="rect.bn" x=(px)10 y=(px)10 w=(px)80 h=(px)60 fill=(token)"color.fill" blur=(px)6
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::BeginMask { .. } | SceneCommand::EndMask)),
        "no mask bracket must appear when mask is absent: {cmds:?}"
    );

    // Exactly one BeginBlur, EndBlur, and a single fill between them.
    let begin = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .expect("BeginBlur");
    let end = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndBlur))
        .expect("EndBlur");
    assert!(begin < end);
    let fills_between = cmds
        .get(begin + 1..end)
        .map(|w| {
            w.iter()
                .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        fills_between, 1,
        "exactly one fill inside the blur bracket: {cmds:?}"
    );
}

// ── connector line-jumps (hops at connector-vs-connector crossings) ───────────
//
// A page-level `line-jumps="arc"` makes the horizontal connector hop over the
// vertical one at their crossing. Without the property the routes are
// byte-identical to today's plain connector routes.

/// Two crossing connectors: a HORIZONTAL one (a→b) and a VERTICAL one (c→d),
/// meeting at (320, 160). All auto anchors.
fn crossing_connectors_src(page_props: &str) -> String {
    format!(
        r##"zenith version=1 {{
  project id="proj.lj" name="LJ"
  tokens format="zenith-token-v1" {{
token id="color.line" type="color" value="#1e3a8a"
  }}
  styles {{}}
  document id="doc.lj" title="LJ" {{
page id="page.lj" w=(px)640 h=(px)360 {page_props} {{
  rect id="a" x=(px)40 y=(px)140 w=(px)80 h=(px)40 stroke=(token)"color.line"
  rect id="b" x=(px)520 y=(px)140 w=(px)80 h=(px)40 stroke=(token)"color.line"
  rect id="c" x=(px)300 y=(px)20 w=(px)40 h=(px)40 stroke=(token)"color.line"
  rect id="d" x=(px)300 y=(px)300 w=(px)40 h=(px)40 stroke=(token)"color.line"
  connector id="ch" from="a" to="b" stroke=(token)"color.line"
  connector id="cv" from="c" to="d" stroke=(token)"color.line"
}}
  }}
}}
"##
    )
}

/// Collect every connector `StrokePolyline`'s points (those with an even number
/// of coords, in emission order). All four rect strokes here are closed
/// outlines; connectors are open-center polylines, so filter on `closed:false`
/// AND a 2-point-or-more open path that is NOT a rect outline. Simplest robust
/// filter: open, center-aligned strokes whose first point matches a connector
/// endpoint. We just collect all open center strokes.
fn open_center_strokes(cmds: &[SceneCommand]) -> Vec<Vec<f64>> {
    cmds.iter()
        .filter_map(|c| match c {
            SceneCommand::StrokePolyline {
                points,
                closed: false,
                align: StrokeAlign::Center,
                ..
            } => Some(points.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn line_jumps_absent_is_byte_identical() {
    let doc = parse(&crossing_connectors_src(""));
    let result = compile(&doc, &default_provider());
    let strokes = open_center_strokes(&result.scene.commands);

    // Two connector polylines, plain straight routes, untouched.
    assert_eq!(
        strokes,
        vec![
            vec![120.0, 160.0, 520.0, 160.0], // horizontal a→b
            vec![320.0, 60.0, 320.0, 300.0],  // vertical c→d
        ],
        "without line-jumps both connectors keep their plain routes"
    );
}

#[test]
fn line_jumps_arc_horizontal_hops() {
    let doc = parse(&crossing_connectors_src(r#"line-jumps="arc""#));
    let result = compile(&doc, &default_provider());
    let strokes = open_center_strokes(&result.scene.commands);
    assert_eq!(strokes.len(), 2, "still two connector polylines");

    let horiz = &strokes[0];
    let vert = &strokes[1];

    // The horizontal connector gains a bump (more than the plain 4 coords);
    // the vertical one is unchanged.
    assert!(
        horiz.len() > 4,
        "horizontal connector should gain bump points: {horiz:?}"
    );
    assert_eq!(
        vert,
        &vec![320.0, 60.0, 320.0, 300.0],
        "vertical connector must be unchanged"
    );
    // Bump bulges above the line (smaller y) near the x=320 crossing.
    let min_y = horiz
        .chunks_exact(2)
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min);
    assert!(min_y < 160.0, "bump must dip above the line: {horiz:?}");
}

/// A self-loop (`from` and `to` name the SAME node) routes as a rectangular loop
/// off the box edge — a 4-point path that bulges above the top edge by default —
/// not a degenerate zero-length line.
#[test]
fn connector_self_loop_routes_a_loop() {
    let src = r##"zenith version=1 {
  project id="proj.cn" name="CN"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#dbeafe"
token id="color.line" type="color" value="#1e3a8a"
token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.cn" title="CN" {
page id="page.cn" w=(px)400 h=(px)300 {
  rect id="a" x=(px)100 y=(px)120 w=(px)120 h=(px)60 fill=(token)"color.fill"
  connector id="c1" from="a" to="a" stroke=(token)"color.line" stroke-width=(token)"size.stroke"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let pts = first_stroke_polyline_points(&result.scene.commands);

    // a: x=100 y=120 w=120 h=60 → top edge y=120, center x=160.
    // Default top loop: two feet on y=120, bulging up by 28px to y=92.
    assert_eq!(
        pts.len(),
        8,
        "self-loop must be a 4-point loop; got {pts:?}"
    );
    // Both feet sit on the top edge; both bulge points are strictly above it.
    assert_eq!(pts[1], 120.0);
    assert_eq!(pts[7], 120.0);
    assert!(
        pts[3] < 120.0 && pts[5] < 120.0,
        "loop must bulge above the top edge; got {pts:?}"
    );
}
