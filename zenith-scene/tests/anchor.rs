//! Integration tests for page-relative, safe-zone-relative, and parent-container-relative anchors.
//!
//! An `anchor` attribute on a node derives its missing `x` and/or `y` from the
//! page dimensions. Explicitly-authored `x`/`y` always win over the anchor-
//! derived value. Unrecognized anchor values produce `anchor.unknown_value` from
//! the validator. A node with a recognized anchor and present `w`/`h` must NOT
//! receive `scene.missing_geometry` for its `x`/`y`.

mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;

// ── Shared document wrapper ───────────────────────────────────────────────────

/// Wrap a single page child (a raw KDL line) in a minimal document on a
/// 400×300 page.
fn doc_with_node(node_kdl: &str) -> String {
    format!(
        r#"zenith version=1 {{
  project id="proj.anc" name="Anchor"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.anc" title="Anchor" {{
page id="page.anc" w=(px)400 h=(px)300 {{
  {node_kdl}
}}
  }}
}}"#
    )
}

// ── Test 1: bottom-right anchor places rect at (page_w-w, page_h-h) ─────────

#[test]
fn anchor_bottom_right_rect() {
    // Page 400×300, rect 100×50 with anchor="bottom-right":
    //   x = 400 - 100 = 300,  y = 300 - 50 = 250
    let src = doc_with_node(
        r##"rect id="r.br" anchor="bottom-right" w=(px)100 h=(px)50 fill="#ff0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    // PushClip for page is index 0; first real node FillRect follows.
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 300.0).abs() < 0.001
                && (y - 250.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 50.0).abs() < 0.001
        }),
        "expected FillRect at (300, 250, 100, 50) for bottom-right anchor; got: {rects:?}"
    );
}

// ── Test 2: center anchor places rect at ((pw-w)/2, (ph-h)/2) ───────────────

#[test]
fn anchor_center_rect() {
    // Page 400×300, rect 200×100 with anchor="center":
    //   x = (400-200)/2 = 100,  y = (300-100)/2 = 100
    let src =
        doc_with_node(r##"rect id="r.ctr" anchor="center" w=(px)200 h=(px)100 fill="#00ff00""##);
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 100.0).abs() < 0.001
                && (y - 100.0).abs() < 0.001
                && (w - 200.0).abs() < 0.001
                && (h - 100.0).abs() < 0.001
        }),
        "expected FillRect at (100, 100, 200, 100) for center anchor; got: {rects:?}"
    );
}

// ── Test 3: explicit y wins over anchor ──────────────────────────────────────

#[test]
fn anchor_explicit_y_wins() {
    // Page 400×300, rect 100×50 with anchor="bottom-right" but y=(px)0:
    //   x is derived: 400-100 = 300
    //   y is explicit: 0
    let src = doc_with_node(
        r##"rect id="r.yw" anchor="bottom-right" w=(px)100 h=(px)50 y=(px)0 fill="#0000ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 300.0).abs() < 0.001
                && (y - 0.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 50.0).abs() < 0.001
        }),
        "expected FillRect at (300, 0, 100, 50): x from anchor, y explicit; got: {rects:?}"
    );
}

// ── Test 4: explicit x wins over anchor ──────────────────────────────────────

#[test]
fn anchor_explicit_x_wins() {
    // Page 400×300, rect 100×50 with anchor="bottom-right" but x=(px)0:
    //   x is explicit: 0
    //   y is derived: 300-50 = 250
    let src = doc_with_node(
        r##"rect id="r.xw" anchor="bottom-right" w=(px)100 h=(px)50 x=(px)0 fill="#00ffff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 0.0).abs() < 0.001
                && (y - 250.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 50.0).abs() < 0.001
        }),
        "expected FillRect at (0, 250, 100, 50): x explicit, y from anchor; got: {rects:?}"
    );
}

// ── Test 5: no anchor → byte-identical to authored x/y ───────────────────────

