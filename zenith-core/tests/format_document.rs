//! Integration tests for the canonical writer: document.
//!
//! Document-structure blocks — components/instances, masters/fields, page
//! margins/parity, sections, libraries, provenance, and actions — round-trip.
//!
//! Moved verbatim from the former in-`src` `format/writer/tests.rs`; the body of
//! every test is unchanged — only import paths were rewritten to the public
//! `zenith_core` surface. Span-stripping helpers live in `common`.

mod common;

use common::*;
use zenith_core::format::format_document;

/// A `.zen` document exercising the `components` block, an `instance` node, and
/// an `override` with a `span` replacement, a `fill`, and a `visible` flag.
const COMPONENT_DOC: &str = r##"zenith version=1 {
  project id="proj.comp" name="Component Project"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
    token id="color.fg" type="color" value="#fafafa"
    token id="color.alt" type="color" value="#ff0000"
    token id="size.body" type="dimension" value=(pt)18
  }
  styles {
  }
  components {
    component id="panel.master" {
      rect id="bg" x=(px)0 y=(px)0 w=(px)200 h=(px)120 fill=(token)"color.bg"
      text id="label" x=(px)10 y=(px)10 w=(px)180 h=(px)40 fill=(token)"color.fg" {
        span "Default"
      }
    }
  }
  document id="doc.comp" title="Comp Doc" {
    page id="page.one" w=(px)640 h=(px)360 background=(token)"color.bg" {
      instance id="inst.1" component="panel.master" x=(px)0 y=(px)0 {
        override ref="label" fill=(token)"color.alt" visible=#true {
          span "Back"
        }
      }
      instance id="inst.2" component="panel.master" x=(px)220 y=(px)0 {
        override ref="label" {
          span "Center"
        }
      }
    }
  }
}
"##;

/// **components / instance / override round-trip**: parse → format → parse must
/// yield the same AST (spans excluded), and the formatter must be idempotent.
#[test]
fn test_component_instance_override_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(COMPONENT_DOC.as_bytes())
        .expect("original parse");

    // Structural sanity: one component, two instances, first with rich override.
    assert_eq!(doc_orig.components.len(), 1);
    assert_eq!(doc_orig.components[0].id, "panel.master");
    assert_eq!(doc_orig.components[0].children.len(), 2);
    match &doc_orig.body.pages[0].children[0] {
        Node::Instance(i) => {
            assert_eq!(i.id, "inst.1");
            assert_eq!(i.component, "panel.master");
            assert_eq!(i.overrides.len(), 1);
            let ov = &i.overrides[0];
            assert_eq!(ov.ref_id, "label");
            assert_eq!(ov.visible, Some(true));
            assert!(ov.fill.is_some());
            let spans = ov.spans.as_ref().expect("override spans");
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].text, "Back");
        }
        other => panic!("expected Instance node, got {other:?}"),
    }

    let formatted = format_document(&doc_orig).expect("format");
    let doc_reparsed = adapter.parse(&formatted).expect("re-parse");

    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "components/instance/override must survive a format round-trip (spans excluded)"
    );

    // Idempotency.
    let s2 = format_document(&adapter.parse(&formatted).expect("re-parse for idempotency"))
        .expect("format 2");
    assert_eq!(
        String::from_utf8(formatted).unwrap(),
        String::from_utf8(s2).unwrap(),
        "component formatting must be idempotent"
    );
}

/// **Page bleed round-trip**: a `bleed` attribute parses, formats back into the
/// canonical text (right after `background`), and survives parse→format→parse.
#[test]
fn test_page_bleed_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.bleed" name="Bleed"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.bleed" title="Bleed" {
    page id="page.bleed" w=(px)400 h=(px)600 bleed=(px)35 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // The parsed page carries the bleed dimension.
    let page = &doc.body.pages[0];
    let bleed = page.bleed.as_ref().expect("bleed must be present");
    assert_eq!(bleed.value, 35.0);
    assert!(matches!(bleed.unit, zenith_core::Unit::Px));

    // Canonical form preserves it verbatim.
    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    assert!(
        text.contains("bleed=(px)35"),
        "formatted output must carry bleed; got:\n{text}"
    );

    // Round-trip AST equality (spans stripped).
    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "bleed must survive parse → format → parse"
    );
}

