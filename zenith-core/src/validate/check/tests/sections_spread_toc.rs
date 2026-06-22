//! `sections_spread_toc` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ══════════════════════════════════════════════════════════════════════
// Section validation tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a document with the given sections appended to a
/// single-page doc.
fn doc_with_sections(sections: Vec<SectionDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(vec![], pages);
    doc.sections = sections;
    doc
}

fn minimal_section(id: &str, start_page: &str) -> SectionDef {
    SectionDef {
        id: id.to_owned(),
        name: id.to_owned(),
        folio_start: None,
        folio_style: None,
        start_page: start_page.to_owned(),
        source_span: None,
    }
}

#[test]
fn clean_sections_block_no_diagnostics() {
    let page = minimal_page("p1", vec![]);
    let sec = minimal_section("sec.front", "p1");
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a clean sections block must produce no diagnostics; got: {:?}",
        codes(&report)
    );
}

#[test]
fn section_unknown_start_page_is_error() {
    let page = minimal_page("p1", vec![]);
    let sec = minimal_section("sec.x", "page.does.not.exist");
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.unknown_start_page"),
        "an unknown start-page reference must be a hard error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_duplicate_start_page_is_error() {
    let p1 = minimal_page("p1", vec![]);
    let p2 = minimal_page("p2", vec![]);
    let sec_a = minimal_section("sec.a", "p1");
    let sec_b = minimal_section("sec.b", "p1"); // same start_page
    let doc = doc_with_sections(vec![sec_a, sec_b], vec![p1, p2]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.duplicate_start_page"),
        "two sections sharing a start-page must be a hard error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_invalid_folio_style_is_warning() {
    let page = minimal_page("p1", vec![]);
    let mut sec = minimal_section("sec.bad", "p1");
    sec.folio_style = Some("arabic".to_owned()); // unrecognized
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.invalid_folio_style"),
        "an unknown folio-style must be a Warning; got {:?}",
        codes(&report)
    );
    // A Warning must NOT be counted as an error.
    assert!(
        !report.has_errors(),
        "section.invalid_folio_style must not be a hard error; got {:?}",
        codes(&report)
    );
}

#[test]
fn section_id_colliding_with_page_id_is_duplicate() {
    let page = minimal_page("shared", vec![]);
    let sec = minimal_section("shared", "shared"); // id == page id
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a section id colliding with a page id must be an id.duplicate error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_valid_folio_styles_produce_no_warning() {
    for style in ["decimal", "lower-roman", "upper-roman"] {
        let page = minimal_page("p1", vec![]);
        let mut sec = minimal_section("sec.ok", "p1");
        sec.folio_style = Some(style.to_owned());
        let doc = doc_with_sections(vec![sec], vec![page]);
        let report = validate(&doc);
        assert!(
            !has_code(&report, "section.invalid_folio_style"),
            "folio-style \"{style}\" must not warn; got {:?}",
            codes(&report)
        );
    }
}

// ── facing-pages / spread-gutter ─────────────────────────────────────────────

/// Parse + round-trip test: `facing-pages` and `spread-gutter` survive a
/// parse → format → parse cycle unchanged.
#[test]
fn facing_pages_and_spread_gutter_parse_and_round_trip() {
    use crate::format::format_document;
    use crate::parse::{KdlAdapter, KdlSource};

    let src = r#"zenith version=1 facing-pages=#true spread-gutter=(px)40 {
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" {
    page id="p1" w=(px)400 h=(px)600 {}
  }
}
"#;
    let doc1 = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    assert_eq!(
        doc1.facing_pages,
        Some(true),
        "facing-pages must parse to Some(true)"
    );
    assert_eq!(
        doc1.spread_gutter,
        Some(Dimension {
            value: 40.0,
            unit: Unit::Px
        }),
        "spread-gutter must parse to (px)40"
    );

    // Round-trip: format → re-parse, fields must survive unchanged.
    let formatted = format_document(&doc1).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("utf-8");
    let doc2 = KdlAdapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse must succeed");
    assert_eq!(
        doc2.facing_pages, doc1.facing_pages,
        "facing-pages must round-trip"
    );
    assert_eq!(
        doc2.spread_gutter, doc1.spread_gutter,
        "spread-gutter must round-trip"
    );
}

/// A `spread-gutter` with a non-px/pt unit (pct) → `document.invalid_spread_gutter` Warning.
#[test]
fn spread_gutter_pct_emits_invalid_spread_gutter_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: 10.0,
        unit: Unit::Pct,
    });
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_spread_gutter"),
        "pct spread-gutter must warn with document.invalid_spread_gutter; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "document.invalid_spread_gutter must not be a hard error; got {:?}",
        codes(&report)
    );
}

/// A negative `spread-gutter` → `document.invalid_spread_gutter` Warning.
#[test]
fn spread_gutter_negative_emits_invalid_spread_gutter_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: -5.0,
        unit: Unit::Px,
    });
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_spread_gutter"),
        "negative spread-gutter must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "document.invalid_spread_gutter must not be a hard error; got {:?}",
        codes(&report)
    );
}

/// A valid (px, non-negative) `spread-gutter` → no diagnostic.
#[test]
fn spread_gutter_valid_px_no_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: 40.0,
        unit: Unit::Px,
    });
    let report = validate(&doc);
    assert!(
        !has_code(&report, "document.invalid_spread_gutter"),
        "valid px spread-gutter must not warn; got {:?}",
        codes(&report)
    );
}

/// When `spread_gutter` is `None` (absent), no diagnostic should be emitted.
#[test]
fn spread_gutter_absent_no_warning() {
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "document.invalid_spread_gutter"),
        "absent spread-gutter must not warn; got {:?}",
        codes(&report)
    );
}

// ── Toc node validation ───────────────────────────────────────────────────────

/// Build a minimal `toc` node (no geometry, no styling).
fn toc_node_bare(id: &str, match_role: Option<&str>, match_style: Option<&str>) -> TocNode {
    TocNode {
        id: id.to_owned(),
        name: None,
        role: None,
        match_role: match_role.map(str::to_owned),
        match_style: match_style.map(str::to_owned),
        leader: None,
        folio_style: None,
        x: Some(px(50.0)),
        y: Some(px(100.0)),
        w: Some(px(400.0)),
        h: Some(px(200.0)),
        style: None,
        fill: None,
        font_family: None,
        font_size: None,
        opacity: None,
        visible: None,
        locked: None,
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn toc_with_match_role_does_not_warn_no_selector() {
    let toc = Node::Toc(toc_node_bare("toc.1", Some("heading"), None));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "toc.no_selector"),
        "toc with match-role must not emit toc.no_selector; got {:?}",
        codes(&report)
    );
}

#[test]
fn toc_with_match_style_does_not_warn_no_selector() {
    let toc = Node::Toc(toc_node_bare("toc.2", None, Some("Heading 1")));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "toc.no_selector"),
        "toc with match-style must not emit toc.no_selector; got {:?}",
        codes(&report)
    );
}

#[test]
fn toc_with_no_selector_warns() {
    let toc = Node::Toc(toc_node_bare("toc.3", None, None));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "toc.no_selector"),
        "toc without selector must warn toc.no_selector; got {:?}",
        codes(&report)
    );
}
