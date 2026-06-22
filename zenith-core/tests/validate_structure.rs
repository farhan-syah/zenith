//! Integration tests: structure validation (masters/fields, sections/
//! spread/toc, components/instances).
//!
//! Test bodies moved verbatim from the former in-`src` `validate/check/tests/`
//! concern files; only import paths changed.

use std::collections::BTreeMap;

mod common;

use common::*;

// ── Master-page + field validation ────────────────────────────────────────

/// Build a `field` node with the given id and type; all other props default.
fn field_node(id: &str, field_type: &str) -> FieldNode {
    FieldNode {
        id: id.to_owned(),
        name: None,
        role: None,
        field_type: field_type.to_owned(),
        recto: None,
        verso: None,
        target: None,
        folio_style: None,
        suppress_first: None,
        x: None,
        y: Some(px(80.0)),
        h: Some(px(40.0)),
        w: None,
        style: None,
        fill: None,
        font_family: None,
        font_size: None,
        opacity: None,
        visible: None,
        locked: None,
        anchor: None,
        anchor_zone: None,
        anchor_parent: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

/// `doc_with` plus a masters block.
fn doc_with_masters(tokens: Vec<Token>, masters: Vec<MasterDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(tokens, pages);
    doc.masters = masters;
    doc
}

#[test]
fn unknown_master_reference_is_error() {
    let mut page = minimal_page("p1", vec![]);
    page.master = Some("m.missing".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "master.unknown_reference"),
        "an unknown master reference must be a hard error; got {:?}",
        codes(&report)
    );
}

#[test]
fn declared_master_reference_is_accepted() {
    let master = MasterDef {
        id: "m.body".to_owned(),
        children: vec![],
        source_span: None,
    };
    let mut page = minimal_page("p1", vec![]);
    page.master = Some("m.body".to_owned());
    let doc = doc_with_masters(vec![], vec![master], vec![page]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "master.unknown_reference"),
        "a declared master reference must validate cleanly; got {:?}",
        codes(&report)
    );
}

#[test]
fn unknown_field_type_is_warning() {
    let field = Node::Field(field_node("f.bad", "marquee"));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![field])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "field.unknown_type"),
        "an unknown field type must be a warning; got {:?}",
        codes(&report)
    );
}

#[test]
fn known_field_types_have_no_unknown_type_warning() {
    for ty in ["running-head", "page-number", "page-ref", "page-count"] {
        let mut f = field_node("f", ty);
        if ty == "page-ref" {
            // give it a resolvable target so we isolate the type check
            f.target = Some("p1".to_owned());
        }
        let doc = doc_with(vec![], vec![minimal_page("p1", vec![Node::Field(f)])]);
        let report = validate(&doc);
        assert!(
            !has_code(&report, "field.unknown_type"),
            "{ty} is a known field type; got {:?}",
            codes(&report)
        );
    }
}

