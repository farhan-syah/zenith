//! The document-level validation driver.
//!
//! Holds the [`validate`] entry point — the single document walk that runs
//! token resolution and every document/page-level semantic check — together
//! with its orchestration helpers (id collection, footnote-ref resolution,
//! per-declaration checks for assets/libraries/provenance, and the styles
//! block). The check module root re-exports [`validate`] (and `register_id`,
//! which the node submodules call) as part of the public surface.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::document::Document;
use crate::ast::style::Style;
use crate::ast::value::{PropertyValue, Unit, dim_to_px};
use crate::color::parse_rgb;
use crate::diagnostics::Diagnostic;
use crate::tokens::{ResolvedToken, ResolvedValue};

use super::contrast::check_text_contrast;
use super::nodes::{WalkCtx, WalkPos, check_sibling_anchors, walk_node};
use super::passes::{
    check_footnote_refs, collect_local_ids, register_id, validate_asset_decl,
    validate_library_decl, validate_provenance_def, validate_style_block,
};
use super::policy::{apply_policy, check_policy_entries};
use super::recipes::check_recipes;
use super::report::ValidationReport;
use super::variants::check_variants;
use super::visual::{VisualExpect, check_visual_prop};
use super::{fold, margin, safezone};

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
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    let mut referenced_token_ids: BTreeSet<String> = BTreeSet::new();

    // Declared asset ids, collected once so the node walk can validate that
    // every `image.asset` reference points at a declared `AssetDecl.id`.
    let declared_asset_ids: BTreeSet<String> =
        doc.assets.assets.iter().map(|d| d.id.clone()).collect();

    // Declared style ids, collected once so the node walk can validate that
    // every `style="..."` node attribute references a declared style.
    let declared_style_ids: BTreeSet<String> =
        doc.styles.styles.iter().map(|s| s.id.clone()).collect();

    // Declared component ids, collected once so the node walk can validate that
    // every `instance component="..."` references a declared component.
    let declared_component_ids: BTreeSet<String> =
        doc.components.iter().map(|c| c.id.clone()).collect();

    // Per-component LOCAL descendant id sets, used to validate that an override
    // `ref` targets a real descendant. Built once before the page walk. Ordered
    // for determinism. A component appears once; a duplicate component id is
    // diagnosed separately (id.duplicate) and the first wins in this map.
    let mut component_local_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for comp in &doc.components {
        let mut local: BTreeSet<String> = BTreeSet::new();
        collect_local_ids(&comp.children, &mut local);
        component_local_ids.entry(comp.id.clone()).or_insert(local);
    }

    // Declared master ids, collected once so the page walk can validate that
    // every `page master="..."` references a declared master.
    let declared_master_ids: BTreeSet<String> = doc.masters.iter().map(|m| m.id.clone()).collect();

    // Declared library ids, collected once so each provenance `origin` record can
    // validate that its `library="..."` references a library declared in the
    // `libraries` block.
    let declared_library_ids: BTreeSet<String> =
        doc.libraries.iter().map(|l| l.id.clone()).collect();

    // Declared token ids, collected once so a provenance `node` target may also
    // reference a local TOKEN (a token imported from a library), not just a node.
    let declared_token_ids: BTreeSet<String> =
        doc.tokens.tokens.iter().map(|t| t.id.clone()).collect();

    // Token id → TokenType map, used by check_recipes to distinguish undeclared
    // tokens from declared-but-non-color tokens in the palette check.
    // BTreeMap for determinism; built once, shared with check_recipes.
    let token_type_map: BTreeMap<&str, &crate::ast::TokenType> = doc
        .tokens
        .tokens
        .iter()
        .map(|t| (t.id.as_str(), &t.token_type))
        .collect();

    // Document-wide set of every node id (across pages, masters, and components),
    // used to resolve a `page-ref` field's `target`. Ordered iteration is not
    // required (membership only); collected once before the walk.
    let mut all_node_ids: BTreeSet<String> = BTreeSet::new();
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

        let mut local_seen: BTreeSet<String> = BTreeSet::new();
        // Components are not page-children: no safe-zones apply.
        let no_zones: BTreeSet<&str> = BTreeSet::new();
        let ctx = WalkCtx {
            resolved_tokens,
            declared_asset_ids: &declared_asset_ids,
            declared_style_ids: &declared_style_ids,
            declared_component_ids: &declared_component_ids,
            component_local_ids: &component_local_ids,
            all_node_ids: &all_node_ids,
            zone_ids: &no_zones,
        };
        for child in &comp.children {
            walk_node(
                child,
                ctx,
                &mut local_seen,
                &mut referenced_token_ids,
                WalkPos {
                    page_px_bounds: None,
                    in_flow_parent: false,
                    enclosing_frame: None,
                    in_container: false,
                    parent_box_known: false,
                },
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

        let mut local_seen: BTreeSet<String> = BTreeSet::new();
        // Masters are not page-children: no safe-zones apply.
        let no_zones: BTreeSet<&str> = BTreeSet::new();
        let ctx = WalkCtx {
            resolved_tokens,
            declared_asset_ids: &declared_asset_ids,
            declared_style_ids: &declared_style_ids,
            declared_component_ids: &declared_component_ids,
            component_local_ids: &component_local_ids,
            all_node_ids: &all_node_ids,
            zone_ids: &no_zones,
        };
        for child in &master.children {
            walk_node(
                child,
                ctx,
                &mut local_seen,
                &mut referenced_token_ids,
                WalkPos {
                    page_px_bounds: None,
                    in_flow_parent: false,
                    enclosing_frame: None,
                    in_container: false,
                    parent_box_known: false,
                },
                &mut diagnostics,
            );
        }
    }

    // ── Section definitions ───────────────────────────────────────────────
    // Collect the full set of page ids once (needed for start_page reference
    // checking). A BTreeSet gives deterministic iteration if we ever need it.
    let page_ids: BTreeSet<&str> = doc.body.pages.iter().map(|p| p.id.as_str()).collect();

    // Per-page descendant node-id map, built once here and shared with the
    // variant check. Each entry maps a page id to the BTreeSet of all node ids
    // (at any depth) within that page. This avoids rebuilding the set once per
    // variant (which would be O(variants × pages × nodes)).
    let page_node_ids: BTreeMap<&str, BTreeSet<String>> = doc
        .body
        .pages
        .iter()
        .map(|p| {
            let mut ids: BTreeSet<String> = BTreeSet::new();
            collect_local_ids(&p.children, &mut ids);
            (p.id.as_str(), ids)
        })
        .collect();

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

    // ── Variants ──────────────────────────────────────────────────────────
    // Validate the top-level `variants` block: duplicate ids, unknown source
    // pages, invalid dimensions, and override-node resolution.
    check_variants(doc, &page_ids, &page_node_ids, &mut diagnostics);

    // ── Recipes ───────────────────────────────────────────────────────────
    // Validate the top-level `recipes` block: duplicate ids, unknown/non-color
    // palette tokens, unknown expanded node ids, and unknown bounds ids.
    check_recipes(
        doc,
        &page_ids,
        &all_node_ids,
        &token_type_map,
        &mut diagnostics,
    );

    // ── Provenance records ────────────────────────────────────────────────
    // Each `origin` id participates in the GLOBAL id-uniqueness set. The record
    // cross-references a target (a document node id OR a declared token id OR a
    // declared action id) AND a declared library id, all of which must exist
    // (`all_node_ids` is fully built above, before the page walk;
    // `declared_token_ids`/`declared_library_ids`/`declared_action_ids` are
    // collected alongside it).
    let declared_action_ids: BTreeSet<String> = doc.actions.iter().map(|a| a.id.clone()).collect();
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

        // ── Per-page line-jump style validity ─────────────────────────────
        // `line-jumps` selects how connector-vs-connector crossings hop. Only
        // "none"/"arc"/"gap" are recognized; any other value is a Warning
        // (forward-compatible — never a hard error) and renders as if absent
        // (no hops).
        if let Some(lj) = &page.line_jumps
            && lj != "none"
            && lj != "arc"
            && lj != "gap"
        {
            diagnostics.push(Diagnostic::warning(
                "page.invalid_line_jumps",
                format!(
                    "page '{}': line-jumps '{}' is not one of none/arc/gap",
                    page.id, lj
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

        // Build the set of safe-zone ids for this page so that check_anchor
        // can validate anchor-zone references.
        let zone_ids: BTreeSet<&str> = page.safe_zones.iter().map(|z| z.id.as_str()).collect();

        let ctx = WalkCtx {
            resolved_tokens,
            declared_asset_ids: &declared_asset_ids,
            declared_style_ids: &declared_style_ids,
            declared_component_ids: &declared_component_ids,
            component_local_ids: &component_local_ids,
            all_node_ids: &all_node_ids,
            zone_ids: &zone_ids,
        };

        // Validate the page-children sibling-anchor graph (the top-level scope)
        // once, before the per-node walk.
        check_sibling_anchors(&page.children, &mut diagnostics);

        for (i, node) in page.children.iter().enumerate() {
            walk_node(
                node,
                ctx,
                &mut seen_ids,
                &mut referenced_token_ids,
                WalkPos {
                    page_px_bounds,
                    in_flow_parent: false,
                    enclosing_frame: None,
                    in_container: false,
                    parent_box_known: false,
                },
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
                margin::PageMarginCtx {
                    page_w,
                    page_h,
                    is_recto,
                    mirror_margins,
                    rtl: rtl_book,
                },
                &mut diagnostics,
            );
        }
    }

    // A recipe `palette` entry is a token reference too (the generator recolors
    // through it), so count palette ids as usage — a token used only by a recipe
    // palette must not be flagged `token.unused`.
    for recipe in &doc.recipes {
        for token_id in &recipe.palette {
            referenced_token_ids.insert(token_id.clone());
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

    // ── Step 4: diagnostic policy ─────────────────────────────────────────
    // Apply the document's `diagnostics { … }` policy to the assembled list
    // FIRST (allow/deny/warn, with Error severity immutable), THEN append
    // self-validation diagnostics ABOUT the policy. The ordering matters: the
    // self-validation is appended after `apply_policy` so a policy can never
    // suppress the warnings that describe its own entries. With no policy block,
    // `apply_policy` is an exact identity pass and `check_policy_entries` adds
    // nothing — the default-off path is byte-identical.
    let mut diagnostics = apply_policy(diagnostics, &doc.diagnostic_policy);
    check_policy_entries(&doc.diagnostic_policy, &mut diagnostics);

    ValidationReport { diagnostics }
}
