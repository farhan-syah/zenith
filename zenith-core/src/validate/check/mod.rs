//! Document-level semantic validation checks.
//!
//! This module is split into cohesive submodules; `validate/mod.rs` re-exports
//! only the public surface (`validate`, `ValidationReport`).
//!
//! Checks performed (in one document walk):
//!
//! 1. **Global ID uniqueness** — every id across tokens, styles, body, pages,
//!    and nodes must be unique. Duplicates → `id.duplicate` (Error).
//! 2. **Required geometry** — `page` requires non-`Unit::Unknown` `width`/
//!    `height`; `rect`/`text` require all four of `x`, `y`, `w`, `h` present
//!    and with known units. Missing → `node.missing_geometry` (Error);
//!    unknown unit → `node.invalid_geometry` (Error).
//! 3. **Token-reference integrity + type compatibility** — visual `TokenRef`
//!    properties that point at an unknown or wrong-type token →
//!    `token.unknown_reference` / `token.incompatible_property` (Error).
//! 4. **Raw visual literal** — a recognized visual property (fill, stroke,
//!    stroke-width, font-family, font-size, radius) whose value is a
//!    `Literal(...)` → `token.raw_visual_literal` (Error).
//! 5. **Unknown node kind** → `node.unknown_kind` (Warning).
//!    **Unknown property** → `node.unknown_property` (Warning).
//! 6. **Unused token** — a token defined but never referenced by any node
//!    visual property or style → `token.unused` (Advisory).
//!
//! Submodules:
//! - [`visual`] — visual-property token type/existence/raw-literal checks.
//! - [`nodes`] — the recursive node walk and geometry helpers.
//! - [`contrast`] — the WCAG 2.2 contrast advisory.
//! - [`safezone`] — safe-zone exclusion/required overlap advisories.
//! - [`fold`] — fold-line content-crossing advisories.

mod contrast;
mod fold;
mod nodes;
mod safezone;
mod visual;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};

use crate::ast::asset::{AssetDecl, AssetKind};
use crate::ast::document::Document;
use crate::ast::style::{Style, StyleBlock};
use crate::ast::value::{PropertyValue, Unit, dim_to_px};
use crate::color::parse_rgb;
use crate::diagnostics::{Diagnostic, Severity};
use crate::tokens::{ResolvedToken, ResolvedValue};

use contrast::check_text_contrast;
use nodes::walk_node;
use visual::{VisualExpect, check_visual_prop};

// ── Public surface ────────────────────────────────────────────────────────────

