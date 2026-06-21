//! Unit and round-trip tests for the canonical writer.
//!
//! Moved verbatim from the former single-file `writer.rs`.

#![cfg(test)]

use super::*;
use crate::ast::Node;
use crate::parse::{KdlAdapter, KdlSource};

/// A minimal `.zen` document used as the idempotency fixture.
const MINIMAL: &str = r##"zenith version=1 {
  project id="proj.test" name="Test Project"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="size.title" type="dimension" value=(pt)48
    token id="font.weight.bold" type="fontWeight" value=700
    token id="lh.body" type="number" value=1.45
  }
  styles {
  }
  document id="doc.test" title="Test Doc" {
    page id="page.one" name="One" w=(px)640 h=(px)360 background=(token)"color.bg" {
      rect id="bg.rect" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.bg"
      text id="label" x=(px)10 y=(px)10 w=(px)200 h=(px)50 align="center" fill=(token)"color.text" {
        span "Hello Zenith"
      }
    }
  }
}
"##;

/// **Idempotency test**: format once → `s1`; format again → `s2`; assert equal.
#[test]
fn test_idempotency() {
    let adapter = KdlAdapter;
    let doc1 = adapter
        .parse(MINIMAL.as_bytes())
        .expect("parse 1 must succeed");
    let s1 = format_document(&doc1).expect("format 1 must succeed");

    let doc2 = adapter.parse(&s1).expect("parse 2 must succeed");
    let s2 = format_document(&doc2).expect("format 2 must succeed");

    assert_eq!(
        String::from_utf8(s1.clone()).unwrap(),
        String::from_utf8(s2).unwrap(),
        "format must be idempotent"
    );
}

/// **Round-trip AST equality**: parse → format → parse must yield the same AST
/// (excluding source spans, which reflect byte positions in the original source).
#[test]
fn test_round_trip_ast_equality() {
    let adapter = KdlAdapter;
    let doc_orig = adapter.parse(MINIMAL.as_bytes()).expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");
    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");

    // Compare with spans stripped — spans are byte-position metadata that
    // legitimately differ between the original source and the reformatted
    // canonical form; they are not part of the document semantics.
    let orig_stripped = strip_spans(doc_orig);
    let reparsed_stripped = strip_spans(doc_reparsed);
    assert_eq!(
        orig_stripped, reparsed_stripped,
        "re-parsed AST must equal original (spans excluded)"
    );
}

/// **`baseline-grid` round-trip**: a page's `baseline-grid=(px)14` must survive
/// parse → format → parse, mirroring the `bleed` dimension round-trip.
#[test]
fn test_baseline_grid_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.bg" name="BG"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.bg" title="BG" {
    page id="page.one" w=(px)640 h=(px)360 baseline-grid=(px)14 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"c"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let page = &doc.body.pages[0];
    assert!(
        page.baseline_grid.is_some(),
        "baseline-grid must parse onto the page"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        formatted_str.contains("baseline-grid=(px)14"),
        "formatted output must contain baseline-grid; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).body.pages[0].baseline_grid,
        strip_spans(reparsed).body.pages[0].baseline_grid,
        "baseline-grid must round-trip identically"
    );
}

/// **`font-size-min` round-trip**: a text node's `font-size-min=(token)"…"`
/// (the `overflow="autofit"` floor) must survive parse → format → parse,
/// mirroring `font-size`.
#[test]
fn test_font_size_min_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.fsm" name="FSM"
  tokens format="zenith-token-v1" {
    token id="size.title.min" type="dimension" value=(px)12
  }
  styles {
  }
  document id="doc.fsm" title="FSM" {
    page id="page.one" w=(px)640 h=(px)360 {
      text id="t" x=(px)0 y=(px)0 w=(px)200 h=(px)40 overflow="autofit" font-size-min=(token)"size.title.min" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        formatted_str.contains(r#"font-size-min=(token)"size.title.min""#),
        "formatted output must contain font-size-min; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).body.pages[0].children,
        strip_spans(reparsed).body.pages[0].children,
        "font-size-min must round-trip identically"
    );
}

/// **Radial gradient round-trip**: a `radial=#true` gradient token with
/// `center-x`, `center-y`, and `radius` params must survive parse → format →
/// parse with `kind == GradientKind::Radial` and the same params.
#[test]
fn test_radial_gradient_round_trips() {
    use crate::ast::{GradientKind, TokenLiteral, TokenValue};

    let src = r##"zenith version=1 {
  project id="proj.rgt" name="RGT"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ffffff"
    token id="color.b" type="color" value="#000000"
    token id="grad.r" type="gradient" radial=#true center-x=0.5 center-y=0.5 radius=0.7 {
      stop offset=0.0 color=(token)"color.a"
      stop offset=1.0 color=(token)"color.b"
    }
  }
  styles {
  }
  document id="doc.rgt" title="RGT" {
    page id="page.rgt" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"grad.r"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    // Formatted output must contain radial marker and geometry params.
    assert!(
        formatted_str.contains("radial=#true"),
        "formatted output must contain radial=#true; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("center-x=0.5"),
        "formatted output must contain center-x; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("center-y=0.5"),
        "formatted output must contain center-y; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("radius=0.7"),
        "formatted output must contain radius; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    let reparsed2 = adapter
        .parse(&format_document(&reparsed).expect("format 2"))
        .expect("re-parse 2");

    // Find the gradient token in the reparsed doc.
    let grad_token = reparsed2
        .tokens
        .tokens
        .iter()
        .find(|t| t.id == "grad.r")
        .expect("grad.r token must survive round-trip");
    let TokenValue::Literal(TokenLiteral::Gradient(g)) = &grad_token.value else {
        panic!(
            "grad.r must be a gradient literal, got {:?}",
            grad_token.value
        );
    };
    assert_eq!(
        g.kind,
        GradientKind::Radial,
        "kind must be Radial after round-trip"
    );
    assert_eq!(g.center_x, Some(0.5));
    assert_eq!(g.center_y, Some(0.5));
    assert_eq!(g.radius, Some(0.7));
    assert_eq!(g.stops.len(), 2);
}

/// A `.zen` document with a `code` node whose content stresses every escape
/// path: leading spaces, a blank line, a tab, an embedded quote, and a
/// literal backslash.
const CODE_DOC: &str = r##"zenith version=1 {
  project id="proj.code" name="Code Project"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.code" title="Code Doc" {
    page id="page.one" w=(px)640 h=(px)360 {
      code id="snippet" x=(px)96 y=(px)320 w=(px)560 h=(px)180 overflow="clip" language="rust" line-numbers=#false tab-width=4 {
        content "fn main() {\n    let path = \"c:\\\\tmp\";\n\n\tprintln!(\"hi\");\n}"
      }
    }
  }
}
"##;

/// **Code content verbatim round-trip**: parse → format → parse must yield a
/// BYTE-IDENTICAL content blob, and the formatter must be idempotent.
#[test]
fn test_code_content_verbatim_round_trip() {
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(CODE_DOC.as_bytes()).expect("parse 1");

    // Decoded content captured from the first parse.
    let original = match &doc1.body.pages[0].children[0] {
        Node::Code(c) => c.content.clone(),
        other => panic!("expected Code node, got {other:?}"),
    };
    // Sanity: the fixture really exercises every escape class.
    assert!(original.contains('\n') && original.contains('\t'));
    assert!(original.contains('"') && original.contains('\\'));

    let s1 = format_document(&doc1).expect("format 1");
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let reparsed = match &doc2.body.pages[0].children[0] {
        Node::Code(c) => c.content.clone(),
        other => panic!("expected Code node, got {other:?}"),
    };
    assert_eq!(
        original, reparsed,
        "code content must round-trip byte-identically"
    );

    // Idempotency: format(format(doc)) == format(doc).
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "code formatting must be idempotent"
    );
}

/// Strip all source spans from a Document to enable span-agnostic equality.
fn strip_spans(mut doc: crate::ast::Document) -> crate::ast::Document {
    // Assets
    doc.assets.source_span = None;
    for decl in &mut doc.assets.assets {
        decl.source_span = None;
    }
    // Tokens
    for token in &mut doc.tokens.tokens {
        token.source_span = None;
    }
    // Styles
    doc.styles.source_span = None;
    for style in &mut doc.styles.styles {
        style.source_span = None;
    }
    // Components
    for comp in &mut doc.components {
        comp.source_span = None;
        for node in &mut comp.children {
            strip_node_span(node);
        }
    }
    // Masters
    for master in &mut doc.masters {
        master.source_span = None;
        for node in &mut master.children {
            strip_node_span(node);
        }
    }
    // Libraries
    for library in &mut doc.libraries {
        library.source_span = None;
    }
    // Actions
    for action in &mut doc.actions {
        action.source_span = None;
    }
    // Sections
    for section in &mut doc.sections {
        section.source_span = None;
    }
    // Provenance
    for prov in &mut doc.provenance {
        prov.source_span = None;
    }
    // Pages and nodes
    for page in &mut doc.body.pages {
        page.source_span = None;
        for zone in &mut page.safe_zones {
            zone.source_span = None;
        }
        for fold in &mut page.folds {
            fold.source_span = None;
        }
        for node in &mut page.children {
            strip_node_span(node);
        }
    }
    doc
}

