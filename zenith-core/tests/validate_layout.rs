//! Integration tests: layout validation.
//!
//! Test bodies moved verbatim from the former in-`src` `validate/check/tests/`
//! concern files; only import paths changed (`crate::`/`super::common` ->
//! `zenith_core::`/`common`).

use std::collections::BTreeMap;

mod common;

use common::*;

// ── Page bleed validation ─────────────────────────────────────────────

/// A page with a valid positive px bleed produces no bleed warning.
#[test]
fn valid_bleed_no_warning() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(px(35.0));
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(
        !has_code(&report, "page.invalid_bleed"),
        "valid bleed must not warn: {:?}",
        codes(&report)
    );
}

/// A bleed declared with a non-resolvable unit (pct) warns but is not an error.
#[test]
fn bleed_bad_unit_warns_not_errors() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(Dimension {
        value: 5.0,
        unit: Unit::Pct,
    });
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(
        has_code(&report, "page.invalid_bleed"),
        "bad-unit bleed must warn: {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "bad-unit bleed must NOT be a hard error: {:?}",
        codes(&report)
    );
}

/// A negative bleed warns but is not an error.
#[test]
fn bleed_negative_warns_not_errors() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(px(-10.0));
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(has_code(&report, "page.invalid_bleed"));
    assert!(!report.has_errors());
}

// ══════════════════════════════════════════════════════════════════════
// margin.violation advisory tests (book live-area)
// ══════════════════════════════════════════════════════════════════════

/// Helper: a book page with the standard four margins set
/// (inner 225, outer 150, top 210, bottom 240 on a 1240×1754 spread).
fn book_page(id: &str, children: Vec<Node>) -> Page {
    let mut page = bounded_page(id, 1240.0, 1754.0, children);
    page.margin_inner = Some(px(225.0));
    page.margin_outer = Some(px(150.0));
    page.margin_top = Some(px(210.0));
    page.margin_bottom = Some(px(240.0));
    page
}

/// Returns `true` when a `margin.violation` advisory names `node_id`.
fn has_margin_violation_for(report: &ValidationReport, node_id: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.code == "margin.violation" && d.subject_id.as_deref() == Some(node_id))
}

