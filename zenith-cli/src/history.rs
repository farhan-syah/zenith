//! Local document-history recording (best-effort attach hook).
//!
//! Wraps the `zenith-session` subsystem with the real OS adapters and the
//! resolved data directory. Every successful write-through edit (`tx --apply`,
//! `library add`) calls [`record_edit`]: it reconciles the document's identity
//! (minting and stamping a `doc-id` on first edit) and records the new state as
//! a Tier-1 session snapshot and a Tier-2 version. Recording is best-effort:
//! failures become a warning message on `Recorded::warning` — they never block
//! the edit.
//!
//! Navigation functions ([`history_view`], [`undo_edit`], [`redo_edit`]) expose
//! the session history to the CLI subcommands.

use std::path::Path;

use zenith_core::{KdlAdapter, KdlSource as _};
use zenith_session::adapter::{OsClock, OsFs, OsRng};
use zenith_session::{
    Outcome, RecordOutcome, StorePaths, VersionMeta, VersionOutcome, current_content,
    list_versions, reconcile, record_state, record_version, resolve_data_dir, resolve_version,
    version_content,
};

// ── Public result type ────────────────────────────────────────────────────────

/// The result of a best-effort history recording.
///
/// `bytes` are ALWAYS the bytes that should be written to disk — identical to
/// the input unless a fresh `doc-id` was minted or forked, in which case they
/// carry the stamped id. `warning` carries a human-readable description of any
/// non-fatal failure that occurred during recording.
pub struct Recorded {
    /// Bytes to write to the `.zen` file (may have a stamped `doc-id`).
    pub bytes: Vec<u8>,
    /// Non-fatal warning produced during history recording, if any.
    pub warning: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Resolve the data directory and record `content` (the about-to-be-written
/// `.zen` bytes) as history for the document at `doc_path`.
///
/// Returns a [`Recorded`] value: `bytes` are always the correct bytes to write
/// (possibly with a freshly minted `doc-id` stamped in), and `warning` carries
/// any non-fatal error message. This function never panics and never returns an
/// error — all failures are surfaced as `warning: Some(...)`.
pub fn record_edit(content: &[u8], doc_path: &Path, op_kind: &str) -> Recorded {
    let paths = match resolve_data_dir() {
        Ok(data_dir) => StorePaths::new(data_dir),
        Err(e) => {
            return Recorded {
                bytes: content.to_vec(),
                warning: Some(format!("history: resolve_data_dir failed: {}", e.message)),
            };
        }
    };
    record_edit_in(&paths, content, doc_path, op_kind)
}

/// Same as [`record_edit`] but with an explicit store root (used by tests).
///
/// Returns a [`Recorded`] value: `bytes` are always the correct bytes to write,
/// and `warning` carries any non-fatal error message. Never panics.
pub fn record_edit_in(
    paths: &StorePaths,
    content: &[u8],
    doc_path: &Path,
    op_kind: &str,
) -> Recorded {
    let fs = OsFs;
    let clock = OsClock;
    let rng = OsRng;

    // Parse to read the embedded doc-id (if any).
    let mut doc = match KdlAdapter.parse(content) {
        Ok(d) => d,
        Err(e) => {
            return Recorded {
                bytes: content.to_vec(),
                warning: Some(format!("history: parse failed: {}", e.message)),
            };
        }
    };

    let reconciled = match reconcile(&fs, paths, &clock, &rng, doc.doc_id.as_deref(), doc_path) {
        Ok(r) => r,
        Err(e) => {
            return Recorded {
                bytes: content.to_vec(),
                warning: Some(format!("history: reconcile failed: {}", e.message)),
            };
        }
    };

    // If a new id was minted or forked, stamp it into the document and re-format
    // so the written file carries the identity.
    let final_bytes: Vec<u8> = match reconciled.outcome {
        Outcome::Minted | Outcome::Copied { .. } => {
            doc.doc_id = Some(reconciled.doc_id.clone());
            match KdlAdapter.format(&doc) {
                Ok(b) => b,
                Err(e) => {
                    return Recorded {
                        bytes: content.to_vec(),
                        warning: Some(format!("history: format failed: {}", e.message)),
                    };
                }
            }
        }
        Outcome::Matched | Outcome::Moved { .. } | Outcome::Adopted => content.to_vec(),
    };

    // Record Tier-1 session snapshot. Best-effort: on failure, return the
    // (possibly stamped) bytes with a warning so the write still proceeds.
    if let Err(e) = record_state(
        &fs,
        paths,
        &clock,
        &rng,
        &reconciled.doc_id,
        &final_bytes,
        Some(op_kind),
    ) {
        return Recorded {
            bytes: final_bytes,
            warning: Some(format!(
                "history: record_state failed: {} (file will still be written)",
                e.message
            )),
        };
    }

    // Record Tier-2 durable version. Best-effort for the same reason.
    if let Err(e) = record_version(
        &fs,
        paths,
        &clock,
        &reconciled.doc_id,
        &final_bytes,
        VersionMeta {
            op_kind: Some(op_kind),
            ..Default::default()
        },
    ) {
        return Recorded {
            bytes: final_bytes,
            warning: Some(format!(
                "history: record_version failed: {} (file will still be written)",
                e.message
            )),
        };
    }

    Recorded {
        bytes: final_bytes,
        warning: None,
    }
}

// ── Navigation types ──────────────────────────────────────────────────────────

/// One line in a history listing (maps 1-to-1 to a Tier-2 [`HistoryRecord`]).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryLine {
    /// Stable record id.
    pub id: String,
    /// Monotonic sequence number (0-based).
    pub seq: u64,
    /// Optional human-facing label / version name.
    pub label: Option<String>,
    /// Optional operation-kind tag (e.g. `"tx.apply"`, `"library.add"`).
    pub op_kind: Option<String>,
    /// Optional unix-ms timestamp.
    pub timestamp_ms: Option<u128>,
}

/// History listing for a single document.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryView {
    /// The document's stable `doc-id`.
    pub doc_id: String,
    /// All Tier-2 version records, oldest first.
    pub versions: Vec<HistoryLine>,
    /// `true` when the Tier-1 session has a current HEAD (unsaved session
    /// content exists), `false` when the session is empty or absent.
    pub has_session: bool,
}

