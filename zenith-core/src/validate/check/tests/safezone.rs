//! `safezone` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ══════════════════════════════════════════════════════════════════════
// safe-zone advisory tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a page with explicit safe-zones and children (px page rect).
fn page_with_zones(
    id: &str,
    w: f64,
    h: f64,
    safe_zones: Vec<SafeZone>,
    children: Vec<Node>,
) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(w),
        height: px(h),
        background: None,
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones,
        folds: Vec::new(),
        children,
        source_span: None,
    }
}

/// Helper: build a safe-zone rect of the given type at (x, y, w, h) px.
fn zone(id: &str, zone_type: SafeZoneType, x: f64, y: f64, w: f64, h: f64) -> SafeZone {
    SafeZone {
        id: id.to_owned(),
        zone_type,
        x: px(x),
        y: px(y),
        w: px(w),
        h: px(h),
        label: None,
        source_span: None,
    }
}

/// Helper: a full-bleed background image covering the whole page rect.
fn image_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
    Node::Image(ImageNode {
        id: id.to_owned(),
        name: None,
        role: None,
        asset: "asset.bg".to_owned(),
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        src_x: None,
        src_y: None,
        src_w: None,
        src_h: None,
        fit: None,
        clip: None,
        clip_radius: None,
        object_position_x: None,
        object_position_y: None,
        opacity: None,
        shadow: None,
        filter: None,
        mask: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

/// An exclusion zone overlapped by a content node → `safe_zone.violation`.
#[test]
fn exclusion_zone_overlapping_node_violates() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.bad", 50.0, 380.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "safe_zone.violation"),
        "expected safe_zone.violation; codes: {:?}",
        codes(&report)
    );
}

/// An exclusion zone NOT overlapped by a content node → no violation.
#[test]
fn exclusion_zone_non_overlapping_node_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.ok", 600.0, 40.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "non-overlapping node must not violate; codes: {:?}",
        codes(&report)
    );
}

/// A full-bleed background image overlapping an exclusion zone → no violation
/// (full-bleed nodes are exempt).
#[test]
fn full_bleed_background_is_exempt_from_exclusion_zone() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![image_at("img.bg", 0.0, 0.0, 1500.0, 500.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "full-bleed background must be exempt; codes: {:?}",
        codes(&report)
    );
}

/// A required zone with a node fully outside → `safe_zone.violation`.
#[test]
fn required_zone_node_fully_outside_violates() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.title",
                SafeZoneType::Required,
                600.0,
                40.0,
                300.0,
                100.0,
            )],
            vec![rect_at("rect.out", 0.0, 400.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "safe_zone.violation"),
        "node outside required zone must violate; codes: {:?}",
        codes(&report)
    );
}

/// A required zone with a node overlapping it → no violation (lenient).
#[test]
fn required_zone_overlapping_node_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.title",
                SafeZoneType::Required,
                600.0,
                40.0,
                300.0,
                100.0,
            )],
            vec![rect_at("rect.in", 650.0, 50.0, 100.0, 40.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "node overlapping required zone must not violate; codes: {:?}",
        codes(&report)
    );
}

/// A safe-zone violation is ADVISORY — it must not flag the report as errored.
#[test]
fn safe_zone_violation_is_advisory_not_error() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.bad", 50.0, 380.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "safe_zone.violation" && d.severity == Severity::Advisory),
        "safe_zone.violation must be Advisory severity; codes: {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "safe_zone.violation must not make the report errored"
    );
}
