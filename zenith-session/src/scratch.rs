//! Scratch/candidate store: content-addressed `.zen` snapshots indexed in
//! `scratch/index.jsonl`.
//!
//! Each [`CandidateEntry`] records a `page_id` (the page or source being
//! snapshotted), a `snapshot_hash` that addresses the raw `.zen` bytes in the
//! shared object store, a [`CandidateStatus`], and optional workflow metadata.
//!
//! # Append-only contract
//!
//! This module is **append-only**: all writes are appends. [`put_scratch`] adds
//! new candidates; [`set_candidate_status`] transitions an existing candidate's
//! status by appending a superseding entry (same `id`/`page_id`/`snapshot_hash`,
//! new status and timestamp). [`list_scratch`] resolves the latest status per
//! `id` via **last-write-wins** deduplication, returning one entry per distinct
//! candidate in first-appearance order. The raw file is fully auditable.

use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::adapter::{Clock, Fs};
use crate::error::SessionError;
use crate::layout::StorePaths;
use crate::manifest::{append_jsonl_record, read_jsonl_records};
use crate::store::{get_object, put_object};

// ── CandidateStatus ───────────────────────────────────────────────────────────

/// Lifecycle state of a scratch candidate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    /// The candidate is still being evaluated.
    Draft,
    /// The candidate has been chosen for promotion.
    Selected,
    /// The candidate has been discarded.
    Rejected,
}

// ── CandidateEntry ────────────────────────────────────────────────────────────

/// A single scratch candidate record appended to `scratch/index.jsonl`.
///
/// `id` and `seq` are derived by [`put_scratch`] from the current index
/// length; callers do not supply them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateEntry {
    /// Stable candidate id within this document's scratch index (e.g. `cand0`).
    pub id: String,
    /// Monotonic sequence number (0-based, append order).
    pub seq: u64,
    /// The page or source document this candidate snapshots.
    pub page_id: String,
    /// SHA-256 content hash of the stored `.zen` snapshot bytes (in `objects/`).
    pub snapshot_hash: String,
    /// Lifecycle status at the time this entry was appended.
    pub status: CandidateStatus,
    /// Optional workflow role label (e.g. `"hero"`, `"fallback"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_role: Option<String>,
    /// Optional target to promote this candidate to (e.g. a branch or slot id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion_target: Option<String>,
    /// Optional policy controlling when this candidate may be cleaned up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_policy: Option<String>,
    /// Optional free-text notes about this candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Unix timestamp in milliseconds when this entry was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
}

// ── CandidateMeta ─────────────────────────────────────────────────────────────

/// Borrowed optional metadata for a new candidate (mirrors `VersionMeta`).
///
/// All fields default to `None`; supply only what is available at call time.
#[derive(Debug, Clone, Copy, Default)]
pub struct CandidateMeta<'a> {
    /// Optional workflow role label for this candidate.
    pub workspace_role: Option<&'a str>,
    /// Optional promotion target for this candidate.
    pub promotion_target: Option<&'a str>,
    /// Optional cleanup policy tag.
    pub cleanup_policy: Option<&'a str>,
    /// Optional free-text notes.
    pub notes: Option<&'a str>,
}

// ── NewCandidate ──────────────────────────────────────────────────────────────

/// The describing inputs for a new candidate snapshot: which page it captures,
/// the `.zen` snapshot bytes, its lifecycle status, and optional metadata.
#[derive(Debug, Clone, Copy)]
pub struct NewCandidate<'a> {
    /// The page or source document this candidate snapshots.
    pub page_id: &'a str,
    /// Raw `.zen` snapshot bytes to store in the object store.
    pub snapshot: &'a [u8],
    /// Lifecycle status for this candidate at creation time.
    pub status: CandidateStatus,
    /// Optional workflow metadata (role, target, policy, notes).
    pub meta: CandidateMeta<'a>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Store a candidate snapshot and append a [`CandidateEntry`] to the scratch
/// index.
///
/// The `.zen` bytes in `candidate.snapshot` are written to the shared object
/// store (content-addressed, idempotent). `seq` and `id` are derived from the
/// current index length so callers do not need to track them. Returns the
/// created entry.
pub fn put_scratch(
    fs: &impl Fs,
    paths: &StorePaths,
    clock: &impl Clock,
    doc_id: &str,
    candidate: NewCandidate<'_>,
) -> Result<CandidateEntry, SessionError> {
    let snapshot_hash = put_object(fs, paths, doc_id, candidate.snapshot)?;
    let seq = u64::try_from(list_scratch(fs, paths, doc_id)?.len())
        .map_err(|_| SessionError::new("candidate count exceeds u64"))?;
    let id = format!("cand{seq}");
    let timestamp_ms = clock
        .now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis());
    let entry = CandidateEntry {
        id,
        seq,
        page_id: candidate.page_id.to_owned(),
        snapshot_hash,
        status: candidate.status,
        workspace_role: candidate.meta.workspace_role.map(str::to_owned),
        promotion_target: candidate.meta.promotion_target.map(str::to_owned),
        cleanup_policy: candidate.meta.cleanup_policy.map(str::to_owned),
        notes: candidate.meta.notes.map(str::to_owned),
        timestamp_ms,
    };
    append_jsonl_record(fs, &paths.scratch_index(doc_id), &entry)?;
    Ok(entry)
}

