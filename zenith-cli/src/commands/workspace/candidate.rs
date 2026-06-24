//! Logic for `zenith workspace candidate` (set lifecycle status).

use std::path::Path;

use zenith_session::adapter::{OsClock, OsFs};
use zenith_session::{StorePaths, set_candidate_status};

use crate::commands::workspace::scratch::{open_store, parse_status};
use crate::history::read_doc_id;

/// Transition `cand_id`'s lifecycle status to `status_str` and return a
/// confirmation line.
pub fn candidate_set_status(
    doc_path: &Path,
    cand_id: &str,
    status_str: &str,
) -> Result<String, String> {
    let paths = open_store()?;
    candidate_set_status_in(&paths, doc_path, cand_id, status_str)
}

/// Testable variant with an explicit store root.
pub fn candidate_set_status_in(
    paths: &StorePaths,
    doc_path: &Path,
    cand_id: &str,
    status_str: &str,
) -> Result<String, String> {
    let doc_id = read_doc_id(doc_path)?;
    let fs = OsFs;
    let clock = OsClock;
    let new_status = parse_status(status_str)?;
    set_candidate_status(&fs, paths, &clock, &doc_id, cand_id, new_status)
        .map_err(|e| e.message)?;
    Ok(format!("candidate {cand_id} → {status_str}"))
}