#[test]
fn no_anchor_byte_identical() {
    // Without anchor, the node must still compile normally when x/y are explicit.
    let with_anchor =
        doc_with_node(r##"rect id="r.na" anchor="top-left" w=(px)80 h=(px)60 fill="#123456""##);
    let without_anchor =
        doc_with_node(r##"rect id="r.na" x=(px)0 y=(px)0 w=(px)80 h=(px)60 fill="#123456""##);

    let doc_a = parse(&with_anchor);
    let doc_b = parse(&without_anchor);
    let res_a = compile(&doc_a, &default_provider());
    let res_b = compile(&doc_b, &default_provider());

    assert!(
        res_a.diagnostics.is_empty(),
        "anchor=top-left should not produce diagnostics: {:?}",
        res_a.diagnostics
    );
    assert!(
        res_b.diagnostics.is_empty(),
        "explicit (0,0) should not produce diagnostics: {:?}",
        res_b.diagnostics
    );

    // Both should produce the same FillRect.
    let rects_a = fill_rects(&res_a);
    let rects_b = fill_rects(&res_b);
    assert_eq!(
        rects_a, rects_b,
        "anchor=top-left and explicit (0,0) must yield identical FillRect geometry"
    );
}

// ── Test 6: unrecognized anchor → anchor.unknown_value error ─────────────────

#[test]
fn anchor_unknown_value_error() {
    // The validator (not the compiler) produces anchor.unknown_value for unknown
    // anchor strings. Use the validate path via zenith_core.
    use zenith_core::{KdlAdapter, KdlSource};

    let src =
        doc_with_node(r##"rect id="r.bad" anchor="bogus" w=(px)100 h=(px)50 fill="#ff0000""##);
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.unknown_value"),
        "expected anchor.unknown_value diagnostic for anchor=\"bogus\"; got: {:?}",
        report.diagnostics
    );
}

// ── Test 7: recognized anchor suppresses missing_geometry for x/y ────────────

#[test]
fn anchor_recognized_suppresses_missing_geometry() {
    // anchor="top-left" with w/h but no x/y: compile must NOT emit
    // scene.missing_geometry (the anchor derives x=0, y=0).
    let src =
        doc_with_node(r##"rect id="r.tl" anchor="top-left" w=(px)80 h=(px)60 fill="#abcdef""##);
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.missing_geometry"),
        "anchor=top-left with w/h must not produce scene.missing_geometry; got: {:?}",
        result.diagnostics
    );

    // The rect must actually be rendered.
    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            x.abs() < 0.001
                && y.abs() < 0.001
                && (w - 80.0).abs() < 0.001
                && (h - 60.0).abs() < 0.001
        }),
        "expected FillRect at (0, 0, 80, 60) for top-left anchor; got: {rects:?}"
    );
}

// ── Test 8: all nine anchors on a 400×300 page, rect 40×30 ─────────────────

#[test]
fn all_nine_anchors_geometry() {
    // For a 400×300 page with a 40×30 rect:
    //   dx = (400-40)/2 = 180,  dy = (300-30)/2 = 135
    let cases: &[(&str, f64, f64)] = &[
        ("top-left", 0.0, 0.0),
        ("top-center", 180.0, 0.0),
        ("top-right", 360.0, 0.0),
        ("center-left", 0.0, 135.0),
        ("center", 180.0, 135.0),
        ("center-right", 360.0, 135.0),
        ("bottom-left", 0.0, 270.0),
        ("bottom-center", 180.0, 270.0),
        ("bottom-right", 360.0, 270.0),
    ];

    for &(anchor_name, exp_x, exp_y) in cases {
        let node_kdl = format!(
            r##"rect id="r.nine" anchor="{anchor_name}" w=(px)40 h=(px)30 fill="#ffffff""##
        );
        let src = doc_with_node(&node_kdl);
        let doc = parse(&src);
        let result = compile(&doc, &default_provider());
        assert!(
            result.diagnostics.is_empty(),
            "anchor=\"{anchor_name}\" produced diagnostics: {:?}",
            result.diagnostics
        );

        let rects = fill_rects(&result);
        assert!(
            rects.iter().any(|&(x, y, w, h)| {
                (x - exp_x).abs() < 0.001
                    && (y - exp_y).abs() < 0.001
                    && (w - 40.0).abs() < 0.001
                    && (h - 30.0).abs() < 0.001
            }),
            "anchor=\"{anchor_name}\": expected ({exp_x}, {exp_y}, 40, 30); got: {rects:?}"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Safe-zone-relative anchors
// ═════════════════════════════════════════════════════════════════════════════

/// Wrap a single page child in a document that also declares one safe-zone.
///
/// Page is 400×300.  Safe-zone "sz.panel" is at (100, 50, 200, 100) —
/// i.e. x=100 y=50 w=200 h=100.
fn doc_with_zone_and_node(node_kdl: &str) -> String {
    format!(
        r#"zenith version=1 {{
  project id="proj.az" name="AnchorZone"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.az" title="AnchorZone" {{
page id="page.az" w=(px)400 h=(px)300 {{
  safe-zone id="sz.panel" type="required" x=(px)100 y=(px)50 w=(px)200 h=(px)100
  {node_kdl}
}}
  }}
}}"#
    )
}

