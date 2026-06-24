//! Resolve a tool's `doc` argument, which may be either a filesystem path or a
//! 26-character ULID `doc-id`.
//!
//! Addressing a document by its stable `doc-id` lets an agent stop juggling
//! absolute paths after the first call: the path is recovered from the store's
//! per-doc `meta.json`. A bare path is used verbatim.

use std::path::PathBuf;

use zenith_session::DocMeta;

use super::resources::open_store;
use crate::history::{ensure_doc_id_in, read_doc_id};

/// A resolved document reference.
pub struct Located {
    /// The document's filesystem path.
    pub path: PathBuf,
    /// The document's `doc-id`, when one is already known (never minted here).
    pub doc_id: Option<String>,
}

/// Resolve `reference` to a path (and `doc-id` if already known) WITHOUT minting.
///
/// - A ULID is looked up in the store's `meta.json` to recover its path.
/// - Anything else is treated as a path; its `doc-id` is read from the file when
///   present (a brand-new file simply yields `doc_id: None`).
pub fn locate(reference: &str) -> Result<Located, String> {
    if is_ulid(reference) {
        let meta = read_meta(reference)?;
        Ok(Located {
            path: PathBuf::from(meta.path),
            doc_id: Some(meta.doc_id),
        })
    } else {
        let path = PathBuf::from(reference);
        let doc_id = read_doc_id(&path).ok();
        Ok(Located { path, doc_id })
    }
}

/// Resolve `reference` to `(path, doc_id)`, minting + stamping a `doc-id` when a
/// path-referenced document does not yet have one (same attach-on-first-use
/// behaviour as `zenith workspace scratch new`).
pub fn ensure(reference: &str) -> Result<(PathBuf, String), String> {
    if is_ulid(reference) {
        let meta = read_meta(reference)?;
        return Ok((PathBuf::from(meta.path), meta.doc_id));
    }
    let path = PathBuf::from(reference);
    let paths = open_store()?;
    let ensured = ensure_doc_id_in(&paths, &path)?;
    Ok((path, ensured.doc_id))
}

/// Read a document's persisted [`DocMeta`] by `doc-id`.
fn read_meta(doc_id: &str) -> Result<DocMeta, String> {
    let paths = open_store()?;
    let meta_path = paths.meta_file(doc_id);
    let bytes = std::fs::read(&meta_path)
        .map_err(|_| format!("unknown doc-id '{doc_id}' (no local history on this machine)"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("corrupt meta for '{doc_id}': {e}"))
}

/// True when `s` is a 26-character Crockford base-32 ULID (the `doc-id` form).
fn is_ulid(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(is_crockford)
}

/// Crockford base-32 digit (ULID alphabet: excludes I, L, O, U).
fn is_crockford(b: u8) -> bool {
    b.is_ascii_digit()
        || matches!(b, b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_shape_is_detected() {
        // Canonical 26-char ULIDs.
        assert!(is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(is_ulid("01BX5ZZKBKACTAV9WEVGEMMVRZ"));
    }

    #[test]
    fn paths_are_not_ulids() {
        assert!(!is_ulid("/tmp/poster.zen"));
        assert!(!is_ulid("poster.zen"));
        // Right length but contains an excluded letter / lowercase.
        assert!(!is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAi"));
        assert!(!is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAL"));
    }
}