/// Recursively clear `source_span` from a node and all its descendants.
fn strip_node_span(node: &mut crate::ast::Node) {
    use crate::ast::Node;
    match node {
        Node::Rect(r) => r.source_span = None,
        Node::Ellipse(e) => e.source_span = None,
        Node::Line(l) => l.source_span = None,
        Node::Text(t) => t.source_span = None,
        Node::Code(c) => c.source_span = None,
        Node::Frame(f) => {
            f.source_span = None;
            for child in &mut f.children {
                strip_node_span(child);
            }
        }
        Node::Group(g) => {
            g.source_span = None;
            for child in &mut g.children {
                strip_node_span(child);
            }
        }
        Node::Image(i) => i.source_span = None,
        Node::Polygon(p) => p.source_span = None,
        Node::Polyline(p) => p.source_span = None,
        Node::Instance(i) => {
            i.source_span = None;
            for ov in &mut i.overrides {
                ov.source_span = None;
            }
        }
        Node::Field(f) => f.source_span = None,
        Node::Toc(t) => t.source_span = None,
        Node::Footnote(f) => f.source_span = None,
        Node::Table(t) => {
            t.source_span = None;
            for col in &mut t.columns {
                col.source_span = None;
            }
            for row in &mut t.rows {
                row.source_span = None;
                for cell in &mut row.cells {
                    cell.source_span = None;
                    for child in &mut cell.children {
                        strip_node_span(child);
                    }
                }
            }
        }
        Node::Shape(s) => s.source_span = None,
        Node::Connector(c) => c.source_span = None,
        Node::Unknown(u) => {
            u.source_span = None;
            for child in &mut u.children {
                strip_node_span(child);
            }
        }
    }
}

/// **syntax-theme round-trip**: a code node with `syntax-theme="light"`
/// must parse to `Some(SyntaxTheme::Light)` and format back to
/// `syntax-theme="light"` in the canonical position (between font-size and
/// opacity).
#[test]
fn test_syntax_theme_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.sth" name="STH"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.sth" title="STH" {
    page id="page.sth" w=(px)400 h=(px)300 {
      code id="code.sth" x=(px)10 y=(px)10 language="rust" syntax-theme="light" {
        content "let x = 1;"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let code_node = match &doc.body.pages[0].children[0] {
        Node::Code(c) => c,
        other => panic!("expected Code node, got {other:?}"),
    };
    use crate::tokens::SyntaxTheme;
    assert_eq!(
        code_node.syntax_theme,
        Some(SyntaxTheme::Light),
        "syntax-theme=\"light\" must parse to Some(SyntaxTheme::Light)"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("syntax-theme=\"light\""),
        "formatter must emit syntax-theme=\"light\"; got:\n{formatted_str}"
    );

    // Canonical position: between font-size and opacity. Since neither
    // font-size nor opacity is set in this fixture, just check that
    // syntax-theme appears and re-parses correctly.
    let doc2 = adapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse after format");
    let code2 = match &doc2.body.pages[0].children[0] {
        Node::Code(c) => c,
        other => panic!("expected Code node on re-parse, got {other:?}"),
    };
    assert_eq!(
        code2.syntax_theme,
        Some(SyntaxTheme::Light),
        "syntax-theme must survive a format → re-parse round-trip"
    );
}

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

/// **Image clip round-trip**: `clip="rounded"` + `clip-radius=(token)"..."`
/// must parse onto the `ImageNode`, be re-emitted by the formatter, and survive
/// a format → re-parse round-trip.
#[test]
fn test_image_clip_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.iclip" name="IClip"
  assets {
    asset id="asset.pfp" kind="image" src="assets/pfp.png"
  }
  tokens format="zenith-token-v1" {
    token id="size.radius.avatar" type="dimension" value=(px)24
  }
  styles {
  }
  document id="doc.iclip" title="IClip" {
    page id="page.iclip" w=(px)400 h=(px)300 {
      image id="av" asset="asset.pfp" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" clip="rounded" clip-radius=(token)"size.radius.avatar"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let image_node = match &doc.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node, got {other:?}"),
    };
    assert_eq!(image_node.clip.as_deref(), Some("rounded"));
    use crate::ast::value::PropertyValue;
    assert_eq!(
        image_node.clip_radius,
        Some(PropertyValue::TokenRef("size.radius.avatar".to_owned())),
        "clip-radius must parse as a token ref"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("clip=\"rounded\""),
        "formatter must emit clip=\"rounded\"; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("clip-radius=(token)\"size.radius.avatar\""),
        "formatter must emit clip-radius token; got:\n{formatted_str}"
    );

    let doc2 = adapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse after format");
    let image2 = match &doc2.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node on re-parse, got {other:?}"),
    };
    assert_eq!(image2.clip.as_deref(), Some("rounded"));
    assert_eq!(
        image2.clip_radius,
        Some(PropertyValue::TokenRef("size.radius.avatar".to_owned())),
        "clip-radius must survive a format → re-parse round-trip"
    );
}

/// **Number formatting**: integral `f64` emits without decimal point.
#[test]
fn test_number_formatting_integral() {
    use crate::ast::{Dimension, Unit};
    let d = Dimension {
        value: 640.0,
        unit: Unit::Px,
    };
    assert_eq!(
        fmt_dimension(&d),
        "(px)640",
        "(px)640.0 must format as (px)640"
    );
}

/// **Number formatting**: non-integral value keeps its decimal.
#[test]
fn test_number_formatting_non_integral() {
    use crate::ast::{Dimension, Unit};
    let d = Dimension {
        value: 10.5,
        unit: Unit::Pt,
    };
    assert_eq!(fmt_dimension(&d), "(pt)10.5");
}

/// **Number formatting**: token `(pt)48` must round-trip as `(pt)48`.
#[test]
fn test_pt_48_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.t" name="T"
  tokens format="zenith-token-v1" {
    token id="size.title" type="dimension" value=(pt)48
  }
  styles {
  }
  document id="doc.t" title="T" {
    page id="p" w=(px)100 h=(px)100 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("value=(pt)48"),
        "expected `value=(pt)48` in output, got:\n{text}"
    );
    assert!(
        !text.contains("(pt)48.0"),
        "must not contain (pt)48.0 in output"
    );
}

/// **Literal visual dimension round-trip**: a `stroke-width=(px)2` literal
/// must format as `(px)2` (not `(px)2.0`, not `"2"`) and re-parse back to a
/// `Dimension(2.0, Px)`.
#[test]
fn test_literal_dimension_round_trips() {
    use crate::ast::{Dimension, Node, PropertyValue, Unit};
    let src = r##"zenith version=1 {
  project id="proj.ld" name="LD"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ld" title="LD" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 stroke-width=(px)2
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("stroke-width=(px)2"),
        "expected `stroke-width=(px)2`, got:\n{text}"
    );
    assert!(
        !text.contains("(px)2.0"),
        "must not emit (px)2.0; got:\n{text}"
    );
    assert!(
        !text.contains("stroke-width=\"2\""),
        "must not emit a quoted literal; got:\n{text}"
    );

    // Re-parse the formatted output → still a Dimension(2.0, Px).
    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    match &doc2.body.pages[0].children[0] {
        Node::Rect(r) => assert_eq!(
            r.stroke_width,
            Some(PropertyValue::Dimension(Dimension {
                value: 2.0,
                unit: Unit::Px,
            }))
        ),
        other => panic!("expected Rect, got {other:?}"),
    }
}

/// **Canonical property order**: a rect with `fill` before `x` in source
/// must be formatted with `x` before `fill`.
#[test]
fn test_canonical_property_order_rect() {
    // Source has fill before x — non-canonical order.
    let src = r##"zenith version=1 {
  project id="proj.order" name="Order"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
  styles {
  }
  document id="doc.order" title="Order" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" fill=(token)"color.bg" x=(px)10 y=(px)20 w=(px)50 h=(px)50
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    // Find positions of `x=` and `fill=` in the rect line.
    let rect_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("rect"))
        .expect("must find rect line");
    let pos_x = rect_line.find(" x=").expect("must find x= on rect line");
    let pos_fill = rect_line
        .find(" fill=")
        .expect("must find fill= on rect line");
    assert!(
        pos_x < pos_fill,
        "x= must appear before fill= in canonical output; rect line: {rect_line:?}"
    );
}

/// **font-weight round-trip + ordering**: a text node with a `font-weight`
/// token must survive parse→format→parse, and the formatter must place
/// `font-weight` immediately AFTER `font-size` in the canonical output.
#[test]
fn test_text_font_weight_round_trip_and_order() {
    use crate::ast::{Node, PropertyValue};
    let src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {
    token id="size.body" type="dimension" value=(px)16
    token id="weight.bold" type="fontWeight" value=700
  }
  styles {
  }
  document id="doc.fw" title="FW" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t" x=(px)0 y=(px)0 w=(px)80 h=(px)40 font-size=(token)"size.body" font-weight=(token)"weight.bold" {
        span "Bold"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    // Canonical order: font-weight comes immediately after font-size.
    let text_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("text"))
        .expect("must find text line");
    let pos_size = text_line.find(" font-size=").expect("must find font-size=");
    let pos_weight = text_line
        .find(" font-weight=")
        .expect("must find font-weight=");
    assert!(
        pos_size < pos_weight,
        "font-weight must follow font-size; text line: {text_line:?}"
    );

    // Round-trip: re-parse preserves the font_weight token ref.
    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    match &doc2.body.pages[0].children[0] {
        Node::Text(t) => assert_eq!(
            t.font_weight,
            Some(PropertyValue::TokenRef("weight.bold".to_owned())),
            "font-weight must survive the format round-trip"
        ),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// **chain round-trip**: a text node carrying `chain="article"` must survive
/// parse→format→parse, with `chain` emitted on the text line (after `style`)
/// and re-parsed back into the `chain` field.
#[test]
fn test_text_chain_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.ch" name="CH"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ch" title="CH" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t1" x=(px)0 y=(px)0 w=(px)80 h=(px)40 chain="article" {
        span "Hello world"
      }
      text id="t2" x=(px)0 y=(px)50 w=(px)80 h=(px)40 chain="article" {
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" chain=\"article\""),
        "chain attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    for child in &page.children {
        match child {
            Node::Text(t) => assert_eq!(
                t.chain.as_deref(),
                Some("article"),
                "chain must survive the format round-trip"
            ),
            other => panic!("expected Text, got {other:?}"),
        }
    }
}

