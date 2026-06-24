//! Tier-2 durable version history: a bounded, flat list of state snapshots in
//! `versions.jsonl` (the "light git per file" — a Google-Docs-style version
//! panel, NOT a full VCS: no branches/merge). Each version is a full content
//! snapshot (content-addressed); named versions carry a label. Restore returns
//! a past version's content for the caller to write back to the `.zen`.

use std::time::UNIX_EPOCH;

use crate::adapter::{Clock, Fs};
use crate::error::SessionError;
use crate::layout::StorePaths;
use crate::manifest::{CheckpointMeta, HistoryRecord, append_record, read_records};
use crate::revspec::resolve_revspec;
use crate::session::find_record;
use crate::store::{get_object, object_hash, put_object_with_hash};

// ── Outcome ───────────────────────────────────────────────────────────────────

/// The outcome of a [`record_version`] call.
#[derive(Debug, Clone, PartialEq)]
pub enum VersionOutcome {
    /// Content was byte-identical to the latest version; no new version created.
    Unchanged,
    /// A new version was recorded.
    Recorded { id: String },
}

// ── Metadata ──────────────────────────────────────────────────────────────────

/// Metadata recorded alongside a durable version: an optional human label, an
/// optional op-kind tag, and optional agent-checkpoint metadata.
#[derive(Debug, Clone, Copy, Default)]
pub struct VersionMeta<'a> {
    pub label: Option<&'a str>,
    pub op_kind: Option<&'a str>,
    pub checkpoint: Option<&'a CheckpointMeta>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// List all durable versions for `doc_id`, oldest first (append order).
pub fn list_versions(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<Vec<HistoryRecord>, SessionError> {
    read_records(fs, &paths.versions_file(doc_id))
}

/// Record `content` as a new durable version. `meta.label` names it (a named
/// version is retained forever by the retention pass). `meta.op_kind` is an
/// optional category label. UNNAMED (auto) versions deduplicate against the
/// LATEST version: if `content` is byte-identical to it, the call returns
/// `Unchanged` and appends nothing. A NAMED version (`meta.label.is_some()`) is
/// an explicit checkpoint and is ALWAYS recorded, even when its content matches
/// the latest version.
pub fn record_version(
    fs: &impl Fs,
    paths: &StorePaths,
    clock: &impl Clock,
    doc_id: &str,
    content: &[u8],
    meta: VersionMeta<'_>,
) -> Result<VersionOutcome, SessionError> {
    let vpath = paths.versions_file(doc_id);
    let versions = read_records(fs, &vpath)?;
    let new_hash = object_hash(content);

    // Dedup auto (unnamed) versions against the latest version (highest seq =
    // last appended). A NAMED version is an explicit checkpoint and is always
    // recorded, even when its content matches the latest version — the object
    // store still dedups the bytes, so only a lightweight record is added.
    if meta.label.is_none()
        && let Some(last) = versions.last()
        && last.snapshot == new_hash
    {
        return Ok(VersionOutcome::Unchanged);
    }

    // Store at the address we already computed for the dedup check above.
    put_object_with_hash(fs, paths, doc_id, content, &new_hash)?;
    let seq = u64::try_from(versions.len())
        .map_err(|_| SessionError::new("version count exceeds u64"))?;
    let id = format!("v{seq}");
    let parent = versions.last().map(|r| r.id.clone());
    let mut rec = HistoryRecord::new(id.clone(), seq, parent, new_hash);
    rec.label = meta.label.map(str::to_owned);
    rec.op_kind = meta.op_kind.map(str::to_owned);
    rec.timestamp_ms = clock
        .now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis());
    if let Some(cm) = meta.checkpoint {
        rec.action_id = cm.action_id.clone();
        rec.action_version = cm.action_version.clone();
        rec.preview_hash = cm.preview_hash.clone();
        rec.replay_eligible = cm.replay_eligible;
    }
    append_record(fs, &vpath, &rec)?;
    Ok(VersionOutcome::Recorded { id })
}

/// The content of the version with the given id.
pub fn version_content(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    version_id: &str,
) -> Result<Vec<u8>, SessionError> {
    let versions = read_records(fs, &paths.versions_file(doc_id))?;
    let rec = find_record(&versions, version_id)
        .ok_or_else(|| SessionError::new(format!("no version with id {version_id}")))?;
    get_object(fs, paths, doc_id, &rec.snapshot)
}

/// Resolve a revspec against the version list (HEAD = the latest version) to a
/// version id. Supports the same forms as the session resolver (`@head`,
/// `@head~N`, seq, `@latest:<label>`, id/prefix, `@time:`).
pub fn resolve_version(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    spec: &str,
) -> Result<String, SessionError> {
    let versions = read_records(fs, &paths.versions_file(doc_id))?;
    let head = versions.last().map(|r| r.id.as_str());
    resolve_revspec(&versions, head, spec)
}

