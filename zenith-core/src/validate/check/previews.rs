//! Validation for the top-level `previews` block.
//!
//! Checks performed:
//!
//! 1. **`preview.unknown_candidate`** (Advisory) — a preview entry's `candidate`
//!    page id is not present in the document's declared pages. Advisory because a
//!    preview recorded against an older doc state may reference a since-removed
//!    page.
//! 2. **`preview.invalid_critique_severity`** (Warning) — a critique entry's
//!    `severity` is not one of `"error"`, `"warning"`, or `"advisory"`.

use std::collections::BTreeSet;

use crate::ast::document::Document;
use crate::diagnostics::Diagnostic;

/// Validate the `previews` block of `doc`.
///
/// `page_ids` — the set of all declared page ids in the document body.
/// Built once by the driver before calling this function; reused here.
pub(in crate::validate::check) fn check_previews(
    doc: &Document,
    page_ids: &BTreeSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for preview in &doc.previews {
        // ── 1. Unknown candidate page id ──────────────────────────────────────
        // Advisory: a preview recorded against an older doc state may reference
        // a since-removed page.
        if !page_ids.contains(preview.candidate.as_str()) {
            diagnostics.push(Diagnostic::advisory(
                "preview.unknown_candidate",
                format!(
                    "preview: candidate page '{}' does not exist in this document; \
                     the preview may have been recorded against an older doc state",
                    preview.candidate
                ),
                preview.source_span,
                Some(preview.candidate.clone()),
            ));
        }

        // ── 2. Invalid critique severity ──────────────────────────────────────
        for critique in &preview.critiques {
            if critique.severity != "error"
                && critique.severity != "warning"
                && critique.severity != "advisory"
            {
                diagnostics.push(Diagnostic::warning(
                    "preview.invalid_critique_severity",
                    format!(
                        "preview for candidate '{}': critique has severity '{}'; \
                         expected \"error\", \"warning\", or \"advisory\"",
                        preview.candidate, critique.severity
                    ),
                    critique.source_span,
                    Some(preview.candidate.clone()),
                ));
            }
        }
    }
}