// ── bottom-right relative to zone ─────────────────────────────

#[test]
fn anchor_zone_bottom_right() {
    // Zone: x=100 y=50 w=200 h=100. Node: w=40 h=30. anchor="bottom-right".
    // Zone-relative:  ox = 200-40 = 160, oy = 100-30 = 70.
    // Absolute:       x  = 100+160 = 260, y = 50+70 = 120.
    let src = doc_with_zone_and_node(
        r##"rect id="r.zbr" anchor="bottom-right" anchor-zone="sz.panel" w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 260.0).abs() < 0.001
                && (y - 120.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (260, 120, 40, 30) for zone bottom-right; got: {rects:?}"
    );
}

// ── center within zone ───────────────────────────────────────────

#[test]
fn anchor_zone_center() {
    // Zone: x=100 y=50 w=200 h=100. Node: w=40 h=20. anchor="center".
    // Zone-relative:  ox = (200-40)/2 = 80, oy = (100-20)/2 = 40.
    // Absolute:       x  = 100+80 = 180, y = 50+40 = 90.
    let src = doc_with_zone_and_node(
        r##"rect id="r.zctr" anchor="center" anchor-zone="sz.panel" w=(px)40 h=(px)20 fill="#00ff00""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 180.0).abs() < 0.001
                && (y - 90.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 20.0).abs() < 0.001
        }),
        "expected FillRect at (180, 90, 40, 20) for zone center; got: {rects:?}"
    );
}

// ── unresolved zone id produces anchor.unresolved_zone error ─────

#[test]
fn anchor_zone_unresolved() {
    use zenith_core::{KdlAdapter, KdlSource};

    // "sz.ghost" does not exist on the page.
    let src = doc_with_zone_and_node(
        r##"rect id="r.zghost" anchor="top-left" anchor-zone="sz.ghost" w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.unresolved_zone"),
        "expected anchor.unresolved_zone diagnostic; got: {:?}",
        report.diagnostics
    );
}

// ── anchor-zone without anchor produces anchor.zone_without_anchor

#[test]
fn anchor_zone_without_anchor() {
    use zenith_core::{KdlAdapter, KdlSource};

    // anchor-zone is set but anchor is absent.
    let src = doc_with_zone_and_node(
        r##"rect id="r.znoanc" anchor-zone="sz.panel" x=(px)0 y=(px)0 w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.zone_without_anchor"),
        "expected anchor.zone_without_anchor warning; got: {:?}",
        report.diagnostics
    );
}

// ── no zone → page-relative behaviour unchanged (regression) ─────

#[test]
fn anchor_no_zone_regression() {
    // Same page, anchor="bottom-right" without anchor-zone: page-relative.
    // Page 400×300, rect 40×30 → x=360, y=270.
    let src = doc_with_zone_and_node(
        r##"rect id="r.nozone" anchor="bottom-right" w=(px)40 h=(px)30 fill="#0000ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 360.0).abs() < 0.001
                && (y - 270.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (360, 270, 40, 30) for page-relative bottom-right; got: {rects:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Parent-container-relative anchors
// ═════════════════════════════════════════════════════════════════════════════

/// Wrap a child node inside a clip-only `frame` on a 400×300 page.
///
/// `frame_attrs` is the frame's attribute string (e.g. its geometry);
/// `child_kdl` is the single frame child.
fn doc_with_frame_child(frame_attrs: &str, child_kdl: &str) -> String {
    format!(
        r#"zenith version=1 {{
  project id="proj.ap" name="AnchorParent"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.ap" title="AnchorParent" {{
page id="page.ap" w=(px)400 h=(px)300 {{
  frame id="fr.box" {frame_attrs} {{
    {child_kdl}
  }}
}}
  }}
}}"#
    )
}

/// Wrap a child node inside a translating `group` on a 400×300 page.
fn doc_with_group_child(group_attrs: &str, child_kdl: &str) -> String {
    format!(
        r#"zenith version=1 {{
  project id="proj.ap" name="AnchorParent"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.ap" title="AnchorParent" {{
page id="page.ap" w=(px)400 h=(px)300 {{
  group id="gr.box" {group_attrs} {{
    {child_kdl}
  }}
}}
  }}
}}"#
    )
}

