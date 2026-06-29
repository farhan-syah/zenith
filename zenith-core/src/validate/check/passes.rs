//! Document-level validation passes and orchestration helpers.
//!
//! These are the cohesive helpers the [`validate`](super::driver::validate)
//! driver calls: id collection and registration, footnote-ref resolution, the
//! per-declaration checks for assets/libraries/provenance, and the styles
//! block. `register_id` is re-exported from the check module root because the
//! node submodules call it via `crate::validate::check::register_id`.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::asset::{AssetDecl, AssetKind};
use crate::ast::library::LibraryDef;
use crate::ast::provenance::ProvenanceDef;
use crate::ast::style::StyleBlock;
use crate::ast::value::PropertyValue;
use crate::diagnostics::Diagnostic;
use crate::tokens::ResolvedToken;

use super::visual::{VisualExpect, check_visual_prop};

/// Recursively collect the LOCAL ids of every id-bearing node in `children`
/// (descending into `group`/`frame`/`instance` containers) into `out`.
///
/// Used to build the per-component descendant-id set so an override `ref` can be
/// checked against the real local ids. Mirrors the container recursion used by
/// the node walk; `Instance` and `Unknown` ids are included where present.
pub(in crate::validate::check) fn collect_local_ids(
    children: &[crate::ast::node::Node],
    out: &mut BTreeSet<String>,
) {
    use crate::ast::node::Node;
    for child in children {
        match child {
            Node::Rect(n) => {
                out.insert(n.id.clone());
            }
            Node::Ellipse(n) => {
                out.insert(n.id.clone());
            }
            Node::Line(n) => {
                out.insert(n.id.clone());
            }
            Node::Text(n) => {
                out.insert(n.id.clone());
            }
            Node::Code(n) => {
                out.insert(n.id.clone());
            }
            Node::Image(n) => {
                out.insert(n.id.clone());
            }
            Node::Polygon(n) => {
                out.insert(n.id.clone());
            }
            Node::Polyline(n) => {
                out.insert(n.id.clone());
            }
            Node::Frame(n) => {
                out.insert(n.id.clone());
                collect_local_ids(&n.children, out);
            }
            Node::Group(n) => {
                out.insert(n.id.clone());
                collect_local_ids(&n.children, out);
            }
            Node::Instance(n) => {
                out.insert(n.id.clone());
            }
            Node::Field(n) => {
                out.insert(n.id.clone());
            }
            Node::Toc(n) => {
                out.insert(n.id.clone());
            }
            Node::Footnote(n) => {
                out.insert(n.id.clone());
            }
            Node::Table(n) => {
                out.insert(n.id.clone());
                for row in &n.rows {
                    for cell in &row.cells {
                        collect_local_ids(&cell.children, out);
                    }
                }
            }
            Node::Shape(n) => {
                out.insert(n.id.clone());
            }
            Node::Connector(n) => {
                out.insert(n.id.clone());
            }
            Node::Pattern(n) => {
                out.insert(n.id.clone());
            }
            Node::Chart(n) => {
                out.insert(n.id.clone());
            }
            Node::Light(n) => {
                out.insert(n.id.clone());
            }
            Node::Mesh(n) => {
                out.insert(n.id.clone());
            }
            Node::Unknown(_) => {}
        }
    }
}

