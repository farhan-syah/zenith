//! Logic for `zenith workspace bundle` and `zenith workspace unbundle`.

use std::path::Path;

use zenith_session::adapter::OsFs;
use zenith_session::{StorePaths, bundle, resolve_data_dir, unbundle};

use crate::commands::workspace::scratch::open_store;
use crate::history::read_doc_id;

// ── bundle ────────────────────────────────────────────────────────────────────

/// Pack the session store for the document at `doc_path` into `out_path`.
///
/// Resolves the real data directory automatically. Use [`bundle_doc_in`] in
/// tests where you want a tempdir-rooted store.
pub fn bundle_doc(doc_path: &Path, out_path: &Path) -> Result<String, String> {
    let paths = open_store()?;
    bundle_doc_in(&paths, doc_path, out_path)
}

/// Testable variant with an explicit store root.
pub fn bundle_doc_in(
    paths: &StorePaths,
    doc_path: &Path,
    out_path: &Path,
) -> Result<String, String> {
    let doc_id = read_doc_id(doc_path)?;
    let fs = OsFs;
    let bytes = bundle(&fs, paths, &doc_id).map_err(|e| e.message)?;
    std::fs::write(out_path, &bytes)
        .map_err(|e| format!("cannot write '{}': {e}", out_path.display()))?;
    Ok(format!("bundled {} → {}", doc_id, out_path.display()))
}

// ── unbundle ──────────────────────────────────────────────────────────────────

/// Restore a document's session store from the `.zenithbundle` at `bundle_path`.
///
/// Resolves the real data directory automatically. Use [`unbundle_doc_in`] in
/// tests where you want a tempdir-rooted store.
pub fn unbundle_doc(bundle_path: &Path) -> Result<String, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    unbundle_doc_in(&paths, bundle_path)
}

/// Testable variant with an explicit store root.
pub fn unbundle_doc_in(paths: &StorePaths, bundle_path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(bundle_path)
        .map_err(|e| format!("cannot read '{}': {e}", bundle_path.display()))?;
    let fs = OsFs;
    let doc_id = unbundle(&fs, paths, &bytes).map_err(|e| e.message)?;
    Ok(doc_id)
}
