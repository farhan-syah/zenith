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
//! - [`margin`] — book live-area (mirrored-margin) violation advisories.

mod contrast;
mod fold;
mod margin;
mod nodes;
mod safezone;
mod visual;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::ast::asset::{AssetDecl, AssetKind};
use crate::ast::document::Document;
use crate::ast::library::LibraryDef;
use crate::ast::provenance::ProvenanceDef;
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

    // ── Document color space ──────────────────────────────────────────────
    // `colorspace` is informational export metadata; it does not affect PNG
    // output. Only "srgb" and "cmyk" are recognized; any other value is a
    // Warning (forward-compatible — never a hard error).
    if let Some(cs) = &doc.colorspace
        && cs != "srgb"
        && cs != "cmyk"
    {
        diagnostics.push(Diagnostic::warning(
            "document.invalid_colorspace",
            format!(
                "document colorspace '{}' is unrecognized; expected \"srgb\" or \
                 \"cmyk\" (this attribute is export metadata and does not change \
                 PNG output)",
                cs
            ),
            None,
            None,
        ));
    }

    // ── Document page-progression ─────────────────────────────────────────
    // `page_progression` is export metadata; it does not affect page render
    // order or PNG output. Only "ltr" and "rtl" are recognized; any other value
    // is a Warning (forward-compatible — never a hard error).
    if let Some(pp) = &doc.page_progression
        && pp != "ltr"
        && pp != "rtl"
    {
        diagnostics.push(Diagnostic::warning(
            "document.invalid_page_progression",
            format!(
                "document page-progression '{}' is unrecognized; expected \"ltr\" or \
                 \"rtl\" (this attribute is export metadata and does not change \
                 page order or PNG output)",
                pp
            ),
            None,
            None,
        ));
    }

    // ── Document page-parity-start ────────────────────────────────────────
    // `page_parity_start` selects whether page 1 is a recto (default) or a verso.
    // Only "recto" and "verso" (case-insensitive) are recognized; any other value
    // is a Warning (forward-compatible — never a hard error) and falls back to the
    // default parity.
    if let Some(pps) = &doc.page_parity_start
        && !pps.eq_ignore_ascii_case("recto")
        && !pps.eq_ignore_ascii_case("verso")
    {
        diagnostics.push(Diagnostic::warning(
            "document.invalid_page_parity_start",
            format!(
                "document page-parity-start '{}' is unrecognized; expected \"recto\" \
                 or \"verso\" (falling back to the default where page 1 is a recto)",
                pps
            ),
            None,
            None,
        ));
    }

    // ── Document spread-gutter ────────────────────────────────────────────
    // `spread_gutter` must resolve to a finite non-negative px value when
    // present. An unresolvable unit (pct/deg/unknown) or a negative value is a
    // Warning; the spread simply renders with no gutter. Never a hard error.
    if let Some(gutter) = &doc.spread_gutter {
        match dim_to_px(gutter.value, &gutter.unit) {
            None => {
                diagnostics.push(Diagnostic::warning(
                    "document.invalid_spread_gutter",
                    "document spread-gutter uses an unresolvable unit; \
                     allowed units are px and pt (spread renders with no gutter)",
                    None,
                    None,
                ));
            }
            Some(px) if px < 0.0 => {
                diagnostics.push(Diagnostic::warning(
                    "document.invalid_spread_gutter",
                    "document spread-gutter must be non-negative \
                     (spread renders with no gutter)",
                    None,
                    None,
                ));
            }
            Some(_) => {}
        }
    }

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

    // Declared component ids, collected once so the node walk can validate that
    // every `instance component="..."` references a declared component.
    let declared_component_ids: HashSet<String> =
        doc.components.iter().map(|c| c.id.clone()).collect();

    // Per-component LOCAL descendant id sets, used to validate that an override
    // `ref` targets a real descendant. Built once before the page walk. Ordered
    // for determinism. A component appears once; a duplicate component id is
    // diagnosed separately (id.duplicate) and the first wins in this map.
    let mut component_local_ids: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for comp in &doc.components {
        let mut local: HashSet<String> = HashSet::new();
        collect_local_ids(&comp.children, &mut local);
        component_local_ids.entry(comp.id.clone()).or_insert(local);
    }

    // Declared master ids, collected once so the page walk can validate that
    // every `page master="..."` references a declared master.
    let declared_master_ids: HashSet<String> = doc.masters.iter().map(|m| m.id.clone()).collect();

    // Declared library ids, collected once so each provenance `origin` record can
    // validate that its `library="..."` references a library declared in the
    // `libraries` block.
    let declared_library_ids: HashSet<String> =
        doc.libraries.iter().map(|l| l.id.clone()).collect();

    // Declared token ids, collected once so a provenance `node` target may also
    // reference a local TOKEN (a token imported from a library), not just a node.
    let declared_token_ids: HashSet<String> =
        doc.tokens.tokens.iter().map(|t| t.id.clone()).collect();

    // Document-wide set of every node id (across pages, masters, and components),
    // used to resolve a `page-ref` field's `target`. Ordered iteration is not
    // required (membership only); collected once before the walk.
    let mut all_node_ids: HashSet<String> = HashSet::new();
    for page in &doc.body.pages {
        collect_local_ids(&page.children, &mut all_node_ids);
    }
    for master in &doc.masters {
        collect_local_ids(&master.children, &mut all_node_ids);
    }
    for comp in &doc.components {
        collect_local_ids(&comp.children, &mut all_node_ids);
    }

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

    // ── Library IDs and per-declaration checks ────────────────────────────
    // Library ids share the global id namespace (like asset/token/style ids),
    // so duplicate library declarations and collisions are caught here.
    for decl in &doc.libraries {
        register_id(&decl.id, &mut seen_ids, &mut diagnostics);
        validate_library_decl(decl, &mut diagnostics);
    }

    // ── Component definitions ─────────────────────────────────────────────
    // The component id participates in the GLOBAL uniqueness set. Each
    // component's CHILD ids are validated for uniqueness within a LOCAL scope
    // (a fresh seen-id set per component) so the same local id may appear in
    // two different components without colliding. Token/asset/style refs inside
    // a component are validated ONCE here at the definition, by walking the
    // component's children exactly like page children (no page bounds → no
    // off_canvas/contrast checks, which are placement-relative).
    for comp in &doc.components {
        register_id(&comp.id, &mut seen_ids, &mut diagnostics);

        let mut local_seen: HashSet<String> = HashSet::new();
        for child in &comp.children {
            walk_node(
                child,
                &mut local_seen,
                &mut referenced_token_ids,
                resolved_tokens,
                &declared_asset_ids,
                &declared_style_ids,
                &declared_component_ids,
                &component_local_ids,
                &all_node_ids,
                None,
                false,
                None,
                &mut diagnostics,
            );
        }
    }

    // ── Master definitions ────────────────────────────────────────────────
    // Mirrors the component-definition validation: the master id participates
    // in the GLOBAL uniqueness set, and each master's CHILD ids are validated
    // for uniqueness within a LOCAL scope (a fresh seen-id set per master) so
    // the same local id may appear in two masters without colliding. Token/
    // asset/style refs and field types inside a master are validated ONCE here
    // at the definition by walking its children exactly like page children.
    for master in &doc.masters {
        register_id(&master.id, &mut seen_ids, &mut diagnostics);

        let mut local_seen: HashSet<String> = HashSet::new();
        for child in &master.children {
            walk_node(
                child,
                &mut local_seen,
                &mut referenced_token_ids,
                resolved_tokens,
                &declared_asset_ids,
                &declared_style_ids,
                &declared_component_ids,
                &component_local_ids,
                &all_node_ids,
                None,
                false,
                None,
                &mut diagnostics,
            );
        }
    }

    // ── Section definitions ───────────────────────────────────────────────
    // Collect the full set of page ids once (needed for start_page reference
    // checking). A BTreeSet gives deterministic iteration if we ever need it.
    let page_ids: BTreeSet<&str> = doc.body.pages.iter().map(|p| p.id.as_str()).collect();

    // Track start_page values seen so far: duplicate start_page on a second
    // section → `section.duplicate_start_page`.
    let mut seen_section_start_pages: BTreeSet<&str> = BTreeSet::new();

    for section in &doc.sections {
        // Section id participates in the GLOBAL id-uniqueness set so a section
        // id colliding with a page / token / master / component id → `id.duplicate`.
        register_id(&section.id, &mut seen_ids, &mut diagnostics);

        // `start_page` must name an existing page id → hard error if not.
        if !page_ids.contains(section.start_page.as_str()) {
            diagnostics.push(Diagnostic::error(
                "section.unknown_start_page",
                format!(
                    "section '{}': start-page '{}' does not reference a declared page",
                    section.id, section.start_page
                ),
                section.source_span,
                Some(section.id.clone()),
            ));
        }

        // No two sections may share the same start_page → hard error on second.
        if !seen_section_start_pages.insert(section.start_page.as_str()) {
            diagnostics.push(Diagnostic::error(
                "section.duplicate_start_page",
                format!(
                    "section '{}': start-page '{}' is already used by an earlier section",
                    section.id, section.start_page
                ),
                section.source_span,
                Some(section.id.clone()),
            ));
        }

        // `folio_style`, if present, must be one of the recognized styles →
        // Warning (forward-compat: an unknown style value is preserved verbatim
        // rather than rejected, so future styles don't break old validators).
        if let Some(style) = &section.folio_style
            && style != "decimal"
            && style != "lower-roman"
            && style != "upper-roman"
        {
            diagnostics.push(Diagnostic::warning(
                "section.invalid_folio_style",
                format!(
                    "section '{}': folio-style '{}' is unrecognized; \
                     expected \"decimal\", \"lower-roman\", or \"upper-roman\"",
                    section.id, style
                ),
                section.source_span,
                Some(section.id.clone()),
            ));
        }
    }

    // ── Provenance records ────────────────────────────────────────────────
    // Each `origin` id participates in the GLOBAL id-uniqueness set. The record
    // cross-references a target (a document node id OR a declared token id OR a
    // declared action id) AND a declared library id, all of which must exist
    // (`all_node_ids` is fully built above, before the page walk;
    // `declared_token_ids`/`declared_library_ids`/`declared_action_ids` are
    // collected alongside it).
    let declared_action_ids: HashSet<String> = doc.actions.iter().map(|a| a.id.clone()).collect();
    for prov in &doc.provenance {
        register_id(&prov.id, &mut seen_ids, &mut diagnostics);
        validate_provenance_def(
            prov,
            &all_node_ids,
            &declared_token_ids,
            &declared_action_ids,
            &declared_library_ids,
            &mut diagnostics,
        );
    }

    // ── Document body id ──────────────────────────────────────────────────
    register_id(&doc.body.id, &mut seen_ids, &mut diagnostics);

    // ── Pages and their children ──────────────────────────────────────────
    // The page index is 1-based (recto = odd, verso = even) and threaded into
    // the margin advisory so it can pick the parity-correct live area.
    let mirror_margins = doc.mirror_margins.unwrap_or(false);
    // RTL book: the binding is on the opposite side, mirroring the recto/verso
    // live-area parity (see `margin::check_margins`).
    let rtl_book = doc.page_progression.as_deref() == Some("rtl");
    // ── A document must contain at least one page ─────────────────────────
    // A zero-page document has no output target; this is a hard error.
    if doc.body.pages.is_empty() {
        diagnostics.push(Diagnostic::error(
            "document.no_pages",
            format!(
                "document '{}': a document must contain at least one page",
                doc.body.id
            ),
            None,
            Some(doc.body.id.clone()),
        ));
    }
    for (page_idx0, page) in doc.body.pages.iter().enumerate() {
        let page_index_1based = page_idx0 + 1;
        register_id(&page.id, &mut seen_ids, &mut diagnostics);

        // ── Per-page parity override validity ─────────────────────────────
        // `parity` forces this page's recto/verso. Only "recto"/"verso"
        // (case-insensitive) are recognized; any other value is a Warning
        // (forward-compatible — never a hard error) and falls back to the derived
        // parity (an invalid value resolves to recto, see `Document::page_is_recto`).
        if let Some(p) = &page.parity
            && !p.eq_ignore_ascii_case("recto")
            && !p.eq_ignore_ascii_case("verso")
        {
            diagnostics.push(Diagnostic::warning(
                "page.invalid_parity",
                format!(
                    "page '{}': parity '{}' is unrecognized; expected \"recto\" or \
                     \"verso\" (falling back to the derived page parity)",
                    page.id, p
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }

        // Single source of truth for this page's parity (drives the margin
        // advisory's binding side + recto/verso label).
        let is_recto = doc.page_is_recto(page, page_index_1based);

        // ── Master reference must resolve to a declared master ────────────
        if let Some(master_id) = &page.master
            && !declared_master_ids.contains(master_id)
        {
            diagnostics.push(Diagnostic::error(
                "master.unknown_reference",
                format!(
                    "page '{}': references master '{}' which is not declared in the \
                     masters block",
                    page.id, master_id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }

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

        // ── Page dimensions must be a strictly positive, finite length ────
        // A zero or negative width/height is a degenerate output target (an
        // empty canvas) and is rejected; `(px)0`, `(px)-100`, NaN, and ∞ all
        // fail here. The unit is validated separately above.
        if !page.width.value.is_finite() || page.width.value <= 0.0 {
            diagnostics.push(Diagnostic::error(
                "value.out_of_range",
                format!(
                    "page '{}': width must be a strictly positive length (got {})",
                    page.id, page.width.value
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }
        if !page.height.value.is_finite() || page.height.value <= 0.0 {
            diagnostics.push(Diagnostic::error(
                "value.out_of_range",
                format!(
                    "page '{}': height must be a strictly positive length (got {})",
                    page.id, page.height.value
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }

        // ── Bleed validation (never a hard error) ─────────────────────────
        // The bleed margin must resolve to pixels (px/pt) and be non-negative.
        // An unresolvable unit (pct/deg/unknown) or a negative value is a
        // Warning: the page still renders, bleed is simply ignored.
        if let Some(bleed) = &page.bleed {
            match dim_to_px(bleed.value, &bleed.unit) {
                None => {
                    diagnostics.push(Diagnostic::warning(
                        "page.invalid_bleed",
                        format!(
                            "page '{}': bleed uses an unresolvable unit; \
                             allowed units are px and pt (bleed is ignored)",
                            page.id
                        ),
                        page.source_span,
                        Some(page.id.clone()),
                    ));
                }
                Some(px) if px < 0.0 => {
                    diagnostics.push(Diagnostic::warning(
                        "page.invalid_bleed",
                        format!(
                            "page '{}': bleed must be non-negative (bleed is ignored)",
                            page.id
                        ),
                        page.source_span,
                        Some(page.id.clone()),
                    ));
                }
                Some(_) => {}
            }
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
                &declared_component_ids,
                &component_local_ids,
                &all_node_ids,
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

        // ── Footnote-ref resolution (structural) ──────────────────────────
        // Collect this page's footnote ids (direct children only — footnotes are
        // page-level furniture) and check every text span's `footnote-ref`
        // against that set. An unresolved ref → Warning `footnote.unresolved_ref`.
        check_footnote_refs(page, &mut diagnostics);

        // ── Safe-zone advisories ──────────────────────────────────────────
        // Only run when the page dimensions resolved; zone/node geometry is
        // compared in the same pixel space the off_canvas check uses.
        if let Some((page_w, page_h)) = page_px_bounds {
            safezone::check_safe_zones(page, page_w, page_h, &mut diagnostics);
            fold::check_folds(page, page_w, page_h, &mut diagnostics);
            margin::check_margins(
                doc,
                page,
                page_w,
                page_h,
                is_recto,
                mirror_margins,
                rtl_book,
                &mut diagnostics,
            );
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

/// Recursively collect the LOCAL ids of every id-bearing node in `children`
/// (descending into `group`/`frame`/`instance` containers) into `out`.
///
/// Used to build the per-component descendant-id set so an override `ref` can be
/// checked against the real local ids. Mirrors the container recursion used by
/// the node walk; `Instance` and `Unknown` ids are included where present.
pub(super) fn collect_local_ids(children: &[crate::ast::node::Node], out: &mut HashSet<String>) {
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
fn check_footnote_refs(page: &crate::ast::document::Page, diagnostics: &mut Vec<Diagnostic>) {
    use crate::ast::node::Node;

    // Page-local footnote ids (direct children only).
    let mut footnote_ids: HashSet<&str> = HashSet::new();
    for child in &page.children {
        if let Node::Footnote(fnote) = child {
            footnote_ids.insert(fnote.id.as_str());
        }
    }

    fn walk(
        children: &[crate::ast::node::Node],
        footnote_ids: &HashSet<&str>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        use crate::ast::node::Node;
        for child in children {
            match child {
                Node::Text(t) => {
                    for span in &t.spans {
                        if let Some(fref) = &span.footnote_ref
                            && !footnote_ids.contains(fref.as_str())
                        {
                            diagnostics.push(Diagnostic::warning(
                                "footnote.unresolved_ref",
                                format!(
                                    "text '{}': span footnote-ref '{}' matches no footnote \
                                     on this page",
                                    t.id, fref
                                ),
                                t.source_span,
                                Some(t.id.clone()),
                            ));
                        }
                    }
                }
                Node::Frame(f) => walk(&f.children, footnote_ids, diagnostics),
                Node::Group(g) => walk(&g.children, footnote_ids, diagnostics),
                Node::Table(t) => {
                    for row in &t.rows {
                        for cell in &row.cells {
                            walk(&cell.children, footnote_ids, diagnostics);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    walk(&page.children, &footnote_ids, diagnostics);
}

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

/// Validate a single [`LibraryDef`] beyond ID uniqueness:
/// - unknown properties → `library.unknown_property` (Warning)
///
/// `version`/`hash` are free-form strings in v0 (a lockfile/external tool owns
/// their format), so no format enforcement is performed here.
fn validate_library_decl(decl: &LibraryDef, diagnostics: &mut Vec<Diagnostic>) {
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
fn validate_provenance_def(
    prov: &ProvenanceDef,
    all_node_ids: &HashSet<String>,
    declared_token_ids: &HashSet<String>,
    declared_action_ids: &HashSet<String>,
    declared_library_ids: &HashSet<String>,
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