/// Check every text span's `footnote-ref` on `page` against the page's set of
/// footnote ids (the ids of the `footnote` DIRECT children of the page).
///
/// A span whose `footnote-ref` names no footnote on this page → Warning
/// `footnote.unresolved_ref`. Footnotes are page-level furniture (only direct
/// page children count); spans are searched in every text node, descending into
/// `frame`/`group` containers in source order (deterministic).
pub(in crate::validate::check) fn check_footnote_refs(
    page: &crate::ast::document::Page,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use crate::ast::node::Node;

    // Page-local footnote ids (direct children only).
    let mut footnote_ids: BTreeSet<&str> = BTreeSet::new();
    for child in &page.children {
        if let Node::Footnote(fnote) = child {
            footnote_ids.insert(fnote.id.as_str());
        }
    }

    // Cross-check every `footnote_ref`-bearing span on a node (text labels and
    // shape labels both carry `Vec<TextSpan>`) against the page's footnote ids.
    fn check_spans(
        kind: &str,
        node_id: &str,
        spans: &[crate::ast::node::TextSpan],
        source_span: Option<crate::ast::Span>,
        footnote_ids: &BTreeSet<&str>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for span in spans {
            if let Some(fref) = &span.footnote_ref
                && !footnote_ids.contains(fref.as_str())
            {
                diagnostics.push(Diagnostic::warning(
                    "footnote.unresolved_ref",
                    format!(
                        "{kind} '{node_id}': span footnote-ref '{fref}' matches no footnote \
                         on this page"
                    ),
                    source_span,
                    Some(node_id.to_owned()),
                ));
            }
        }
    }

    fn walk(
        children: &[crate::ast::node::Node],
        footnote_ids: &BTreeSet<&str>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        use crate::ast::node::Node;
        for child in children {
            match child {
                Node::Text(t) => check_spans(
                    "text",
                    &t.id,
                    &t.spans,
                    t.source_span,
                    footnote_ids,
                    diagnostics,
                ),
                Node::Shape(s) => check_spans(
                    "shape",
                    &s.id,
                    &s.spans,
                    s.source_span,
                    footnote_ids,
                    diagnostics,
                ),
                Node::Frame(f) => walk(&f.children, footnote_ids, diagnostics),
                Node::Group(g) => walk(&g.children, footnote_ids, diagnostics),
                Node::Table(t) => {
                    for row in &t.rows {
                        for cell in &row.cells {
                            walk(&cell.children, footnote_ids, diagnostics);
                        }
                    }
                }
                Node::Rect(_)
                | Node::Ellipse(_)
                | Node::Line(_)
                | Node::Code(_)
                | Node::Image(_)
                | Node::Polygon(_)
                | Node::Polyline(_)
                | Node::Instance(_)
                | Node::Field(_)
                | Node::Toc(_)
                | Node::Footnote(_)
                | Node::Connector(_)
                | Node::Pattern(_)
                | Node::Chart(_)
                | Node::Light(_)
                | Node::Mesh(_)
                | Node::Unknown(_) => {}
            }
        }
    }

    walk(&page.children, &footnote_ids, diagnostics);
}

/// Register a single id; push `id.duplicate` if already seen.
///
/// Used for tokens, styles, body, pages, and all node kinds — any id-bearing
/// element in the document participates in the same global uniqueness check.
pub(in crate::validate::check) fn register_id(
    id: &str,
    seen: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !seen.insert(id.to_owned()) {
        diagnostics.push(Diagnostic::error(
            "id.duplicate",
            format!(
                "id '{}' is declared more than once; IDs must be globally unique",
                id
            ),
            None,
            Some(id.to_owned()),
        ));
    }
}