/// **Page mirrored margins + document mirror-margins + page-progression
/// round-trip**: the four page margins, the document `mirror-margins` toggle,
/// and `page-progression` all parse, format into canonical text, and survive
/// parse → format → parse.
#[test]
fn test_book_margins_round_trip() {
    let src = r##"zenith version=1 mirror-margins=#true page-progression="rtl" {
  project id="proj.book" name="Book"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.book" title="Book" {
    page id="page.recto" w=(px)1240 h=(px)1754 margin-inner=(px)225 margin-outer=(px)150 margin-top=(px)210 margin-bottom=(px)240 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // Document-level toggles.
    assert_eq!(doc.mirror_margins, Some(true));
    assert_eq!(doc.page_progression.as_deref(), Some("rtl"));

    // Page-level margins.
    let page = &doc.body.pages[0];
    assert_eq!(page.margin_inner.as_ref().expect("inner").value, 225.0);
    assert_eq!(page.margin_outer.as_ref().expect("outer").value, 150.0);
    assert_eq!(page.margin_top.as_ref().expect("top").value, 210.0);
    assert_eq!(page.margin_bottom.as_ref().expect("bottom").value, 240.0);

    // Canonical form preserves every attribute verbatim.
    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    for needle in [
        "mirror-margins=#true",
        "page-progression=\"rtl\"",
        "margin-inner=(px)225",
        "margin-outer=(px)150",
        "margin-top=(px)210",
        "margin-bottom=(px)240",
    ] {
        assert!(
            text.contains(needle),
            "formatted output must carry `{needle}`; got:\n{text}"
        );
    }

    // Round-trip AST equality (spans stripped).
    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "book margins + mirror-margins + page-progression must survive round-trip"
    );
}

/// A `.zen` document exercising the masters block, a page `master` attribute,
/// and all three field types (running-head, page-number, page-ref) — both via a
/// master and inline in a page.
const MASTER_FIELD_DOC: &str = r##"zenith version=1 mirror-margins=#true {
  project id="proj.mf" name="MF"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#111111"
  }
  styles {
  }
  masters {
    master id="m.body" {
      field id="rh" type="running-head" recto="Recto Title" verso="Verso Title" y=(px)80 h=(px)40 fill=(token)"color.ink"
      field id="folio" type="page-number" y=(px)1820 h=(px)40 fill=(token)"color.ink"
    }
  }
  document id="doc.mf" title="MF" {
    page id="p1" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
      field id="xref" type="page-ref" target="anchor" x=(px)10 y=(px)10 w=(px)80 h=(px)30 fill=(token)"color.ink"
    }
    page id="p2" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
      text id="anchor" x=(px)160 y=(px)200 w=(px)900 h=(px)40 fill=(token)"color.ink" {
        span "Body"
      }
    }
  }
}
"##;

/// **Masters + field round-trip**: the masters block, the page `master`
/// attribute, and every field node must survive parse → format → parse with an
/// identical AST (spans excluded), and the formatter must be idempotent.
#[test]
fn test_master_field_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(MASTER_FIELD_DOC.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");
    let text = String::from_utf8(formatted.clone()).expect("utf8");

    // The masters block emits after components / before document, and a field
    // node carries the canonical attribute order.
    assert!(text.contains("masters {"), "masters block missing:\n{text}");
    assert!(
        text.contains("master id=\"m.body\""),
        "master decl missing:\n{text}"
    );
    assert!(
        text.contains(
            "field id=\"rh\" type=\"running-head\" recto=\"Recto Title\" verso=\"Verso Title\""
        ),
        "running-head field line missing/incorrect:\n{text}"
    );
    assert!(
        text.contains("master=\"m.body\""),
        "page master attribute missing:\n{text}"
    );
    assert!(
        text.contains("field id=\"xref\" type=\"page-ref\" target=\"anchor\""),
        "page-ref field line missing/incorrect:\n{text}"
    );

    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");

    // Idempotency (format the re-parsed doc before it is consumed by strip).
    let s2 = format_document(&doc_reparsed).expect("format 2");
    assert_eq!(
        text,
        String::from_utf8(s2).expect("utf8 2"),
        "format must be idempotent for masters + fields"
    );

    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "masters + field must survive a format round-trip (spans excluded)"
    );
}

