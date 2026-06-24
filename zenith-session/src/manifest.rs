//! History records and the append-only JSONL manifest.
//!
//! A record's SUBSTANCE is `snapshot` — the content-addressed object hash of the
//! full `.zen` state at this point. Restore replaces the working file with that
//! snapshot; the operation/label fields are OPTIONAL display metadata and are
//! never required to reconstruct state (most edits — GUI drags, hand-edits, git
//! checkouts — carry no operation, so an op-log could not capture them).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::adapter::Fs;
use crate::error::SessionError;

// ── CheckpointMeta ────────────────────────────────────────────────────────────

/// Optional agent-checkpoint metadata attached to a durable version record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckpointMeta {
    /// Id of the agent action that produced this record.
    pub action_id: Option<String>,
    /// Version pin for `action_id` (e.g. an action revision string).
    pub action_version: Option<String>,
    /// Content hash linking this record to a rendered preview artifact.
    pub preview_hash: Option<String>,
    /// Whether this record can be deterministically re-run.
    pub replay_eligible: bool,
}

// ── Record ────────────────────────────────────────────────────────────────────

/// A single entry in the history manifest.
///
/// The `snapshot` field is the substance: the content-addressed object hash of
/// the full document state at this point in history. All other fields are
/// optional display metadata and are never required to reconstruct state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryRecord {
    /// Stable record id (unique within a manifest).
    pub id: String,
    /// Monotonic sequence number within this manifest (0-based).
    pub seq: u64,
    /// Parent record id in the history DAG (None for the first record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// THE SUBSTANCE: object hash of the full snapshot for this record.
    pub snapshot: String,
    /// Optional label for the kind of operation that produced this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_kind: Option<String>,
    /// Optional human-facing label / version name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional list of affected node ids (display only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected: Vec<String>,
    /// Optional unix-ms timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    /// Optional author/participant id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Id of the agent action that produced this record (agent checkpoints only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    /// Version pin for `action_id` (e.g. an action revision string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_version: Option<String>,
    /// Content hash linking this record to a rendered preview artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_hash: Option<String>,
    /// Whether this record can be deterministically re-run. Defaults to false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub replay_eligible: bool,
}

impl HistoryRecord {
    /// Construct a lean record: the required core (id, seq, parent, snapshot)
    /// with all optional metadata empty. Set the optional fields afterward.
    pub fn new(
        id: impl Into<String>,
        seq: u64,
        parent: Option<String>,
        snapshot: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            seq,
            parent,
            snapshot: snapshot.into(),
            op_kind: None,
            label: None,
            affected: Vec::new(),
            timestamp_ms: None,
            author: None,
            action_id: None,
            action_version: None,
            preview_hash: None,
            replay_eligible: false,
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ── Manifest I/O ──────────────────────────────────────────────────────────────

/// Append one serde-serializable record as a JSON line to `path`, creating the
/// file and its parent directory if needed.
pub(crate) fn append_jsonl_record<T: serde::Serialize>(
    fs: &impl Fs,
    path: &Path,
    record: &T,
) -> Result<(), SessionError> {
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(record)
        .map_err(|e| SessionError::new(format!("serialize record: {e}")))?;
    line.push(b'\n');
    fs.append(path, &line)
}

/// Read all JSON-line records of type `T` from `path`. Returns an empty vec if
/// the file is absent. Blank lines are skipped; a malformed line is a hard error.
pub(crate) fn read_jsonl_records<T: serde::de::DeserializeOwned>(
    fs: &impl Fs,
    path: &Path,
) -> Result<Vec<T>, SessionError> {
    if !fs.exists(path) {
        return Ok(Vec::new());
    }
    let bytes = fs.read(path)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| SessionError::new(format!("manifest is not utf-8: {e}")))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let rec = serde_json::from_str(line)
            .map_err(|e| SessionError::new(format!("parse record: {e}")))?;
        out.push(rec);
    }
    Ok(out)
}

/// Append one record to the JSONL manifest at `path`, creating the file (and its
/// parent directory) if needed. Each record is written as a single JSON line.
pub fn append_record(
    fs: &impl Fs,
    path: &Path,
    record: &HistoryRecord,
) -> Result<(), SessionError> {
    append_jsonl_record(fs, path, record)
}