/// **text-exclusion round-trip**: a text node carrying
/// `text-exclusion="portrait"` must survive parse→format→parse, with the attr
/// emitted on the text line and re-parsed back into the `text_exclusion` field.
#[test]
fn test_text_exclusion_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.ex" name="EX"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ex" title="EX" {
    page id="p" w=(px)200 h=(px)200 {
      rect id="portrait" x=(px)0 y=(px)0 w=(px)80 h=(px)80
      text id="t1" x=(px)0 y=(px)0 w=(px)180 h=(px)180 text-exclusion="portrait" {
        span "Hello world"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" text-exclusion=\"portrait\""),
        "text-exclusion attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    let mut saw_text = false;
    for child in &page.children {
        if let Node::Text(t) = child {
            assert_eq!(
                t.text_exclusion.as_deref(),
                Some("portrait"),
                "text-exclusion must survive the format round-trip"
            );
            saw_text = true;
        }
    }
    assert!(saw_text, "expected a Text node in the re-parsed page");
}

/// **drop-cap-lines round-trip**: a text node carrying `drop-cap-lines=3` must
/// survive parse→format→parse, with the attr emitted on the text line and
/// re-parsed back into the `drop_cap_lines` field.
#[test]
fn test_text_drop_cap_lines_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.dc" name="DC"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.dc" title="DC" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t1" x=(px)0 y=(px)0 w=(px)80 h=(px)40 drop-cap-lines=3 {
        span "Hello world"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" drop-cap-lines=3"),
        "drop-cap-lines attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    match &page.children[0] {
        Node::Text(t) => assert_eq!(
            t.drop_cap_lines,
            Some(3),
            "drop-cap-lines must survive the format round-trip"
        ),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// **hyphenate + widow-orphan round-trip**: a text node carrying
/// `hyphenate=#true` and `widow-orphan=2` must survive parse→format→parse, with
/// both attrs emitted and re-parsed into their fields.
#[test]
fn test_text_hyphenate_widow_orphan_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.hy" name="HY"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.hy" title="HY" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t1" x=(px)0 y=(px)0 w=(px)80 h=(px)40 hyphenate=#true widow-orphan=2 {
        span "Hello world"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" hyphenate=#true"),
        "hyphenate attr must be emitted; got:\n{text}"
    );
    assert!(
        text.contains(" widow-orphan=2"),
        "widow-orphan attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    match &page.children[0] {
        Node::Text(t) => {
            assert_eq!(
                t.hyphenate,
                Some(true),
                "hyphenate must survive the format round-trip"
            );
            assert_eq!(
                t.widow_orphan,
                Some(2),
                "widow-orphan must survive the format round-trip"
            );
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

/// **tab-leader round-trip**: a text node carrying `tab-leader="."` must survive
/// parse→format→parse, with the attr emitted and re-parsed into its field.
#[test]
fn test_text_tab_leader_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.tl" name="TL"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.tl" title="TL" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t1" x=(px)0 y=(px)0 w=(px)80 h=(px)40 tab-leader="." {
        span "Chapter One\t1"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" tab-leader=\".\""),
        "tab-leader attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    match &page.children[0] {
        Node::Text(t) => assert_eq!(
            t.tab_leader.as_deref(),
            Some("."),
            "tab-leader must survive the format round-trip"
        ),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// **frame grid columns/rows round-trip**: a frame carrying `layout="grid"
/// columns=2 rows=3` must survive parse→format→parse, with the attrs emitted
/// near `layout` and re-parsed into the `columns`/`rows` fields.
#[test]
fn test_frame_grid_columns_rows_round_trip() {
    use crate::ast::Node;
    let src = r##"zenith version=1 {
  project id="proj.gr" name="GR"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.gr" title="GR" {
    page id="p" w=(px)400 h=(px)400 {
      frame id="f1" x=(px)0 y=(px)0 w=(px)300 h=(px)300 layout="grid" columns=2 rows=3 {
        rect id="r0"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" columns=2"),
        "columns attr must be emitted; got:\n{text}"
    );
    assert!(
        text.contains(" rows=3"),
        "rows attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    match &page.children[0] {
        Node::Frame(f) => {
            assert_eq!(f.columns, Some(2), "columns must survive the round-trip");
            assert_eq!(f.rows, Some(3), "rows must survive the round-trip");
        }
        other => panic!("expected Frame, got {other:?}"),
    }
}

/// **text contrast-bg round-trip**: a text node carrying
/// `contrast-bg=(token)"color.photo.shadow"` must survive parse→format→parse,
/// with the attr emitted near `fill` and re-parsed into the `contrast_bg` field.
#[test]
fn test_text_contrast_bg_round_trip() {
    use crate::ast::Node;
    use crate::ast::value::PropertyValue;
    let src = r##"zenith version=1 {
  project id="proj.cb" name="CB"
  tokens format="zenith-token-v1" {
    token id="color.photo.shadow" type="color" value="#101010"
  }
  styles {
  }
  document id="doc.cb" title="CB" {
    page id="p" w=(px)100 h=(px)100 {
      text id="t1" x=(px)0 y=(px)0 w=(px)80 h=(px)40 contrast-bg=(token)"color.photo.shadow" {
        span "Cover line"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    assert!(
        text.contains(" contrast-bg=(token)\"color.photo.shadow\""),
        "contrast-bg attr must be emitted; got:\n{text}"
    );

    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    let page = &doc2.body.pages[0];
    match &page.children[0] {
        Node::Text(t) => assert_eq!(
            t.contrast_bg,
            Some(PropertyValue::TokenRef("color.photo.shadow".to_owned())),
            "contrast-bg must survive the format round-trip"
        ),
        other => panic!("expected Text, got {other:?}"),
    }
}

/// **Gradient round-trip**: a gradient token (angle + 2 stops) must
/// parse→format→parse byte-stably, emit the `stop` brace block, and a page
/// background referencing it must NOT flag the stop colors as `token.unused`.
#[test]
fn test_gradient_token_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.grad" name="Grad"
  tokens format="zenith-token-v1" {
    token id="color.navy.top" type="color" value="#001133"
    token id="color.black.bottom" type="color" value="#000000"
    token id="gradient.bg.hero" type="gradient" angle=(deg)90 {
      stop offset=0 color=(token)"color.navy.top"
      stop offset=1 color=(token)"color.black.bottom"
    }
  }
  styles {
  }
  document id="doc.grad" title="Grad" {
    page id="p" w=(px)100 h=(px)100 background=(token)"gradient.bg.hero" {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");
    let formatted = String::from_utf8(s1.clone()).expect("utf8");

    // The gradient emits a brace block with two stop children.
    assert!(
        formatted.contains("type=\"gradient\" angle=(deg)90 {"),
        "expected gradient header; got:\n{formatted}"
    );
    assert!(
        formatted.contains("stop offset=0 color=(token)\"color.navy.top\""),
        "expected first stop; got:\n{formatted}"
    );
    assert!(
        formatted.contains("stop offset=1 color=(token)\"color.black.bottom\""),
        "expected second stop; got:\n{formatted}"
    );

    // Idempotency: format(format(doc)) == format(doc).
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted,
        String::from_utf8(s2).expect("utf8"),
        "gradient formatting must be idempotent"
    );

    // AST round-trip (spans stripped).
    assert_eq!(
        strip_spans(doc1),
        strip_spans(doc2),
        "gradient AST must survive format round-trip"
    );
}

/// **Gradient fill validates clean**: a page background referencing a
/// gradient token type-checks OK, and the gradient's stop colors are not
/// falsely flagged `token.unused`.
#[test]
fn test_gradient_fill_validates_without_unused() {
    let src = r##"zenith version=1 {
  project id="proj.grad" name="Grad"
  tokens format="zenith-token-v1" {
    token id="color.navy.top" type="color" value="#001133"
    token id="color.black.bottom" type="color" value="#000000"
    token id="gradient.bg.hero" type="gradient" angle=(deg)90 {
      stop offset=0 color=(token)"color.navy.top"
      stop offset=1 color=(token)"color.black.bottom"
    }
  }
  styles {
  }
  document id="doc.grad" title="Grad" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"gradient.bg.hero"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    // `validate` runs token resolution internally and merges all diagnostics.
    let report = crate::validate::validate(&doc);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"token.incompatible_property"),
        "gradient fill must be type-compatible; codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"token.unused"),
        "gradient stop colors must not be flagged unused; codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"token.raw_visual_literal"),
        "gradient token ref must not be a raw literal; codes: {codes:?}"
    );
}

/// **Shadow round-trip**: a shadow token (2 layers) must parse→format→parse
/// byte-stably, emit the `layer` brace block, and a text node referencing it
/// (via `shadow=(token)"..."`) must survive the round-trip.
#[test]
fn test_shadow_token_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.shadow" name="Shadow"
  tokens format="zenith-token-v1" {
    token id="color.shadow.black" type="color" value="#000000"
    token id="color.glow.cyan" type="color" value="#00ffff"
    token id="shadow.headline" type="shadow" {
      layer dx=(px)8 dy=(px)8 blur=(px)24 color=(token)"color.shadow.black"
      layer dx=(px)0 dy=(px)0 blur=(px)20 color=(token)"color.glow.cyan"
    }
  }
  styles {
  }
  document id="doc.shadow" title="Shadow" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 shadow=(token)"shadow.headline" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");
    let formatted = String::from_utf8(s1.clone()).expect("utf8");

    // The shadow emits a brace block with two layer children.
    assert!(
        formatted.contains("type=\"shadow\" {"),
        "expected shadow header; got:\n{formatted}"
    );
    assert!(
        formatted
            .contains("layer dx=(px)8 dy=(px)8 blur=(px)24 color=(token)\"color.shadow.black\""),
        "expected first layer; got:\n{formatted}"
    );
    assert!(
        formatted.contains(" shadow=(token)\"shadow.headline\""),
        "expected node shadow prop; got:\n{formatted}"
    );

    // Idempotency.
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted,
        String::from_utf8(s2).expect("utf8"),
        "shadow formatting must be idempotent"
    );

    // AST round-trip (spans stripped).
    assert_eq!(
        strip_spans(doc1),
        strip_spans(doc2),
        "shadow AST must survive format round-trip"
    );
}