/// List candidate entries for `doc_id`, one per distinct candidate id.
///
/// Reads all appended records and deduplicates by `id`: for each distinct `id`,
/// the **last** record in file order is kept (last-write-wins). Entries are
/// returned in **first-appearance order** (the order each `id` was first seen).
///
/// Returns an empty vec when no scratch index exists for the document.
pub fn list_scratch(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<Vec<CandidateEntry>, SessionError> {
    let raw = read_jsonl_records(fs, &paths.scratch_index(doc_id))?;
    Ok(dedup_last_write_wins(raw))
}

/// Deduplicate `records` by `id`, keeping the last occurrence of each `id`
/// and emitting entries in first-appearance order. Deterministic; uses only
/// `Vec` and `BTreeMap`.
fn dedup_last_write_wins(records: Vec<CandidateEntry>) -> Vec<CandidateEntry> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::BTreeMap<String, CandidateEntry> =
        std::collections::BTreeMap::new();
    for entry in records {
        if !map.contains_key(&entry.id) {
            order.push(entry.id.clone());
        }
        map.insert(entry.id.clone(), entry);
    }
    order.iter().filter_map(|id| map.get(id).cloned()).collect()
}

/// Transition a candidate's lifecycle status by appending a superseding entry
/// (same `id`/`page_id`/`snapshot_hash`, new `status` + fresh timestamp). The
/// scratch index stays append-only and auditable; [`list_scratch`] resolves the
/// latest status per `id` via last-write-wins.
///
/// Returns `SessionError` if `cand_id` does not match any known candidate.
pub fn set_candidate_status(
    fs: &impl Fs,
    paths: &StorePaths,
    clock: &impl Clock,
    doc_id: &str,
    cand_id: &str,
    new_status: CandidateStatus,
) -> Result<CandidateEntry, SessionError> {
    let entries = list_scratch(fs, paths, doc_id)?;
    let existing = entries
        .iter()
        .find(|e| e.id == cand_id)
        .ok_or_else(|| SessionError::new(format!("candidate not found: {cand_id}")))?;
    let timestamp_ms = clock
        .now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis());
    let updated = CandidateEntry {
        status: new_status,
        timestamp_ms,
        ..existing.clone()
    };
    append_jsonl_record(fs, &paths.scratch_index(doc_id), &updated)?;
    Ok(updated)
}

// ── FinalizeReport ────────────────────────────────────────────────────────────

/// Report of a finalize pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinalizeReport {
    /// Candidate ids removed from the index (had `status == Rejected` and
    /// `cleanup_policy == Some("delete")`).
    pub deleted: Vec<String>,
    /// Number of distinct candidates remaining in the index after the pass.
    pub kept: usize,
}

/// Apply each rejected candidate's cleanup-policy to the scratch store.
///
/// Candidates that are `Rejected` with `cleanup_policy == Some("delete")` are
/// removed from the index entirely. Their snapshot objects are left in
/// `objects/` for a future GC pass. All other candidates (non-rejected, or
/// rejected with a different/absent cleanup policy) are preserved.
///
/// The scratch index is normally append-only; this is an explicit compaction
/// that rewrites it to exclude deleted candidates' lines while preserving
/// every other candidate's full append history (all raw lines for kept ids are
/// retained in their original order).
///
/// Returns `Ok(FinalizeReport { deleted: [], kept: N })` without touching the
/// file when there is nothing to delete.
pub fn finalize_candidates(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<FinalizeReport, SessionError> {
    let resolved = list_scratch(fs, paths, doc_id)?;

    let mut to_delete: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &resolved {
        match entry.status {
            CandidateStatus::Rejected => {
                if entry.cleanup_policy.as_deref() == Some("delete") {
                    to_delete.insert(entry.id.clone());
                }
            }
            CandidateStatus::Draft | CandidateStatus::Selected => {}
        }
    }

    if to_delete.is_empty() {
        return Ok(FinalizeReport {
            deleted: vec![],
            kept: resolved.len(),
        });
    }

    let raw = read_jsonl_records::<CandidateEntry>(fs, &paths.scratch_index(doc_id))?;
    let kept_raw: Vec<&CandidateEntry> =
        raw.iter().filter(|r| !to_delete.contains(&r.id)).collect();

    let mut bytes: Vec<u8> = Vec::new();
    for entry in &kept_raw {
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| SessionError::new(format!("serialize candidate: {e}")))?;
        line.push(b'\n');
        bytes.extend_from_slice(&line);
    }

    let index_path = paths.scratch_index(doc_id);
    if let Some(parent) = index_path.parent() {
        fs.create_dir_all(parent)?;
    }
    fs.write(&index_path, &bytes)?;

    let kept = resolved.len() - to_delete.len();
    Ok(FinalizeReport {
        deleted: to_delete.into_iter().collect(),
        kept,
    })
}