/// Read and parse every record from the JSONL manifest at `path`. Returns an
/// empty vec if the manifest does not exist. Blank lines are skipped; a malformed
/// line is a hard error (the manifest is corrupt).
pub fn read_records(fs: &impl Fs, path: &Path) -> Result<Vec<HistoryRecord>, SessionError> {
    read_jsonl_records::<HistoryRecord>(fs, path)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::adapter::{Fs, MemFs};

    fn manifest_path() -> PathBuf {
        PathBuf::from("/data/m.jsonl")
    }

    fn make_fs() -> MemFs {
        MemFs::new()
    }

    #[test]
    fn append_then_read_one() {
        let fs = make_fs();
        let path = manifest_path();
        let rec = HistoryRecord::new("r0", 0, None, "deadbeef");
        append_record(&fs, &path, &rec).unwrap();
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], rec);
    }

    #[test]
    fn append_multiple_preserves_order() {
        let fs = make_fs();
        let path = manifest_path();
        let r0 = HistoryRecord::new("r0", 0, None, "aaa");
        let r1 = HistoryRecord::new("r1", 1, Some("r0".to_string()), "bbb");
        let r2 = HistoryRecord::new("r2", 2, Some("r1".to_string()), "ccc");
        append_record(&fs, &path, &r0).unwrap();
        append_record(&fs, &path, &r1).unwrap();
        append_record(&fs, &path, &r2).unwrap();
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0], r0);
        assert_eq!(records[1], r1);
        assert_eq!(records[2], r2);
    }

    #[test]
    fn read_missing_is_empty() {
        let fs = make_fs();
        let path = PathBuf::from("/nonexistent/m.jsonl");
        let records = read_records(&fs, &path).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn lean_record_omits_optionals() {
        let fs = make_fs();
        let path = manifest_path();
        let rec = HistoryRecord::new("r0", 0, None, "cafebabe");
        append_record(&fs, &path, &rec).unwrap();
        let raw = fs.read(&path).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();
        assert!(
            !line.contains("op_kind"),
            "op_kind must be absent in lean form"
        );
        assert!(!line.contains("label"), "label must be absent in lean form");
        assert!(
            !line.contains("affected"),
            "affected must be absent in lean form"
        );
        assert!(
            !line.contains("timestamp_ms"),
            "timestamp_ms must be absent in lean form"
        );
        assert!(
            !line.contains("author"),
            "author must be absent in lean form"
        );
        assert!(
            !line.contains("action_id"),
            "action_id must be absent in lean form"
        );
        assert!(
            !line.contains("action_version"),
            "action_version must be absent in lean form"
        );
        assert!(
            !line.contains("preview_hash"),
            "preview_hash must be absent in lean form"
        );
        assert!(
            !line.contains("replay_eligible"),
            "replay_eligible must be absent in lean form"
        );
        assert!(line.contains("\"snapshot\""), "snapshot must be present");
        assert!(line.contains("\"seq\""), "seq must be present");
    }

    #[test]
    fn checkpoint_record_roundtrips() {
        let fs = make_fs();
        let path = manifest_path();
        let mut rec = HistoryRecord::new("cp0", 0, None, "cafef00d");
        rec.action_id = Some("act-1".to_string());
        rec.action_version = Some("rev-3".to_string());
        rec.preview_hash = Some("preview123".to_string());
        rec.replay_eligible = true;
        append_record(&fs, &path, &rec).unwrap();
        let raw = fs.read(&path).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();
        assert!(line.contains("action_id"), "action_id must appear when set");
        assert!(
            line.contains("action_version"),
            "action_version must appear when set"
        );
        assert!(
            line.contains("preview_hash"),
            "preview_hash must appear when set"
        );
        assert!(
            line.contains("replay_eligible"),
            "replay_eligible must appear when true"
        );
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], rec);
    }

    #[test]
    fn old_manifest_without_checkpoint_fields_deserializes() {
        let fs = make_fs();
        let path = manifest_path();
        // A JSONL line as produced before the checkpoint fields were added.
        let old_line = b"{\"id\":\"r0\",\"seq\":0,\"snapshot\":\"oldhash\",\"op_kind\":\"edit\"}\n";
        fs.create_dir_all(path.parent().unwrap()).unwrap();
        fs.write(&path, old_line).unwrap();
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action_id, None);
        assert_eq!(records[0].action_version, None);
        assert_eq!(records[0].preview_hash, None);
        assert!(!records[0].replay_eligible);
    }

    #[test]
    fn full_record_roundtrips() {
        let fs = make_fs();
        let path = manifest_path();
        let rec = HistoryRecord {
            id: "full".to_string(),
            seq: 7,
            parent: Some("prev".to_string()),
            snapshot: "abc123".to_string(),
            op_kind: Some("move".to_string()),
            label: Some("v2".to_string()),
            affected: vec!["node-a".to_string(), "node-b".to_string()],
            timestamp_ms: Some(1_700_000_000_000),
            author: Some("alice".to_string()),
            action_id: Some("act-42".to_string()),
            action_version: Some("rev-7".to_string()),
            preview_hash: Some("deadbeef".to_string()),
            replay_eligible: true,
        };
        append_record(&fs, &path, &rec).unwrap();
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], rec);
    }

    #[test]
    fn blank_lines_skipped() {
        let fs = make_fs();
        let path = manifest_path();
        let rec = HistoryRecord::new("r0", 0, None, "deadbeef");
        append_record(&fs, &path, &rec).unwrap();
        // Inject blank lines directly.
        fs.append(&path, b"\n  \n").unwrap();
        let records = read_records(&fs, &path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], rec);
    }

    #[test]
    fn malformed_line_errors() {
        let fs = make_fs();
        let path = manifest_path();
        // Create the parent dir and write corrupt content.
        fs.create_dir_all(path.parent().unwrap()).unwrap();
        fs.write(&path, b"{not json}\n").unwrap();
        let result = read_records(&fs, &path);
        assert!(result.is_err(), "expected error on malformed JSON line");
    }
}