/// **Page parity attributes round-trip**: the document `page-parity-start` and a
/// per-page `parity` override parse, format into canonical text, and survive
/// parse → format → parse.
#[test]
fn test_page_parity_round_trip() {
    let src = r##"zenith version=1 page-parity-start="verso" {
  project id="proj.par" name="Parity"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.par" title="Parity" {
    page id="page.one" w=(px)1240 h=(px)1754 parity="recto" {
    }
    page id="page.two" w=(px)1240 h=(px)1754 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.page_parity_start.as_deref(), Some("verso"));
    assert_eq!(doc.body.pages[0].parity.as_deref(), Some("recto"));
    assert_eq!(doc.body.pages[1].parity, None);

    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    for needle in ["page-parity-start=\"verso\"", "parity=\"recto\""] {
        assert!(
            text.contains(needle),
            "formatted output must carry `{needle}`; got:\n{text}"
        );
    }

    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "page-parity-start + page parity must survive round-trip"
    );
}

/// **Document-level default margins round-trip**: the four `margin-*` attributes
/// on the root `zenith` node parse, format into canonical text, and survive
/// parse → format → parse.
#[test]
fn test_document_default_margins_round_trip() {
    let src = r##"zenith version=1 mirror-margins=#true margin-inner=(px)225 margin-outer=(px)150 margin-top=(px)210 margin-bottom=(px)240 {
  project id="proj.dm" name="DocMargins"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.dm" title="DocMargins" {
    page id="page.one" w=(px)1240 h=(px)1754 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.margin_inner.as_ref().expect("inner").value, 225.0);
    assert_eq!(doc.margin_outer.as_ref().expect("outer").value, 150.0);
    assert_eq!(doc.margin_top.as_ref().expect("top").value, 210.0);
    assert_eq!(doc.margin_bottom.as_ref().expect("bottom").value, 240.0);
    // The page declares no margins — it inherits the document defaults.
    assert_eq!(doc.body.pages[0].margin_inner, None);

    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    for needle in [
        "margin-inner=(px)225",
        "margin-outer=(px)150",
        "margin-top=(px)210",
        "margin-bottom=(px)240",
    ] {
        assert!(
            text.contains(needle),
            "formatted output must carry `{needle}`; got:\n{text}"
        );
    }

    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "document default margins must survive round-trip"
    );
}

/// **Parse test**: a `sections { section … }` block round-trips into
/// `Document.sections` with correct field values (including folio-start,
/// folio-style, and underscore aliases for both).
#[test]
fn test_sections_parse_fields() {
    let src = r##"zenith version=1 {
  project id="proj.s" name="S"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  sections {
    section id="sec.front" name="Front Matter" folio-start=1 folio-style="lower-roman" start-page="page.cover"
    section id="sec.body" name="Body" folio_start=10 folio_style="decimal" start_page="page.body"
  }
  document id="doc.s" title="S" {
    page id="page.cover" w=(px)640 h=(px)360 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"c"
    }
    page id="page.body" w=(px)640 h=(px)360 {
      rect id="r2" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"c"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse sections doc");

    assert_eq!(doc.sections.len(), 2, "expected 2 sections");

    let front = &doc.sections[0];
    assert_eq!(front.id, "sec.front");
    assert_eq!(front.name, "Front Matter");
    assert_eq!(front.folio_start, Some(1));
    assert_eq!(front.folio_style.as_deref(), Some("lower-roman"));
    assert_eq!(front.start_page, "page.cover");

    let body = &doc.sections[1];
    assert_eq!(body.id, "sec.body");
    assert_eq!(body.name, "Body");
    assert_eq!(body.folio_start, Some(10));
    assert_eq!(body.folio_style.as_deref(), Some("decimal"));
    assert_eq!(body.start_page, "page.body");
}

/// **Serialize round-trip**: parse a doc with sections → format → re-parse →
/// sections identical (spans stripped). Also assert the formatted output
/// contains the `section id=..` line.
#[test]
fn test_sections_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.rt" name="RT"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  sections {
    section id="sec.intro" name="Introduction" folio-start=1 folio-style="lower-roman" start-page="pg1"
    section id="sec.main" name="Main" start-page="pg2"
  }
  document id="doc.rt" title="RT" {
    page id="pg1" w=(px)640 h=(px)360 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
    }
    page id="pg2" w=(px)640 h=(px)360 {
      rect id="r2" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        formatted_str.contains(r#"section id="sec.intro""#),
        "formatted output must contain section id; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("folio-start=1"),
        "formatted output must contain folio-start; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"folio-style="lower-roman""#),
        "formatted output must contain folio-style; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).sections,
        strip_spans(reparsed).sections,
        "sections must survive a parse → format → parse round-trip"
    );
}