// ── center inside a frame ────────────────────────────────────────

#[test]
fn anchor_parent_frame_center() {
    // Frame at x=50 y=40 w=200 h=100; rect 40×30 anchor="center".
    //   ox = (200-40)/2 = 80, oy = (100-30)/2 = 35.
    //   x = 50+80 = 130, y = 40+35 = 75.
    let src = doc_with_frame_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)100",
        r##"rect id="r.fc" anchor="center" anchor-parent=#true w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 130.0).abs() < 0.001
                && (y - 75.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (130, 75, 40, 30) for frame center; got: {rects:?}"
    );
}

// ── bottom-right inside a frame ──────────────────────────────────

#[test]
fn anchor_parent_frame_bottom_right() {
    // Frame at x=50 y=40 w=200 h=100; rect 40×30 anchor="bottom-right".
    //   ox = 200-40 = 160, oy = 100-30 = 70.
    //   x = 50+160 = 210, y = 40+70 = 110.
    let src = doc_with_frame_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)100",
        r##"rect id="r.fbr" anchor="bottom-right" anchor-parent=#true w=(px)40 h=(px)30 fill="#00ff00""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 210.0).abs() < 0.001
                && (y - 110.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (210, 110, 40, 30) for frame bottom-right; got: {rects:?}"
    );
}

// ── center inside a group (group translates the child) ───────────

#[test]
fn anchor_parent_group_center() {
    // Group at x=50 y=40 w=200 h=100; rect 40×30 anchor="center".
    //   within-group offset: ox=80, oy=35.
    //   absolute (group translates by 50,40): x = 50+80 = 130, y = 40+35 = 75.
    let src = doc_with_group_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)100",
        r##"rect id="r.gc" anchor="center" anchor-parent=#true w=(px)40 h=(px)30 fill="#0000ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 130.0).abs() < 0.001
                && (y - 75.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (130, 75, 40, 30) for group center; got: {rects:?}"
    );
}

// ── all nine anchors inside a frame ──────────────────────────────

#[test]
fn anchor_parent_frame_all_nine() {
    // Frame at x=50 y=40 w=200 h=100; rect 40×30.
    //   dx = (200-40)/2 = 80, dy = (100-30)/2 = 35.
    //   right = 200-40 = 160, bottom = 100-30 = 70.
    // Absolute = frame origin (50,40) + offset.
    let cases: &[(&str, f64, f64)] = &[
        ("top-left", 50.0, 40.0),
        ("top-center", 130.0, 40.0),
        ("top-right", 210.0, 40.0),
        ("center-left", 50.0, 75.0),
        ("center", 130.0, 75.0),
        ("center-right", 210.0, 75.0),
        ("bottom-left", 50.0, 110.0),
        ("bottom-center", 130.0, 110.0),
        ("bottom-right", 210.0, 110.0),
    ];

    for &(anchor_name, exp_x, exp_y) in cases {
        let child = format!(
            r##"rect id="r.fn9" anchor="{anchor_name}" anchor-parent=#true w=(px)40 h=(px)30 fill="#ffffff""##
        );
        let src = doc_with_frame_child("x=(px)50 y=(px)40 w=(px)200 h=(px)100", &child);
        let doc = parse(&src);
        let result = compile(&doc, &default_provider());
        assert!(
            result.diagnostics.is_empty(),
            "anchor-parent \"{anchor_name}\" produced diagnostics: {:?}",
            result.diagnostics
        );

        let rects = fill_rects(&result);
        assert!(
            rects.iter().any(|&(x, y, w, h)| {
                (x - exp_x).abs() < 0.001
                    && (y - exp_y).abs() < 0.001
                    && (w - 40.0).abs() < 0.001
                    && (h - 30.0).abs() < 0.001
            }),
            "anchor-parent \"{anchor_name}\": expected ({exp_x}, {exp_y}, 40, 30); got: {rects:?}"
        );
    }
}

// ── explicit x wins over anchor-parent ───────────────────────────

#[test]
fn anchor_parent_explicit_x_wins() {
    // Frame at x=50 y=40 w=200 h=100; rect anchor="center" anchor-parent but x=0.
    //   x explicit: 0 (device, no frame translation).
    //   y derived: 40 + (100-30)/2 = 75.
    let src = doc_with_frame_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)100",
        r##"rect id="r.fxw" anchor="center" anchor-parent=#true x=(px)0 w=(px)40 h=(px)30 fill="#00ffff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 0.0).abs() < 0.001
                && (y - 75.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (0, 75, 40, 30): x explicit, y from parent anchor; got: {rects:?}"
    );
}

