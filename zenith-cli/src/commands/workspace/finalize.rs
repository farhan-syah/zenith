//! Logic for `zenith workspace finalize`.

use std::path::Path;

use zenith_session::adapter::OsFs;
use zenith_session::{FinalizeReport, StorePaths, finalize_candidates};

use crate::commands::serialize_pretty;
use crate::commands::workspace::scratch::open_store;
use crate::history::read_doc_id;

// ── View type for JSON output ─────────────────────────────────────────────────

// FinalizeReport derives Serialize in zenith-session, so we use it directly.

// ── finalize ──────────────────────────────────────────────────────────────────

/// Apply each rejected candidate's cleanup-policy to the scratch store and
/// return a human-readable or JSON report.
pub fn finalize(doc_path: &Path, json: bool) -> Result<String, String> {
    let paths = open_store()?;
    finalize_in(&paths, doc_path, json)
}

/// Testable variant with an explicit store root.
pub fn finalize_in(paths: &StorePaths, doc_path: &Path, json: bool) -> Result<String, String> {
    let doc_id = read_doc_id(doc_path)?;
    let fs = OsFs;
    let report = finalize_candidates(&fs, paths, &doc_id).map_err(|e| e.message)?;

    if json {
        Ok(serialize_pretty(&report))
    } else {
        Ok(format_report(&report))
    }
}

// ── formatting ────────────────────────────────────────────────────────────────

fn format_report(report: &FinalizeReport) -> String {
    if report.deleted.is_empty() {
        format!(
            "finalized: nothing to delete, {} candidate(s) kept",
            report.kept
        )
    } else {
        format!(
            "finalized: deleted {} candidate(s) [{}], {} kept",
            report.deleted.len(),
            report.deleted.join(", "),
            report.kept,
        )
    }
}
