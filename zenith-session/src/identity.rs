//! Document-identity reconciliation: bind a `.zen`'s doc-id to local history,
//! detecting first-edit / move / copy / adoption. Pure over injected adapters.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::adapter::{Clock, Fs, Rng};
use crate::docid::mint_ulid;
use crate::error::SessionError;
use crate::layout::StorePaths;

// ── Data types ────────────────────────────────────────────────────────────────

/// Persisted per-doc metadata, stored as JSON at `meta_file(id)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocMeta {
    /// The document id this metadata belongs to.
    pub doc_id: String,
    /// Last-known absolute path of the `.zen` on this machine (lossy UTF-8).
    pub path: String,
    /// Unix-ms when this local history was first created.
    pub created_ms: u128,
    /// Unix-ms of the last reconcile that touched this doc.
    pub updated_ms: u128,
}

/// Result of a single reconciliation call, describing what was detected.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `.zen` had no doc-id; a fresh id was minted. Caller must stamp it into the file.
    Minted,
    /// Same doc at its known path; nothing changed but the updated_ms.
    Matched,
    /// The doc-id's previously-bound path no longer exists; rebound to the new path.
    Moved { from: String },
    /// The doc-id is still bound to a DIFFERENT, still-existing file → this is a copy.
    /// A new id was minted for this copy; caller must stamp `doc_id` into the file.
    Copied { previous: String },
    /// The doc-id was present in the file but no local history existed (e.g. cloned
    /// from a remote, or first use on this machine); local history was created.
    Adopted,
}