// ── page-level anchor without anchor-parent unchanged (page-relative regression)

#[test]
fn anchor_parent_absent_page_relative_unchanged() {
    // A top-level rect with anchor="center" and NO anchor-parent must stay
    // page-relative: page 400×300, rect 40×30 → (180, 135).
    let src = doc_with_node(r##"rect id="r.pr" anchor="center" w=(px)40 h=(px)30 fill="#123456""##);
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 180.0).abs() < 0.001
                && (y - 135.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (180, 135, 40, 30) for page-relative center; got: {rects:?}"
    );
}

// ── anchor-zone takes precedence over anchor-parent ──────────────

#[test]
fn anchor_zone_precedence_over_parent() {
    // A rect inside a frame that declares BOTH anchor-zone and anchor-parent:
    // the zone wins. Zone "sz.panel" at (100,50,200,100); rect 40×30 center.
    //   zone-relative center: x = 100+(200-40)/2 = 180, y = 50+(100-30)/2 = 85.
    // (If anchor-parent had won, the frame box (10,10,100,80) would have placed
    //  it at x = 10+30 = 40, y = 10+25 = 35 — a clearly different result.)
    let src = r##"zenith version=1 {
  project id="proj.ap" name="AnchorParent"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ap" title="AnchorParent" {
page id="page.ap" w=(px)400 h=(px)300 {
  safe-zone id="sz.panel" type="required" x=(px)100 y=(px)50 w=(px)200 h=(px)100
  frame id="fr.zp" x=(px)10 y=(px)10 w=(px)100 h=(px)80 {
    rect id="r.zp" anchor="center" anchor-zone="sz.panel" anchor-parent=#true w=(px)40 h=(px)30 fill="#ff00ff"
  }
}
  }
}"##
    .to_string();
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 180.0).abs() < 0.001
                && (y - 85.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (180, 85, 40, 30) — zone wins over parent; got: {rects:?}"
    );
}

// ── page-level anchor-parent → anchor.unresolvable_parent error ───

#[test]
fn anchor_parent_unresolvable_at_page_root() {
    use zenith_core::{KdlAdapter, KdlSource};

    // A top-level node with anchor-parent has no enclosing container.
    let src = doc_with_node(
        r##"rect id="r.up" anchor="center" anchor-parent=#true w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.unresolvable_parent"),
        "expected anchor.unresolvable_parent error at page root; got: {:?}",
        report.diagnostics
    );
}

// ── group without w/h + anchor-parent → unresolvable_parent ──────

#[test]
fn anchor_parent_group_without_box_unresolvable() {
    use zenith_core::{KdlAdapter, KdlSource};

    // Group declares x/y but no w/h, so its reference box is unknown.
    let src = doc_with_group_child(
        "x=(px)50 y=(px)40",
        r##"rect id="r.gnb" anchor="center" anchor-parent=#true w=(px)40 h=(px)30 fill="#00ff00""##,
    );
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.unresolvable_parent"),
        "expected anchor.unresolvable_parent for w/h-less group; got: {:?}",
        report.diagnostics
    );
}

// ── sibling-relative anchoring ───────────────────────────────────────────

/// Wrap a raw block of page children (multiple KDL lines) in a 400×300 page.
fn doc_with_children(children_kdl: &str) -> String {
    format!(
        r#"zenith version=1 {{
  project id="proj.sib" name="AnchorSibling"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.sib" title="AnchorSibling" {{
page id="page.sib" w=(px)400 h=(px)300 {{
  {children_kdl}
}}
  }}
}}"#
    )
}

// ── top-left / center / bottom-right against a sibling box ────────