#[test]
fn margin_recto_node_inside_live_area_no_violation() {
    // recto live area: x∈[225, 1090], y∈[210, 1514]. A rect fully inside.
    let doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("ok", 300.0, 300.0, 400.0, 400.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "node inside the live area must not trip margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_recto_node_left_of_inner_violates() {
    // mirror on, page 1 = recto → inner (225) insets the LEFT. A rect at x=100
    // crosses the left margin edge.
    let mut doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "a recto node left of margin-inner must trip margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_verso_parity_flips_inner_side() {
    // A rect at x=160 sits BETWEEN outer (150) and inner (225).
    // mirror on:
    //   - page 1 (recto): left inset = inner = 225 → 160 < 225 → VIOLATION.
    //   - page 2 (verso): left inset = outer = 150 → 160 ≥ 150 → NO violation.
    let recto_rect = rect_at("r.node", 160.0, 300.0, 400.0, 400.0);
    let verso_rect = rect_at("v.node", 160.0, 300.0, 400.0, 400.0);
    let mut doc = doc_with(
        vec![],
        vec![
            book_page("page.recto", vec![recto_rect]),
            book_page("page.verso", vec![verso_rect]),
        ],
    );
    doc.mirror_margins = Some(true);
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "r.node"),
        "recto node at x=160 (< inner 225) must violate; got {:?}",
        codes(&report)
    );
    assert!(
        !has_margin_violation_for(&report, "v.node"),
        "verso node at x=160 (≥ outer 150) must NOT violate (inner side flipped); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_rtl_parity_is_mirror_of_ltr() {
    // page-progression="rtl" mirrors the spread: recto binding is on the RIGHT
    // (left inset = outer = 150), verso binding on the LEFT (left inset = inner
    // = 225) — the exact opposite of the LTR parity above. A rect at x=160:
    //   - page 1 (recto, RTL): left inset = outer = 150 → 160 ≥ 150 → NO violation.
    //   - page 2 (verso, RTL): left inset = inner = 225 → 160 < 225 → VIOLATION.
    let recto_rect = rect_at("r.node", 160.0, 300.0, 400.0, 400.0);
    let verso_rect = rect_at("v.node", 160.0, 300.0, 400.0, 400.0);
    let mut doc = doc_with(
        vec![],
        vec![
            book_page("page.recto", vec![recto_rect]),
            book_page("page.verso", vec![verso_rect]),
        ],
    );
    doc.mirror_margins = Some(true);
    doc.page_progression = Some("rtl".to_owned());
    let report = validate(&doc);
    assert!(
        !has_margin_violation_for(&report, "r.node"),
        "RTL recto node at x=160 (≥ outer 150) must NOT violate (inner on right); got {:?}",
        codes(&report)
    );
    assert!(
        has_margin_violation_for(&report, "v.node"),
        "RTL verso node at x=160 (< inner 225) must violate (inner on left); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_guide_role_is_exempt() {
    // A node with role="guide" intentionally lives in the margins → exempt.
    let mut guide = rect_at("guide.line", 0.0, 300.0, 50.0, 50.0);
    if let Node::Rect(r) = &mut guide {
        r.role = Some("guide".to_owned());
    }
    let doc = doc_with(vec![], vec![book_page("page.recto", vec![guide])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "a role=guide node must be exempt from margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_absent_skips_check() {
    // A plain page with no margins → the check is skipped entirely.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.plain",
            1240.0,
            1754.0,
            vec![rect_at("any", 0.0, 0.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "absent margins must skip the margin check; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_document_default_cascades_to_bare_page() {
    // The page declares NO margins, but the DOCUMENT sets all four defaults
    // (inner 225, outer 150, top 210, bottom 240). The bare page inherits them,
    // so its live area is computed and a node outside it trips margin.violation.
    // recto live area: x∈[225, 1090]. A rect at x=100 crosses the left edge.
    let mut doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.bare",
            1240.0,
            1754.0,
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    doc.margin_inner = Some(px(225.0));
    doc.margin_outer = Some(px(150.0));
    doc.margin_top = Some(px(210.0));
    doc.margin_bottom = Some(px(240.0));
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "a bare page must inherit the document default margins; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_page_inner_overrides_doc_default() {
    // Doc default inner = 225; the page overrides inner = 100 (keeps doc
    // outer/top/bottom). recto left inset becomes 100, so a rect at x=120 is now
    // INSIDE the live area and must NOT violate — proving the per-page override
    // wins over the doc default for inner only.
    let mut page = bounded_page(
        "page.over",
        1240.0,
        1754.0,
        vec![rect_at("ok", 120.0, 300.0, 50.0, 50.0)],
    );
    page.margin_inner = Some(px(100.0));
    let mut doc = doc_with(vec![], vec![page]);
    doc.mirror_margins = Some(true);
    doc.margin_inner = Some(px(225.0));
    doc.margin_outer = Some(px(150.0));
    doc.margin_top = Some(px(210.0));
    doc.margin_bottom = Some(px(240.0));
    let report = validate(&doc);
    assert!(
        !has_margin_violation_for(&report, "ok"),
        "the page's own inner margin (100) must override the doc default (225); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_doc_default_off_is_byte_identical_to_page_only() {
    // Regression guard for the default-off path: a doc with page margins but NO
    // document margins must produce EXACTLY the diagnostics it did before the
    // cascade existed. We assert against an explicit per-page book page with no
    // doc-level margins set.
    let mut doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    // No doc-level margins set — the cascade reads the page's own values verbatim.
    assert!(doc.margin_inner.is_none());
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "page-only margins must behave exactly as before; got {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// document.invalid_page_progression warning tests
// ══════════════════════════════════════════════════════════════════════

#[test]
fn page_progression_rtl_is_valid() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_progression = Some("rtl".to_owned());
    let report = validate(&doc);
    assert!(!has_code(&report, "document.invalid_page_progression"));
}

#[test]
fn page_progression_invalid_warns() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_progression = Some("sideways".to_owned());
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_page_progression"),
        "an unrecognized page-progression must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page-progression warning must not be a hard error"
    );
}

// ══════════════════════════════════════════════════════════════════════
// page-parity-start / page parity warning tests
// ══════════════════════════════════════════════════════════════════════

#[test]
fn page_parity_start_verso_is_valid() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_parity_start = Some("verso".to_owned());
    let report = validate(&doc);
    assert!(!has_code(&report, "document.invalid_page_parity_start"));
    assert!(!report.has_errors());
}

#[test]
fn page_parity_start_invalid_warns() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_parity_start = Some("sideways".to_owned());
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_page_parity_start"),
        "an unrecognized page-parity-start must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page-parity-start warning must not be a hard error"
    );
}

#[test]
fn page_parity_override_valid_does_not_warn() {
    let mut page = minimal_page("page.one", vec![]);
    page.parity = Some("verso".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(!has_code(&report, "page.invalid_parity"));
    assert!(!report.has_errors());
}

#[test]
fn page_parity_override_invalid_warns() {
    let mut page = minimal_page("page.one", vec![]);
    page.parity = Some("upside-down".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "page.invalid_parity"),
        "an unrecognized per-page parity must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page parity warning must not be a hard error"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Configurable parity drives the mirrored-margin live area
// ══════════════════════════════════════════════════════════════════════

/// With `mirror-margins`, `page-parity-start="verso"` makes page 1 a VERSO, so
/// its binding (inner) margin moves to the right and the left inset becomes the
/// OUTER margin — flipping the `margin.violation` advisory's named parity and
/// live-area x relative to the default (page 1 = recto).
#[test]
fn page_parity_start_verso_flips_page_one_live_area() {
    // book_page: inner=225, outer=150 on a 1240-wide page.
    // Default (recto): live x = inner = 225. A node at x=160 crosses the LEFT.
    // start=verso (page 1 = verso): live x = outer = 150. The SAME node at x=160
    // is now INSIDE on the left, but a node at x=140 would cross.
    let probe = rect_at("probe", 160.0, 300.0, 400.0, 400.0);

    // Default: page 1 recto, inner=225 → node at 160 is left of the live area.
    let mut doc_default = doc_with(vec![], vec![book_page("p1", vec![probe.clone()])]);
    doc_default.mirror_margins = Some(true);
    let report_default = validate(&doc_default);
    assert!(
        has_margin_violation_for(&report_default, "probe"),
        "recto page-1 default: node at x=160 must violate the inner(225) live edge; got {:?}",
        codes(&report_default)
    );

    // start=verso: page 1 verso, outer=150 → node at 160 is now inside on the left.
    let mut doc_verso = doc_with(vec![], vec![book_page("p1", vec![probe.clone()])]);
    doc_verso.mirror_margins = Some(true);
    doc_verso.page_parity_start = Some("verso".to_owned());
    let report_verso = validate(&doc_verso);
    assert!(
        !has_margin_violation_for(&report_verso, "probe"),
        "verso page-1: node at x=160 must sit inside the outer(150) live edge; got {:?}",
        codes(&report_verso)
    );
}

/// An explicit per-page `parity="recto"` override flips a page back even when
/// `page-parity-start="verso"` would otherwise make it a verso.
#[test]
fn page_parity_override_flips_one_page_live_area() {
    let probe = rect_at("probe", 160.0, 300.0, 400.0, 400.0);

    let mut page = book_page("p1", vec![probe]);
    page.parity = Some("recto".to_owned());
    let mut doc = doc_with(vec![], vec![page]);
    doc.mirror_margins = Some(true);
    doc.page_parity_start = Some("verso".to_owned());
    let report = validate(&doc);
    // Override forces recto → inner=225 live edge → node at x=160 violates again.
    assert!(
        has_margin_violation_for(&report, "probe"),
        "explicit parity=recto must restore the inner(225) live edge; got {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// Fold content-crossing advisories
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a page with explicit folds and children (px page rect).
fn page_with_folds(id: &str, w: f64, h: f64, folds: Vec<Fold>, children: Vec<Node>) -> Page {
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
        safe_zones: Vec::new(),
        folds,
        children,
        source_span: None,
    }
}

/// Helper: build a fold of the given orientation at the given px position.
fn fold(id: &str, orientation: &str, position: f64) -> Fold {
    Fold {
        id: id.to_owned(),
        orientation: orientation.to_owned(),
        position: Some(px(position)),
        source_span: None,
    }
}

/// A vertical fold at x=1169 with a node spanning x=80..2430 → crossing.
#[test]
fn vertical_fold_crossed_by_node_advises() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "fold.content_crossing"),
        "expected fold.content_crossing; codes: {:?}",
        codes(&report)
    );
}

/// A node entirely left of the vertical fold → no crossing.
#[test]
fn vertical_fold_not_crossed_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            // Right edge at 80+200 = 280 < 1169 → fully left of the fold.
            vec![rect_at("rect.left", 80.0, 100.0, 200.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "node left of fold must not cross; codes: {:?}",
        codes(&report)
    );
}

/// A horizontal fold at y=500 with a node spanning y=100..900 → crossing.
#[test]
fn horizontal_fold_crossed_by_node_advises() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.h", "horizontal", 500.0)],
            vec![rect_at("rect.tall", 100.0, 100.0, 200.0, 800.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "fold.content_crossing"),
        "expected fold.content_crossing for horizontal fold; codes: {:?}",
        codes(&report)
    );
}

/// A node entirely above the horizontal fold → no crossing.
#[test]
fn horizontal_fold_not_crossed_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.h", "horizontal", 500.0)],
            // Bottom edge at 100+200 = 300 < 500 → fully above the fold.
            vec![rect_at("rect.top", 100.0, 100.0, 200.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "node above fold must not cross; codes: {:?}",
        codes(&report)
    );
}

/// A fold content-crossing is ADVISORY — it must not flag the report errored.
#[test]
fn fold_content_crossing_is_advisory_not_error() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fold.content_crossing" && d.severity == Severity::Advisory),
        "fold.content_crossing must be Advisory; codes: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

/// A fold with no resolvable position → no crossing advisory (skipped).
#[test]
fn fold_without_position_is_skipped() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![Fold {
                id: "fold.none".to_owned(),
                orientation: "vertical".to_owned(),
                position: None,
                source_span: None,
            }],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "fold without position must be skipped; codes: {:?}",
        codes(&report)
    );
}

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
        anchor_parent: None,
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