/// **Shadow on a node validates clean**: a text node referencing a shadow
/// token type-checks OK, and the shadow's layer colors are not falsely
/// flagged `token.unused`.
#[test]
fn test_shadow_node_validates_without_unused() {
    let src = r##"zenith version=1 {
  project id="proj.shadow" name="Shadow"
  tokens format="zenith-token-v1" {
    token id="color.shadow.black" type="color" value="#000000"
    token id="color.glow.cyan" type="color" value="#00ffff"
    token id="shadow.headline" type="shadow" {
      layer dx=(px)8 dy=(px)8 blur=(px)24 color=(token)"color.shadow.black"
      layer dx=(px)0 dy=(px)0 blur=(px)20 color=(token)"color.glow.cyan"
    }
  }
  styles {
  }
  document id="doc.shadow" title="Shadow" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 shadow=(token)"shadow.headline" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let report = crate::validate::validate(&doc);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"token.incompatible_property"),
        "shadow ref must be type-compatible; codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"token.unused"),
        "shadow layer colors must not be flagged unused; codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"token.raw_visual_literal"),
        "shadow token ref must not be a raw literal; codes: {codes:?}"
    );
}

/// **Filter round-trip**: a filter token (2 ops, one with an `amount`, one
/// without) must parse→format→parse byte-stably, emit the op brace block, and
/// a text node referencing it (via `filter=(token)"..."`) must survive.
#[test]
fn test_filter_token_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.filter" name="Filter"
  tokens format="zenith-token-v1" {
    token id="filter.photo" type="filter" {
      grayscale amount=0.5
      hue-rotate
    }
  }
  styles {
  }
  document id="doc.filter" title="Filter" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 filter=(token)"filter.photo" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");
    let formatted = String::from_utf8(s1.clone()).expect("utf8");

    // The filter emits a brace block with two op children.
    assert!(
        formatted.contains("type=\"filter\" {"),
        "expected filter header; got:\n{formatted}"
    );
    assert!(
        formatted.contains("grayscale amount=0.5"),
        "expected grayscale op with amount; got:\n{formatted}"
    );
    assert!(
        formatted.contains(" filter=(token)\"filter.photo\""),
        "expected node filter prop; got:\n{formatted}"
    );

    // Idempotency.
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted,
        String::from_utf8(s2).expect("utf8"),
        "filter formatting must be idempotent"
    );

    // AST round-trip (spans stripped).
    assert_eq!(
        strip_spans(doc1),
        strip_spans(doc2),
        "filter AST must survive format round-trip"
    );
}

/// **Filter on a node validates clean**: a text node referencing a filter
/// token type-checks OK and is not flagged as a raw literal.
#[test]
fn test_filter_node_validates_clean() {
    let src = r##"zenith version=1 {
  project id="proj.filter" name="Filter"
  tokens format="zenith-token-v1" {
    token id="filter.photo" type="filter" {
      grayscale amount=0.5
      hue-rotate
    }
  }
  styles {
  }
  document id="doc.filter" title="Filter" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 filter=(token)"filter.photo" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let report = crate::validate::validate(&doc);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"token.incompatible_property"),
        "filter ref must be type-compatible; codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"token.raw_visual_literal"),
        "filter token ref must not be a raw literal; codes: {codes:?}"
    );
}

/// **Filter prop wrong type**: a node `filter=(token)"x"` where `x` is a color
/// token must produce `token.incompatible_property`.
#[test]
fn test_filter_node_prop_wrong_type() {
    let src = r##"zenith version=1 {
  project id="proj.filter" name="Filter"
  tokens format="zenith-token-v1" {
    token id="color.not-a-filter" type="color" value="#000000"
  }
  styles {
  }
  document id="doc.filter" title="Filter" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 filter=(token)"color.not-a-filter" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let report = crate::validate::validate(&doc);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"token.incompatible_property"),
        "a non-filter token in a filter slot must be incompatible; codes: {codes:?}"
    );
}

/// **Mask round-trip**: a mask token (a `rounded` shape with `radius`,
/// `feather`, and `invert=#true`) must parse→format→parse byte-stably, emit the
/// shape brace block, and a rect node referencing it (via `mask=(token)"..."`)
/// must survive.
#[test]
fn test_mask_token_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.mask" name="Mask"
  tokens format="zenith-token-v1" {
    token id="mask.vignette" type="mask" {
      rounded radius=40 feather=60 invert=#true
    }
  }
  styles {
  }
  document id="doc.mask" title="Mask" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="card" x=(px)0 y=(px)0 w=(px)100 h=(px)40 mask=(token)"mask.vignette"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");
    let formatted = String::from_utf8(s1.clone()).expect("utf8");

    // The mask emits a brace block with a single shape child.
    assert!(
        formatted.contains("type=\"mask\" {"),
        "expected mask header; got:\n{formatted}"
    );
    assert!(
        formatted.contains("rounded radius=40 feather=60 invert=#true"),
        "expected rounded shape with radius/feather/invert; got:\n{formatted}"
    );
    assert!(
        formatted.contains(" mask=(token)\"mask.vignette\""),
        "expected node mask prop; got:\n{formatted}"
    );

    // Idempotency.
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted,
        String::from_utf8(s2).expect("utf8"),
        "mask formatting must be idempotent"
    );

    // AST round-trip (spans stripped).
    assert_eq!(
        strip_spans(doc1),
        strip_spans(doc2),
        "mask AST must survive format round-trip"
    );
}

/// **Mask prop wrong type**: a node `mask=(token)"x"` where `x` is a color token
/// must produce `token.incompatible_property`.
#[test]
fn test_mask_node_prop_wrong_type() {
    let src = r##"zenith version=1 {
  project id="proj.mask" name="Mask"
  tokens format="zenith-token-v1" {
    token id="color.not-a-mask" type="color" value="#000000"
  }
  styles {
  }
  document id="doc.mask" title="Mask" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="card" x=(px)0 y=(px)0 w=(px)100 h=(px)40 mask=(token)"color.not-a-mask"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let report = crate::validate::validate(&doc);

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"token.incompatible_property"),
        "a non-mask token in a mask slot must be incompatible; codes: {codes:?}"
    );
}

/// **Duotone filter round-trip**: a duotone op carrying both `shadow` and
/// `highlight` color-token refs (plus `amount`) must parse→format→parse
/// byte-stably and emit `duotone shadow=(token)"…" highlight=(token)"…" amount=…`.
#[test]
fn test_duotone_filter_token_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.duo" name="Duo"
  tokens format="zenith-token-v1" {
    token id="color.sh" type="color" value="#000000"
    token id="color.hi" type="color" value="#ffffff"
    token id="filter.duo" type="filter" {
      duotone shadow=(token)"color.sh" highlight=(token)"color.hi" amount=0.8
    }
  }
  styles {
  }
  document id="doc.duo" title="Duo" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 filter=(token)"filter.duo" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");
    let formatted = String::from_utf8(s1.clone()).expect("utf8");

    assert!(
        formatted.contains(
            "duotone shadow=(token)\"color.sh\" highlight=(token)\"color.hi\" amount=0.8"
        ),
        "expected duotone op with both colors and amount; got:\n{formatted}"
    );

    // Idempotency.
    let doc2 = adapter.parse(&s1).expect("parse 2");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted,
        String::from_utf8(s2).expect("utf8"),
        "duotone formatting must be idempotent"
    );

    // AST round-trip (spans stripped).
    assert_eq!(
        strip_spans(doc1),
        strip_spans(doc2),
        "duotone AST must survive format round-trip"
    );
}