#[test]
fn anchor_sibling_three_points() {
    // Sibling `s` at x=100 y=80 w=120 h=60. Three nodes anchor to it:
    //   tl (20×10), anchor=top-left:      ox=0,            oy=0
    //       → (100+0, 80+0)          = (100, 80)
    //   ct (40×20), anchor=center:        ox=(120-40)/2=40, oy=(60-20)/2=20
    //       → (100+40, 80+20)        = (140, 100)
    //   br (30×15), anchor=bottom-right:  ox=120-30=90,     oy=60-15=45
    //       → (100+90, 80+45)        = (190, 125)
    let src = doc_with_children(
        r##"rect id="s" x=(px)100 y=(px)80 w=(px)120 h=(px)60 fill="#888888"
  rect id="tl" anchor="top-left" anchor-sibling="s" w=(px)20 h=(px)10 fill="#ff0000"
  rect id="ct" anchor="center" anchor-sibling="s" w=(px)40 h=(px)20 fill="#00ff00"
  rect id="br" anchor="bottom-right" anchor-sibling="s" w=(px)30 h=(px)15 fill="#0000ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    let has = |x: f64, y: f64, w: f64, h: f64| {
        rects.iter().any(|&(rx, ry, rw, rh)| {
            (rx - x).abs() < 0.001
                && (ry - y).abs() < 0.001
                && (rw - w).abs() < 0.001
                && (rh - h).abs() < 0.001
        })
    };
    assert!(has(100.0, 80.0, 20.0, 10.0), "top-left; got: {rects:?}");
    assert!(has(140.0, 100.0, 40.0, 20.0), "center; got: {rects:?}");
    assert!(
        has(190.0, 125.0, 30.0, 15.0),
        "bottom-right; got: {rects:?}"
    );
}

// ── chain A→B→C resolves transitively, source order independent ──

#[test]
fn anchor_sibling_chain_topo_order() {
    // C is authored BEFORE A in source to prove the toposort orders by
    // dependency, not source order.
    //   a: x=10 y=10 w=100 h=100 (explicit origin).
    //   b anchors center to a:  ox=(100-20)/2=40, oy=(100-20)/2=40
    //       → (10+40, 10+40) = (50, 50); b is 20×20.
    //   c anchors top-left to b: ox=0, oy=0
    //       → (50, 50); c is 8×8.
    let src = doc_with_children(
        r##"rect id="c" anchor="top-left" anchor-sibling="b" w=(px)8 h=(px)8 fill="#0000ff"
  rect id="a" x=(px)10 y=(px)10 w=(px)100 h=(px)100 fill="#888888"
  rect id="b" anchor="center" anchor-sibling="a" w=(px)20 h=(px)20 fill="#00ff00""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    let has = |x: f64, y: f64, w: f64, h: f64| {
        rects.iter().any(|&(rx, ry, rw, rh)| {
            (rx - x).abs() < 0.001
                && (ry - y).abs() < 0.001
                && (rw - w).abs() < 0.001
                && (rh - h).abs() < 0.001
        })
    };
    assert!(has(50.0, 50.0, 20.0, 20.0), "b@center(a); got: {rects:?}");
    assert!(has(50.0, 50.0, 8.0, 8.0), "c@top-left(b); got: {rects:?}");
}

// ── anchor-zone takes precedence over anchor-sibling ─────────────

#[test]
fn anchor_zone_beats_sibling() {
    // Node sets BOTH anchor-zone and anchor-sibling. Zone must win.
    //   zone z: x=200 y=0 w=200 h=300. Node 50×50 anchor=top-left in zone
    //       → (200, 0). Sibling s is at (0,0) 100×100; if sibling had won the
    //       node would land at (0,0) — proving precedence.
    let src = r##"zenith version=1 {
  project id="proj.zs" name="ZoneSibling"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.zs" title="ZoneSibling" {
page id="page.zs" w=(px)400 h=(px)300 {
  safe-zone id="z" type="required" x=(px)200 y=(px)0 w=(px)200 h=(px)300
  rect id="s" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill="#888888"
  rect id="n" anchor="top-left" anchor-zone="z" anchor-sibling="s" w=(px)50 h=(px)50 fill="#ff0000"
}
  }
}"##
    .to_string();
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 200.0).abs() < 0.001
                && (y - 0.0).abs() < 0.001
                && (w - 50.0).abs() < 0.001
                && (h - 50.0).abs() < 0.001
        }),
        "expected zone-derived (200, 0, 50, 50); got: {rects:?}"
    );
}

// ── sibling inside a group resolves in the group's local space ───

#[test]
fn anchor_sibling_inside_group() {
    // Group translates children by (50, 40). Both siblings live in the group.
    //   s: x=10 y=10 w=80 h=40 (local). n anchors bottom-right to s:
    //       ox=80-20=60, oy=40-10=30 → local (10+60, 10+30) = (70, 40); n 20×10.
    //   device = group(50,40) + local(70,40) = (120, 80).
    let src = doc_with_group_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)200",
        r##"rect id="s" x=(px)10 y=(px)10 w=(px)80 h=(px)40 fill="#888888"
    rect id="n" anchor="bottom-right" anchor-sibling="s" w=(px)20 h=(px)10 fill="#ff0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 120.0).abs() < 0.001
                && (y - 80.0).abs() < 0.001
                && (w - 20.0).abs() < 0.001
                && (h - 10.0).abs() < 0.001
        }),
        "expected group-local device (120, 80, 20, 10); got: {rects:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Adjacent-edge placement: anchor-edge + anchor-gap
