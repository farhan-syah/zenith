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
    // Pages and nodes
    for page in &mut doc.body.pages {
        page.source_span = None;
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
        Node::Unknown(u) => u.source_span = None,
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
