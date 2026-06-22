//! `masters_fields` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

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
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
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
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
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
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
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