#[test]
fn unresolved_page_ref_target_is_warning() {
    let mut f = field_node("f.ref", "page-ref");
    f.target = Some("nowhere".to_owned());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![Node::Field(f)])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "field.unresolved_ref"),
        "a page-ref to a missing target must warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn resolved_page_ref_target_does_not_warn() {
    // The page contains a node with id "anchor"; a page-ref to it resolves.
    let anchor = Node::Rect(Box::new(RectNode {
        id: "anchor".to_owned(),
        name: None,
        role: None,
        x: Some(px(0.0)),
        y: Some(px(0.0)),
        w: Some(px(10.0)),
        h: Some(px(10.0)),
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
        shadow: None,
        filter: None,
        mask: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        anchor: None,
        anchor_zone: None,
        anchor_parent: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let mut f = field_node("f.ref", "page-ref");
    f.target = Some("anchor".to_owned());
    let doc = doc_with(
        vec![],
        vec![minimal_page("p1", vec![anchor, Node::Field(f)])],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "field.unresolved_ref"),
        "a page-ref to a present target must not warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn unresolved_footnote_ref_is_warning() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "Dangling" footnote-ref="fn.missing"
      }
    }
  }
}
"##;
    let doc = <zenith_core::KdlAdapter as zenith_core::KdlSource>::parse(
        &zenith_core::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "footnote.unresolved_ref"),
        "a span footnote-ref to a missing footnote must warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn unresolved_footnote_ref_on_shape_label_is_warning() {
    // A shape label carries `Vec<TextSpan>` just like a text node, so a dangling
    // `footnote-ref` on a shape-label span must be cross-checked too.
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      shape id="badge" x=(px)10 y=(px)10 w=(px)120 h=(px)60 kind="process" {
        span "Dangling" footnote-ref="fn.missing"
      }
    }
  }
}
"##;
    let doc = <zenith_core::KdlAdapter as zenith_core::KdlSource>::parse(
        &zenith_core::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "footnote.unresolved_ref"),
        "a shape-label span footnote-ref to a missing footnote must warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn resolved_footnote_ref_does_not_warn_and_id_is_unique() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "Evidence" footnote-ref="fn.1"
      }
      footnote id="fn.1" {
        span "See Chapter 4."
      }
    }
  }
}
"##;
    let doc = <zenith_core::KdlAdapter as zenith_core::KdlSource>::parse(
        &zenith_core::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "footnote.unresolved_ref"),
        "a span footnote-ref to a present footnote must not warn; got {:?}",
        codes(&report)
    );
    // The footnote id participates in global id-uniqueness: no duplicate is
    // flagged for a unique id, but a colliding id would be.
    assert!(
        !has_code(&report, "id.duplicate"),
        "a unique footnote id must not be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn duplicate_footnote_id_is_flagged() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      footnote id="dup" {
        span "First."
      }
      footnote id="dup" {
        span "Second."
      }
    }
  }
}
"##;
    let doc = <zenith_core::KdlAdapter as zenith_core::KdlSource>::parse(
        &zenith_core::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a footnote id colliding with another node must be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn master_id_participates_in_global_uniqueness() {
    // A master id colliding with a page id is a duplicate-id error.
    let master = MasterDef {
        id: "dup".to_owned(),
        children: vec![],
        source_span: None,
    };
    let mut page = minimal_page("dup", vec![]);
    page.master = Some("dup".to_owned());
    let doc = doc_with_masters(vec![], vec![master], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a master id colliding with a page id must be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn master_local_ids_are_scoped_per_master() {
    // The same local id may appear in two different masters without colliding.
    let m1 = MasterDef {
        id: "m1".to_owned(),
        children: vec![Node::Field(field_node("shared", "page-number"))],
        source_span: None,
    };
    let m2 = MasterDef {
        id: "m2".to_owned(),
        children: vec![Node::Field(field_node("shared", "page-number"))],
        source_span: None,
    };
    let doc = doc_with_masters(vec![], vec![m1, m2], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "id.duplicate"),
        "the same local id in two masters must not collide; got {:?}",
        codes(&report)
    );
}

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
    use zenith_core::format::format_document;
    use zenith_core::{KdlAdapter, KdlSource};

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
        anchor_parent: None,
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

// ── Component / instance validation ─────────────────────────────────────────

mod component_validation {
    use zenith_core::validate;
    use zenith_core::{KdlAdapter, KdlSource};

    fn parse_doc(src: &str) -> zenith_core::Document {
        KdlAdapter.parse(src.as_bytes()).expect("must parse")
    }

    fn has_code(report: &zenith_core::ValidationReport, code: &str) -> bool {
        report.diagnostics.iter().any(|d| d.code == code)
    }

    const BASE_TOKENS: &str = r##"  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
    token id="color.fg" type="color" value="#fafafa"
  }
  styles {}"##;

    #[test]
    fn unknown_component_reference_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="real.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="missing" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_reference"),
            "expected component.unknown_reference: {:?}",
            report.diagnostics
        );
        assert!(report.has_errors());
    }

    #[test]
    fn unknown_override_target_is_warning() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="c.one" x=(px)0 y=(px)0 {{
        override ref="does.not.exist" {{ span "X" }}
      }}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_override_target"),
            "expected component.unknown_override_target: {:?}",
            report.diagnostics
        );
        // It is a Warning, not a hard error.
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "component.unknown_override_target"
                    && d.severity == zenith_core::Severity::Error)
        );
    }

    #[test]
    fn duplicate_component_id_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="dup" {{
      rect id="a" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
    component id="dup" {{
      rect id="b" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "duplicate component id must be id.duplicate: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn local_child_ids_do_not_collide_across_components() {
        // Two components both declare a child id "bg" and "label" — this must
        // NOT trigger id.duplicate because component child ids are local.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.a" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "A" }}
    }}
    component id="c.b" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "B" }}
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            !has_code(&report, "id.duplicate"),
            "component-local ids must not collide across components: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn instance_id_participates_in_global_uniqueness() {
        // An instance id that collides with a page node id → id.duplicate.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      rect id="dup.id" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      instance id="dup.id" component="c.one" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "instance id must participate in global uniqueness: {:?}",
            report.diagnostics
        );
    }
}