/// Recover the stored `.zen` snapshot bytes for a candidate entry.
///
/// Decompresses and verifies the object addressed by `entry.snapshot_hash`.
pub fn get_scratch_snapshot(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    entry: &CandidateEntry,
) -> Result<Vec<u8>, SessionError> {
    get_object(fs, paths, doc_id, &entry.snapshot_hash)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::adapter::{FakeClock, MemFs};
    use crate::layout::StorePaths;

    fn setup() -> (MemFs, StorePaths) {
        (MemFs::new(), StorePaths::new("/data"))
    }

    fn clock() -> FakeClock {
        FakeClock(UNIX_EPOCH + Duration::from_millis(100))
    }

    #[test]
    fn put_then_list_scratch_roundtrip() {
        let (fs, paths) = setup();
        let clk = clock();

        let meta_full = CandidateMeta {
            workspace_role: Some("hero"),
            promotion_target: Some("slot-a"),
            cleanup_policy: Some("on_select"),
            notes: Some("first pass"),
        };
        let e0 = put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen content A",
                status: CandidateStatus::Draft,
                meta: meta_full,
            },
        )
        .unwrap();

        let e1 = put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen content B",
                status: CandidateStatus::Selected,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        assert_eq!(e0.seq, 0);
        assert_eq!(e0.id, "cand0");
        assert_eq!(e0.page_id, "page-a");
        assert_eq!(e0.status, CandidateStatus::Draft);
        assert_eq!(e0.workspace_role, Some("hero".to_owned()));
        assert_eq!(e0.promotion_target, Some("slot-a".to_owned()));
        assert_eq!(e0.cleanup_policy, Some("on_select".to_owned()));
        assert_eq!(e0.notes, Some("first pass".to_owned()));

        assert_eq!(e1.seq, 1);
        assert_eq!(e1.id, "cand1");
        assert_eq!(e1.page_id, "page-b");
        assert_eq!(e1.status, CandidateStatus::Selected);
        assert_eq!(e1.workspace_role, None);

        let entries = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], e0);
        assert_eq!(entries[1], e1);
    }

    #[test]
    fn snapshot_bytes_recovered_intact() {
        let (fs, paths) = setup();
        let clk = clock();
        let zen_bytes = b"node layout { width 100 }";

        let entry = put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-x",
                snapshot: zen_bytes,
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let recovered = get_scratch_snapshot(&fs, &paths, "doc1", &entry).unwrap();
        assert_eq!(recovered.as_slice(), zen_bytes.as_slice());
    }

    #[test]
    fn lean_candidate_omits_optionals() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-lean",
                snapshot: b"lean",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let raw = fs.read(&paths.scratch_index("doc1")).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();

        assert!(
            !line.contains("workspace_role"),
            "workspace_role must be absent in lean form"
        );
        assert!(
            !line.contains("promotion_target"),
            "promotion_target must be absent in lean form"
        );
        assert!(
            !line.contains("cleanup_policy"),
            "cleanup_policy must be absent in lean form"
        );
        assert!(
            !line.contains("\"notes\""),
            "notes must be absent in lean form"
        );
    }

    #[test]
    fn status_serializes_snake_case() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-sel",
                snapshot: b"sel",
                status: CandidateStatus::Selected,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let raw = fs.read(&paths.scratch_index("doc1")).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();
        assert!(
            line.contains("\"selected\""),
            "Selected status must serialize as \"selected\""
        );
    }

    #[test]
    fn list_scratch_absent_is_empty() {
        let (fs, paths) = setup();
        let entries = list_scratch(&fs, &paths, "no-such-doc").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn set_status_draft_to_selected() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let updated = set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand0",
            CandidateStatus::Selected,
        )
        .unwrap();
        assert_eq!(updated.status, CandidateStatus::Selected);
        assert_eq!(updated.id, "cand0");

        let entries = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(entries.len(), 1, "dedup must yield exactly one entry");
        assert_eq!(entries[0].status, CandidateStatus::Selected);
    }

    #[test]
    fn set_status_draft_to_rejected() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand0",
            CandidateStatus::Rejected,
        )
        .unwrap();

        let entries = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, CandidateStatus::Rejected);
    }

    #[test]
    fn list_scratch_dedup_keeps_latest_and_order() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();
        set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand0",
            CandidateStatus::Selected,
        )
        .unwrap();

        let entries = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(entries.len(), 2, "must have exactly 2 distinct candidates");
        assert_eq!(entries[0].id, "cand0");
        assert_eq!(entries[0].status, CandidateStatus::Selected);
        assert_eq!(entries[1].id, "cand1");
        assert_eq!(entries[1].status, CandidateStatus::Draft);
    }

    #[test]
    fn set_status_unknown_candidate_errors() {
        let (fs, paths) = setup();
        let clk = clock();

        let result = set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand99",
            CandidateStatus::Selected,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("cand99"),
            "error message should include the missing id"
        );
    }

    #[test]
    fn finalize_deletes_rejected_delete_policy() {
        let (fs, paths) = setup();
        let clk = clock();

        // cand0: rejected + delete  → must be removed
        let e0 = put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Rejected,
                meta: CandidateMeta {
                    cleanup_policy: Some("delete"),
                    ..CandidateMeta::default()
                },
            },
        )
        .unwrap();

        // cand1: draft → kept
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        // cand2: selected → kept
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-c",
                snapshot: b"zen C",
                status: CandidateStatus::Selected,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let report = finalize_candidates(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, vec![e0.id.clone()]);
        assert_eq!(report.kept, 2);

        let remaining = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(
            remaining.iter().all(|e| e.id != e0.id),
            "deleted candidate must not appear in list"
        );
    }

    #[test]
    fn finalize_keeps_archived_and_selected() {
        let (fs, paths) = setup();
        let clk = clock();

        // cand0: rejected + archive → kept
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Rejected,
                meta: CandidateMeta {
                    cleanup_policy: Some("archive"),
                    ..CandidateMeta::default()
                },
            },
        )
        .unwrap();

        // cand1: selected → kept
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Selected,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let report = finalize_candidates(&fs, &paths, "doc1").unwrap();
        assert!(report.deleted.is_empty(), "nothing should be deleted");
        assert_eq!(report.kept, 2);

        let remaining = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn finalize_noop_when_nothing_to_delete() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        let bytes_before = fs.read(&paths.scratch_index("doc1")).unwrap();
        let report = finalize_candidates(&fs, &paths, "doc1").unwrap();
        assert!(report.deleted.is_empty());
        assert_eq!(report.kept, 2);
        // No-op path: file must be unchanged.
        let bytes_after = fs.read(&paths.scratch_index("doc1")).unwrap();
        assert_eq!(bytes_before, bytes_after, "file must be unchanged on noop");
    }

    #[test]
    fn finalize_preserves_other_candidates_history() {
        let (fs, paths) = setup();
        let clk = clock();

        // cand0: draft → selected (two raw lines in the index)
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();
        set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand0",
            CandidateStatus::Selected,
        )
        .unwrap();

        // cand1: rejected + delete → removed
        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Rejected,
                meta: CandidateMeta {
                    cleanup_policy: Some("delete"),
                    ..CandidateMeta::default()
                },
            },
        )
        .unwrap();

        let report = finalize_candidates(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, vec!["cand1".to_owned()]);
        assert_eq!(report.kept, 1);

        let remaining = list_scratch(&fs, &paths, "doc1").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "cand0");
        assert_eq!(
            remaining[0].status,
            CandidateStatus::Selected,
            "cand0 history lines survived; resolved status must be Selected"
        );
    }

    #[test]
    fn put_after_status_change_gets_correct_next_seq() {
        let (fs, paths) = setup();
        let clk = clock();

        put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-a",
                snapshot: b"zen A",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();
        set_candidate_status(
            &fs,
            &paths,
            &clk,
            "doc1",
            "cand0",
            CandidateStatus::Selected,
        )
        .unwrap();

        let e1 = put_scratch(
            &fs,
            &paths,
            &clk,
            "doc1",
            NewCandidate {
                page_id: "page-b",
                snapshot: b"zen B",
                status: CandidateStatus::Draft,
                meta: CandidateMeta::default(),
            },
        )
        .unwrap();

        assert_eq!(e1.seq, 1, "seq must be 1 (one distinct candidate existed)");
        assert_eq!(e1.id, "cand1");
    }
}