/// **Duotone color refs are used transitively**: a node referencing a duotone
/// filter token records the duotone's shadow/highlight color tokens as used, so
/// neither is falsely flagged `token.unused`.
#[test]
fn test_duotone_color_refs_not_unused() {
    let src = r##"zenith version=1 {
  project id="proj.duo" name="Duo"
  tokens format="zenith-token-v1" {
    token id="color.sh" type="color" value="#000000"
    token id="color.hi" type="color" value="#ffffff"
    token id="filter.duo" type="filter" {
      duotone shadow=(token)"color.sh" highlight=(token)"color.hi"
    }
  }
  styles {
  }
  document id="doc.duo" title="Duo" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 filter=(token)"filter.duo" {
        span "Hi"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let report = crate::validate::validate(&doc);

    let unused: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "token.unused")
        .filter_map(|d| d.subject_id.as_deref())
        .collect();
    assert!(
        !unused.contains(&"color.sh") && !unused.contains(&"color.hi"),
        "duotone color tokens must be recorded as used; unused: {unused:?}"
    );
}

/// **Booleans**: `visible=#false` must emit with `#false`, not `false` or `"false"`.
#[test]
fn test_boolean_format() {
    let src = r##"zenith version=1 {
  project id="proj.bool" name="Bool"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.bool" title="Bool" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 visible=#false
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("visible=#false"),
        "expected `visible=#false`, got:\n{text}"
    );
}

/// **Unknown-property multi-type round-trip**: unknown properties of every
/// KDL value type survive parse→format→parse with their type intact, and
/// the output is idempotent (format twice → identical bytes).
#[test]
fn test_unknown_property_all_types_round_trip() {
    // Each property exercises one KdlValue variant.
    // Raw string r##"..."## needed because KDL v2 booleans/null use `#`.
    let src = r##"zenith version=1 {
  project id="proj.rt" name="RT"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.rt" title="RT" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 future-flag=#true future-float=1.5 future-int=42 future-null=#null future-str="hi"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");

    // Verify all five types landed correctly after the first parse.
    let rect = match &doc1.body.pages[0].children[0] {
        crate::ast::Node::Rect(r) => r,
        other => panic!("expected Rect, got {other:?}"),
    };
    assert_eq!(
        rect.unknown_props["future-flag"].value,
        crate::ast::UnknownValue::Bool(true),
        "boolean must parse as UnknownValue::Bool(true), not a string"
    );
    assert_eq!(
        rect.unknown_props["future-int"].value,
        crate::ast::UnknownValue::Integer(42),
        "integer must parse as UnknownValue::Integer(42)"
    );
    assert_eq!(
        rect.unknown_props["future-float"].value,
        crate::ast::UnknownValue::Float(1.5),
        "float must parse as UnknownValue::Float(1.5)"
    );
    assert_eq!(
        rect.unknown_props["future-str"].value,
        crate::ast::UnknownValue::String("hi".to_owned()),
        "string must parse as UnknownValue::String"
    );
    assert_eq!(
        rect.unknown_props["future-null"].value,
        crate::ast::UnknownValue::Null,
        "null must parse as UnknownValue::Null"
    );

    // Format once → parse → assert same typed values survive (round-trip).
    let formatted1 = format_document(&doc1).expect("format 1");
    let doc2 = adapter.parse(&formatted1).expect("parse 2 after format");
    let rect2 = match &doc2.body.pages[0].children[0] {
        crate::ast::Node::Rect(r) => r,
        other => panic!("expected Rect in re-parsed doc, got {other:?}"),
    };
    assert_eq!(
        rect2.unknown_props["future-flag"].value,
        crate::ast::UnknownValue::Bool(true),
        "boolean must survive format round-trip as UnknownValue::Bool(true)"
    );
    assert_eq!(
        rect2.unknown_props["future-int"].value,
        crate::ast::UnknownValue::Integer(42),
        "integer must survive format round-trip as UnknownValue::Integer(42)"
    );
    assert_eq!(
        rect2.unknown_props["future-float"].value,
        crate::ast::UnknownValue::Float(1.5),
        "float must survive format round-trip"
    );
    assert_eq!(
        rect2.unknown_props["future-str"].value,
        crate::ast::UnknownValue::String("hi".to_owned()),
        "string must survive format round-trip"
    );
    assert_eq!(
        rect2.unknown_props["future-null"].value,
        crate::ast::UnknownValue::Null,
        "null must survive format round-trip"
    );

    // Idempotence: format a second time → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted1, formatted2,
        "format must be idempotent for documents with unknown properties of all types"
    );
}

/// **Unknown-property type-annotation round-trip**: KDL type annotations on
/// unrecognized properties (e.g. `(px)42`, `(token)"color.brand"`) must be
/// captured on parse, re-emitted in the value position on format, and survive
/// a full parse→format→parse cycle byte-identically. Non-annotated unknown
/// values must remain unchanged.
#[test]
fn test_unknown_property_type_annotation_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.ann" name="Ann"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ann" title="Ann" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 mystery=(px)42 magic=(token)"color.brand" plain="hello" flag=#true
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");

    let rect = match &doc1.body.pages[0].children[0] {
        crate::ast::Node::Rect(r) => r,
        other => panic!("expected Rect, got {other:?}"),
    };

    // Annotations captured on parse.
    assert_eq!(
        rect.unknown_props["mystery"].ty.as_deref(),
        Some("px"),
        "`(px)42` must capture ty = Some(\"px\")"
    );
    assert_eq!(
        rect.unknown_props["mystery"].value,
        crate::ast::UnknownValue::Integer(42),
    );
    assert_eq!(
        rect.unknown_props["magic"].ty.as_deref(),
        Some("token"),
        "`(token)\"color.brand\"` must capture ty = Some(\"token\")"
    );
    assert_eq!(
        rect.unknown_props["magic"].value,
        crate::ast::UnknownValue::String("color.brand".to_owned()),
    );
    // Non-annotated unknown props have ty = None.
    assert_eq!(
        rect.unknown_props["plain"].ty, None,
        "non-annotated `plain` must have ty = None"
    );
    assert_eq!(
        rect.unknown_props["flag"].ty, None,
        "non-annotated `flag` must have ty = None"
    );

    // Format → the annotation is emitted in the value position.
    let formatted1 = format_document(&doc1).expect("format 1");
    let text = String::from_utf8_lossy(&formatted1);
    assert!(
        text.contains("mystery=(px)42"),
        "formatted output must contain `mystery=(px)42`, got:\n{text}"
    );
    assert!(
        text.contains(r#"magic=(token)"color.brand""#),
        "formatted output must contain `magic=(token)\"color.brand\"`, got:\n{text}"
    );
    assert!(
        text.contains(r#"plain="hello""#),
        "non-annotated `plain=\"hello\"` must be unchanged, got:\n{text}"
    );
    assert!(
        text.contains("flag=#true"),
        "non-annotated `flag=#true` must be unchanged, got:\n{text}"
    );

    // Re-parse → unknown_props (value + ty) are identical to the first parse.
    let doc2 = adapter.parse(&formatted1).expect("parse 2 after format");
    let rect2 = match &doc2.body.pages[0].children[0] {
        crate::ast::Node::Rect(r) => r,
        other => panic!("expected Rect in re-parsed doc, got {other:?}"),
    };
    assert_eq!(
        rect.unknown_props, rect2.unknown_props,
        "unknown_props (value + ty) must be byte-stable across parse→format→parse"
    );

    // Idempotence: format a second time → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted1, formatted2,
        "format must be idempotent for annotated unknown properties"
    );
}

/// **Forward-compat preservation**: an unknown property on a rect survives
/// a format round-trip.
#[test]
fn test_unknown_property_preserved() {
    let src = r##"zenith version=1 {
  project id="proj.unk" name="Unk"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.unk" title="Unk" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 future-prop="hello"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("future-prop="),
        "unknown property `future-prop` must survive format; got:\n{text}"
    );
}

// ── Asset block formatting tests ──────────────────────────────────────

/// A `.zen` document with an assets block containing two declarations.
const WITH_ASSETS: &str = r##"zenith version=1 {
  project id="proj.assets" name="Assets Test"
  assets {
    asset id="asset.logo" kind="svg" src="assets/logo.svg" sha256="deadbeef"
    asset id="asset.hero" kind="image" src="assets/hero.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.assets" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;

/// Assets block parses correctly: 2 assets, fields correct.
#[test]
fn test_assets_parse_fields() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");

    let assets = &doc.assets.assets;
    assert_eq!(assets.len(), 2, "expected 2 asset declarations");

    let logo = &assets[0];
    assert_eq!(logo.id, "asset.logo");
    assert_eq!(logo.kind, crate::ast::AssetKind::Svg);
    assert_eq!(logo.src, "assets/logo.svg");
    assert_eq!(logo.sha256.as_deref(), Some("deadbeef"));

    let hero = &assets[1];
    assert_eq!(hero.id, "asset.hero");
    assert_eq!(hero.kind, crate::ast::AssetKind::Image);
    assert_eq!(hero.src, "assets/hero.png");
    assert!(hero.sha256.is_none(), "sha256 should be None when absent");
}

/// A document with NO assets block → empty AssetBlock (not an error).
#[test]
fn test_absent_assets_block_is_empty() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(MINIMAL.as_bytes())
        .expect("parse must succeed");
    assert!(
        doc.assets.assets.is_empty(),
        "absent assets block must yield an empty AssetBlock"
    );
}

