//! Hand-written deterministic serializer for the Zenith AST.
//!
//! Produces canonical `.zen` text from a [`Document`]. The output is
//! idempotent: `format(format(doc)) == format(doc)` for all valid documents.
//!
//! Rules (from doc 08 and doc 16):
//! - Two-space indentation per nesting level.
//! - Root `zenith` node at column 0.
//! - Child order under `zenith`: project, tokens, styles, document.
//! - Structural containers (`tokens`, `styles`, `document`, `page`) always emit
//!   a brace block, even when empty.
//! - Leaf nodes (`project`, a `rect` with no children) emit a single line.
//! - `text` emits a brace block containing `span` children.
//! - Numbers: integral `f64` values emit without a decimal point (`640`, not
//!   `640.0`); non-integral emit the shortest representation.
//! - Booleans: `#true` / `#false` (KDL v2 form).
//! - Token refs: `fill=(token)"color.bg"`. String values: `name="One"`.
//! - Dimensions: `x=(px)0`.
//! - Unknown properties emit after known ones, in BTreeMap (sorted) key order.
//! - File ends with a single trailing newline.

use std::fmt::Write as _;

use crate::ast::{
    Dimension, Document, DocumentBody, Node, Page, Project, PropertyValue, RectNode, TextNode,
    TextSpan, Token, TokenBlock, TokenLiteral, TokenType, TokenValue, Unit, UnknownValue,
};
use crate::error::FormatError;

// ---------------------------------------------------------------------------
// Unknown property value formatting
// ---------------------------------------------------------------------------

/// Produce a KDL-valid serialization for an `UnknownValue`, preserving the
/// original KDL type so that parse→format→parse is a perfect round-trip:
///
/// - `String(s)` → a double-quoted, escaped KDL string (`"hello"`)
/// - `Integer(n)` → a bare decimal integer (`42`)
/// - `Float(f)` → a bare number via the canonical f64 formatter (integral
///   floats emit without `.0`: `1` not `1.0`)
/// - `Bool(b)` → KDL v2 boolean keyword (`#true` / `#false`)
/// - `Null` → KDL v2 null keyword (`#null`)
fn fmt_unknown_value(v: &UnknownValue) -> String {
    match v {
        UnknownValue::String(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    other => out.push(other),
                }
            }
            out.push('"');
            out
        }
        UnknownValue::Integer(n) => n.to_string(),
        UnknownValue::Float(f) => fmt_f64(*f),
        UnknownValue::Bool(b) => (if *b { "#true" } else { "#false" }).to_owned(),
        UnknownValue::Null => "#null".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Serialize `doc` to canonical `.zen` UTF-8 bytes.
pub fn format_document(doc: &Document) -> Result<Vec<u8>, FormatError> {
    let mut out = String::new();
    write_document(doc, &mut out);
    out.push('\n');
    Ok(out.into_bytes())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Append `count * 2` spaces of indentation.
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth * 2 {
        out.push(' ');
    }
}

/// Format a `f64` canonically: no trailing `.0` for integral values.
fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Format a dimension annotation + value, e.g. `(px)640` or `(pt)10.5`.
fn fmt_dimension(d: &Dimension) -> String {
    let ann = match &d.unit {
        Unit::Px => "px",
        Unit::Pt => "pt",
        Unit::Pct => "pct",
        Unit::Deg => "deg",
        Unit::Unknown(s) => s.as_str(),
    };
    format!("({ann}){}", fmt_f64(d.value))
}

/// Format a `PropertyValue` as a KDL value.
///
/// - `TokenRef("color.bg")`  →  `(token)"color.bg"`
/// - `Literal("center")`     →  `"center"`
fn fmt_property_value(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::TokenRef(id) => format!("(token)\"{id}\""),
        PropertyValue::Literal(s) => format!("\"{s}\""),
    }
}

/// Emit `key=value` for a `PropertyValue` property (if present).
fn write_opt_property_value(out: &mut String, key: &str, opt: &Option<PropertyValue>) {
    if let Some(pv) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_property_value(pv));
    }
}

/// Emit `key=(unit)N` for an optional `Dimension`.
fn write_opt_dimension(out: &mut String, key: &str, opt: &Option<Dimension>) {
    if let Some(d) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_dimension(d));
    }
}