// ── libraries: parse, serialize, and round-trip ───────────────────────

/// **Serialize round-trip**: parse a doc with a `libraries` block (id/version/
/// hash plus an annotated unknown prop) → format → re-parse → libraries
/// identical (spans stripped). Also assert the formatted output contains both
/// `library` lines with their id/version/hash and that the annotated unknown
/// prop survives. Mirrors `test_sections_round_trip`.
#[test]
fn test_libraries_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.lib" name="LIB"
  libraries {
    library id="@acme/brand-kit" version="1.4.0" hash="sha256-abc" registry=(token)"x"
    library id="@acme/icons" version="2.0.1"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.lib" title="LIB" {
    page id="pg1" w=(px)640 h=(px)360 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.libraries.len(), 2, "expected 2 libraries");
    let brand = &doc.libraries[0];
    assert_eq!(brand.id, "@acme/brand-kit");
    assert_eq!(brand.version.as_deref(), Some("1.4.0"));
    assert_eq!(brand.hash.as_deref(), Some("sha256-abc"));
    let registry = brand
        .unknown_props
        .get("registry")
        .expect("annotated unknown prop must be preserved");
    assert_eq!(
        registry.ty.as_deref(),
        Some("token"),
        "unknown prop annotation must survive"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        formatted_str.contains(r#"library id="@acme/brand-kit" version="1.4.0" hash="sha256-abc""#),
        "formatted output must contain the brand-kit library line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"library id="@acme/icons" version="2.0.1""#),
        "formatted output must contain the icons library line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"registry=(token)"x""#),
        "annotated unknown prop must round-trip; got:\n{formatted_str}"
    );
    // Canonical order: assets, then libraries, then tokens.
    let libs_at = formatted_str.find("libraries {").expect("libraries block");
    let tokens_at = formatted_str.find("tokens ").expect("tokens block");
    assert!(
        libs_at < tokens_at,
        "libraries must be emitted before tokens; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).libraries,
        strip_spans(reparsed).libraries,
        "libraries must survive a parse → format → parse round-trip (idempotent)"
    );
}

// ── provenance: parse, serialize, and round-trip ──────────────────────