/// Outcome of a [`undo_edit`] or [`redo_edit`] navigation call.
#[derive(Debug, Clone, PartialEq)]
pub enum NavOutcome {
    /// Navigation succeeded; the document was rewritten with the restored content.
    Moved,
    /// Nothing to undo/redo (already at the boundary, or no session exists yet).
    NothingToDo,
}

// ── Navigation helpers ────────────────────────────────────────────────────────

/// Read `doc_path` and return its raw bytes plus its embedded `doc-id`.
///
/// Returns a human-readable error if the file cannot be read, cannot be parsed,
/// or has no `doc-id` attribute yet (meaning it has never been edited through
/// zenith's history pipeline).
fn read_doc_with_id(doc_path: &Path) -> Result<(Vec<u8>, String), String> {
    let bytes = std::fs::read(doc_path)
        .map_err(|e| format!("cannot read '{}': {e}", doc_path.display()))?;
    let doc = KdlAdapter
        .parse(&bytes)
        .map_err(|e| format!("cannot parse '{}': {}", doc_path.display(), e.message))?;
    let id = doc.doc_id.ok_or_else(|| {
        format!(
            "'{}' has no history yet (no doc-id); edit it with `zenith tx --apply` or \
             `zenith library add` first",
            doc_path.display()
        )
    })?;
    Ok((bytes, id))
}

/// Read `doc_path` and extract its embedded `doc-id`.
///
/// Delegates to [`read_doc_with_id`]; returns only the id.
fn doc_id_at(doc_path: &Path) -> Result<String, String> {
    read_doc_with_id(doc_path).map(|(_, id)| id)
}

