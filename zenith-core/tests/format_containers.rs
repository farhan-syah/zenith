//! Integration tests for the canonical writer: containers.
//!
//! Container nodes — tables, shapes, and unknown (forward-compatible) nodes —
//! parse, serialize, and round-trip.
//!
//! Moved verbatim from the former in-`src` `format/writer/tests.rs`; the body of
//! every test is unchanged — only import paths were rewritten to the public
//! `zenith_core` surface. Span-stripping helpers live in `common`.

mod common;

use common::*;
use zenith_core::format::format_document;

/// **table parse + format round-trip**: a table with columns/rows/cells and a
/// colspan parses into a `Node::Table`, and a parse → format → parse cycle
/// preserves the structure (spans excluded).
#[test]
fn test_table_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.tbl" name="TBL"
  tokens format="zenith-token-v1" {
    token id="color.line" type="color" value="#cccccc"
    token id="color.cellbg" type="color" value="#f0f0f0"
  }
  styles {
  }
  document id="doc.tbl" title="TBL" {
    page id="page.tbl" w=(px)640 h=(px)400 {
      table id="t1" x=(px)40 y=(px)40 w=(px)520 h=(px)240 border=(token)"color.line" border-width=(px)1 fill=(token)"color.cellbg" cell-padding=(px)8 gap=(px)0 h-align="start" v-align="middle" header-rows=1 {
        column width=(px)160
        column
        column width=(px)120
        row {
          cell { text id="c11" { span "Name" } }
          cell colspan=2 { text id="c12" { span "Details" } }
        }
        row {
          cell { text id="c21" { span "Ada" } }
          cell { text id="c22" { span "Lovelace" } }
          cell { text id="c23" { span "1815" } }
        }
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let table = match &doc.body.pages[0].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table node, got {other:?}"),
    };
    assert_eq!(table.id, "t1");
    assert_eq!(table.columns.len(), 3);
    assert!(table.columns[1].width.is_none(), "column 2 must be auto");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[0].cells[1].colspan, 2);
    assert_eq!(table.header_rows, Some(1));

    // Round-trip: parse → format → parse must yield the same AST (spans excluded).
    let formatted = format_document(&doc).expect("format must succeed");
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(doc2).body.pages[0].children,
        "table must survive a parse → format → parse round-trip"
    );
}

/// **Table flow round-trip**: a SOURCE `table flows="t"` carrying rows + a
/// CONTINUATION `table flows="t"` with EMPTY rows (a multi-page flow member)
/// must parse the `flows` id onto both `TableNode`s, be re-emitted by the
/// formatter, and survive a parse → format → parse round-trip.
#[test]
fn test_table_flows_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.fl" name="FL"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.fl" title="FL" {
    page id="page.fl1" w=(px)400 h=(px)400 {
      table id="src" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)120 header-rows=1 {
        column
        row {
          cell { text id="h1" { span "HEAD" } }
        }
        row {
          cell { text id="r1" { span "row-1" } }
        }
      }
    }
    page id="page.fl2" w=(px)400 h=(px)400 {
      table id="cont" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)400 header-rows=1 {
        column
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let src_table = match &doc.body.pages[0].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table node, got {other:?}"),
    };
    let cont_table = match &doc.body.pages[1].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table node, got {other:?}"),
    };
    assert_eq!(src_table.flows.as_deref(), Some("t"));
    assert_eq!(src_table.rows.len(), 2);
    // The continuation member carries the same flow id but NO rows.
    assert_eq!(cont_table.flows.as_deref(), Some("t"));
    assert!(
        cont_table.rows.is_empty(),
        "a flow continuation member legitimately has zero rows"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages,
        strip_spans(doc2.clone()).body.pages,
        "table flows must survive a parse → format → parse round-trip"
    );
}