/// Restore: resolve `spec` to a version and return its content (the caller writes
/// it back to the `.zen`). Does NOT itself mutate the working file or record a new
/// version — that is the caller's responsibility so the write-through stays in one
/// place.
pub fn restore_content(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    spec: &str,
) -> Result<Vec<u8>, SessionError> {
    let id = resolve_version(fs, paths, doc_id, spec)?;
    version_content(fs, paths, doc_id, &id)
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

    fn clock_at(ms: u64) -> FakeClock {
        FakeClock(UNIX_EPOCH + Duration::from_millis(ms))
    }

    #[test]
    fn first_version_recorded() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        let outcome =
            record_version(&fs, &paths, &clock, "doc1", b"v1", VersionMeta::default()).unwrap();
        assert_eq!(
            outcome,
            VersionOutcome::Recorded {
                id: "v0".to_owned()
            }
        );
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(version_content(&fs, &paths, "doc1", "v0").unwrap(), b"v1");
    }

    #[test]
    fn dedup_latest() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(&fs, &paths, &clock, "doc1", b"v1", VersionMeta::default()).unwrap();
        let second =
            record_version(&fs, &paths, &clock, "doc1", b"v1", VersionMeta::default()).unwrap();
        assert_eq!(second, VersionOutcome::Unchanged);
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn named_version_not_deduped_when_content_matches() {
        // Naming a checkpoint must always record a version, even when the content
        // is identical to the latest auto-version (the label must not be dropped).
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(&fs, &paths, &clock, "doc1", b"v1", VersionMeta::default()).unwrap();
        let named = record_version(
            &fs,
            &paths,
            &clock,
            "doc1",
            b"v1",
            VersionMeta {
                label: Some("release-1"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            named,
            VersionOutcome::Recorded {
                id: "v1".to_owned()
            }
        );
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(
            versions.len(),
            2,
            "named checkpoint must append a new record"
        );
        assert_eq!(versions[1].label, Some("release-1".to_owned()));
        // Both records share the same underlying object (bytes deduped in store).
        assert_eq!(versions[0].snapshot, versions[1].snapshot);
    }

    #[test]
    fn second_version_chains_parent() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(&fs, &paths, &clock, "doc1", b"v1", VersionMeta::default()).unwrap();
        record_version(&fs, &paths, &clock, "doc1", b"v2", VersionMeta::default()).unwrap();
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[1].parent, Some("v0".to_owned()));
        assert_eq!(version_content(&fs, &paths, "doc1", "v1").unwrap(), b"v2");
    }

    #[test]
    fn named_version_stores_label() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(
            &fs,
            &paths,
            &clock,
            "doc1",
            b"v1",
            VersionMeta {
                label: Some("release-1.0"),
                ..Default::default()
            },
        )
        .unwrap();
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions[0].label, Some("release-1.0".to_owned()));
    }

    #[test]
    fn resolve_version_forms() {
        let (fs, paths) = setup();
        // v0 at 100ms, v1 at 200ms (label "rc1"), v2 at 300ms
        record_version(
            &fs,
            &paths,
            &clock_at(100),
            "doc1",
            b"content-0",
            VersionMeta::default(),
        )
        .unwrap();
        record_version(
            &fs,
            &paths,
            &clock_at(200),
            "doc1",
            b"content-1",
            VersionMeta {
                label: Some("rc1"),
                ..Default::default()
            },
        )
        .unwrap();
        record_version(
            &fs,
            &paths,
            &clock_at(300),
            "doc1",
            b"content-2",
            VersionMeta::default(),
        )
        .unwrap();

        assert_eq!(resolve_version(&fs, &paths, "doc1", "@head").unwrap(), "v2");
        assert_eq!(
            resolve_version(&fs, &paths, "doc1", "@head~1").unwrap(),
            "v1"
        );
        assert_eq!(resolve_version(&fs, &paths, "doc1", "1").unwrap(), "v1");
        assert_eq!(
            resolve_version(&fs, &paths, "doc1", "@latest:rc1").unwrap(),
            "v1"
        );
    }

    #[test]
    fn restore_content_returns_past_bytes() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(&fs, &paths, &clock, "doc1", b"A", VersionMeta::default()).unwrap();
        record_version(&fs, &paths, &clock, "doc1", b"B", VersionMeta::default()).unwrap();
        assert_eq!(
            restore_content(&fs, &paths, "doc1", "@head~1").unwrap(),
            b"A"
        );
        assert_eq!(restore_content(&fs, &paths, "doc1", "v1").unwrap(), b"B");
    }

    #[test]
    fn restore_unknown_errors() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(&fs, &paths, &clock, "doc1", b"A", VersionMeta::default()).unwrap();
        assert!(restore_content(&fs, &paths, "doc1", "v99").is_err());
    }

    #[test]
    fn list_empty() {
        let (fs, paths) = setup();
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn checkpoint_metadata_is_persisted() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        let cm = CheckpointMeta {
            action_id: Some("act-99".to_string()),
            action_version: Some("rev-2".to_string()),
            preview_hash: Some("abc123".to_string()),
            replay_eligible: true,
        };
        record_version(
            &fs,
            &paths,
            &clock,
            "doc1",
            b"content",
            VersionMeta {
                checkpoint: Some(&cm),
                ..Default::default()
            },
        )
        .unwrap();
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].action_id, Some("act-99".to_string()));
        assert_eq!(versions[0].action_version, Some("rev-2".to_string()));
        assert_eq!(versions[0].preview_hash, Some("abc123".to_string()));
        assert!(versions[0].replay_eligible);
    }

    #[test]
    fn no_checkpoint_leaves_fields_unset() {
        let (fs, paths) = setup();
        let clock = clock_at(100);
        record_version(
            &fs,
            &paths,
            &clock,
            "doc1",
            b"content",
            VersionMeta::default(),
        )
        .unwrap();
        let versions = list_versions(&fs, &paths, "doc1").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].action_id, None);
        assert_eq!(versions[0].action_version, None);
        assert_eq!(versions[0].preview_hash, None);
        assert!(!versions[0].replay_eligible);
    }
}