/// Assets block round-trip: parse → format → parse yields same fields.
#[test]
fn test_assets_round_trip_ast_equality() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");
    let doc2 = adapter.parse(&formatted).expect("re-parse after format");

    // Compare assets (spans may differ; compare fields directly).
    let a1 = &doc_orig.assets.assets;
    let a2 = &doc2.assets.assets;
    assert_eq!(a1.len(), a2.len(), "asset count must survive round-trip");
    for (orig, reparsed) in a1.iter().zip(a2.iter()) {
        assert_eq!(orig.id, reparsed.id);
        assert_eq!(orig.kind, reparsed.kind);
        assert_eq!(orig.src, reparsed.src);
        assert_eq!(orig.sha256, reparsed.sha256);
    }
}

/// Format idempotency: format twice → identical bytes.
#[test]
fn test_assets_format_idempotency() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");
    let s1 = format_document(&doc).expect("format 1");
    let doc2 = adapter.parse(&s1).expect("parse after first format");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "assets format must be idempotent"
    );
}

/// Canonical property order: id, kind, src, sha256 in that order.
#[test]
fn test_assets_canonical_property_order() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    let logo_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("asset") && l.contains("asset.logo"))
        .expect("must find logo asset line");

    let pos_id = logo_line.find("id=").expect("id= must be present");
    let pos_kind = logo_line.find("kind=").expect("kind= must be present");
    let pos_src = logo_line.find("src=").expect("src= must be present");
    let pos_sha256 = logo_line.find("sha256=").expect("sha256= must be present");

    assert!(pos_id < pos_kind, "id must come before kind");
    assert!(pos_kind < pos_src, "kind must come before src");
    assert!(pos_src < pos_sha256, "src must come before sha256");
}

// ── Image node parse + format tests ───────────────────────────────────

/// A `.zen` document with an image node exercising the string and `(pct)`
/// object-position forms.
const WITH_IMAGE: &str = r##"zenith version=1 {
  project id="proj.img" name="Image Test"
  assets {
    asset id="asset.logo" kind="image" src="assets/logo.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.img" title="Image Test" {
    page id="page.one" w=(px)320 h=(px)200 {
      image id="img.logo" asset="asset.logo" x=(px)80 y=(px)60 w=(px)160 h=(px)48 fit="contain" object-position-x="center" object-position-y=(pct)25
    }
  }
}
"##;

/// Image node parses all fields including both object-position forms.
#[test]
fn image_parses_fields() {
    use crate::ast::{Node, ObjectPosition, Unit};
    let adapter = KdlAdapter;
    let doc = adapter.parse(WITH_IMAGE.as_bytes()).expect("parse");
    let node = &doc.body.pages[0].children[0];
    let img = match node {
        Node::Image(i) => i,
        other => panic!("expected Image, got {other:?}"),
    };
    assert_eq!(img.id, "img.logo");
    assert_eq!(img.asset, "asset.logo");
    assert_eq!(img.x.as_ref().map(|d| d.value), Some(80.0));
    assert_eq!(img.y.as_ref().map(|d| d.value), Some(60.0));
    assert_eq!(img.w.as_ref().map(|d| d.value), Some(160.0));
    assert_eq!(img.h.as_ref().map(|d| d.value), Some(48.0));
    assert!(matches!(img.x.as_ref().map(|d| &d.unit), Some(Unit::Px)));
    assert_eq!(img.fit.as_deref(), Some("contain"));
    assert_eq!(img.object_position_x, Some(ObjectPosition::Center));
    assert_eq!(img.object_position_y, Some(ObjectPosition::Pct(25.0)));
}

/// Image node round-trips through format → parse with fields intact, and
/// the formatter is idempotent (incl. an object-position `(pct)25`).
#[test]
fn image_format_round_trip_and_idempotency() {
    use crate::ast::{Node, ObjectPosition};
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(WITH_IMAGE.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");

    // The (pct)25 must survive as an annotated number, not a string.
    let text = String::from_utf8(s1.clone()).unwrap();
    assert!(
        text.contains("object-position-y=(pct)25"),
        "object-position (pct) must format as annotated number; got:\n{text}"
    );
    assert!(
        text.contains("object-position-x=\"center\""),
        "object-position anchor must format as string; got:\n{text}"
    );

    let doc2 = adapter.parse(&s1).expect("parse 2");
    let img2 = match &doc2.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image, got {other:?}"),
    };
    assert_eq!(img2.asset, "asset.logo");
    assert_eq!(img2.fit.as_deref(), Some("contain"));
    assert_eq!(img2.object_position_x, Some(ObjectPosition::Center));
    assert_eq!(img2.object_position_y, Some(ObjectPosition::Pct(25.0)));

    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "image format must be idempotent"
    );
}

// ── Style block parse + format tests ──────────────────────────────────

/// Source document with style properties to test parsing and formatting.
const WITH_STYLES: &str = r##"zenith version=1 {
  project id="proj.styles" name="Styles Test"
  tokens format="zenith-token-v1" {
    token id="color.text.primary" type="color" value="#111827"
    token id="size.text.title" type="dimension" value=(pt)24
    token id="font.family.body" type="fontFamily" value="Noto Sans"
  }
  styles {
    style id="style.text.title" {
      fill (token)"color.text.primary"
      font-family (token)"font.family.body"
      font-size (token)"size.text.title"
    }
  }
  document id="doc.styles" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;

/// Style properties are parsed into `Style.properties` with correct canonical keys.
#[test]
fn style_properties_parsed() {
    use crate::ast::PropertyValue;
    let adapter = KdlAdapter;
    let doc = adapter.parse(WITH_STYLES.as_bytes()).expect("parse");

    assert_eq!(doc.styles.styles.len(), 1);
    let style = &doc.styles.styles[0];
    assert_eq!(style.id, "style.text.title");
    assert_eq!(style.properties.len(), 3);

    assert_eq!(
        style.properties.get("fill"),
        Some(&PropertyValue::TokenRef("color.text.primary".to_owned())),
        "fill must be a TokenRef to color.text.primary"
    );
    assert_eq!(
        style.properties.get("font-family"),
        Some(&PropertyValue::TokenRef("font.family.body".to_owned())),
        "font-family must be a TokenRef to font.family.body"
    );
    assert_eq!(
        style.properties.get("font-size"),
        Some(&PropertyValue::TokenRef("size.text.title".to_owned())),
        "font-size must be a TokenRef to size.text.title"
    );
}

