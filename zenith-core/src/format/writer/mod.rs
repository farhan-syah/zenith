//! Hand-written deterministic serializer for the Zenith AST.
//!
//! Produces canonical `.zen` text from a [`Document`]. The output is
//! idempotent: `format(format(doc)) == format(doc)` for all valid documents.
//!
//! Rules:
//! - Two-space indentation per nesting level.
//! - Root `zenith` node at column 0.
//! - Child order under `zenith`: project, assets, libraries, tokens, styles, components, masters, sections, provenance, variants, recipes, actions, document.
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
//!   `assets`/`libraries`/`components`/`masters`/`sections` orchestration, and
//!   the shared low-level primitives;
//! - [`tokens`] writes the `tokens` block;
//! - [`styles`] writes the `styles` block;
//! - [`nodes`] writes the `document` body, pages, and every node kind.

use std::fmt::Write as _;

use crate::ast::{
    ActionDef, AssetBlock, AssetDecl, ComponentDef, Dimension, Document, LibraryDef, MasterDef,
    ObjectPosition, Project, PropertyValue, ProvenanceDef, RecipeDef, RecipeParam, SectionDef,
    UnknownProperty, UnknownValue, VariantDef,
};
use crate::error::FormatError;

mod nodes;
mod styles;
mod tokens;

#[cfg(test)]
mod tests;

use nodes::{write_component_children, write_document_body};
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