/// The outcome of a full document validation pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// All diagnostics collected during validation (token resolution +
    /// document-level checks). Never causes a hard panic; always complete.
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// Returns `true` if any diagnostic has [`Severity::Error`].
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Run the full document validation pass.
///
/// Internally runs `resolve_tokens` on `doc.tokens`, merges those diagnostics,
/// then walks the full document collecting all semantic diagnostics.
/// Never hard-fails; all findings are returned in the [`ValidationReport`].
pub fn validate(doc: &Document) -> ValidationReport {
    // ── Step 1: token resolution ──────────────────────────────────────────
    let token_resolution = crate::tokens::resolve_tokens(&doc.tokens);
    let resolved_tokens: &BTreeMap<String, ResolvedToken> = &token_resolution.resolved;

    let mut diagnostics: Vec<Diagnostic> = token_resolution.diagnostics;

    // ── Step 2: collect all IDs and gather referenced token ids ──────────
    // `seen_ids` accumulates every id encountered across the whole document.
    // When we encounter a duplicate we push `id.duplicate`.
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut referenced_token_ids: HashSet<String> = HashSet::new();

    // Declared asset ids, collected once so the node walk can validate that
    // every `image.asset` reference points at a declared `AssetDecl.id`.
    let declared_asset_ids: HashSet<String> =
        doc.assets.assets.iter().map(|d| d.id.clone()).collect();

    // Declared style ids, collected once so the node walk can validate that
    // every `style="..."` node attribute references a declared style.
    let declared_style_ids: HashSet<String> =
        doc.styles.styles.iter().map(|s| s.id.clone()).collect();

    // Style lookup by id, so the contrast check can resolve a text node's
    // style-inherited fill / font-size / font-weight. Ordered for determinism.
    let style_map: BTreeMap<&str, &Style> = doc
        .styles
        .styles
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    // ── Token IDs ─────────────────────────────────────────────────────────
    for token in &doc.tokens.tokens {
        register_id(&token.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Style IDs ─────────────────────────────────────────────────────────
    for style in &doc.styles.styles {
        register_id(&style.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Style property validation ─────────────────────────────────────────
    validate_style_block(
        &doc.styles,
        resolved_tokens,
        &mut referenced_token_ids,
        &mut diagnostics,
    );

    // ── Asset IDs and per-declaration checks ──────────────────────────────
    for decl in &doc.assets.assets {
        register_id(&decl.id, &mut seen_ids, &mut diagnostics);
        validate_asset_decl(decl, &mut diagnostics);
    }

    // ── Document body id ──────────────────────────────────────────────────
    register_id(&doc.body.id, &mut seen_ids, &mut diagnostics);

    // ── Pages and their children ──────────────────────────────────────────
    for page in &doc.body.pages {
        register_id(&page.id, &mut seen_ids, &mut diagnostics);

        // ── Check page geometry (unit must be known) ──────────────────────
        if matches!(page.width.unit, Unit::Unknown(_)) {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "page '{}': property 'width' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }
        if matches!(page.height.unit, Unit::Unknown(_)) {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "page '{}': property 'height' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }

        // ── Page background token: validate type/existence and record the
        //    reference so it is not falsely reported as an unused token.
        check_visual_prop(
            &page.id,
            "background",
            page.background.as_ref(),
            VisualExpect::ColorOrGradient,
            &mut referenced_token_ids,
            resolved_tokens,
            &mut diagnostics,
        );

        // ── Resolve page dimensions to px for off_canvas checks ──────────
        // If either dimension is unresolvable (e.g. Pct/Deg unit — already
        // diagnosed above as node.invalid_geometry), skip off_canvas checks
        // for this page to avoid spurious noise.
        let page_px_bounds = dim_to_px(page.width.value, &page.width.unit)
            .zip(dim_to_px(page.height.value, &page.height.unit));

        // ── Resolve page background color for contrast checks ────────────
        // Only a TokenRef → Color token produces a usable RGB triple.
        // If the page has no background or the token is unresolvable, we
        // set None and silently skip contrast checks for this page — we
        // cannot determine what the background is without it.
        let page_bg_rgb: Option<(u8, u8, u8)> = page.background.as_ref().and_then(|pv| {
            if let PropertyValue::TokenRef(id) = pv {
                resolved_tokens.get(id.as_str()).and_then(|rt| {
                    if let ResolvedValue::Color(hex) = &rt.value {
                        parse_rgb(hex)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        // ── Walk page children ────────────────────────────────────────────
        // Page pixel bounds for backdrop bbox math; when the page unit was bad
        // (already diagnosed) bounds are unresolved and we use (0, 0) — no
        // shape will contain the text, so contrast falls back to the page bg.
        let (page_w, page_h) = page_px_bounds.unwrap_or((0.0, 0.0));
        for (i, node) in page.children.iter().enumerate() {
            walk_node(
                node,
                &mut seen_ids,
                &mut referenced_token_ids,
                resolved_tokens,
                &declared_asset_ids,
                &declared_style_ids,
                page_px_bounds,
                false,
                None,
                &mut diagnostics,
            );
            // Contrast check runs after the structural walk so that
            // token-reference errors are already diagnosed and we can
            // safely skip nodes whose tokens didn't resolve. The slice
            // `page.children[..i]` is the set of siblings painted UNDER this
            // node (lower z-order) — the candidate backdrops.
            check_text_contrast(
                node,
                page_bg_rgb,
                &page.children[..i],
                (page_w, page_h),
                resolved_tokens,
                &style_map,
                &mut diagnostics,
            );
        }

        // ── Safe-zone advisories ──────────────────────────────────────────
        // Only run when the page dimensions resolved; zone/node geometry is
        // compared in the same pixel space the off_canvas check uses.
        if let Some((page_w, page_h)) = page_px_bounds {
            safezone::check_safe_zones(page, page_w, page_h, &mut diagnostics);
            fold::check_folds(page, page_w, page_h, &mut diagnostics);
        }
    }

    // ── Step 3: unused token check ────────────────────────────────────────
    // Every token id that appears in `doc.tokens` but is not in
    // `referenced_token_ids` → advisory `token.unused`.
    for token in &doc.tokens.tokens {
        if !referenced_token_ids.contains(&token.id) {
            diagnostics.push(Diagnostic::advisory(
                "token.unused",
                format!(
                    "token '{}' is defined but never referenced by any node \
                     visual property or style in this document",
                    token.id
                ),
                token.source_span,
                Some(token.id.clone()),
            ));
        }
    }

    ValidationReport { diagnostics }
}

// ── Tiny helpers ──────────────────────────────────────────────────────────────

/// Register a single id; push `id.duplicate` if already seen.
///
/// Used for tokens, styles, body, pages, and all node kinds — any id-bearing
/// element in the document participates in the same global uniqueness check.
pub(super) fn register_id(id: &str, seen: &mut HashSet<String>, diagnostics: &mut Vec<Diagnostic>) {
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
fn validate_asset_decl(decl: &AssetDecl, diagnostics: &mut Vec<Diagnostic>) {
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

// ── Style helpers ─────────────────────────────────────────────────────────────

/// Validate the contents of the `styles` block:
/// - Each `(key, value)` in `Style.properties` is type-checked against the
///   expected token category and tracked as a token reference.
/// - Each entry in `Style.unknown_props` produces a `style.unknown_property`
///   Warning.
fn validate_style_block(
    block: &StyleBlock,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    referenced_token_ids: &mut HashSet<String>,
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