// ═════════════════════════════════════════════════════════════════════════════
//
// Shared scene: sibling `s` at x=50 y=60 w=120 h=80. All edge tests use that
// sibling unless noted otherwise. Page is 400×300.
//
// Helper constants (using the doc_with_children wrapper from above):
//   sib_x=50, sib_y=60, sib_w=120, sib_h=80

// ── below with gap, no 9-pt anchor → leading-edge (left) x, y past bottom ──

#[test]
fn anchor_edge_below_gap_no_anchor() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=100 h=40, anchor-edge="below", anchor-gap=(px)10, no anchor.
    // Expected:
    //   x = sib_x = 50  (leading edge default, cross-axis left)
    //   y = sib_y + sib_h + gap = 60 + 80 + 10 = 150
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor-sibling="s" anchor-edge="below" anchor-gap=(px)10 w=(px)100 h=(px)40 fill="#ff0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 50.0).abs() < 0.001
                && (y - 150.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 40.0).abs() < 0.001
        }),
        "expected FillRect at (50, 150, 100, 40) for edge=below gap=10; got: {rects:?}"
    );
}

// ── above with gap, no anchor → leading-edge x, y before top ────────────────

#[test]
fn anchor_edge_above_gap_no_anchor() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=100 h=40, anchor-edge="above", anchor-gap=(px)10, no anchor.
    // Expected:
    //   x = sib_x = 50  (leading edge default)
    //   y = sib_y - gap - node_h = 60 - 10 - 40 = 10
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor-sibling="s" anchor-edge="above" anchor-gap=(px)10 w=(px)100 h=(px)40 fill="#0000ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 50.0).abs() < 0.001
                && (y - 10.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 40.0).abs() < 0.001
        }),
        "expected FillRect at (50, 10, 100, 40) for edge=above gap=10; got: {rects:?}"
    );
}

// ── after with gap, no anchor → x past right edge, leading-edge (top) y ─────

#[test]
fn anchor_edge_after_gap_no_anchor() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=60 h=30, anchor-edge="after", anchor-gap=(px)8, no anchor.
    // Expected:
    //   x = sib_x + sib_w + gap = 50 + 120 + 8 = 178
    //   y = sib_y = 60  (leading edge default, cross-axis top)
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor-sibling="s" anchor-edge="after" anchor-gap=(px)8 w=(px)60 h=(px)30 fill="#00ff00""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 178.0).abs() < 0.001
                && (y - 60.0).abs() < 0.001
                && (w - 60.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (178, 60, 60, 30) for edge=after gap=8; got: {rects:?}"
    );
}

// ── before with gap, no anchor → x before left edge, leading-edge (top) y ───

#[test]
fn anchor_edge_before_gap_no_anchor() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=60 h=30, anchor-edge="before", anchor-gap=(px)5, no anchor.
    // Expected:
    //   x = sib_x - gap - node_w = 50 - 5 - 60 = -15
    //   y = sib_y = 60  (leading edge default)
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor-sibling="s" anchor-edge="before" anchor-gap=(px)5 w=(px)60 h=(px)30 fill="#ff00ff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - (-15.0)).abs() < 0.001
                && (y - 60.0).abs() < 0.001
                && (w - 60.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (-15, 60, 60, 30) for edge=before gap=5; got: {rects:?}"
    );
}

// ── below + anchor="top-center" → horizontally centered on the sibling ───────

#[test]
fn anchor_edge_below_cross_axis_center() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=60 h=30, anchor-edge="below", anchor-gap=(px)6, anchor="top-center".
    // cross_h: "top-center" → center horizontally:
    //   x = sib_x + (sib_w - node_w) / 2 = 50 + (120 - 60) / 2 = 50 + 30 = 80
    //   y = sib_y + sib_h + gap = 60 + 80 + 6 = 146
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor="top-center" anchor-sibling="s" anchor-edge="below" anchor-gap=(px)6 w=(px)60 h=(px)30 fill="#ffaa00""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 80.0).abs() < 0.001
                && (y - 146.0).abs() < 0.001
                && (w - 60.0).abs() < 0.001
                && (h - 30.0).abs() < 0.001
        }),
        "expected FillRect at (80, 146, 60, 30) for edge=below anchor=top-center; got: {rects:?}"
    );
}

