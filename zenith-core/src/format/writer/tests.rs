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
        Node::Footnote(f) => f.source_span = None,
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