/// **Table cell unknown-property round-trip**: a `cell` carrying an unknown
/// property survives parse → format → parse with the property intact.
#[test]
fn test_table_cell_unknown_property_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.cunk" name="CUnk"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.cunk" title="CUnk" {
    page id="p" w=(px)400 h=(px)300 {
      table id="t" x=(px)10 y=(px)10 w=(px)380 h=(px)280 {
        column width=(px)190
        column width=(px)190
        row {
          cell future-cell-attr="yes" { text id="c1" { span "A" } }
          cell { text id="c2" { span "B" } }
        }
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let table = match &doc.body.pages[0].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table, got {other:?}"),
    };
    assert!(
        table.rows[0].cells[0]
            .unknown_props
            .contains_key("future-cell-attr"),
        "unknown cell attr must be parsed into unknown_props"
    );
    let formatted = format_document(&doc).expect("format must succeed");
    assert!(
        String::from_utf8_lossy(&formatted).contains("future-cell-attr"),
        "formatted output must contain the unknown cell property"
    );
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages,
        strip_spans(doc2.clone()).body.pages,
        "cell unknown property must survive parse → format → parse"
    );
    // Idempotence: format twice → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2 must succeed");
    assert_eq!(
        formatted, formatted2,
        "cell unknown property formatting must be idempotent"
    );
}

/// **Table column unknown-property round-trip**: a `column` carrying an unknown
/// property survives parse → format → parse with the property intact.
#[test]
fn test_table_column_unknown_property_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.colunk" name="ColUnk"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.colunk" title="ColUnk" {
    page id="p" w=(px)400 h=(px)300 {
      table id="t" x=(px)10 y=(px)10 w=(px)380 h=(px)280 {
        column width=(px)190 future-col-hint=#true
        column width=(px)190
        row {
          cell { text id="c1" { span "A" } }
          cell { text id="c2" { span "B" } }
        }
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let table = match &doc.body.pages[0].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table, got {other:?}"),
    };
    assert!(
        table.columns[0]
            .unknown_props
            .contains_key("future-col-hint"),
        "unknown column attr must be parsed into unknown_props"
    );
    let formatted = format_document(&doc).expect("format must succeed");
    assert!(
        String::from_utf8_lossy(&formatted).contains("future-col-hint"),
        "formatted output must contain the unknown column property"
    );
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages,
        strip_spans(doc2.clone()).body.pages,
        "column unknown property must survive parse → format → parse"
    );
    // Idempotence: format twice → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2 must succeed");
    assert_eq!(
        formatted, formatted2,
        "column unknown property formatting must be idempotent"
    );
}