// ── after + anchor="center-right" → cross-axis bottom-aligned vertically ─────

#[test]
fn anchor_edge_after_cross_axis_bottom() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node: w=40 h=20, anchor-edge="after", anchor-gap=(px)4, anchor="bottom-left".
    // cross_v: "bottom-left" → bottom-align:
    //   y = sib_y + sib_h - node_h = 60 + 80 - 20 = 120
    //   x = sib_x + sib_w + gap = 50 + 120 + 4 = 174
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor="bottom-left" anchor-sibling="s" anchor-edge="after" anchor-gap=(px)4 w=(px)40 h=(px)20 fill="#00ccff""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 174.0).abs() < 0.001
                && (y - 120.0).abs() < 0.001
                && (w - 40.0).abs() < 0.001
                && (h - 20.0).abs() < 0.001
        }),
        "expected FillRect at (174, 120, 40, 20) for edge=after anchor=bottom-left; got: {rects:?}"
    );
}

// ── explicit x/y still win over anchor-edge ──────────────────────────────────

#[test]
fn anchor_edge_explicit_xy_wins() {
    // Sibling: x=50 y=60 w=120 h=80.
    // Node has anchor-edge="below" gap=10 but ALSO explicit x=(px)0 y=(px)0.
    // Explicit x/y override the anchor-derived placement on both axes,
    // so the node lands at (0, 0), not (50, 150).
    let src = doc_with_children(
        r##"rect id="s" x=(px)50 y=(px)60 w=(px)120 h=(px)80 fill="#888888"
  rect id="n" anchor-sibling="s" anchor-edge="below" anchor-gap=(px)10 x=(px)0 y=(px)0 w=(px)100 h=(px)40 fill="#cc0000""##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 0.0).abs() < 0.001
                && (y - 0.0).abs() < 0.001
                && (w - 100.0).abs() < 0.001
                && (h - 40.0).abs() < 0.001
        }),
        "expected FillRect at (0, 0, 100, 40): explicit x/y override anchor-edge; got: {rects:?}"
    );
}

// ── anchor-edge with absent/unresolved sibling → no anchor-derived position ──

#[test]
fn anchor_edge_without_sibling_no_position() {
    // A node with anchor-edge="below" but NO anchor-sibling. The validator
    // emits anchor.edge_without_sibling, and the compiler produces no entry
    // in the anchor map — the node falls back to its authored x/y.
    // We verify: (a) the diagnostic is produced, (b) the node lands at its
    // explicit (x=5, y=5) rather than at any anchor-derived position.
    use zenith_core::{KdlAdapter, KdlSource};

    let src = doc_with_children(
        r##"rect id="n" anchor-edge="below" x=(px)5 y=(px)5 w=(px)80 h=(px)40 fill="#333333""##,
    );

    // Validate path confirms the diagnostic.
    let kdl_doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&kdl_doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.edge_without_sibling"),
        "expected anchor.edge_without_sibling diagnostic; got: {:?}",
        report.diagnostics
    );

    // Compile path: node still renders at its authored (5, 5).
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    let rects = fill_rects(&result);
    assert!(
        rects.iter().any(|&(x, y, w, h)| {
            (x - 5.0).abs() < 0.001
                && (y - 5.0).abs() < 0.001
                && (w - 80.0).abs() < 0.001
                && (h - 40.0).abs() < 0.001
        }),
        "expected FillRect at (5, 5, 80, 40): fallback to authored x/y; got: {rects:?}"
    );
}

// ── anchor-parent without anchor → parent_without_anchor warning ─

#[test]
fn anchor_parent_without_anchor_warns() {
    use zenith_core::{KdlAdapter, KdlSource};

    // anchor-parent is set but there is no anchor value to position.
    let src = doc_with_frame_child(
        "x=(px)50 y=(px)40 w=(px)200 h=(px)100",
        r##"rect id="r.pna" anchor-parent=#true x=(px)0 y=(px)0 w=(px)40 h=(px)30 fill="#ff0000""##,
    );
    let doc = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "anchor.parent_without_anchor"),
        "expected anchor.parent_without_anchor warning; got: {:?}",
        report.diagnostics
    );
}
