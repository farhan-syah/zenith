//! Agent-run provenance records: schema and append-only JSONL log.
//!
//! Each [`RunRecord`] captures the intent, steps, and output hash of one
//! agent invocation. Records are written by the caller after the run
//! completes; this module performs no clock reads or hash computation —
//! those values arrive pre-computed on the record.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adapter::Fs;
use crate::error::SessionError;
use crate::layout::StorePaths;
use crate::manifest::{append_jsonl_record, read_jsonl_records};

// ── RunDiagnostic ─────────────────────────────────────────────────────────────

/// A single diagnostic emitted during a run step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunDiagnostic {
    /// Severity level (e.g. `"error"`, `"warning"`, `"info"`).
    pub severity: String,
    /// Machine-readable diagnostic code (e.g. `"font.glyph_missing"`).
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
}

// ── RunStep ───────────────────────────────────────────────────────────────────

/// One discrete step within a [`RunRecord`].
///
/// A step corresponds to a single action invocation. The `params` map holds a
/// flat representation of the action's inputs: each value is the caller's
/// canonical string form of the original typed value (the store holds no KDL
/// types; callers are responsible for converting their typed values to a
/// display string before recording).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStep {
    /// Stable step id (unique within its run).
    pub id: String,
    /// Parent step id in the step DAG (None for root steps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Name of the action invoked (e.g. `"move_node"`, `"apply_style"`).
    pub action: String,
    /// Optional version pin for `action` (e.g. an action revision string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_version: Option<String>,
    /// Optional content hash of the action definition itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_hash: Option<String>,
    /// Flat map of action parameters. Values are caller-supplied display
    /// strings; the store does not interpret them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// Ids of document nodes affected by this step (display only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_nodes: Vec<String>,
    /// Diagnostics emitted during this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<RunDiagnostic>,
    /// Optional content hash of the source artifact this step consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

// ── RunRecord ─────────────────────────────────────────────────────────────────

/// A top-level agent-run provenance record appended to `runs.jsonl`.
///
/// The caller is responsible for computing `timestamp_ms` (unix milliseconds)
/// and `snapshot_hash` (the content hash of the document state produced by the
/// run) before calling [`append_run`]. This module performs no clock reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Stable run id (unique within a document's runs log).
    pub id: String,
    /// Monotonic sequence number within this log (0-based).
    pub seq: u64,
    /// Short human-readable description of what the agent was asked to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    /// Optional constraints or guardrails supplied to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<String>,
    /// Optional plan or reasoning trace produced before execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Ordered list of steps executed during this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<RunStep>,
    /// Unix timestamp in milliseconds at which the run completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
    /// Content hash of the document state this run produced (into `objects/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
}

// ── I/O ───────────────────────────────────────────────────────────────────────

/// Append one agent-run record to the document's runs log.
///
/// Creates the log file and its parent directory if they do not yet exist.
pub fn append_run(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    record: &RunRecord,
) -> Result<(), SessionError> {
    append_jsonl_record(fs, &paths.runs_file(doc_id), record)
}