/// Public thin wrapper around [`doc_id_at`] for use by sibling command modules.
///
/// Returns a human-readable error if the file cannot be read, cannot be
/// parsed, or has no `doc-id` attribute yet (meaning it has never been
/// recorded through the history pipeline).
pub fn read_doc_id(doc_path: &Path) -> Result<String, String> {
    doc_id_at(doc_path)
}

// ── history_view ──────────────────────────────────────────────────────────────

/// Build the history view for the document at `doc_path`.
///
/// Resolves the real data directory automatically. Use [`history_view_in`] in
/// tests where you want a tempdir-rooted store.
pub fn history_view(doc_path: &Path) -> Result<HistoryView, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    history_view_in(&paths, doc_path)
}

/// Same as [`history_view`] but with an explicit store root (used by tests).
pub fn history_view_in(paths: &StorePaths, doc_path: &Path) -> Result<HistoryView, String> {
    let doc_id = doc_id_at(doc_path)?;
    let fs = OsFs;
    let versions = list_versions(&fs, paths, &doc_id)
        .map_err(|e| e.message)?
        .into_iter()
        .map(|r| HistoryLine {
            id: r.id,
            seq: r.seq,
            label: r.label,
            op_kind: r.op_kind,
            timestamp_ms: r.timestamp_ms,
        })
        .collect();
    let has_session = current_content(&fs, paths, &doc_id)
        .map_err(|e| e.message)?
        .is_some();
    Ok(HistoryView {
        doc_id,
        versions,
        has_session,
    })
}

// ── undo_edit ─────────────────────────────────────────────────────────────────

/// Undo the last edit for the document at `doc_path`, rewriting the file in place.
///
/// Resolves the real data directory automatically. Use [`undo_edit_in`] in tests.
pub fn undo_edit(doc_path: &Path) -> Result<NavOutcome, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    undo_edit_in(&paths, doc_path)
}

/// Same as [`undo_edit`] but with an explicit store root (used by tests).
pub fn undo_edit_in(paths: &StorePaths, doc_path: &Path) -> Result<NavOutcome, String> {
    let doc_id = doc_id_at(doc_path)?;
    let fs = OsFs;
    match zenith_session::undo(&fs, paths, &doc_id).map_err(|e| e.message)? {
        Some(content) => {
            std::fs::write(doc_path, &content)
                .map_err(|e| format!("cannot write '{}': {e}", doc_path.display()))?;
            Ok(NavOutcome::Moved)
        }
        None => Ok(NavOutcome::NothingToDo),
    }
}

// ── redo_edit ─────────────────────────────────────────────────────────────────

/// Redo the last undone edit for the document at `doc_path`, rewriting the file in place.
///
/// Resolves the real data directory automatically. Use [`redo_edit_in`] in tests.
pub fn redo_edit(doc_path: &Path) -> Result<NavOutcome, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    redo_edit_in(&paths, doc_path)
}

/// Same as [`redo_edit`] but with an explicit store root (used by tests).
pub fn redo_edit_in(paths: &StorePaths, doc_path: &Path) -> Result<NavOutcome, String> {
    let doc_id = doc_id_at(doc_path)?;
    let fs = OsFs;
    match zenith_session::redo(&fs, paths, &doc_id).map_err(|e| e.message)? {
        Some(content) => {
            std::fs::write(doc_path, &content)
                .map_err(|e| format!("cannot write '{}': {e}", doc_path.display()))?;
            Ok(NavOutcome::Moved)
        }
        None => Ok(NavOutcome::NothingToDo),
    }
}

// ── name_version ──────────────────────────────────────────────────────────────

/// Save the current on-disk content of `doc_path` as a NAMED Tier-2 version.
///
/// Resolves the real data directory automatically. Use [`name_version_in`] in
/// tests where you want a tempdir-rooted store.
///
/// Returns the new (or existing latest) version id (e.g. `"v3"`), or a
/// human-readable error.
pub fn name_version(doc_path: &Path, name: &str) -> Result<String, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    name_version_in(&paths, doc_path, name)
}