/// **Serialize round-trip**: parse a doc with a `provenance` block (two origin
/// records, one carrying every field + an annotated unknown prop) plus a
/// matching `libraries` block and the referenced nodes → format → re-parse →
/// provenance identical (spans stripped). Also assert all fields are emitted,
/// the block comes AFTER `sections`, and the annotated unknown prop survives.
/// Mirrors `test_libraries_round_trip`.
#[test]
fn test_provenance_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.prov" name="PROV"
  libraries {
    library id="@acme/brand-kit" version="1.4.0"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  sections {
    section id="sec.body" name="Body" start-page="pg1"
  }
  provenance {
    origin id="prov.btn" node="btn" library="@acme/brand-kit" item="button" linked=#true registry=(token)"x"
    origin id="prov.x" node="x" library="@acme/brand-kit"
  }
  document id="doc.prov" title="PROV" {
    page id="pg1" w=(px)640 h=(px)360 {
      rect id="btn" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
      rect id="x" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.provenance.len(), 2, "expected 2 provenance records");
    let btn = &doc.provenance[0];
    assert_eq!(btn.id, "prov.btn");
    assert_eq!(btn.node, "btn");
    assert_eq!(btn.library, "@acme/brand-kit");
    assert_eq!(btn.item.as_deref(), Some("button"));
    assert_eq!(btn.linked, Some(true));
    let registry = btn
        .unknown_props
        .get("registry")
        .expect("annotated unknown prop must be preserved");
    assert_eq!(
        registry.ty.as_deref(),
        Some("token"),
        "unknown prop annotation must survive"
    );
    let x = &doc.provenance[1];
    assert_eq!(x.item, None);
    assert_eq!(x.linked, None);

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        formatted_str.contains(
            r#"origin id="prov.btn" node="btn" library="@acme/brand-kit" item="button" linked=#true"#
        ),
        "formatted output must contain the full btn origin line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"origin id="prov.x" node="x" library="@acme/brand-kit""#),
        "formatted output must contain the minimal x origin line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"registry=(token)"x""#),
        "annotated unknown prop must round-trip; got:\n{formatted_str}"
    );
    // Canonical order: sections, then provenance, then document.
    let sections_at = formatted_str.find("sections {").expect("sections block");
    let prov_at = formatted_str
        .find("provenance {")
        .expect("provenance block");
    let doc_at = formatted_str.find("document ").expect("document block");
    assert!(
        sections_at < prov_at && prov_at < doc_at,
        "provenance must be emitted after sections and before document; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).provenance,
        strip_spans(reparsed).provenance,
        "provenance must survive a parse → format → parse round-trip (idempotent)"
    );
}

// ── actions: parse, serialize, and round-trip ─────────────────────────

/// **Serialize round-trip**: parse a doc with an `actions` block (two action
/// entries — one carrying every field plus an annotated unknown prop, one
/// minimal) → format → re-parse → actions identical (spans stripped). Also
/// assert all fields are emitted, the block comes AFTER `provenance`, and the
/// annotated unknown prop and the `tx` JSON payload survive. Mirrors
/// `test_provenance_round_trip`.
#[test]
fn test_actions_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.act" name="ACT"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  actions {
    action id="apply-brand-kit" label="Apply Brand Kit" version="1.0.0" meta=(token)"x" {
      tx "{\"ops\":[{\"op\":\"update_token_value\",\"id\":\"color.brand\",\"value\":\"#e11d48\"}]}"
    }
    action id="reset-spacing" {
      tx "{\"ops\":[]}"
    }
  }
  document id="doc.act" title="ACT" {
    page id="pg1" w=(px)640 h=(px)360 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)10 h=(px)10
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.actions.len(), 2, "expected 2 actions");
    let brand = &doc.actions[0];
    assert_eq!(brand.id, "apply-brand-kit");
    assert_eq!(brand.label.as_deref(), Some("Apply Brand Kit"));
    assert_eq!(brand.version.as_deref(), Some("1.0.0"));
    assert!(
        brand.tx_json.contains("update_token_value"),
        "tx_json must contain the op name"
    );
    let meta = brand
        .unknown_props
        .get("meta")
        .expect("annotated unknown prop must be preserved");
    assert_eq!(
        meta.ty.as_deref(),
        Some("token"),
        "unknown prop annotation must survive"
    );

    let reset = &doc.actions[1];
    assert_eq!(reset.id, "reset-spacing");
    assert_eq!(reset.label, None);
    assert_eq!(reset.version, None);

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        formatted_str
            .contains(r#"action id="apply-brand-kit" label="Apply Brand Kit" version="1.0.0""#),
        "formatted output must contain the full action line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"action id="reset-spacing""#),
        "formatted output must contain the minimal action line; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("update_token_value"),
        "tx payload must survive formatting; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"meta=(token)"x""#),
        "annotated unknown prop must round-trip; got:\n{formatted_str}"
    );

    // actions must appear after provenance and before document.
    let actions_at = formatted_str.find("actions {").expect("actions block");
    let doc_at = formatted_str.find("document ").expect("document block");
    assert!(
        actions_at < doc_at,
        "actions must be emitted before document; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).actions,
        strip_spans(reparsed).actions,
        "actions must survive a parse → format → parse round-trip (idempotent)"
    );
}