/// Serialize an [`UnknownProperty`]'s value, including its KDL type annotation
/// when present, so that an annotated value round-trips byte-identically.
///
/// The annotation is emitted as a `(ty)` prefix in the value position, matching
/// KDL v2 syntax `name=(type)value`:
///
/// - annotated → `(px)10`, `(token)"color.navy"`
/// - unannotated → identical to [`fmt_unknown_value`]
pub(super) fn fmt_unknown_property(p: &UnknownProperty) -> String {
    match &p.ty {
        Some(ty) => format!("({}){}", ty, fmt_unknown_value(&p.value)),
        None => fmt_unknown_value(&p.value),
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
    d.to_kdl_string()
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

/// Emit `key="string"` for an optional string (quoted, no escaping).
pub(super) fn write_opt_str(out: &mut String, key: &str, opt: &Option<String>) {
    if let Some(s) = opt {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(s);
        out.push('"');
    }
}

/// Emit `key="string"` for an optional string, running the value through
/// [`escape_kdl_string`] so that backslashes, quotes, and whitespace control
/// characters survive as a single-line KDL string.
pub(super) fn write_opt_str_escaped(out: &mut String, key: &str, opt: &Option<String>) {
    if let Some(s) = opt {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(&escape_kdl_string(s));
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
    // Optional export color space attribute, emitted right after version so the
    // canonical form round-trips (parse → format → parse is byte-stable).
    write_opt_str(out, "colorspace", &doc.colorspace);
    // Optional stable document identity (ULID, Crockford base-32). Value is
    // always safe to emit without escaping (no special characters). Emitted
    // right after colorspace — grouped with version/colorspace as identity metadata.
    write_opt_str(out, "doc-id", &doc.doc_id);
    write_opt_bool(out, "mirror-margins", &doc.mirror_margins);
    // Facing-pages and spread-gutter are emitted right after mirror-margins (the
    // spread-layout metadata group). Both are omitted when None so a document
    // without these attrs round-trips byte-identically.
    write_opt_bool(out, "facing-pages", &doc.facing_pages);
    write_opt_dimension(out, "spread-gutter", &doc.spread_gutter);
    // Document-level default margins, grouped right after `mirror-margins` (the
    // other margin doc attr). Canonical order: inner, outer, top, bottom — same
    // order and spelling as on a page. Emitted only when set, so a document with
    // no defaults round-trips byte-identically.
    write_opt_dimension(out, "margin-inner", &doc.margin_inner);
    write_opt_dimension(out, "margin-outer", &doc.margin_outer);
    write_opt_dimension(out, "margin-top", &doc.margin_top);
    write_opt_dimension(out, "margin-bottom", &doc.margin_bottom);
    write_opt_str(out, "page-progression", &doc.page_progression);
    write_opt_str(out, "page-parity-start", &doc.page_parity_start);
    out.push_str(" {\n");

    // Child order: project, assets, libraries, tokens, styles, components, masters, sections, provenance, variants, recipes, actions, document.
    if let Some(proj) = &doc.project {
        write_project(proj, out, 1);
    }
    write_asset_block(&doc.assets, out, 1);
    write_library_block(&doc.libraries, out, 1);
    write_token_block(&doc.tokens, out, 1);
    write_style_block(&doc.styles, out, 1);
    write_component_block(&doc.components, out, 1);
    write_master_block(&doc.masters, out, 1);
    write_section_block(&doc.sections, out, 1);
    write_provenance_block(&doc.provenance, out, 1);
    write_variants_block(&doc.variants, out, 1);
    write_recipes_block(&doc.recipes, out, 1);
    write_action_block(&doc.actions, out, 1);
    write_document_body(&doc.body, out, 1);

    out.push('}');
}

// ---------------------------------------------------------------------------
// Masters
// ---------------------------------------------------------------------------

/// Emit the `masters { … }` block.
///
/// Stable position: after `components`, before `document`. Emitted ONLY when at
/// least one master is declared, so documents without masters keep their
/// existing canonical form (and round-trip) unchanged. Each master emits
/// `master id="…" { <child nodes> }`. Mirrors [`write_component_block`].
fn write_master_block(masters: &[MasterDef], out: &mut String, depth: usize) {
    if masters.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("masters {\n");
    for def in masters {
        indent(out, depth + 1);
        out.push_str("master id=\"");
        out.push_str(&def.id);
        out.push_str("\" {\n");
        write_component_children(&def.children, out, depth + 1);
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// Emit the `sections { … }` block.
///
/// Stable position: after `masters`, before `document`. Emitted ONLY when at
/// least one section is declared, so documents without sections keep their
/// existing canonical form (and round-trip) unchanged. Each section emits a
/// single leaf line: `section id="…" name="…" folio-start=N folio-style="…"
/// start-page="…"`. Optional attributes are omitted when `None`. Mirrors
/// [`write_master_block`].
fn write_section_block(sections: &[SectionDef], out: &mut String, depth: usize) {
    if sections.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("sections {\n");
    for def in sections {
        indent(out, depth + 1);
        out.push_str("section id=\"");
        out.push_str(&def.id);
        out.push_str("\" name=\"");
        out.push_str(&escape_kdl_string(&def.name));
        out.push('"');
        if let Some(fs) = def.folio_start {
            out.push_str(" folio-start=");
            // Writing to a String via fmt::Write is infallible; the Err variant
            // is unreachable but we must handle it.
            let _ = write!(out, "{fs}");
        }
        write_opt_str(out, "folio-style", &def.folio_style);
        out.push_str(" start-page=\"");
        out.push_str(&def.start_page);
        out.push_str("\"\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Emit the `components { … }` block.
///
/// Stable position: after `styles`, before `document`. The block is emitted ONLY
/// when at least one component is declared, so documents without components keep
/// their existing canonical form (and round-trip) unchanged. Each component emits
/// `component id="…" { <child nodes> }`.
fn write_component_block(components: &[ComponentDef], out: &mut String, depth: usize) {
    if components.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("components {\n");
    for def in components {
        indent(out, depth + 1);
        out.push_str("component id=\"");
        out.push_str(&def.id);
        out.push_str("\" {\n");
        write_component_children(&def.children, out, depth + 1);
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
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

    // Canonical property order: id, kind, src, sha256, ai-* provenance fields
    // (in the order below), then unknown_props (sorted).
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

    // AI-generation provenance fields — all optional, emitted only when Some.
    // Free-form string fields pass through escape_kdl_string so quotes and
    // newlines (common in prompts) survive as single-line KDL strings.
    write_opt_str_escaped(out, "ai-prompt", &decl.ai_prompt);
    write_opt_str_escaped(out, "ai-model", &decl.ai_model);
    write_opt_str_escaped(out, "ai-provider", &decl.ai_provider);
    if let Some(seed) = decl.ai_seed {
        out.push_str(" ai-seed=");
        let _ = write!(out, "{seed}");
    }
    write_opt_str_escaped(out, "ai-generation-date", &decl.ai_generation_date);
    write_opt_str_escaped(out, "ai-license", &decl.ai_license);
    write_opt_str_escaped(out, "ai-source-rights", &decl.ai_source_rights);
    write_opt_str_escaped(out, "ai-safety-status", &decl.ai_safety_status);
    write_opt_str_escaped(out, "ai-reuse-policy", &decl.ai_reuse_policy);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &decl.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

/// Emit the `libraries { … }` block.
///
/// Stable position: after `assets`, before `tokens`. Emitted ONLY when at least
/// one library is declared, so documents without imported packages keep their
/// existing canonical form (and round-trip) unchanged. Each library emits a
/// single leaf line: `library id="…" version="…" hash="…"`, with optional
/// attributes omitted when `None`, then any unknown props in BTreeMap key order.
/// Mirrors [`write_section_block`].
fn write_library_block(libraries: &[LibraryDef], out: &mut String, depth: usize) {
    if libraries.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("libraries {\n");
    for def in libraries {
        indent(out, depth + 1);
        out.push_str("library id=\"");
        out.push_str(&def.id);
        out.push('"');
        if let Some(version) = &def.version {
            out.push_str(" version=\"");
            out.push_str(version);
            out.push('"');
        }
        if let Some(hash) = &def.hash {
            out.push_str(" hash=\"");
            out.push_str(hash);
            out.push('"');
        }
        // Unknown properties in sorted key order (BTreeMap iteration is sorted).
        for (key, prop) in &def.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push('\n');
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Emit the `provenance { … }` block.
///
/// Stable position: after `sections`, before `document`. Emitted ONLY when at
/// least one origin record is declared, so documents without provenance keep
/// their existing canonical form (and round-trip) unchanged. Each record emits a
/// single leaf line: `origin id="…" node="…" library="…"`, then optional
/// `item="…"` and `linked=#true`/`#false` when set, then any unknown props in
/// BTreeMap key order. Mirrors [`write_library_block`].
fn write_provenance_block(provenance: &[ProvenanceDef], out: &mut String, depth: usize) {
    if provenance.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("provenance {\n");
    for def in provenance {
        indent(out, depth + 1);
        out.push_str("origin id=\"");
        out.push_str(&def.id);
        out.push_str("\" node=\"");
        out.push_str(&def.node);
        out.push_str("\" library=\"");
        out.push_str(&def.library);
        out.push('"');
        if let Some(item) = &def.item {
            out.push_str(" item=\"");
            out.push_str(item);
            out.push('"');
        }
        write_opt_bool(out, "linked", &def.linked);
        // Unknown properties in sorted key order (BTreeMap iteration is sorted).
        for (key, prop) in &def.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push('\n');
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

/// Emit the `variants { … }` block.
///
/// Stable position: after `provenance`, before `actions`. Emitted ONLY when at
/// least one variant is declared, so documents without variants keep their
/// existing canonical form (and round-trip) unchanged. Each variant emits:
///
/// ```text
/// variant id="…" source="…" w=(px)N h=(px)N {
///   override node="…" visible=#false text="…" fill=…
/// }
/// ```
///
/// Optional override props (`visible`, `text`, `fill`) are omitted when `None`.
/// Unknown props follow known ones in BTreeMap key order. Variants with no
/// overrides still emit a brace block (consistent with other block nodes).
/// Mirrors [`write_provenance_block`].
fn write_variants_block(variants: &[VariantDef], out: &mut String, depth: usize) {
    if variants.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("variants {\n");
    for def in variants {
        indent(out, depth + 1);
        out.push_str("variant id=\"");
        out.push_str(&def.id);
        out.push_str("\" source=\"");
        out.push_str(&def.source);
        out.push_str("\" w=");
        out.push_str(&fmt_dimension(&def.w));
        out.push_str(" h=");
        out.push_str(&fmt_dimension(&def.h));
        // Unknown props on the variant node itself, in sorted key order.
        for (key, prop) in &def.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push_str(" {\n");
        for ov in &def.overrides {
            indent(out, depth + 2);
            out.push_str("override node=\"");
            out.push_str(&ov.node);
            out.push('"');
            write_opt_bool(out, "visible", &ov.visible);
            if let Some(t) = &ov.text {
                out.push_str(" text=\"");
                out.push_str(&escape_kdl_string(t));
                out.push('"');
            }
            write_opt_property_value(out, "fill", &ov.fill);
            // Unknown props on the override node, in sorted key order.
            for (key, prop) in &ov.unknown_props {
                out.push(' ');
                out.push_str(key);
                out.push('=');
                out.push_str(&fmt_unknown_property(prop));
            }
            out.push('\n');
        }
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Recipes
// ---------------------------------------------------------------------------

/// Emit the `recipes { … }` block.
///
/// Stable position: after `variants`, before `actions`. Emitted ONLY when at
/// least one recipe is declared, so documents without recipes keep their
/// existing canonical form (and round-trip) unchanged. Each recipe emits:
///
/// ```text
/// recipe id="…" kind="…" seed=N generator="…" bounds="…" detached=#false {
///   param name="…" value=…
///   palette token="…"
///   expanded node="…"
/// }
/// ```
///
/// Optional props (`seed`, `generator`, `bounds`, `detached`) are omitted when
/// `None`. Unknown props follow known ones in BTreeMap key order. Free-form
/// string fields (`generator`, `bounds`) pass through the same `escape_kdl_string`
/// guard as `variants` uses for `text`. Mirrors [`write_variants_block`].
fn write_recipes_block(recipes: &[RecipeDef], out: &mut String, depth: usize) {
    if recipes.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("recipes {\n");
    for def in recipes {
        indent(out, depth + 1);
        out.push_str("recipe id=\"");
        out.push_str(&def.id);
        out.push_str("\" kind=\"");
        out.push_str(&escape_kdl_string(&def.kind));
        out.push('"');
        if let Some(seed) = def.seed {
            out.push_str(" seed=");
            let _ = write!(out, "{seed}");
        }
        if let Some(generator) = &def.generator {
            out.push_str(" generator=\"");
            out.push_str(&escape_kdl_string(generator));
            out.push('"');
        }
        if let Some(bounds) = &def.bounds {
            out.push_str(" bounds=\"");
            out.push_str(&escape_kdl_string(bounds));
            out.push('"');
        }
        write_opt_bool(out, "detached", &def.detached);
        // Unknown props on the recipe node itself, in sorted key order.
        for (key, prop) in &def.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push_str(" {\n");
        for param in &def.params {
            write_recipe_param(param, out, depth + 2);
        }
        for token_id in &def.palette {
            indent(out, depth + 2);
            out.push_str("palette token=\"");
            out.push_str(token_id);
            out.push_str("\"\n");
        }
        for node_id in &def.expanded {
            indent(out, depth + 2);
            out.push_str("expanded node=\"");
            out.push_str(node_id);
            out.push_str("\"\n");
        }
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_recipe_param(param: &RecipeParam, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("param name=\"");
    out.push_str(&param.name);
    out.push_str("\" value=");
    out.push_str(&fmt_property_value(&param.value));
    // Unknown props on the param node, in sorted key order.
    for (key, prop) in &param.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Emit the `actions { … }` block.
///
/// Stable position: after `provenance`, before `document`. Emitted ONLY when at
/// least one action is declared, so documents without actions keep their
/// existing canonical form (and round-trip) unchanged. Each action emits:
///
/// ```text
/// action id="…" label="…" version="…" {
///   tx "…"
/// }
/// ```
///
/// Optional attributes are omitted when `None`. Unknown props follow known
/// ones in BTreeMap key order. The `tx` payload is emitted as a single escaped
/// string child node (same encoding as `content` in a `code` node), so
/// characters that require escaping (`"`, `\`, `\n`, etc.) survive
/// round-trips. Mirrors [`write_provenance_block`].
fn write_action_block(actions: &[ActionDef], out: &mut String, depth: usize) {
    if actions.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("actions {\n");
    for def in actions {
        indent(out, depth + 1);
        out.push_str("action id=\"");
        out.push_str(&def.id);
        out.push('"');
        if let Some(label) = &def.label {
            out.push_str(" label=\"");
            out.push_str(&escape_kdl_string(label));
            out.push('"');
        }
        if let Some(version) = &def.version {
            out.push_str(" version=\"");
            out.push_str(version);
            out.push('"');
        }
        // Unknown properties in sorted key order (BTreeMap iteration is sorted).
        for (key, prop) in &def.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push_str(" {\n");
        // Emit the tx payload as a single escaped-string child node. This
        // mirrors how `code` nodes emit their `content` child: the JSON is
        // stored decoded and re-encoded here so quotes and backslashes survive
        // round-trips.
        indent(out, depth + 2);
        out.push_str("tx \"");
        out.push_str(&escape_kdl_string(&def.tx_json));
        out.push_str("\"\n");
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}