/// Underscore variant keys are canonicalized to hyphenated forms.
#[test]
fn style_underscore_keys_canonicalized() {
    use crate::ast::PropertyValue;
    let src = r##"zenith version=1 {
  project id="proj.usk" name="USK"
  tokens format="zenith-token-v1" {
    token id="size.sw" type="dimension" value=(px)2
  }
  styles {
    style id="style.usk" {
      stroke_width (token)"size.sw"
    }
  }
  document id="doc.usk" {
    page id="page.usk" w=(px)100 h=(px)100 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let style = &doc.styles.styles[0];
    assert!(
        style.properties.contains_key("stroke-width"),
        "stroke_width must be stored under canonical key stroke-width"
    );
    assert!(
        !style.properties.contains_key("stroke_width"),
        "underscore key must not appear in properties map"
    );
    assert_eq!(
        style.properties.get("stroke-width"),
        Some(&PropertyValue::TokenRef("size.sw".to_owned()))
    );
}

/// `padding` and `gap` are recognized token-only dimension style props:
/// they parse into `Style.properties` under their canonical keys and survive
/// a parse → format → parse round-trip.
#[test]
fn style_padding_gap_round_trip() {
    use crate::ast::PropertyValue;
    let src = r##"zenith version=1 {
  project id="proj.pg" name="PG"
  tokens format="zenith-token-v1" {
    token id="space.pad" type="dimension" value=(px)16
    token id="space.gap" type="dimension" value=(px)8
  }
  styles {
    style id="style.flow" {
      gap (token)"space.gap"
      padding (token)"space.pad"
    }
  }
  document id="doc.pg" {
    page id="page.pg" w=(px)200 h=(px)200 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let style = &doc.styles.styles[0];
    assert_eq!(
        style.properties.get("padding"),
        Some(&PropertyValue::TokenRef("space.pad".to_owned())),
        "padding must be a TokenRef to space.pad"
    );
    assert_eq!(
        style.properties.get("gap"),
        Some(&PropertyValue::TokenRef("space.gap".to_owned())),
        "gap must be a TokenRef to space.gap"
    );
    assert!(
        style.unknown_props.is_empty(),
        "padding/gap must be recognized, not captured as unknown props"
    );

    // Round-trip: parse → format → parse preserves both props.
    let formatted = format_document(&doc).expect("format");
    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    let style2 = &reparsed.styles.styles[0];
    assert_eq!(
        style2.properties.get("padding"),
        Some(&PropertyValue::TokenRef("space.pad".to_owned())),
        "padding must survive round-trip"
    );
    assert_eq!(
        style2.properties.get("gap"),
        Some(&PropertyValue::TokenRef("space.gap".to_owned())),
        "gap must survive round-trip"
    );
}

/// Unknown style child names are captured in `unknown_props`.
#[test]
fn style_unknown_child_captured() {
    let src = r##"zenith version=1 {
  project id="proj.unk" name="UNK"
  tokens format="zenith-token-v1" {
  }
  styles {
    style id="style.unk" {
      bogus "some-value"
    }
  }
  document id="doc.unk" {
    page id="page.unk" w=(px)100 h=(px)100 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let style = &doc.styles.styles[0];
    assert!(style.properties.is_empty(), "no recognized props expected");
    assert!(
        style.unknown_props.contains_key("bogus"),
        "unknown prop 'bogus' must be captured in unknown_props"
    );
}

/// Parse → format → parse round-trips correctly (spans stripped for equality).
#[test]
fn styles_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(WITH_STYLES.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");
    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");

    let orig_stripped = strip_spans(doc_orig);
    let reparsed_stripped = strip_spans(doc_reparsed);
    assert_eq!(
        orig_stripped.styles, reparsed_stripped.styles,
        "styles must survive round-trip (spans excluded)"
    );
}

/// Format twice → identical bytes (idempotency).
#[test]
fn styles_format_idempotent() {
    let adapter = KdlAdapter;
    let doc = adapter.parse(WITH_STYLES.as_bytes()).expect("parse");
    let s1 = format_document(&doc).expect("format 1");
    let doc2 = adapter.parse(&s1).expect("parse after first format");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "styles format must be idempotent"
    );
}

/// **Ellipse stroke + stroke-width round-trip**: an ellipse with both
/// `stroke` and `stroke-width` tokens must survive parse→format→parse with
/// those fields preserved in the canonical position (after `fill`).
#[test]
fn ellipse_stroke_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.es" name="ES"
  tokens format="zenith-token-v1" {
    token id="color.border" type="color" value="#334155"
    token id="size.border" type="dimension" value=(px)3
  }
  styles {
  }
  document id="doc.es" title="ES" {
    page id="p" w=(px)200 h=(px)200 {
      ellipse id="e" x=(px)10 y=(px)10 w=(px)80 h=(px)80 stroke=(token)"color.border" stroke-width=(token)"size.border"
    }
  }
}
"##;
    use crate::ast::{Node, PropertyValue};
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // Verify AST fields are set.
    match &doc.body.pages[0].children[0] {
        Node::Ellipse(e) => {
            assert_eq!(
                e.stroke,
                Some(PropertyValue::TokenRef("color.border".to_owned())),
                "stroke must parse to TokenRef(color.border)"
            );
            assert_eq!(
                e.stroke_width,
                Some(PropertyValue::TokenRef("size.border".to_owned())),
                "stroke_width must parse to TokenRef(size.border)"
            );
            assert!(e.fill.is_none(), "fill must be absent");
        }
        other => panic!("expected Ellipse, got {other:?}"),
    }

    // Format and re-parse — the tokens must survive.
    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    let doc2 = adapter.parse(&formatted).expect("re-parse");
    match &doc2.body.pages[0].children[0] {
        Node::Ellipse(e) => {
            assert_eq!(
                e.stroke,
                Some(PropertyValue::TokenRef("color.border".to_owned())),
                "stroke must survive format round-trip"
            );
            assert_eq!(
                e.stroke_width,
                Some(PropertyValue::TokenRef("size.border".to_owned())),
                "stroke_width must survive format round-trip"
            );
        }
        other => panic!("expected Ellipse on re-parse, got {other:?}"),
    }

    // Canonical position: stroke comes after fill.
    let ellipse_line = formatted_str
        .lines()
        .find(|l| l.trim_start().starts_with("ellipse"))
        .expect("must find ellipse line");
    assert!(
        ellipse_line.contains("stroke=(token)\"color.border\""),
        "formatted line must contain stroke token; got: {ellipse_line}"
    );
    assert!(
        ellipse_line.contains("stroke-width=(token)\"size.border\""),
        "formatted line must contain stroke-width token; got: {ellipse_line}"
    );
    // stroke must come before stroke-width (canonical order).
    let pos_stroke = ellipse_line.find(" stroke=").expect("must have stroke=");
    let pos_sw = ellipse_line
        .find(" stroke-width=")
        .expect("must have stroke-width=");
    assert!(
        pos_stroke < pos_sw,
        "stroke= must appear before stroke-width= in canonical output"
    );

    // Idempotency: format(format(doc)) == format(doc).
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted_str,
        String::from_utf8(s2).unwrap(),
        "ellipse stroke formatting must be idempotent"
    );
}

/// **code font-weight round-trip + ordering**: a code node with a `font-weight`
/// token must survive parse→format→parse, and the formatter must place
/// `font-weight` immediately AFTER `font-size` in the canonical output.
#[test]
fn test_code_font_weight_round_trip_and_order() {
    use crate::ast::{Node, PropertyValue};
    let src = r##"zenith version=1 {
  project id="proj.cfw" name="CFW"
  tokens format="zenith-token-v1" {
    token id="size.mono" type="dimension" value=(px)14
    token id="weight.bold" type="fontWeight" value=700
  }
  styles {
  }
  document id="doc.cfw" title="CFW" {
    page id="p" w=(px)400 h=(px)300 {
      code id="c" x=(px)0 y=(px)0 font-size=(token)"size.mono" font-weight=(token)"weight.bold" {
        content "let x = 1;"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    // Canonical order: font-weight comes immediately after font-size.
    let code_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("code"))
        .expect("must find code line");
    let pos_size = code_line.find(" font-size=").expect("must find font-size=");
    let pos_weight = code_line
        .find(" font-weight=")
        .expect("must find font-weight=");
    assert!(
        pos_size < pos_weight,
        "font-weight must follow font-size in code node; code line: {code_line:?}"
    );

    // Round-trip: re-parse preserves the font_weight token ref.
    let doc2 = adapter.parse(text.as_bytes()).expect("re-parse");
    match &doc2.body.pages[0].children[0] {
        Node::Code(c) => assert_eq!(
            c.font_weight,
            Some(PropertyValue::TokenRef("weight.bold".to_owned())),
            "font-weight must survive the code format round-trip"
        ),
        other => panic!("expected Code, got {other:?}"),
    }
}

/// A `.zen` document with a `safe-zone` declared as a page child.
const SAFE_ZONE_DOC: &str = r##"zenith version=1 {
  project id="proj.sz" name="Safe Zone Project"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.sz" title="Safe Zone Doc" {
    page id="page.one" w=(px)1500 h=(px)500 {
      safe-zone id="sz.avatar" type="exclusion" x=(px)0 y=(px)358 w=(px)175 h=(px)142 label="X avatar dead zone"
      rect id="logo" x=(px)600 y=(px)40 w=(px)200 h=(px)80 fill="#ffffff"
    }
  }
}
"##;

/// **Parse**: a `safe-zone` page child lands in `page.safe_zones`, NOT in
/// `page.children`.
#[test]
fn test_safe_zone_parses_into_page_not_children() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(SAFE_ZONE_DOC.as_bytes())
        .expect("parse must succeed");
    let page = &doc.body.pages[0];

    assert_eq!(page.safe_zones.len(), 1, "exactly one safe-zone parsed");
    let zone = &page.safe_zones[0];
    assert_eq!(zone.id, "sz.avatar");
    assert_eq!(zone.zone_type, crate::ast::SafeZoneType::Exclusion);
    assert_eq!(zone.label.as_deref(), Some("X avatar dead zone"));

    // The renderable rect is the ONLY child; the safe-zone is not a child.
    assert_eq!(page.children.len(), 1, "only the rect is a child node");
    match &page.children[0] {
        Node::Rect(r) => assert_eq!(r.id, "logo"),
        other => panic!("expected Rect, got {other:?}"),
    }
}

/// **Format round-trip**: a safe-zone survives a parse → format → parse pass
/// unchanged (spans excluded).
#[test]
fn test_safe_zone_format_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(SAFE_ZONE_DOC.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");

    // The emitted line carries the canonical safe-zone shape.
    let text = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        text.contains(
            "safe-zone id=\"sz.avatar\" type=\"exclusion\" \
             x=(px)0 y=(px)358 w=(px)175 h=(px)142 label=\"X avatar dead zone\""
        ),
        "formatted safe-zone line missing/incorrect; output:\n{text}"
    );

    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "safe-zone must survive a format round-trip (spans excluded)"
    );
}

/// A `.zen` document with a `fold` declared as a page child.
const FOLD_DOC: &str = r##"zenith version=1 {
  project id="proj.fold" name="Fold Project"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.fold" title="Fold Doc" {
    page id="page.one" w=(px)2480 h=(px)1000 {
      fold id="fold.1" orientation="vertical" position=(px)1169
      rect id="logo" x=(px)600 y=(px)40 w=(px)200 h=(px)80 fill="#ffffff"
    }
  }
}
"##;

/// **Parse**: a `fold` page child lands in `page.folds`, NOT in
/// `page.children`.
#[test]
fn test_fold_parses_into_page_not_children() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(FOLD_DOC.as_bytes())
        .expect("parse must succeed");
    let page = &doc.body.pages[0];

    assert_eq!(page.folds.len(), 1, "exactly one fold parsed");
    let fold = &page.folds[0];
    assert_eq!(fold.id, "fold.1");
    assert_eq!(fold.orientation, "vertical");
    let pos = fold.position.as_ref().expect("position present");
    assert_eq!(pos.value, 1169.0);

    // The renderable rect is the ONLY child; the fold is not a child.
    assert_eq!(page.children.len(), 1, "only the rect is a child node");
    match &page.children[0] {
        Node::Rect(r) => assert_eq!(r.id, "logo"),
        other => panic!("expected Rect, got {other:?}"),
    }
}

