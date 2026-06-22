//! `fold` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use super::common::*;

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
