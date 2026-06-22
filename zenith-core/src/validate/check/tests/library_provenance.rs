//! `library_provenance` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ══════════════════════════════════════════════════════════════════════
// Library block validation tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a document with the given libraries appended to a
/// single-page doc.
fn doc_with_libraries(libraries: Vec<LibraryDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(vec![], pages);
    doc.libraries = libraries;
    doc
}

fn minimal_library(id: &str) -> LibraryDef {
    LibraryDef {
        id: id.to_owned(),
        version: None,
        hash: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn clean_libraries_block_no_diagnostics() {
    let lib = LibraryDef {
        id: "@acme/brand-kit".to_owned(),
        version: Some("1.4.0".to_owned()),
        hash: Some("sha256-abc".to_owned()),
        source_span: None,
        unknown_props: BTreeMap::new(),
    };
    let doc = doc_with_libraries(vec![lib], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a well-formed libraries block must produce no diagnostics; got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn library_duplicate_id_is_error() {
    let a = minimal_library("@acme/brand-kit");
    let b = minimal_library("@acme/brand-kit"); // duplicate id
    let doc = doc_with_libraries(vec![a, b], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "two libraries sharing an id must trigger id.duplicate; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn library_unknown_property_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "registry".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("x".to_owned()),
            ty: Some("token".to_owned()),
        },
    );
    let lib = LibraryDef {
        id: "@acme/brand-kit".to_owned(),
        version: None,
        hash: None,
        source_span: None,
        unknown_props,
    };
    let doc = doc_with_libraries(vec![lib], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "library.unknown_property"),
        "an unknown prop on a library must warn; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "library.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ── provenance: cross-reference validation ────────────────────────────

/// A document with the given provenance records, libraries, and page children.
/// The page (id "p1") carries `children`, so node refs can resolve.
fn doc_with_provenance(
    provenance: Vec<ProvenanceDef>,
    libraries: Vec<LibraryDef>,
    children: Vec<Node>,
) -> Document {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", children)]);
    doc.libraries = libraries;
    doc.provenance = provenance;
    doc
}

fn minimal_provenance(id: &str, node: &str, library: &str) -> ProvenanceDef {
    ProvenanceDef {
        id: id.to_owned(),
        node: node.to_owned(),
        library: library.to_owned(),
        item: None,
        linked: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn clean_provenance_record_no_diagnostics() {
    // node "btn" exists on the page; library "@acme/brand-kit" is declared.
    let prov = ProvenanceDef {
        id: "prov.btn".to_owned(),
        node: "btn".to_owned(),
        library: "@acme/brand-kit".to_owned(),
        item: Some("button".to_owned()),
        linked: Some(true),
        source_span: None,
        unknown_props: BTreeMap::new(),
    };
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a fully-resolved provenance record must produce no diagnostics; got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn provenance_unknown_node_is_error() {
    let prov = minimal_provenance("prov.ghost", "ghost", "@acme/brand-kit");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_node"),
        "a provenance record referencing a non-existent node must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_node_may_be_a_declared_token() {
    // A provenance record whose `node` is a declared TOKEN id (a token imported
    // from a library) validates clean — the target need not be a node.
    let prov = minimal_provenance("prov.noir", "noir", "@zenith/filters");
    let mut doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@zenith/filters")],
        vec![minimal_rect("btn", None)],
    );
    doc.tokens.tokens.push(color_token_hex("noir", "#000000"));
    let report = validate(&doc);
    assert!(
        !has_code(&report, "provenance.unknown_node"),
        "a provenance record targeting a declared token must not error; got {:?}",
        codes(&report)
    );
}

#[test]
fn provenance_unknown_library_is_error() {
    let prov = minimal_provenance("prov.btn", "btn", "@nope/x");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_library"),
        "a provenance record referencing an undeclared library must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_duplicate_id_is_error() {
    let a = minimal_provenance("prov.dup", "btn", "@acme/brand-kit");
    let b = minimal_provenance("prov.dup", "btn", "@acme/brand-kit"); // duplicate id
    let doc = doc_with_provenance(
        vec![a, b],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "two provenance records sharing an id must trigger id.duplicate; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_unknown_property_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "registry".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("x".to_owned()),
            ty: Some("token".to_owned()),
        },
    );
    let prov = ProvenanceDef {
        id: "prov.btn".to_owned(),
        node: "btn".to_owned(),
        library: "@acme/brand-kit".to_owned(),
        item: None,
        linked: None,
        source_span: None,
        unknown_props,
    };
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_property"),
        "an unknown prop on an origin must warn; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "provenance.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    // The unknown-property warning is not itself an error; node + library resolve.
    assert!(!report.has_errors());
}

#[test]
fn provenance_node_may_be_a_declared_action() {
    // A provenance record whose `node` is a declared ACTION id validates clean —
    // an action imported from a library is a valid provenance target.
    let prov = minimal_provenance("prov.brand", "apply-brand-kit", "@acme/brand-kit");
    let mut doc = doc_with_provenance(vec![prov], vec![minimal_library("@acme/brand-kit")], vec![]);
    doc.actions.push(ActionDef {
        id: "apply-brand-kit".to_owned(),
        label: Some("Apply Brand Kit".to_owned()),
        version: None,
        tx_json: "{}".to_owned(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    });
    let report = validate(&doc);
    assert!(
        !has_code(&report, "provenance.unknown_node"),
        "a provenance record targeting a declared action must not fire unknown_node; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn provenance_node_nonexistent_id_still_errors() {
    // Negative case: a provenance `node` that matches neither any node id, nor
    // any declared token id, nor any declared action id must still fire
    // `provenance.unknown_node`.
    let prov = minimal_provenance("prov.ghost2", "does-not-exist", "@acme/brand-kit");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_node"),
        "a provenance record with a non-existent node/token/action id must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}