/// Read all agent-run records for a document in append order.
///
/// Returns an empty vec when no runs log exists for the document.
pub fn read_runs(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<Vec<RunRecord>, SessionError> {
    read_jsonl_records(fs, &paths.runs_file(doc_id))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::adapter::MemFs;
    use crate::layout::StorePaths;

    fn paths() -> StorePaths {
        StorePaths::new("/data")
    }

    fn make_fs() -> MemFs {
        MemFs::new()
    }

    fn full_step(id: &str) -> RunStep {
        let mut params = BTreeMap::new();
        params.insert("x".to_string(), "10".to_string());
        params.insert("y".to_string(), "20".to_string());
        RunStep {
            id: id.to_string(),
            parent: None,
            action: "move_node".to_string(),
            action_version: Some("rev-2".to_string()),
            action_hash: Some("acthash42".to_string()),
            params,
            affected_nodes: vec!["node-a".to_string(), "node-b".to_string()],
            diagnostics: vec![RunDiagnostic {
                severity: "warning".to_string(),
                code: "font.glyph_missing".to_string(),
                message: "glyph U+FFFD not found".to_string(),
            }],
            source_hash: Some("src123".to_string()),
        }
    }

    fn minimal_step(id: &str) -> RunStep {
        RunStep {
            id: id.to_string(),
            parent: None,
            action: "noop".to_string(),
            action_version: None,
            action_hash: None,
            params: BTreeMap::new(),
            affected_nodes: Vec::new(),
            diagnostics: Vec::new(),
            source_hash: None,
        }
    }

    #[test]
    fn append_then_read_runs_roundtrip() {
        let fs = make_fs();
        let paths = paths();

        let r0 = RunRecord {
            id: "run-0".to_string(),
            seq: 0,
            brief: Some("move two nodes".to_string()),
            constraints: None,
            plan: Some("step A then step B".to_string()),
            steps: vec![full_step("s0"), minimal_step("s1")],
            timestamp_ms: Some(1_700_000_000_100),
            snapshot_hash: Some("snap0".to_string()),
        };
        let r1 = RunRecord {
            id: "run-1".to_string(),
            seq: 1,
            brief: None,
            constraints: Some("read-only".to_string()),
            plan: None,
            steps: Vec::new(),
            timestamp_ms: Some(1_700_000_001_000),
            snapshot_hash: None,
        };

        append_run(&fs, &paths, "doc1", &r0).unwrap();
        append_run(&fs, &paths, "doc1", &r1).unwrap();

        let records = read_runs(&fs, &paths, "doc1").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], r0);
        assert_eq!(records[1], r1);
    }

    #[test]
    fn lean_run_omits_optionals() {
        let fs = make_fs();
        let paths = paths();

        let rec = RunRecord {
            id: "run-lean".to_string(),
            seq: 0,
            brief: None,
            constraints: None,
            plan: None,
            steps: Vec::new(),
            timestamp_ms: None,
            snapshot_hash: None,
        };

        append_run(&fs, &paths, "doc1", &rec).unwrap();

        let raw = fs.read(&paths.runs_file("doc1")).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();

        assert!(!line.contains("brief"), "brief must be absent in lean form");
        assert!(
            !line.contains("constraints"),
            "constraints must be absent in lean form"
        );
        assert!(!line.contains("plan"), "plan must be absent in lean form");
        assert!(!line.contains("steps"), "steps must be absent in lean form");
        assert!(
            !line.contains("timestamp_ms"),
            "timestamp_ms must be absent in lean form"
        );
        assert!(
            !line.contains("snapshot_hash"),
            "snapshot_hash must be absent in lean form"
        );
        assert!(line.contains("\"id\""), "id must be present");
        assert!(line.contains("\"seq\""), "seq must be present");
    }

    #[test]
    fn old_run_line_without_new_fields_deserializes() {
        let fs = make_fs();
        let paths = paths();

        // Simulate a JSONL line written before optional fields existed.
        let old_line = b"{\"id\":\"run-old\",\"seq\":3}\n";
        let run_path = paths.runs_file("doc1");
        fs.create_dir_all(run_path.parent().unwrap()).unwrap();
        fs.write(&run_path, old_line).unwrap();

        let records = read_runs(&fs, &paths, "doc1").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "run-old");
        assert_eq!(records[0].seq, 3);
        assert_eq!(records[0].brief, None);
        assert_eq!(records[0].constraints, None);
        assert_eq!(records[0].plan, None);
        assert!(records[0].steps.is_empty());
        assert_eq!(records[0].timestamp_ms, None);
        assert_eq!(records[0].snapshot_hash, None);
    }

    #[test]
    fn read_runs_absent_is_empty() {
        let fs = make_fs();
        let paths = paths();

        let records = read_runs(&fs, &paths, "no-such-doc").unwrap();
        assert!(records.is_empty());
    }
}
