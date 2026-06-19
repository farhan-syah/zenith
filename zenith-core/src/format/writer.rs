//! Hand-written deterministic serializer for the Zenith AST.
//!
//! Produces canonical `.zen` text from a [`Document`]. The output is
//! idempotent: `format(format(doc)) == format(doc)` for all valid documents.
//!
//! Rules (from doc 08 and doc 16):
//! - Two-space indentation per nesting level.
//! - Root `zenith` node at column 0.
//! - Child order under `zenith`: project, assets, tokens, styles, document.
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
    AssetBlock, AssetDecl, CodeNode, Dimension, Document, DocumentBody, EllipseNode, FrameNode,
    GroupNode, ImageNode, LineNode, Node, ObjectPosition, Page, Point, PolygonNode, PolylineNode,
    Project, PropertyValue, RectNode, StyleBlock, TextNode, TextSpan, Token, TokenBlock,
    TokenLiteral, TokenType, TokenValue, Unit, UnknownValue,
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
            out.push_str(&escape_kdl_string(s));
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
/// - `Dimension((px)24)`     →  `(px)24`
fn fmt_property_value(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::TokenRef(id) => format!("(token)\"{id}\""),
        PropertyValue::Literal(s) => format!("\"{s}\""),
        PropertyValue::Dimension(d) => fmt_dimension(d),
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

/// Emit `key="anchor"` (string) or `key=(pct)N` (annotated number) for an
/// optional [`ObjectPosition`].
fn write_opt_object_position(out: &mut String, key: &str, opt: &Option<ObjectPosition>) {
    if let Some(pos) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        match pos {
            ObjectPosition::Start => out.push_str("\"start\""),
            ObjectPosition::Center => out.push_str("\"center\""),
            ObjectPosition::End => out.push_str("\"end\""),
            ObjectPosition::Pct(n) => {
                out.push_str("(pct)");
                out.push_str(&fmt_f64(*n));
            }
        }
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

    // Child order: project, assets, tokens, styles, document.
    if let Some(proj) = &doc.project {
        write_project(proj, out, 1);
    }
    write_asset_block(&doc.assets, out, 1);
    write_token_block(&doc.tokens, out, 1);
    write_style_block(&doc.styles, out, 1);
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
// Assets
// ---------------------------------------------------------------------------

/// Emit the `assets { … }` block.
///
/// Mirrors `write_token_block`: always emits the block (even when empty),
/// consistent with how `tokens` and `styles` always emit their brace blocks.
fn write_asset_block(block: &AssetBlock, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("assets {\n");

    for decl in &block.assets {
        write_asset_decl(decl, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

fn write_asset_decl(decl: &AssetDecl, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("asset");

    // Canonical property order: id, kind, src, sha256, then unknown_props (sorted).
    out.push_str(" id=\"");
    out.push_str(&decl.id);
    out.push('"');

    out.push_str(" kind=\"");
    out.push_str(decl.kind.kind_str());
    out.push('"');

    out.push_str(" src=\"");
    out.push_str(&decl.src);
    out.push('"');

    if let Some(sha256) = &decl.sha256 {
        out.push_str(" sha256=\"");
        out.push_str(sha256);
        out.push('"');
    }

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &decl.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
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

fn write_style_block(block: &StyleBlock, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("styles {\n");

    for style in &block.styles {
        let has_body = !style.properties.is_empty() || !style.unknown_props.is_empty();
        indent(out, depth + 1);
        out.push_str("style id=\"");
        out.push_str(&style.id);
        out.push('"');

        if has_body {
            out.push_str(" {\n");

            // Recognized properties in BTreeMap (sorted) key order — deterministic.
            for (key, value) in &style.properties {
                indent(out, depth + 2);
                out.push_str(key);
                out.push(' ');
                out.push_str(&fmt_property_value(value));
                out.push('\n');
            }

            // Unknown properties in sorted key order.
            for (key, prop) in &style.unknown_props {
                indent(out, depth + 2);
                out.push_str(key);
                out.push_str(" \"");
                out.push_str(&escape_kdl_string(&prop.raw));
                out.push_str("\"\n");
            }

            indent(out, depth + 1);
            out.push_str("}\n");
        } else {
            out.push('\n');
        }
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
    write_children_block(&page.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit each child node in source order at `depth + 1` indentation.
///
/// Used by `write_page`, `write_group`, and `write_frame` so the child-block
/// logic lives in exactly one place.
///
/// # Known limitation
/// Frames and groups nest recursively via `write_node` → `write_frame` /
/// `write_group` → `write_children_block` with no depth guard.  This is an
/// accepted v0 limitation; stack overflow is only possible with pathologically
/// deep trees.
fn write_children_block(children: &[Node], out: &mut String, depth: usize) {
    for child in children {
        write_node(child, out, depth + 1);
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

fn write_node(node: &Node, out: &mut String, depth: usize) {
    match node {
        Node::Rect(r) => write_rect(r, out, depth),
        Node::Ellipse(e) => write_ellipse(e, out, depth),
        Node::Line(l) => write_line(l, out, depth),
        Node::Text(t) => write_text(t, out, depth),
        Node::Code(c) => write_code(c, out, depth),
        Node::Frame(f) => write_frame(f, out, depth),
        Node::Group(g) => write_group(g, out, depth),
        Node::Image(i) => write_image(i, out, depth),
        Node::Polygon(p) => write_polygon(p, out, depth),
        Node::Polyline(p) => write_polyline(p, out, depth),
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

fn write_image(i: &ImageNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("image");

    // Canonical property order: id, name, role, asset, x, y, w, h, fit,
    // object-position-x, object-position-y, opacity, visible, locked, rotate,
    // style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&i.id);
    out.push('"');
    write_opt_str(out, "name", &i.name);
    write_opt_str(out, "role", &i.role);
    out.push_str(" asset=\"");
    out.push_str(&i.asset);
    out.push('"');
    write_opt_dimension(out, "x", &i.x);
    write_opt_dimension(out, "y", &i.y);
    write_opt_dimension(out, "w", &i.w);
    write_opt_dimension(out, "h", &i.h);
    write_opt_str(out, "fit", &i.fit);
    write_opt_object_position(out, "object-position-x", &i.object_position_x);
    write_opt_object_position(out, "object-position-y", &i.object_position_y);
    write_opt_f64(out, "opacity", &i.opacity);
    write_opt_bool(out, "visible", &i.visible);
    write_opt_bool(out, "locked", &i.locked);
    write_opt_dimension(out, "rotate", &i.rotate);
    write_opt_str(out, "style", &i.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &i.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_ellipse(e: &EllipseNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("ellipse");

    // Canonical property order: id, name, role, x, y, w, h, fill, stroke,
    // stroke-width, opacity, visible, locked, rotate, style.
    // NOTE: stroke-alignment is not supported for ellipse in v0 (centered only).
    out.push_str(" id=\"");
    out.push_str(&e.id);
    out.push('"');
    write_opt_str(out, "name", &e.name);
    write_opt_str(out, "role", &e.role);
    write_opt_dimension(out, "x", &e.x);
    write_opt_dimension(out, "y", &e.y);
    write_opt_dimension(out, "w", &e.w);
    write_opt_dimension(out, "h", &e.h);
    write_opt_property_value(out, "fill", &e.fill);
    write_opt_property_value(out, "stroke", &e.stroke);
    write_opt_property_value(out, "stroke-width", &e.stroke_width);
    write_opt_f64(out, "opacity", &e.opacity);
    write_opt_bool(out, "visible", &e.visible);
    write_opt_bool(out, "locked", &e.locked);
    write_opt_dimension(out, "rotate", &e.rotate);
    write_opt_str(out, "style", &e.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &e.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_line(l: &LineNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("line");

    // Canonical property order: id, name, role, x1, y1, x2, y2, stroke,
    // stroke-width, opacity, visible, locked, style, then unknown props.
    out.push_str(" id=\"");
    out.push_str(&l.id);
    out.push('"');
    write_opt_str(out, "name", &l.name);
    write_opt_str(out, "role", &l.role);
    write_opt_dimension(out, "x1", &l.x1);
    write_opt_dimension(out, "y1", &l.y1);
    write_opt_dimension(out, "x2", &l.x2);
    write_opt_dimension(out, "y2", &l.y2);
    write_opt_property_value(out, "stroke", &l.stroke);
    write_opt_property_value(out, "stroke-width", &l.stroke_width);
    write_opt_f64(out, "opacity", &l.opacity);
    write_opt_bool(out, "visible", &l.visible);
    write_opt_bool(out, "locked", &l.locked);
    write_opt_str(out, "style", &l.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &l.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_frame(f: &FrameNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("frame");

    // Canonical property order: id, name, role, x, y, w, h, layout, opacity,
    // visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
    write_opt_dimension(out, "x", &f.x);
    write_opt_dimension(out, "y", &f.y);
    write_opt_dimension(out, "w", &f.w);
    write_opt_dimension(out, "h", &f.h);
    write_opt_str(out, "layout", &f.layout);
    write_opt_f64(out, "opacity", &f.opacity);
    write_opt_bool(out, "visible", &f.visible);
    write_opt_bool(out, "locked", &f.locked);
    write_opt_dimension(out, "rotate", &f.rotate);
    write_opt_str(out, "style", &f.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &f.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    write_children_block(&f.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_group(g: &GroupNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("group");

    // Canonical property order: id, name, role, x, y, w, h, opacity,
    // visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&g.id);
    out.push('"');
    write_opt_str(out, "name", &g.name);
    write_opt_str(out, "role", &g.role);
    write_opt_dimension(out, "x", &g.x);
    write_opt_dimension(out, "y", &g.y);
    write_opt_dimension(out, "w", &g.w);
    write_opt_dimension(out, "h", &g.h);
    write_opt_f64(out, "opacity", &g.opacity);
    write_opt_bool(out, "visible", &g.visible);
    write_opt_bool(out, "locked", &g.locked);
    write_opt_dimension(out, "rotate", &g.rotate);
    write_opt_str(out, "style", &g.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &g.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    write_children_block(&g.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_text(t: &TextNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("text");

    // Canonical property order: id, name, role, x, y, w, h, align, direction,
    // overflow, fill, font-family, font-size, font-weight, opacity, visible,
    // locked, rotate, style
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
    write_opt_property_value(out, "font-weight", &t.font_weight);
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
    out.push_str(&escape_kdl_string(&span.text));
    out.push('"');

    // Inline props: fill, font-weight, italic, underline, strikethrough.
    write_opt_property_value(out, "fill", &span.fill);
    write_opt_property_value(out, "font-weight", &span.font_weight);
    write_opt_bool(out, "italic", &span.italic);
    write_opt_bool(out, "underline", &span.underline);
    write_opt_bool(out, "strikethrough", &span.strikethrough);

    out.push('\n');
}

/// Escape a string for emission as a single-line KDL v2 quoted string.
///
/// Unlike the inline span/unknown-prop escapers (which only handle `\` and `"`),
/// this also encodes the whitespace control characters `\n`, `\r`, and `\t` as
/// backslash escapes so that a multi-line `code` blob survives as ONE physical
/// line. All other characters pass through verbatim. This is the inverse of the
/// `kdl` crate's decode on parse, guaranteeing a byte-exact content round-trip.
fn escape_kdl_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn write_code(c: &CodeNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("code");

    // Canonical property order: id, name, role, x, y, w, h, overflow, language,
    // line-numbers, tab-width, style, fill, font-family, font-size, opacity,
    // visible, locked, rotate, then unknown props.
    out.push_str(" id=\"");
    out.push_str(&c.id);
    out.push('"');
    write_opt_str(out, "name", &c.name);
    write_opt_str(out, "role", &c.role);
    write_opt_dimension(out, "x", &c.x);
    write_opt_dimension(out, "y", &c.y);
    write_opt_dimension(out, "w", &c.w);
    write_opt_dimension(out, "h", &c.h);
    write_opt_str(out, "overflow", &c.overflow);
    write_opt_str(out, "language", &c.language);
    write_opt_bool(out, "line-numbers", &c.line_numbers);
    if let Some(tw) = c.tab_width {
        let _ = write!(out, " tab-width={tw}");
    }
    write_opt_str(out, "style", &c.style);
    write_opt_property_value(out, "fill", &c.fill);
    write_opt_property_value(out, "font-family", &c.font_family);
    write_opt_property_value(out, "font-size", &c.font_size);
    if let Some(t) = c.syntax_theme {
        let _ = write!(out, " syntax-theme=\"{}\"", t.as_str());
    }
    write_opt_f64(out, "opacity", &c.opacity);
    write_opt_bool(out, "visible", &c.visible);
    write_opt_bool(out, "locked", &c.locked);
    write_opt_dimension(out, "rotate", &c.rotate);

    // Unknown properties in sorted key order.
    for (key, prop) in &c.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // The verbatim source is emitted as a single escaped `content` child line.
    // It is NEVER re-indented/trimmed: the content is one escaped single-line
    // KDL string (KDL v2 multi-line dedent rules would otherwise mutate it).
    out.push_str(" {\n");
    indent(out, depth + 1);
    out.push_str("content \"");
    out.push_str(&escape_kdl_string(&c.content));
    out.push_str("\"\n");
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit a `point x=(unit)N y=(unit)N` line for each vertex in the list.
///
/// The block is always emitted (even for zero points) to maintain a consistent
/// brace-block style, mirroring how `write_text` always emits its `{ … }`.
fn write_points(points: &[Point], out: &mut String, depth: usize) {
    for pt in points {
        indent(out, depth);
        out.push_str("point");
        write_opt_dimension(out, "x", &pt.x);
        write_opt_dimension(out, "y", &pt.y);
        out.push('\n');
    }
}

fn write_polygon(p: &PolygonNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("polygon");

    // Canonical property order: id, name, role, fill, stroke, stroke-width,
    // stroke-alignment, fill-rule, opacity, visible, locked, rotate, style,
    // then unknown props, then the points block.
    out.push_str(" id=\"");
    out.push_str(&p.id);
    out.push('"');
    write_opt_str(out, "name", &p.name);
    write_opt_str(out, "role", &p.role);
    write_opt_property_value(out, "fill", &p.fill);
    write_opt_property_value(out, "stroke", &p.stroke);
    write_opt_property_value(out, "stroke-width", &p.stroke_width);
    // DEFERRED: stroke-alignment offset (rendered centered in v0)
    write_opt_str(out, "stroke-alignment", &p.stroke_alignment);
    write_opt_str(out, "fill-rule", &p.fill_rule);
    write_opt_f64(out, "opacity", &p.opacity);
    write_opt_bool(out, "visible", &p.visible);
    write_opt_bool(out, "locked", &p.locked);
    write_opt_dimension(out, "rotate", &p.rotate);
    write_opt_str(out, "style", &p.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &p.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // Points block: always emit braces (container style).
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_polyline(p: &PolylineNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("polyline");

    // Canonical property order: id, name, role, fill, stroke, stroke-width,
    // fill-rule, opacity, visible, locked, rotate, style,
    // then unknown props, then the points block.
    // NOTE: polyline has NO stroke-alignment.
    out.push_str(" id=\"");
    out.push_str(&p.id);
    out.push('"');
    write_opt_str(out, "name", &p.name);
    write_opt_str(out, "role", &p.role);
    write_opt_property_value(out, "fill", &p.fill);
    write_opt_property_value(out, "stroke", &p.stroke);
    write_opt_property_value(out, "stroke-width", &p.stroke_width);
    write_opt_str(out, "fill-rule", &p.fill_rule);
    write_opt_f64(out, "opacity", &p.opacity);
    write_opt_bool(out, "visible", &p.visible);
    write_opt_bool(out, "locked", &p.locked);
    write_opt_dimension(out, "rotate", &p.rotate);
    write_opt_str(out, "style", &p.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &p.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // Points block.
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
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
}