/// Emit `key="string"` for an optional string (quoted).
fn write_opt_str(out: &mut String, key: &str, opt: &Option<String>) {
    if let Some(s) = opt {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(s);
        out.push('"');
    }
}

/// Emit `key=#true` or `key=#false` for an optional bool.
fn write_opt_bool(out: &mut String, key: &str, opt: &Option<bool>) {
    if let Some(b) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(if *b { "#true" } else { "#false" });
    }
}

/// Emit `key=N` for an optional `f64` (bare number, no unit).
fn write_opt_f64(out: &mut String, key: &str, opt: &Option<f64>) {
    if let Some(v) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_f64(*v));
    }
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

fn write_document(doc: &Document, out: &mut String) {
    // `zenith version=1 {`
    out.push_str("zenith version=");
    // Writing to a String via fmt::Write is infallible; the Err variant is
    // unreachable but we must handle it — discard rather than unwrap.
    let _ = write!(out, "{}", doc.version);
    out.push_str(" {\n");

    // Child order: project, tokens, styles, document.
    if let Some(proj) = &doc.project {
        write_project(proj, out, 1);
    }
    write_token_block(&doc.tokens, out, 1);
    write_style_block(&doc.styles.styles, out, 1);
    write_document_body(&doc.body, out, 1);

    out.push('}');
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

fn write_project(proj: &Project, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("project");
    // Canonical order: id, name
    out.push_str(" id=\"");
    out.push_str(&proj.id);
    out.push('"');
    out.push_str(" name=\"");
    out.push_str(&proj.name);
    out.push('"');
    // author: if present, emit as a block child
    if let Some(author) = &proj.author {
        out.push_str(" {\n");
        indent(out, depth + 1);
        out.push_str("author \"");
        out.push_str(author);
        out.push_str("\"\n");
        indent(out, depth);
        out.push_str("}\n");
    } else {
        out.push('\n');
    }
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

fn write_token_block(block: &TokenBlock, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("tokens format=\"");
    out.push_str(&block.format);
    out.push_str("\" {\n");

    for token in &block.tokens {
        write_token(token, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

fn write_token(token: &Token, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("token");
    // Canonical order: id, type, value
    out.push_str(" id=\"");
    out.push_str(&token.id);
    out.push('"');

    // type
    let type_str = match &token.token_type {
        TokenType::Color => "color",
        TokenType::Dimension => "dimension",
        TokenType::Number => "number",
        TokenType::FontFamily => "fontFamily",
        TokenType::FontWeight => "fontWeight",
        TokenType::Unknown(s) => s.as_str(),
    };
    out.push_str(" type=\"");
    out.push_str(type_str);
    out.push('"');

    // value
    out.push_str(" value=");
    match &token.value {
        TokenValue::Literal(lit) => match lit {
            TokenLiteral::String(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            TokenLiteral::Dimension(d) => {
                out.push_str(&fmt_dimension(d));
            }
            TokenLiteral::Number(n) => {
                out.push_str(&fmt_f64(*n));
            }
        },
        TokenValue::Reference { token_id } => {
            out.push_str("(token)\"");
            out.push_str(token_id);
            out.push('"');
        }
    }

    out.push('\n');
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

fn write_style_block(styles: &[crate::ast::Style], out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("styles {\n");

    for style in styles {
        indent(out, depth + 1);
        out.push_str("style id=\"");
        out.push_str(&style.id);
        out.push_str("\"\n");
    }

    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Document body
// ---------------------------------------------------------------------------

fn write_document_body(body: &DocumentBody, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("document");
    out.push_str(" id=\"");
    out.push_str(&body.id);
    out.push('"');
    write_opt_str(out, "title", &body.title);
    out.push_str(" {\n");

    for page in &body.pages {
        write_page(page, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

fn write_page(page: &Page, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("page");
    // Canonical order: id, name, w, h, background
    out.push_str(" id=\"");
    out.push_str(&page.id);
    out.push('"');
    write_opt_str(out, "name", &page.name);
    out.push_str(" w=");
    out.push_str(&fmt_dimension(&page.width));
    out.push_str(" h=");
    out.push_str(&fmt_dimension(&page.height));
    write_opt_property_value(out, "background", &page.background);

    out.push_str(" {\n");
    for child in &page.children {
        write_node(child, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

fn write_node(node: &Node, out: &mut String, depth: usize) {
    match node {
        Node::Rect(r) => write_rect(r, out, depth),
        Node::Text(t) => write_text(t, out, depth),
        Node::Unknown(u) => write_unknown_node(u, out, depth),
    }
}

fn write_rect(r: &RectNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("rect");

    // Canonical property order: id, name, role, x, y, w, h, radius, fill,
    // stroke, stroke-width, stroke-alignment, opacity, visible, locked, rotate, style
    out.push_str(" id=\"");
    out.push_str(&r.id);
    out.push('"');
    write_opt_str(out, "name", &r.name);
    write_opt_str(out, "role", &r.role);
    write_opt_dimension(out, "x", &r.x);
    write_opt_dimension(out, "y", &r.y);
    write_opt_dimension(out, "w", &r.w);
    write_opt_dimension(out, "h", &r.h);
    write_opt_property_value(out, "radius", &r.radius);
    write_opt_property_value(out, "fill", &r.fill);
    write_opt_property_value(out, "stroke", &r.stroke);
    write_opt_property_value(out, "stroke-width", &r.stroke_width);
    write_opt_str(out, "stroke-alignment", &r.stroke_alignment);
    write_opt_f64(out, "opacity", &r.opacity);
    write_opt_bool(out, "visible", &r.visible);
    write_opt_bool(out, "locked", &r.locked);
    write_opt_dimension(out, "rotate", &r.rotate);
    write_opt_str(out, "style", &r.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &r.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_text(t: &TextNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("text");

    // Canonical property order: id, name, role, x, y, w, h, align, direction,
    // overflow, fill, font-family, font-size, opacity, visible, locked, rotate, style
    out.push_str(" id=\"");
    out.push_str(&t.id);
    out.push('"');
    write_opt_str(out, "name", &t.name);
    write_opt_str(out, "role", &t.role);
    write_opt_dimension(out, "x", &t.x);
    write_opt_dimension(out, "y", &t.y);
    write_opt_dimension(out, "w", &t.w);
    write_opt_dimension(out, "h", &t.h);
    write_opt_str(out, "align", &t.align);
    write_opt_str(out, "direction", &t.direction);
    write_opt_str(out, "overflow", &t.overflow);
    write_opt_property_value(out, "fill", &t.fill);
    write_opt_property_value(out, "font-family", &t.font_family);
    write_opt_property_value(out, "font-size", &t.font_size);
    write_opt_f64(out, "opacity", &t.opacity);
    write_opt_bool(out, "visible", &t.visible);
    write_opt_bool(out, "locked", &t.locked);
    write_opt_dimension(out, "rotate", &t.rotate);
    write_opt_str(out, "style", &t.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &t.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    for span in &t.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_span(span: &TextSpan, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("span \"");
    // Escape backslashes and double-quotes inside span text.
    for ch in span.text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out.push('"');

    // Inline props: fill, font-weight, italic, underline, strikethrough.
    write_opt_property_value(out, "fill", &span.fill);
    write_opt_property_value(out, "font-weight", &span.font_weight);
    write_opt_bool(out, "italic", &span.italic);
    write_opt_bool(out, "underline", &span.underline);
    write_opt_bool(out, "strikethrough", &span.strikethrough);

    out.push('\n');
}

fn write_unknown_node(u: &crate::ast::UnknownNode, out: &mut String, depth: usize) {
    // Emit `<kind>` as a leaf (UnknownNode has no property map in current AST).
    indent(out, depth);
    out.push_str(&u.kind);
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Strip all source spans from a Document to enable span-agnostic equality.
    fn strip_spans(mut doc: crate::ast::Document) -> crate::ast::Document {
        use crate::ast::Node;
        for token in &mut doc.tokens.tokens {
            token.source_span = None;
        }
        for page in &mut doc.body.pages {
            page.source_span = None;
            for node in &mut page.children {
                match node {
                    Node::Rect(r) => r.source_span = None,
                    Node::Text(t) => t.source_span = None,
                    Node::Unknown(u) => u.source_span = None,
                }
            }
        }
        doc
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
}
