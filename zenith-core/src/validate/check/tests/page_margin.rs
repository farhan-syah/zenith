//! `page_margin` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use super::common::*;

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