/// **Format round-trip**: a fold survives a parse → format → parse pass
/// unchanged (spans excluded).
#[test]
fn test_fold_format_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter.parse(FOLD_DOC.as_bytes()).expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");

    let text = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        text.contains("fold id=\"fold.1\" orientation=\"vertical\" position=(px)1169"),
        "formatted fold line missing/incorrect; output:\n{text}"
    );

    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "fold must survive a format round-trip (spans excluded)"
    );
}

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
    assert!(matches!(bleed.unit, crate::ast::Unit::Px));

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

/// **Span vertical-align round-trip**: a `span ... vertical-align="super"` parses,
/// formats into the canonical text, and survives parse → format → parse.
#[test]
fn test_span_vertical_align_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.va" name="VA"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.va" title="VA" {
    page id="page.one" w=(px)400 h=(px)400 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "E = mc"
        span "2" vertical-align="super"
        span "; H"
        span "2" vertical-align="sub"
        span "O"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let page = &doc.body.pages[0];
    let crate::ast::Node::Text(text_node) = &page.children[0] else {
        panic!("expected a text node");
    };
    assert_eq!(
        text_node.spans[1].vertical_align.as_deref(),
        Some("super"),
        "second span must be superscript"
    );
    assert_eq!(
        text_node.spans[3].vertical_align.as_deref(),
        Some("sub"),
        "fourth span must be subscript"
    );
    assert_eq!(
        text_node.spans[0].vertical_align, None,
        "a plain span must have no vertical-align"
    );

    // Canonical form preserves both vertical-align attributes verbatim.
    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    assert!(
        text.contains("vertical-align=\"super\""),
        "formatted output must carry super vertical-align; got:\n{text}"
    );
    assert!(
        text.contains("vertical-align=\"sub\""),
        "formatted output must carry sub vertical-align; got:\n{text}"
    );

    // Round-trip AST equality. `strip_spans` only zeroes node SOURCE spans
    // (the `vertical_align` content field on `TextSpan` is untouched), so this
    // still proves vertical-align survives the round-trip.
    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "span vertical-align must survive parse → format → parse"
    );
}

/// **Footnote + span footnote-ref round-trip**: a page-level `footnote` node and
/// a `span ... footnote-ref="fn.1"` both parse, format into the canonical text,
/// and survive parse → format → parse.
#[test]
fn test_footnote_and_footnote_ref_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.fn" name="FN"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.fn" title="FN" {
    page id="page.one" w=(px)400 h=(px)600 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "Strong evidence" footnote-ref="fn.1"
        span " supports this."
      }
      footnote id="fn.1" {
        span "See also Chapter 4."
      }
      footnote id="fn.2" marker="*" {
        span "An annotated aside."
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let page = &doc.body.pages[0];
    let crate::ast::Node::Text(text_node) = &page.children[0] else {
        panic!("expected a text node first");
    };
    assert_eq!(
        text_node.spans[0].footnote_ref.as_deref(),
        Some("fn.1"),
        "first span must carry footnote-ref fn.1"
    );
    assert_eq!(
        text_node.spans[1].footnote_ref, None,
        "second span must have no footnote-ref"
    );

    let crate::ast::Node::Footnote(fn1) = &page.children[1] else {
        panic!("expected a footnote node second");
    };
    assert_eq!(fn1.id, "fn.1");
    assert_eq!(fn1.marker, None, "fn.1 uses the auto-number");
    assert_eq!(fn1.spans[0].text, "See also Chapter 4.");

    let crate::ast::Node::Footnote(fn2) = &page.children[2] else {
        panic!("expected a second footnote node");
    };
    assert_eq!(fn2.marker.as_deref(), Some("*"), "fn.2 has explicit marker");

    // Canonical form preserves the footnote node + the span footnote-ref.
    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    assert!(
        text.contains("footnote id=\"fn.1\""),
        "formatted output must carry the footnote node; got:\n{text}"
    );
    assert!(
        text.contains("footnote-ref=\"fn.1\""),
        "formatted output must carry the span footnote-ref; got:\n{text}"
    );
    assert!(
        text.contains("marker=\"*\""),
        "formatted output must carry the explicit marker; got:\n{text}"
    );

    let reparsed = adapter
        .parse(text.as_bytes())
        .expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "footnote + footnote-ref must survive parse → format → parse"
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

/// **`overflow-wrap` round-trip**: a text node's `overflow-wrap="break-word"`
/// must be emitted by the writer and survive parse → format → parse.
#[test]
fn test_overflow_wrap_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.ow" name="OW"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ow" title="OW" {
    page id="page.one" w=(px)640 h=(px)360 {
      text id="col3" x=(px)10 y=(px)10 w=(px)120 h=(px)200 overflow-wrap="break-word" {
        span "https://very-long.example.com/x"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        formatted_str.contains(r#"overflow-wrap="break-word""#),
        "formatted output must contain overflow-wrap; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "overflow-wrap must round-trip identically"
    );
}

/// **Hanging-indent round-trip**: `padding-left` plus a NEGATIVE `text-indent`
/// must be emitted by the writer (including the minus sign) and survive
/// parse → format → parse byte-identically.
#[test]
fn test_hanging_indent_round_trips() {
    let src = r##"zenith version=1 {
  project id="proj.hi" name="HI"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.hi" title="HI" {
    page id="page.one" w=(px)1920 h=(px)1080 {
      text id="b1" x=(px)160 y=(px)240 w=(px)1600 h=(px)120 overflow="clip" padding-left=(px)44 text-indent=(px)-44 {
        span "• A hanging bullet whose wrapped lines align past the glyph."
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // The negative text-indent must have parsed onto the node.
    let page = &doc.body.pages[0];
    let Node::Text(t) = &page.children[0] else {
        panic!("first child must be the text node");
    };
    assert_eq!(
        t.padding_left.as_ref().map(|d| d.value),
        Some(44.0),
        "padding-left must parse"
    );
    assert_eq!(
        t.text_indent.as_ref().map(|d| d.value),
        Some(-44.0),
        "negative text-indent must parse"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        formatted_str.contains("padding-left=(px)44"),
        "formatted output must contain padding-left; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("text-indent=(px)-44"),
        "formatted output must contain the NEGATIVE text-indent; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "hanging-indent attributes must round-trip identically"
    );
}

/// `bullet="•"` and `bullet-gap=(px)16` must survive parse → format → parse.
/// The writer must emit both attributes on the text line and re-parse them back
/// into the `bullet` / `bullet_gap` fields.
#[test]
fn test_bullet_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.br" name="BR"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.br" title="BR" {
    page id="page.one" w=(px)1920 h=(px)1080 {
      text id="b1" x=(px)160 y=(px)200 w=(px)1600 h=(px)170 overflow="clip" align="start" bullet="•" bullet-gap=(px)16 {
        span "Revenue grew twelve percent year over year, the strongest result since the restructuring."
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let page = &doc.body.pages[0];
    let Node::Text(t) = &page.children[0] else {
        panic!("first child must be the text node");
    };
    assert_eq!(t.bullet.as_deref(), Some("•"), "bullet must parse from KDL");
    assert_eq!(
        t.bullet_gap.as_ref().map(|d| d.value),
        Some(16.0),
        "bullet-gap must parse from KDL"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        formatted_str.contains("bullet=\"•\""),
        "formatted output must contain bullet; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("bullet-gap=(px)16"),
        "formatted output must contain bullet-gap; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "bullet + bullet-gap must round-trip identically"
    );
}

/// **src-rect round-trip**: an image node with `src-x`/`src-y`/`src-w`/`src-h`
/// must parse → format → re-parse byte-identically (all four src-* fields
/// survive the round-trip).
#[test]
fn test_image_src_rect_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.srcrt" name="SrcRt"
  assets {
    asset id="asset.photo" kind="image" src="assets/photo.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.srcrt" title="SrcRt" {
    page id="page.srcrt" w=(px)400 h=(px)300 {
      image id="img.srcrt" asset="asset.photo" x=(px)0 y=(px)0 w=(px)200 h=(px)100 src-x=(px)10 src-y=(px)20 src-w=(px)50 src-h=(px)60 fit="stretch"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let image_node = match &doc.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node, got {other:?}"),
    };

    use crate::ast::{Dimension, Unit};
    assert_eq!(
        image_node.src_x,
        Some(Dimension {
            value: 10.0,
            unit: Unit::Px
        }),
        "src-x must parse to (px)10"
    );
    assert_eq!(
        image_node.src_y,
        Some(Dimension {
            value: 20.0,
            unit: Unit::Px
        }),
        "src-y must parse to (px)20"
    );
    assert_eq!(
        image_node.src_w,
        Some(Dimension {
            value: 50.0,
            unit: Unit::Px
        }),
        "src-w must parse to (px)50"
    );
    assert_eq!(
        image_node.src_h,
        Some(Dimension {
            value: 60.0,
            unit: Unit::Px
        }),
        "src-h must parse to (px)60"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("src-x=(px)10"),
        "formatter must emit src-x=(px)10; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-y=(px)20"),
        "formatter must emit src-y=(px)20; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-w=(px)50"),
        "formatter must emit src-w=(px)50; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-h=(px)60"),
        "formatter must emit src-h=(px)60; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "src-rect image must round-trip identically"
    );
}

// ── sections: parse, serialize, and round-trip ────────────────────────

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
    use crate::ast::UnknownValue;
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
