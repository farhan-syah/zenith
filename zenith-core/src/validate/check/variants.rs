//! Validation for the top-level `variants` block.
//!
//! Checks performed (all `Error` severity):
//!
//! 1. **`variant.duplicate_id`** — two `variant` entries share the same `id`.
//!    Variant ids live in their own namespace (they are not document node ids)
//!    so a dedicated check is used rather than the global `register_id` funnel.
//! 2. **`variant.unknown_source`** — `variant.source` names a page id that does
//!    not exist in the document.
//! 3. **`variant.invalid_dimension`** — `variant.w` or `variant.h` is not
//!    px-convertible (`dim_to_px` returns `None`) OR resolves to `<= 0.0`.
//! 4. **`variant.override_unknown_node`** — an `override.node` names a node id
//!    absent from the variant's source page. Suppressed when the source page
//!    itself failed to resolve (diagnostic #2) to avoid cascading noise.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::document::Document;
use crate::ast::value::dim_to_px;
use crate::diagnostics::Diagnostic;

/// Validate the `variants` block of `doc`.
///
/// `page_ids` — the set of page ids present in the document body.
///
/// `page_node_ids` — a map from page id to the set of all node ids
/// (descendants at any depth) within that page. Built once by the driver
/// before calling this function; both collections use `BTreeSet`/`BTreeMap`
/// for determinism.
pub(in crate::validate::check) fn check_variants(
    doc: &Document,
    page_ids: &BTreeSet<&str>,
    page_node_ids: &BTreeMap<&str, BTreeSet<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── 1. Duplicate variant id detection ────────────────────────────────────
    // Variant ids live in the variants namespace; they are NOT document node ids.
    // We track them in a local BTreeSet and emit `variant.duplicate_id` for each
    // duplicate (matching the pattern of `section.duplicate_start_page` and
    // `id.duplicate`).
    let mut seen_variant_ids: BTreeSet<&str> = BTreeSet::new();

    for variant in &doc.variants {
        if !seen_variant_ids.insert(variant.id.as_str()) {
            diagnostics.push(Diagnostic::error(
                "variant.duplicate_id",
                format!(
                    "variant '{}': id is declared more than once; \
                     variant ids must be unique within the variants block",
                    variant.id
                ),
                variant.source_span,
                Some(variant.id.clone()),
            ));
        }

        // ── 2. Unknown source page ────────────────────────────────────────────
        let source_known = page_ids.contains(variant.source.as_str());
        if !source_known {
            diagnostics.push(Diagnostic::error(
                "variant.unknown_source",
                format!(
                    "variant '{}': source page '{}' does not exist in this document",
                    variant.id, variant.source
                ),
                variant.source_span,
                Some(variant.id.clone()),
            ));
        }

        // ── 3. Invalid dimensions ─────────────────────────────────────────────
        match dim_to_px(variant.w.value, &variant.w.unit) {
            None => {
                diagnostics.push(Diagnostic::error(
                    "variant.invalid_dimension",
                    format!(
                        "variant '{}': width uses an unresolvable unit; \
                         allowed units are px and pt",
                        variant.id
                    ),
                    variant.source_span,
                    Some(variant.id.clone()),
                ));
            }
            Some(px) if px <= 0.0 => {
                diagnostics.push(Diagnostic::error(
                    "variant.invalid_dimension",
                    format!(
                        "variant '{}': width must be a strictly positive value (got {})",
                        variant.id, px
                    ),
                    variant.source_span,
                    Some(variant.id.clone()),
                ));
            }
            Some(_) => {}
        }

        match dim_to_px(variant.h.value, &variant.h.unit) {
            None => {
                diagnostics.push(Diagnostic::error(
                    "variant.invalid_dimension",
                    format!(
                        "variant '{}': height uses an unresolvable unit; \
                         allowed units are px and pt",
                        variant.id
                    ),
                    variant.source_span,
                    Some(variant.id.clone()),
                ));
            }
            Some(px) if px <= 0.0 => {
                diagnostics.push(Diagnostic::error(
                    "variant.invalid_dimension",
                    format!(
                        "variant '{}': height must be a strictly positive value (got {})",
                        variant.id, px
                    ),
                    variant.source_span,
                    Some(variant.id.clone()),
                ));
            }
            Some(_) => {}
        }

        // ── 4. Override node resolution ───────────────────────────────────────
        // Only checked when the source page resolved successfully (#2). If the
        // source is unknown we skip override checks to avoid diagnostic noise.
        if source_known {
            let node_ids = page_node_ids.get(variant.source.as_str());
            for ov in &variant.overrides {
                let resolved = node_ids.is_some_and(|ids| ids.contains(&ov.node));
                if !resolved {
                    diagnostics.push(Diagnostic::error(
                        "variant.override_unknown_node",
                        format!(
                            "variant '{}': override targets node '{}' which does not exist \
                             in source page '{}'",
                            variant.id, ov.node, variant.source
                        ),
                        ov.source_span,
                        Some(variant.id.clone()),
                    ));
                }
            }
        }
    }
}
