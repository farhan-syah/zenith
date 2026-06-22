//! `offcanvas` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ══════════════════════════════════════════════════════════════════════
// off_canvas advisory tests
// ══════════════════════════════════════════════════════════════════════

/// A rect with x=-20 on a 100×100 page → off_canvas advisory.
#[test]
fn rect_negative_x_is_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.out", -20.0, 0.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "expected off_canvas advisory; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert_eq!(diag.subject_id.as_deref(), Some("rect.out"));
    // off_canvas is advisory only — no errors.
    assert!(!report.has_errors());
}

/// A rect fully inside the page → NO off_canvas advisory.
#[test]
fn rect_fully_inside_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.in", 10.0, 10.0, 80.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "rect fully inside should NOT get off_canvas; codes: {:?}",
        codes(&report)
    );
}

/// A rect at x=80, w=40 (right edge=120 > page_w=100) → off_canvas.
#[test]
fn rect_overflowing_right_edge_is_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.wide", 80.0, 0.0, 40.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "rect extending past right edge should be off_canvas; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert!(!report.has_errors());
}

/// A rect exactly touching the page edges (x=0,y=0,w=100,h=100) → no off_canvas.
#[test]
fn rect_exactly_on_page_edge_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.edge", 0.0, 0.0, 100.0, 100.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "rect exactly on page boundary should NOT be off_canvas; codes: {:?}",
        codes(&report)
    );
}

/// Helper: rect at (x, y, w, h) in px with an optional rotation in degrees.
fn rect_at_rotated(id: &str, x: f64, y: f64, w: f64, h: f64, rotate_deg: Option<f64>) -> Node {
    let rotate = rotate_deg.map(|deg| Dimension {
        value: deg,
        unit: Unit::Deg,
    });
    Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: None,
        stroke: None,
        stroke_width: None,
        stroke_alignment: None,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
        border_top: None,
        border_bottom: None,
        border_left: None,
        border_right: None,
        border_width: None,
        stroke_outer: None,
        stroke_outer_width: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate,
        blend_mode: None,
        blur: None,
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// A rect centered on the page, small enough that its authored bbox is fully
/// inside, but rotated 45° so its rotated AABB extends beyond the page edge →
/// off_canvas advisory fires.
///
/// Page: 100×100. Rect: x=40, y=40, w=20, h=20 (authored bbox fully inside,
/// center at (50,50)). At 45° the AABB half-extents are
/// hw_rot = (10+10)*cos(45°) ≈ 14.14, so AABB = (35.86..64.14, 35.86..64.14)
/// which is still inside. Use a more extreme rect: x=35, y=35, w=30, h=30,
/// center (50,50). AABB half-extent = (15+15)/sqrt(2)*sqrt(2) = 15*sqrt(2) ≈
/// 21.2. AABB: (28.8..71.2, 28.8..71.2) — still inside.
///
/// Simpler: rect x=0, y=40, w=80, h=20 centered at (40, 50). At 45° the
/// AABB half-extents: x-half = |40*cos45 - 10*sin45| + ... use the standard
/// formula: hw = (|w/2|*|cos| + |h/2|*|sin|) = 40*cos45 + 10*sin45 ≈ 35.36.
/// hh = 40*sin45 + 10*cos45 ≈ 35.36. AABB: (40-35.36, 50-35.36) = (4.64, 14.64)
/// to (75.36, 85.36) — still inside. Need larger rect.
///
/// Rect x=0, y=0, w=80, h=20, center (40, 10). At 45°: hw=40*cos45+10*sin45≈35.36,
/// hh=40*sin45+10*cos45≈35.36. AABB: (4.64-35.36=-30.72, ...) → off_canvas!
#[test]
fn rotated_aabb_off_canvas_fires() {
    // A wide, short rect in the top-left: authored bbox inside the page, but
    // its 45° rotated AABB extends outside.
    // Page: 200×200. Rect: x=0, y=0, w=160, h=20 → center (80, 10).
    // At 45°: hw = 80*cos45 + 10*sin45 ≈ 63.6, hh = 80*sin45 + 10*cos45 ≈ 63.6.
    // AABB: (80-63.6, 10-63.6) = (16.4, -53.6) → ay < 0 → off_canvas.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.rot",
            200.0,
            200.0,
            vec![rect_at_rotated(
                "rect.rot",
                0.0,
                0.0,
                160.0,
                20.0,
                Some(45.0),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "rotated rect whose AABB exits page should fire off_canvas; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert_eq!(diag.subject_id.as_deref(), Some("rect.rot"));
}

/// Same authored box as above but unrotated (rotate=None) → authored bbox is
/// fully inside the page → no off_canvas advisory.
#[test]
fn unrotated_inside_page_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.norot",
            200.0,
            200.0,
            vec![rect_at_rotated("rect.norot", 0.0, 0.0, 160.0, 20.0, None)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "unrotated rect inside page should NOT fire off_canvas; codes: {:?}",
        codes(&report)
    );
}
