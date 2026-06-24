//! Logic for `zenith workspace promote`.

use std::path::Path;

use zenith_core::{KdlAdapter, KdlSource, Severity, validate};
use zenith_session::adapter::OsFs;
use zenith_session::{CandidateStatus, StorePaths, get_scratch_snapshot, list_scratch};
use zenith_tx::merge_candidate_page;

use crate::commands::workspace::scratch::open_store;
use crate::history::{Recorded, record_edit_in};

/// Promote candidate `cand_id` from the session store into the page `into_page`
/// of the deliverable document at `doc_path`, and write the result back.
///
/// Returns a confirmation string on success, or a human-readable error.
pub fn promote(
    doc_path: &Path,
    cand_id: &str,
    into_page: &str,
    id_suffix: &str,
) -> Result<String, String> {
    let paths = open_store()?;
    promote_in(&paths, doc_path, cand_id, into_page, id_suffix)
}

/// Testable variant with an explicit store root.
pub fn promote_in(
    paths: &StorePaths,
    doc_path: &Path,
    cand_id: &str,
    into_page: &str,
    id_suffix: &str,
) -> Result<String, String> {
    let fs = OsFs;

    // 1. Read and parse the deliverable document; extract its doc-id.
    let main_bytes = std::fs::read(doc_path)
        .map_err(|e| format!("cannot read '{}': {e}", doc_path.display()))?;
    let mut main_doc = KdlAdapter
        .parse(main_bytes.as_slice())
        .map_err(|e| format!("cannot parse '{}': {}", doc_path.display(), e.message))?;
    // Extract the doc-id from the already-parsed document (avoids a second read).
    let doc_id = main_doc.doc_id.clone().ok_or_else(|| {
        format!(
            "'{}' has no history yet (no doc-id); edit it with `zenith tx --apply` or \
             `zenith library add` first",
            doc_path.display()
        )
    })?;

    // 2. List scratch candidates and find the requested one.
    let entries = list_scratch(&fs, paths, &doc_id).map_err(|e| e.message)?;
    let entry = entries
        .iter()
        .find(|e| e.id == cand_id)
        .ok_or_else(|| format!("candidate not found: {cand_id}"))?;

    // 3. Require Selected status.
    if entry.status != CandidateStatus::Selected {
        let actual = match entry.status {
            CandidateStatus::Draft => "draft",
            CandidateStatus::Selected => "selected",
            CandidateStatus::Rejected => "rejected",
        };
        return Err(format!(
            "candidate {cand_id} must have status \"selected\" to promote, but its status is \"{actual}\"; \
             use `zenith workspace candidate` to mark it selected first"
        ));
    }

    // 4. Fetch the candidate snapshot and parse it.
    let snap_bytes = get_scratch_snapshot(&fs, paths, &doc_id, entry).map_err(|e| e.message)?;
    let cand_doc = KdlAdapter.parse(snap_bytes.as_slice()).map_err(|e| {
        format!(
            "cannot parse candidate snapshot for {cand_id}: {}",
            e.message
        )
    })?;

    // 5. Find the source page in the candidate document.
    //    Use entry.page_id; fall back to the first page when page_id is "*".
    let source_page = if entry.page_id == "*" {
        cand_doc
            .body
            .pages
            .first()
            .ok_or_else(|| format!("candidate snapshot for {cand_id} has no pages"))?
    } else {
        cand_doc
            .body
            .pages
            .iter()
            .find(|p| p.id == entry.page_id)
            .or_else(|| cand_doc.body.pages.first())
            .ok_or_else(|| {
                format!(
                    "candidate snapshot for {cand_id} has no page with id {:?} and no pages at all",
                    entry.page_id
                )
            })?
    };

    // 6. Find the target page in the main document (mutable).
    let target_page = main_doc
        .body
        .pages
        .iter_mut()
        .find(|p| p.id == into_page)
        .ok_or_else(|| {
            format!(
                "target page {:?} not found in '{}'",
                into_page,
                doc_path.display()
            )
        })?;

    // 7. Merge. Source and target are in DIFFERENT documents — no borrow conflict.
    merge_candidate_page(source_page, target_page, id_suffix);

    // 8. Validate the mutated main document.
    let report = validate(&main_doc);
    let errors: Vec<&zenith_core::Diagnostic> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        let msgs: Vec<String> = errors
            .iter()
            .map(|d| format!("  error[{}]: {}", d.code, d.message))
            .collect();
        return Err(format!(
            "promote produced validation errors — document not written:\n{}",
            msgs.join("\n")
        ));
    }

    // 9. Format and persist (mirrors the `tx --apply` path).
    let formatted = KdlAdapter
        .format(&main_doc)
        .map_err(|e| format!("format failed: {}", e.message))?;
    let Recorded { bytes, warning } =
        record_edit_in(paths, &formatted, doc_path, "workspace.promote");
    if let Some(w) = &warning {
        eprintln!("warning: {w}");
    }
    std::fs::write(doc_path, &bytes)
        .map_err(|e| format!("cannot write '{}': {e}", doc_path.display()))?;

    Ok(format!("promoted {cand_id} → page {into_page}"))
}