/// **Table row unknown-property round-trip**: a `row` carrying an unknown
/// property survives parse → format → parse with the property intact.
#[test]
fn test_table_row_unknown_property_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.rowunk" name="RowUnk"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.rowunk" title="RowUnk" {
    page id="p" w=(px)400 h=(px)300 {
      table id="t" x=(px)10 y=(px)10 w=(px)380 h=(px)280 {
        column width=(px)380
        row future-row-meta=42 {
          cell { text id="c1" { span "A" } }
        }
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let table = match &doc.body.pages[0].children[0] {
        Node::Table(t) => t,
        other => panic!("expected Table, got {other:?}"),
    };
    assert!(
        table.rows[0].unknown_props.contains_key("future-row-meta"),
        "unknown row attr must be parsed into unknown_props"
    );
    let formatted = format_document(&doc).expect("format must succeed");
    assert!(
        String::from_utf8_lossy(&formatted).contains("future-row-meta"),
        "formatted output must contain the unknown row property"
    );
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages,
        strip_spans(doc2.clone()).body.pages,
        "row unknown property must survive parse → format → parse"
    );
    // Idempotence: format twice → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2 must succeed");
    assert_eq!(
        formatted, formatted2,
        "row unknown property formatting must be idempotent"
    );
}

/// **Table clean round-trip**: a well-formed table with NO unknown props on any
/// cell/row/column must round-trip byte-identically and produce no
/// node.unknown_property diagnostic.
#[test]
fn test_table_clean_round_trips_without_unknown_property() {
    let src = r##"zenith version=1 {
  project id="proj.tclean" name="TClean"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.tclean" title="TClean" {
    page id="p" w=(px)400 h=(px)300 {
      table id="t" x=(px)10 y=(px)10 w=(px)380 h=(px)280 {
        column width=(px)190
        column width=(px)190
        row {
          cell { text id="c11" { span "A" } }
          cell { text id="c12" { span "B" } }
        }
        row {
          cell { text id="c21" { span "C" } }
          cell { text id="c22" { span "D" } }
        }
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let formatted = format_document(&doc).expect("format must succeed");
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages,
        strip_spans(doc2.clone()).body.pages,
        "clean table must round-trip"
    );
    // Idempotence.
    let formatted2 = format_document(&doc2).expect("format 2 must succeed");
    assert_eq!(
        formatted, formatted2,
        "clean table formatting must be idempotent"
    );
}

/// **shape parse + format round-trip**: a `kind="decision"` shape carrying
/// geometry, token-ref fill/stroke/stroke-width/radius, stroke-alignment, h/v
/// align, a text-style ref, and a `span` label parses into a `Node::Shape`, is
/// re-emitted with all key attributes (and the span), and survives a parse →
/// format → parse round-trip (spans excluded). Mirrors
/// `test_table_parse_format_round_trip`.
#[test]
fn test_shape_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.shp" name="SHP"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#dbeafe"
    token id="color.line" type="color" value="#1e3a8a"
    token id="size.stroke" type="dimension" value=(px)2
    token id="size.radius" type="dimension" value=(px)8
  }
  styles {
  }
  document id="doc.shp" title="SHP" {
    page id="page.shp" w=(px)640 h=(px)360 {
      shape id="s1" x=(px)40 y=(px)40 w=(px)200 h=(px)120 kind="decision" fill=(token)"color.fill" stroke=(token)"color.line" stroke-width=(token)"size.stroke" radius=(token)"size.radius" stroke-alignment="inside" h-align="center" v-align="middle" text-style="label.body" {
        span "Label"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let shape = match &doc.body.pages[0].children[0] {
        Node::Shape(s) => s,
        other => panic!("expected Shape node, got {other:?}"),
    };
    assert_eq!(shape.id, "s1");
    assert_eq!(shape.kind.as_deref(), Some("decision"));
    assert_eq!(shape.stroke_alignment.as_deref(), Some("inside"));
    assert_eq!(shape.h_align.as_deref(), Some("center"));
    assert_eq!(shape.v_align.as_deref(), Some("middle"));
    assert_eq!(shape.text_style.as_deref(), Some("label.body"));
    assert_eq!(shape.spans.len(), 1);
    assert_eq!(shape.spans[0].text, "Label");

    // Format must re-emit the shape with its key attributes and the span.
    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8_lossy(&formatted);
    for needle in [
        r#"shape id="s1""#,
        r#"kind="decision""#,
        r#"fill=(token)"color.fill""#,
        r#"stroke=(token)"color.line""#,
        r#"stroke-width=(token)"size.stroke""#,
        r#"radius=(token)"size.radius""#,
        r#"stroke-alignment="inside""#,
        r#"h-align="center""#,
        r#"v-align="middle""#,
        r#"text-style="label.body""#,
        r#"span "Label""#,
    ] {
        assert!(
            formatted_str.contains(needle),
            "formatted output must contain {needle:?}; got:\n{formatted_str}"
        );
    }

    // Round-trip: parse → format → parse must yield the same AST (spans excluded).
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(doc2).body.pages[0].children,
        "shape must survive a parse → format → parse round-trip"
    );
}

/// **Unknown-node lossless round-trip**: an unrecognized `filter` node carrying
/// an id, annotated properties, and unknown-kind `param` children must survive
/// parse → format → parse with its id, every property (incl. type annotations),
/// and all children preserved. This is the forward-compatibility cornerstone:
/// an older Zenith preserves node kinds it doesn't understand. Note `param` is
/// itself an unknown node, so this exercises unknown-within-unknown recursion.
#[test]
fn test_unknown_node_lossless_round_trip() {
    use zenith_core::UnknownValue;
    let src = r##"zenith version=1 {
  project id="proj.unk" name="Unk"
  tokens format="zenith-token-v1" {
    token id="color.navy" type="color" value="#001f54"
    token id="color.gold" type="color" value="#d4af37"
  }
  styles {
  }
  document id="doc.unk" title="Unk" {
    page id="page.one" w=(px)640 h=(px)360 {
      filter id="fx.duo" kind="duotone" target="page.cover" {
        param name="dark" value=(token)"color.navy"
        param name="light" value=(token)"color.gold"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    // The `filter` node parsed into an UnknownNode with id + props + children.
    let unknown = match &doc.body.pages[0].children[0] {
        Node::Unknown(u) => u,
        other => panic!("expected Unknown node, got {other:?}"),
    };
    assert_eq!(unknown.kind, "filter");
    assert_eq!(unknown.id.as_deref(), Some("fx.duo"));
    assert!(
        unknown.unknown_props.contains_key("kind"),
        "kind property must be preserved"
    );
    assert!(
        unknown.unknown_props.contains_key("target"),
        "target property must be preserved"
    );
    // id is captured first-class, NOT duplicated into unknown_props.
    assert!(
        !unknown.unknown_props.contains_key("id"),
        "id must be captured first-class, not in unknown_props"
    );
    assert_eq!(unknown.children.len(), 2, "both param children preserved");
    // The children are themselves unknown nodes with annotated `value` props.
    let dark = match &unknown.children[0] {
        Node::Unknown(c) => c,
        other => panic!("expected Unknown param child, got {other:?}"),
    };
    assert_eq!(dark.kind, "param");
    let value = dark
        .unknown_props
        .get("value")
        .expect("param child must carry a `value` property");
    assert_eq!(value.ty.as_deref(), Some("token"), "annotation preserved");
    assert_eq!(
        value.value,
        UnknownValue::String("color.navy".to_owned()),
        "token ref string preserved"
    );

    // The formatted output must contain the kind, id, props, children, and the
    // annotation on the nested value.
    let formatted = format_document(&doc).expect("format must succeed");
    let text = String::from_utf8_lossy(&formatted);
    assert!(text.contains("filter"), "formatted output keeps the kind");
    assert!(text.contains("id=\"fx.duo\""), "formatted output keeps id");
    assert!(
        text.contains("target=\"page.cover\""),
        "formatted output keeps target prop"
    );
    assert!(
        text.contains("kind=\"duotone\""),
        "formatted output keeps kind prop"
    );
    assert!(text.contains("param"), "formatted output keeps children");
    assert!(
        text.contains("value=(token)\"color.navy\""),
        "formatted output keeps the value annotation on a nested child"
    );

    // Full round-trip stability: parse → format → parse yields the same AST.
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(doc2.clone()).body.pages[0].children,
        "unknown node (id, props incl ty, children) must survive parse → format → parse"
    );
    // Idempotence: format twice → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2 must succeed");
    assert_eq!(
        formatted, formatted2,
        "unknown node formatting must be idempotent"
    );
}

/// **Nested KNOWN child survives an unknown parent**: a real `rect` declared
/// inside an unrecognized node must round-trip as a `Node::Rect` child (with
/// its attributes), proving that known children of an unknown container are
/// transformed and re-emitted, not collapsed into opaque text.
#[test]
fn test_unknown_node_preserves_known_child() {
    let src = r##"zenith version=1 {
  project id="proj.unk2" name="Unk2"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
  }
  styles {
  }
  document id="doc.unk2" title="Unk2" {
    page id="page.one" w=(px)640 h=(px)360 {
      sparkle id="s1" {
        rect id="inner" x=(px)5 y=(px)6 w=(px)20 h=(px)30 fill=(token)"color.bg"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let unknown = match &doc.body.pages[0].children[0] {
        Node::Unknown(u) => u,
        other => panic!("expected Unknown node, got {other:?}"),
    };
    assert_eq!(unknown.kind, "sparkle");
    assert_eq!(unknown.children.len(), 1);
    let rect = match &unknown.children[0] {
        Node::Rect(r) => r,
        other => panic!("expected a real Rect child, got {other:?}"),
    };
    assert_eq!(rect.id, "inner");

    let formatted = format_document(&doc).expect("format must succeed");
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    // The known rect child survives as a Node::Rect after the round-trip.
    let unknown2 = match &doc2.body.pages[0].children[0] {
        Node::Unknown(u) => u,
        other => panic!("expected Unknown node after round-trip, got {other:?}"),
    };
    assert!(
        matches!(&unknown2.children[0], Node::Rect(r) if r.id == "inner"),
        "known rect child must survive the round-trip as a Node::Rect"
    );
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(doc2).body.pages[0].children,
        "unknown node with a known child must survive parse → format → parse"
    );
}

/// **group semantic scalars round-trip**: a group with all three semantic-role,
/// intensity, and layer-priority set parses into the correct AST fields and
/// survives a parse → format → parse cycle with values preserved.
#[test]
fn group_semantic_scalars_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.sem" name="SEM"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.sem" title="SEM" {
    page id="page.sem" w=(px)800 h=(px)600 {
      group id="grp.sem" semantic-role="overlay" intensity=0.75 layer-priority=3 {
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let grp = match &doc.body.pages[0].children[0] {
        Node::Group(g) => g,
        other => panic!("expected Group node, got {other:?}"),
    };
    assert_eq!(grp.semantic_role.as_deref(), Some("overlay"));
    assert_eq!(grp.intensity, Some(0.75));
    assert_eq!(grp.layer_priority, Some(3));

    let formatted = format_document(&doc).expect("format must succeed");
    let doc2 = adapter
        .parse(&formatted)
        .expect("re-parse after format must succeed");
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(doc2).body.pages[0].children,
        "group with semantic scalars must survive parse → format → parse"
    );
}

/// **group semantic scalars absent — byte identity**: a plain group with none of
/// the three fields must not emit any of semantic-role, intensity, or
/// layer-priority in its formatted output.
#[test]
fn group_semantic_scalars_absent_byte_identity() {
    let src = r##"zenith version=1 {
  project id="proj.plain" name="PLAIN"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.plain" title="PLAIN" {
    page id="page.plain" w=(px)800 h=(px)600 {
      group id="grp.plain" {
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let formatted = format_document(&doc).expect("format must succeed");
    let text = String::from_utf8_lossy(&formatted);
    assert!(
        !text.contains("semantic-role"),
        "plain group must not emit semantic-role; got:\n{text}"
    );
    assert!(
        !text.contains("intensity"),
        "plain group must not emit intensity; got:\n{text}"
    );
    assert!(
        !text.contains("layer-priority"),
        "plain group must not emit layer-priority; got:\n{text}"
    );
}

/// **group semantic scalars format idempotency**: formatting a document that
/// contains all three semantic scalar fields twice must produce the same output.
#[test]
fn group_semantic_scalars_format_idempotency() {
    let src = r##"zenith version=1 {
  project id="proj.idem" name="IDEM"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.idem" title="IDEM" {
    page id="page.idem" w=(px)800 h=(px)600 {
      group id="grp.idem" semantic-role="background" intensity=0.5 layer-priority=-1 {
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let first = format_document(&doc).expect("first format must succeed");
    let doc2 = adapter.parse(&first).expect("re-parse must succeed");
    let second = format_document(&doc2).expect("second format must succeed");
    assert_eq!(
        first, second,
        "format must be idempotent for group semantic scalars"
    );
}