/// Validate a single [`AssetDecl`] beyond ID uniqueness:
/// - unknown kind → `asset.invalid_kind` (Error)
/// - unsafe src path → `asset.invalid_src` (Error)
/// - unknown properties → `asset.unknown_property` (Warning)
pub(in crate::validate::check) fn validate_asset_decl(
    decl: &AssetDecl,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── Kind check ────────────────────────────────────────────────────────
    if let AssetKind::Unknown(unknown_kind) = &decl.kind {
        diagnostics.push(Diagnostic::error(
            "asset.invalid_kind",
            format!(
                "asset '{}': unknown kind '{}'; \
                 recognized kinds are: image, svg, font",
                decl.id, unknown_kind
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }

    // ── Src sanity check ──────────────────────────────────────────────────
    // Reject: absolute paths (starts with `/` or Windows drive `X:\`),
    // parent-traversal segments (`..`), and URLs (contain `://`).
    let src = &decl.src;
    let is_absolute_unix = src.starts_with('/');
    // Windows drive: one ASCII letter followed by `:\` or `:/`
    let is_absolute_windows = src.len() >= 3
        && src.as_bytes()[0].is_ascii_alphabetic()
        && src.as_bytes()[1] == b':'
        && (src.as_bytes()[2] == b'\\' || src.as_bytes()[2] == b'/');
    let is_url = src.contains("://");
    // Parent traversal: segment `..` in any position.
    let has_traversal = src == ".."
        || src.starts_with("../")
        || src.starts_with("..\\")
        || src.contains("/../")
        || src.contains("/..\\")
        || src.contains("\\..\\")
        || src.contains("\\../")
        || src.ends_with("/..")
        || src.ends_with("\\..");

    if is_absolute_unix || is_absolute_windows || is_url || has_traversal {
        diagnostics.push(Diagnostic::error(
            "asset.invalid_src",
            format!(
                "asset '{}': src '{}' is not a safe relative path; \
                 absolute paths, parent-traversal segments ('..'), \
                 and URLs are not allowed",
                decl.id, src
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }

    // ── Unknown properties ────────────────────────────────────────────────
    for prop_name in decl.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "asset.unknown_property",
            format!(
                "asset '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                decl.id, prop_name
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }
}

/// Validate a single [`LibraryDef`] beyond ID uniqueness:
/// - unknown properties → `library.unknown_property` (Warning)
///
/// `version`/`hash` are free-form strings in v0 (a lockfile/external tool owns
/// their format), so no format enforcement is performed here.
pub(in crate::validate::check) fn validate_library_decl(
    decl: &LibraryDef,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for prop_name in decl.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "library.unknown_property",
            format!(
                "library '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                decl.id, prop_name
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }
}

/// Validate a single [`ProvenanceDef`] beyond ID uniqueness:
/// - `node` must reference an existing document node OR a declared token OR a
///   declared action → `provenance.unknown_node` (Error). A provenance record
///   links a LOCAL target (a node, a token imported from a library, or a
///   declared action) back to its origin, so declared token and action ids are
///   accepted targets. Mirrors `master.unknown_reference`.
/// - `library` must reference a library declared in the `libraries` block →
///   `provenance.unknown_library` (Error).
/// - unknown properties → `provenance.unknown_property` (Warning).
pub(in crate::validate::check) fn validate_provenance_def(
    prov: &ProvenanceDef,
    all_node_ids: &BTreeSet<String>,
    declared_token_ids: &BTreeSet<String>,
    declared_action_ids: &BTreeSet<String>,
    declared_library_ids: &BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !all_node_ids.contains(&prov.node)
        && !declared_token_ids.contains(&prov.node)
        && !declared_action_ids.contains(&prov.node)
    {
        diagnostics.push(Diagnostic::error(
            "provenance.unknown_node",
            format!(
                "provenance '{}': references node, token, or action '{}' which does not exist",
                prov.id, prov.node
            ),
            prov.source_span,
            Some(prov.id.clone()),
        ));
    }
    if !declared_library_ids.contains(&prov.library) {
        diagnostics.push(Diagnostic::error(
            "provenance.unknown_library",
            format!(
                "provenance '{}': references library '{}' which is not declared in the \
                 libraries block",
                prov.id, prov.library
            ),
            prov.source_span,
            Some(prov.id.clone()),
        ));
    }
    for prop_name in prov.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "provenance.unknown_property",
            format!(
                "provenance '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                prov.id, prop_name
            ),
            prov.source_span,
            Some(prov.id.clone()),
        ));
    }
}

// ── Style helpers ─────────────────────────────────────────────────────────────

/// Validate the contents of the `styles` block:
/// - Each `(key, value)` in `Style.properties` is type-checked against the
///   expected token category and tracked as a token reference.
/// - Each entry in `Style.unknown_props` produces a `style.unknown_property`
///   Warning.
pub(in crate::validate::check) fn validate_style_block(
    block: &StyleBlock,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    referenced_token_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for style in &block.styles {
        // Check recognized properties.
        for (key, value) in &style.properties {
            let expect = style_prop_expect(key);
            if let Some(expect) = expect {
                check_visual_prop(
                    &style.id,
                    key,
                    Some(value),
                    expect,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
            } else {
                // stroke-alignment and font-weight: no strict type check;
                // still track token refs so they count as used.
                if let PropertyValue::TokenRef(tid) = value {
                    referenced_token_ids.insert(tid.clone());
                }
            }
        }

        // Warn on unknown properties.
        for prop_name in style.unknown_props.keys() {
            diagnostics.push(Diagnostic::warning(
                "style.unknown_property",
                format!(
                    "style '{}': unknown property '{}' (not a recognized visual property; \
                     this property will not be applied to nodes that reference this style)",
                    style.id, prop_name
                ),
                style.source_span,
                Some(style.id.clone()),
            ));
        }
    }
}

/// Map a canonical style property key to its expected token type.
///
/// Returns `None` for keys that have no strict type expectation in v0
/// (`stroke-alignment`, `font-weight`).
fn style_prop_expect(key: &str) -> Option<VisualExpect> {
    match key {
        "fill" | "stroke" => Some(VisualExpect::Color),
        "stroke-width" | "font-size" | "line-height" | "radius" | "padding" | "gap" => {
            Some(VisualExpect::Dimension)
        }
        "font-family" => Some(VisualExpect::FontFamily),
        // stroke-alignment: plain enum string, not type-checked.
        // font-weight: fontWeight token type — no VisualExpect variant for it; skip check.
        _ => None,
    }
}