/// Same as [`name_version`] but with an explicit store root (used by tests).
pub fn name_version_in(paths: &StorePaths, doc_path: &Path, name: &str) -> Result<String, String> {
    let (bytes, doc_id) = read_doc_with_id(doc_path)?;
    let fs = OsFs;
    let clock = OsClock;
    match record_version(
        &fs,
        paths,
        &clock,
        &doc_id,
        &bytes,
        VersionMeta {
            label: Some(name),
            op_kind: Some("named"),
            ..Default::default()
        },
    ) {
        Ok(VersionOutcome::Recorded { id }) => Ok(id),
        Ok(VersionOutcome::Unchanged) => {
            // No content change since the last version: still report success by
            // returning the latest version id via a fresh resolve of "@head".
            resolve_version(&fs, paths, &doc_id, "@head").map_err(|e| e.message)
        }
        Err(e) => Err(e.message),
    }
}

// ── sync_external ─────────────────────────────────────────────────────────────

/// Outcome of a [`sync_external`] or [`sync_external_in`] call.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    /// The on-disk state differed from HEAD and was captured as a new record.
    Captured { id: String },
    /// The on-disk state already matched HEAD; nothing to capture.
    AlreadyInSync,
}

/// Capture the current on-disk content of `doc_path` into Tier-1 history as an
/// external change, if it differs from the session HEAD. Resolves the real data dir.
pub fn sync_external(doc_path: &Path) -> Result<SyncOutcome, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    sync_external_in(&paths, doc_path)
}

/// Testable variant with an explicit store root.
pub fn sync_external_in(paths: &StorePaths, doc_path: &Path) -> Result<SyncOutcome, String> {
    let (bytes, doc_id) = read_doc_with_id(doc_path)?;
    let fs = OsFs;
    let clock = OsClock;
    let rng = OsRng;
    match record_state(&fs, paths, &clock, &rng, &doc_id, &bytes, Some("external")) {
        Ok(RecordOutcome::Recorded { id }) => Ok(SyncOutcome::Captured { id }),
        Ok(RecordOutcome::Unchanged) => Ok(SyncOutcome::AlreadyInSync),
        Err(e) => Err(e.message),
    }
}

// ── restore ───────────────────────────────────────────────────────────────────

/// Outcome of a [`restore`] or [`restore_in`] call.
pub struct RestoreOutcome {
    /// The resolved version id that was restored (e.g. `"v2"`).
    pub version_id: String,
    /// Non-fatal warning from recording the restore as a new edit, if any.
    pub warning: Option<String>,
}

/// Resolve `spec` to a past version, write its content back to `doc_path`, and
/// record the restore as a new (undoable) edit.
///
/// Resolves the real data directory automatically. Use [`restore_in`] in tests
/// where you want a tempdir-rooted store.
pub fn restore(doc_path: &Path, spec: &str) -> Result<RestoreOutcome, String> {
    let data_dir = resolve_data_dir().map_err(|e| e.message)?;
    let paths = StorePaths::new(data_dir);
    restore_in(&paths, doc_path, spec)
}

/// Same as [`restore`] but with an explicit store root (used by tests).
pub fn restore_in(
    paths: &StorePaths,
    doc_path: &Path,
    spec: &str,
) -> Result<RestoreOutcome, String> {
    let doc_id = doc_id_at(doc_path)?;
    let fs = OsFs;
    let version_id = resolve_version(&fs, paths, &doc_id, spec).map_err(|e| e.message)?;
    let content = version_content(&fs, paths, &doc_id, &version_id).map_err(|e| e.message)?;
    // Record the restore as a new write-through edit (Tier-1 + Tier-2), then write.
    let recorded = record_edit_in(paths, &content, doc_path, "restore");
    std::fs::write(doc_path, &recorded.bytes)
        .map_err(|e| format!("cannot write '{}': {e}", doc_path.display()))?;
    Ok(RestoreOutcome {
        version_id,
        warning: recorded.warning,
    })
}