/// The return value of [`reconcile`].
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciled {
    /// The effective doc-id that MUST be present in the `.zen` after reconcile
    /// (newly minted for Minted/Copied; unchanged otherwise).
    pub doc_id: String,
    /// What the reconciler determined about this document's identity.
    pub outcome: Outcome,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Reconcile a `.zen`'s identity against local history.
///
/// `file_doc_id` is the id currently embedded in the `.zen` (None if absent).
/// `doc_path` is the document's path as the caller observes it — callers SHOULD
/// pass an absolute/canonical path; reconcile compares it verbatim (string form).
pub fn reconcile(
    fs: &impl Fs,
    paths: &StorePaths,
    clock: &impl Clock,
    rng: &impl Rng,
    file_doc_id: Option<&str>,
    doc_path: &Path,
) -> Result<Reconciled, SessionError> {
    let now_ms = clock
        .now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SessionError::new(format!("system clock is before the unix epoch: {e}")))?
        .as_millis();

    let path_str = doc_path.to_string_lossy().into_owned();

    match file_doc_id {
        None => {
            // No id in file: mint a fresh one.
            let id = mint_ulid(clock, rng)?;
            write_meta(
                fs,
                paths,
                &DocMeta {
                    doc_id: id.clone(),
                    path: path_str,
                    created_ms: now_ms,
                    updated_ms: now_ms,
                },
            )?;
            Ok(Reconciled {
                doc_id: id,
                outcome: Outcome::Minted,
            })
        }
        Some(id) => {
            match read_meta(fs, paths, id)? {
                None => {
                    // Id present but no local history: adopt it.
                    write_meta(
                        fs,
                        paths,
                        &DocMeta {
                            doc_id: id.to_string(),
                            path: path_str,
                            created_ms: now_ms,
                            updated_ms: now_ms,
                        },
                    )?;
                    Ok(Reconciled {
                        doc_id: id.to_string(),
                        outcome: Outcome::Adopted,
                    })
                }
                Some(mut meta) => {
                    if meta.path == path_str {
                        // Same doc at same path: just update the timestamp.
                        meta.updated_ms = now_ms;
                        write_meta(fs, paths, &meta)?;
                        Ok(Reconciled {
                            doc_id: id.to_string(),
                            outcome: Outcome::Matched,
                        })
                    } else if fs.exists(Path::new(&meta.path)) {
                        // Old path still exists: this is a copy.
                        let new_id = mint_ulid(clock, rng)?;
                        write_meta(
                            fs,
                            paths,
                            &DocMeta {
                                doc_id: new_id.clone(),
                                path: path_str,
                                created_ms: now_ms,
                                updated_ms: now_ms,
                            },
                        )?;
                        Ok(Reconciled {
                            doc_id: new_id,
                            outcome: Outcome::Copied {
                                previous: id.to_string(),
                            },
                        })
                    } else {
                        // Old path is gone: the file was moved/renamed.
                        let old_path = std::mem::replace(&mut meta.path, path_str);
                        meta.updated_ms = now_ms;
                        write_meta(fs, paths, &meta)?;
                        Ok(Reconciled {
                            doc_id: id.to_string(),
                            outcome: Outcome::Moved { from: old_path },
                        })
                    }
                }
            }
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn write_meta(fs: &impl Fs, paths: &StorePaths, meta: &DocMeta) -> Result<(), SessionError> {
    fs.create_dir_all(&paths.doc_dir(&meta.doc_id))?;
    let json = serde_json::to_vec_pretty(meta)
        .map_err(|e| SessionError::new(format!("serialize doc meta: {e}")))?;
    fs.write(&paths.meta_file(&meta.doc_id), &json)
}

fn read_meta(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<Option<DocMeta>, SessionError> {
    let p = paths.meta_file(doc_id);
    if !fs.exists(&p) {
        return Ok(None);
    }
    let bytes = fs.read(&p)?;
    let meta = serde_json::from_slice(&bytes)
        .map_err(|e| SessionError::new(format!("parse doc meta: {e}")))?;
    Ok(Some(meta))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{FakeClock, FakeRng, MemFs};
    use std::time::{Duration, UNIX_EPOCH};

    fn make_paths() -> StorePaths {
        StorePaths::new("/data")
    }

    #[test]
    fn mints_when_no_id() {
        let fs = MemFs::new();
        let paths = make_paths();
        let clock = FakeClock(UNIX_EPOCH + Duration::from_millis(1000));
        let rng = FakeRng(0x42);
        let doc_path = Path::new("/docs/a.zen");

        let result = reconcile(&fs, &paths, &clock, &rng, None, doc_path).unwrap();

        assert!(
            matches!(result.outcome, Outcome::Minted),
            "expected Minted, got {:?}",
            result.outcome
        );
        assert_eq!(result.doc_id.len(), 26, "doc_id should be 26 chars");

        // meta.json must now exist under the store.
        let meta_path = paths.meta_file(&result.doc_id);
        assert!(fs.exists(&meta_path), "meta.json should exist after mint");

        // The stored meta must bind to the given path.
        let stored: DocMeta = serde_json::from_slice(&fs.read(&meta_path).unwrap()).unwrap();
        assert_eq!(stored.path, doc_path.to_string_lossy().as_ref());
    }

    #[test]
    fn matches_same_path() {
        let fs = MemFs::new();
        let paths = make_paths();
        let clock1 = FakeClock(UNIX_EPOCH + Duration::from_millis(1000));
        let rng = FakeRng(0x11);
        let doc_path = Path::new("/docs/a.zen");

        // First: mint.
        let minted = reconcile(&fs, &paths, &clock1, &rng, None, doc_path).unwrap();
        assert!(matches!(minted.outcome, Outcome::Minted));

        // Second: reconcile same id at same path, with a later clock.
        let clock2 = FakeClock(UNIX_EPOCH + Duration::from_millis(2000));
        let result = reconcile(&fs, &paths, &clock2, &rng, Some(&minted.doc_id), doc_path).unwrap();

        assert!(
            matches!(result.outcome, Outcome::Matched),
            "expected Matched, got {:?}",
            result.outcome
        );
        assert_eq!(result.doc_id, minted.doc_id, "doc_id must be unchanged");

        // updated_ms must reflect the second clock.
        let meta_path = paths.meta_file(&result.doc_id);
        let stored: DocMeta = serde_json::from_slice(&fs.read(&meta_path).unwrap()).unwrap();
        assert_eq!(stored.updated_ms, 2000, "updated_ms should be advanced");
        assert_eq!(stored.created_ms, 1000, "created_ms should be unchanged");
    }

    #[test]
    fn adopts_unknown_id() {
        let fs = MemFs::new();
        let paths = make_paths();
        let clock = FakeClock(UNIX_EPOCH + Duration::from_millis(5000));
        let rng = FakeRng(0x00);
        let doc_path = Path::new("/docs/remote.zen");
        let foreign_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

        let result = reconcile(&fs, &paths, &clock, &rng, Some(foreign_id), doc_path).unwrap();

        assert!(
            matches!(result.outcome, Outcome::Adopted),
            "expected Adopted, got {:?}",
            result.outcome
        );
        assert_eq!(result.doc_id, foreign_id, "doc_id must stay unchanged");

        // Local history must now exist.
        let meta_path = paths.meta_file(foreign_id);
        assert!(fs.exists(&meta_path), "meta.json should exist after adopt");
        let stored: DocMeta = serde_json::from_slice(&fs.read(&meta_path).unwrap()).unwrap();
        assert_eq!(stored.path, doc_path.to_string_lossy().as_ref());
    }

    #[test]
    fn moves_when_old_path_gone() {
        let fs = MemFs::new();
        let paths = make_paths();
        let clock = FakeClock(UNIX_EPOCH + Duration::from_millis(1000));
        let rng = FakeRng(0xAA);
        let old_path = Path::new("/x/a.zen");
        let new_path = Path::new("/y/a.zen");

        // Mint for the original path.
        let minted = reconcile(&fs, &paths, &clock, &rng, None, old_path).unwrap();
        assert!(matches!(minted.outcome, Outcome::Minted));
        // Note: /x/a.zen was never created in the MemFs as a file — only meta under
        // /data exists — so fs.exists("/x/a.zen") is false, triggering the Move branch.

        let result = reconcile(&fs, &paths, &clock, &rng, Some(&minted.doc_id), new_path).unwrap();

        match result.outcome {
            Outcome::Moved { from } => {
                assert_eq!(from, "/x/a.zen", "from path should be the original path");
            }
            other => panic!("expected Moved, got {other:?}"),
        }
        assert_eq!(
            result.doc_id, minted.doc_id,
            "doc_id must be unchanged on move"
        );

        // meta.path should now point to the new path.
        let meta_path = paths.meta_file(&result.doc_id);
        let stored: DocMeta = serde_json::from_slice(&fs.read(&meta_path).unwrap()).unwrap();
        assert_eq!(stored.path, "/y/a.zen");
    }

    #[test]
    fn copies_when_old_path_still_exists() {
        let fs = MemFs::new();
        let paths = make_paths();
        // Use an earlier clock for the original mint so the timestamp prefix differs
        // from the copy's mint, ensuring distinct ULIDs even with the same rng seed.
        let clock_original = FakeClock(UNIX_EPOCH + Duration::from_millis(1000));
        let clock_copy = FakeClock(UNIX_EPOCH + Duration::from_millis(2000));
        let rng = FakeRng(0x55);
        let path_p1 = Path::new("/docs/original.zen");
        let path_p2 = Path::new("/docs/copy.zen");

        // Mint for path_p1.
        let minted = reconcile(&fs, &paths, &clock_original, &rng, None, path_p1).unwrap();
        assert!(matches!(minted.outcome, Outcome::Minted));
        let original_id = minted.doc_id.clone();

        // Materialise path_p1 in the MemFs so fs.exists(path_p1) returns true.
        fs.create_dir_all(Path::new("/docs")).unwrap();
        fs.write(path_p1, b"zen file content").unwrap();
        assert!(fs.exists(path_p1), "path_p1 must exist for the copy branch");

        // Reconcile the same id at a different path with a later clock.
        let result =
            reconcile(&fs, &paths, &clock_copy, &rng, Some(&original_id), path_p2).unwrap();

        match &result.outcome {
            Outcome::Copied { previous } => {
                assert_eq!(previous, &original_id, "previous should be the original id");
            }
            other => panic!("expected Copied, got {other:?}"),
        }
        assert_ne!(result.doc_id, original_id, "copy must get a new doc_id");
        assert_eq!(result.doc_id.len(), 26, "new doc_id should be 26 chars");

        // New meta must exist bound to path_p2.
        let new_meta_path = paths.meta_file(&result.doc_id);
        assert!(fs.exists(&new_meta_path), "new meta.json should exist");
        let new_meta: DocMeta = serde_json::from_slice(&fs.read(&new_meta_path).unwrap()).unwrap();
        assert_eq!(new_meta.path, "/docs/copy.zen");

        // Original meta must be untouched (still bound to path_p1).
        let orig_meta_path = paths.meta_file(&original_id);
        let orig_meta: DocMeta =
            serde_json::from_slice(&fs.read(&orig_meta_path).unwrap()).unwrap();
        assert_eq!(orig_meta.path, "/docs/original.zen");
    }
}
