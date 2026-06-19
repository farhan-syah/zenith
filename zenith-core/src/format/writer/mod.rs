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
//!
//! The implementation is split across focused submodules:
//! - this module root holds the public entry point, the `zenith`/`project`/
//!   `assets` orchestration, and the shared low-level primitives;
//! - [`tokens`] writes the `tokens` block;
//! - [`styles`] writes the `styles` block;
//! - [`nodes`] writes the `document` body, pages, and every node kind.

use std::fmt::Write as _;

use crate::ast::{
    AssetBlock, AssetDecl, Dimension, Document, ObjectPosition, Project, PropertyValue, Unit,
    UnknownValue,
};
use crate::error::FormatError;

mod nodes;
mod styles;
mod tokens;

#[cfg(test)]
mod tests;

use nodes::write_document_body;
use styles::write_style_block;
use tokens::write_token_block;

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
pub(super) fn fmt_unknown_value(v: &UnknownValue) -> String {
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
pub(super) fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth * 2 {
        out.push(' ');
    }
}

/// Format a `f64` canonically: no trailing `.0` for integral values.
pub(super) fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Format a dimension annotation + value, e.g. `(px)640` or `(pt)10.5`.
pub(super) fn fmt_dimension(d: &Dimension) -> String {
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
pub(super) fn fmt_property_value(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::TokenRef(id) => format!("(token)\"{id}\""),
        PropertyValue::Literal(s) => format!("\"{s}\""),
        PropertyValue::Dimension(d) => fmt_dimension(d),
    }
}

/// Emit `key=value` for a `PropertyValue` property (if present).
pub(super) fn write_opt_property_value(out: &mut String, key: &str, opt: &Option<PropertyValue>) {
    if let Some(pv) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_property_value(pv));
    }
}

/// Emit `key=(unit)N` for an optional `Dimension`.
pub(super) fn write_opt_dimension(out: &mut String, key: &str, opt: &Option<Dimension>) {
    if let Some(d) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_dimension(d));
    }
}

/// Emit `key="string"` for an optional string (quoted).
pub(super) fn write_opt_str(out: &mut String, key: &str, opt: &Option<String>) {
    if let Some(s) = opt {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(s);
        out.push('"');
    }
}

/// Emit `key=#true` or `key=#false` for an optional bool.
pub(super) fn write_opt_bool(out: &mut String, key: &str, opt: &Option<bool>) {
    if let Some(b) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(if *b { "#true" } else { "#false" });
    }
}

/// Emit `key="anchor"` (string) or `key=(pct)N` (annotated number) for an
/// optional [`ObjectPosition`].
pub(super) fn write_opt_object_position(out: &mut String, key: &str, opt: &Option<ObjectPosition>) {
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
pub(super) fn write_opt_f64(out: &mut String, key: &str, opt: &Option<f64>) {
    if let Some(v) = opt {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_f64(*v));
    }
}

/// Escape a string for emission as a single-line KDL v2 quoted string.
///
/// Unlike the inline span/unknown-prop escapers (which only handle `\` and `"`),
/// this also encodes the whitespace control characters `\n`, `\r`, and `\t` as
/// backslash escapes so that a multi-line `code` blob survives as ONE physical
/// line. All other characters pass through verbatim. This is the inverse of the
/// `kdl` crate's decode on parse, guaranteeing a byte-exact content round-trip.
pub(super) fn escape_kdl_string(s: &str) -> String {
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
