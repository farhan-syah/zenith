//! Preview-artifact records: schema and append-only JSONL log.
//!
//! Each [`PreviewRecord`] captures the source, output path, and critique
//! annotations of one rendered preview artifact. Records are written by the
//! caller after the render completes; this module performs no clock reads or
//! hash computation — those values arrive pre-computed on the record.

use serde::{Deserialize, Serialize};

use crate::adapter::Fs;
use crate::error::SessionError;
use crate::layout::StorePaths;
use crate::manifest::{append_jsonl_record, read_jsonl_records};

// ── PreviewCritique ───────────────────────────────────────────────────────────

/// A single critique annotation attached to a preview artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewCritique {
    /// Severity level (e.g. `"error"`, `"warning"`, `"info"`).
    pub severity: String,
    /// Machine-readable critique code (e.g. `"contrast.too_low"`).
    pub code: String,
    /// Human-readable critique message.
    pub message: String,
}

// ── PreviewRecord ─────────────────────────────────────────────────────────────

/// A top-level preview-artifact record appended to `previews.jsonl`.
///
/// The caller is responsible for computing `timestamp_ms` (unix milliseconds),
/// `source_hash`, `output_hash`, and `output` (the rendered file path) before
/// calling [`append_preview`]. This module performs no clock reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewRecord {
    /// Stable preview id (unique within a document's previews log).
    pub id: String,
    /// Monotonic sequence number within this log (0-based).
    pub seq: u64,
    /// Id of the page this preview is a rendering of.
    pub candidate_page_id: String,
    /// Optional content hash of the source artifact consumed to produce this preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Optional output file path of the rendered preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Optional content hash of the rendered output file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_hash: Option<String>,
    /// Optional id of the parent revision this preview was derived from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<String>,
    /// Critique annotations attached to this preview.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub critiques: Vec<PreviewCritique>,
    /// Unix timestamp in milliseconds at which the preview was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u128>,
}

// ── I/O ───────────────────────────────────────────────────────────────────────

/// Append one preview-artifact record to the document's previews log.
///
/// Creates the log file and its parent directory if they do not yet exist.
pub fn append_preview(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    record: &PreviewRecord,
) -> Result<(), SessionError> {
    append_jsonl_record(fs, &paths.previews_file(doc_id), record)
}

/// Read all preview-artifact records for a document in append order.
///
/// Returns an empty vec when no previews log exists for the document.
pub fn read_previews(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
) -> Result<Vec<PreviewRecord>, SessionError> {
    read_jsonl_records(fs, &paths.previews_file(doc_id))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MemFs;
    use crate::layout::StorePaths;

    fn paths() -> StorePaths {
        StorePaths::new("/data")
    }

    fn make_fs() -> MemFs {
        MemFs::new()
    }

    #[test]
    fn append_then_read_previews_roundtrip() {
        let fs = make_fs();
        let paths = paths();

        let p0 = PreviewRecord {
            id: "prev-0".to_string(),
            seq: 0,
            candidate_page_id: "page-a".to_string(),
            source_hash: Some("srchash0".to_string()),
            output: Some("/out/prev-0.png".to_string()),
            output_hash: Some("outhash0".to_string()),
            parent_revision: Some("rev-1".to_string()),
            critiques: vec![
                PreviewCritique {
                    severity: "warning".to_string(),
                    code: "contrast.too_low".to_string(),
                    message: "contrast ratio below 4.5:1".to_string(),
                },
                PreviewCritique {
                    severity: "info".to_string(),
                    code: "layout.overflow".to_string(),
                    message: "text overflows bounding box by 2px".to_string(),
                },
            ],
            timestamp_ms: Some(1_700_000_000_200),
        };
        let p1 = PreviewRecord {
            id: "prev-1".to_string(),
            seq: 1,
            candidate_page_id: "page-b".to_string(),
            source_hash: None,
            output: None,
            output_hash: None,
            parent_revision: None,
            critiques: Vec::new(),
            timestamp_ms: None,
        };

        append_preview(&fs, &paths, "doc1", &p0).unwrap();
        append_preview(&fs, &paths, "doc1", &p1).unwrap();

        let records = read_previews(&fs, &paths, "doc1").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], p0);
        assert_eq!(records[1], p1);
    }

    #[test]
    fn lean_preview_omits_optionals() {
        let fs = make_fs();
        let paths = paths();

        let rec = PreviewRecord {
            id: "prev-lean".to_string(),
            seq: 0,
            candidate_page_id: "page-c".to_string(),
            source_hash: None,
            output: None,
            output_hash: None,
            parent_revision: None,
            critiques: Vec::new(),
            timestamp_ms: None,
        };

        append_preview(&fs, &paths, "doc1", &rec).unwrap();

        let raw = fs.read(&paths.previews_file("doc1")).unwrap();
        let line = std::str::from_utf8(&raw).unwrap();

        assert!(
            !line.contains("source_hash"),
            "source_hash must be absent in lean form"
        );
        assert!(
            !line.contains("output"),
            "output must be absent in lean form"
        );
        assert!(
            !line.contains("output_hash"),
            "output_hash must be absent in lean form"
        );
        assert!(
            !line.contains("parent_revision"),
            "parent_revision must be absent in lean form"
        );
        assert!(
            !line.contains("critiques"),
            "critiques must be absent in lean form"
        );
        assert!(
            !line.contains("timestamp_ms"),
            "timestamp_ms must be absent in lean form"
        );
        assert!(line.contains("\"id\""), "id must be present");
        assert!(line.contains("\"seq\""), "seq must be present");
        assert!(
            line.contains("\"candidate_page_id\""),
            "candidate_page_id must be present"
        );
    }

    #[test]
    fn old_preview_line_without_new_fields_deserializes() {
        let fs = make_fs();
        let paths = paths();

        // Simulate a JSONL line written before optional fields existed.
        let old_line = b"{\"id\":\"prev-old\",\"seq\":3,\"candidate_page_id\":\"page-z\"}\n";
        let preview_path = paths.previews_file("doc1");
        fs.create_dir_all(preview_path.parent().unwrap()).unwrap();
        fs.write(&preview_path, old_line).unwrap();

        let records = read_previews(&fs, &paths, "doc1").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "prev-old");
        assert_eq!(records[0].seq, 3);
        assert_eq!(records[0].candidate_page_id, "page-z");
        assert_eq!(records[0].source_hash, None);
        assert_eq!(records[0].output, None);
        assert_eq!(records[0].output_hash, None);
        assert_eq!(records[0].parent_revision, None);
        assert!(records[0].critiques.is_empty());
        assert_eq!(records[0].timestamp_ms, None);
    }

    #[test]
    fn read_previews_absent_is_empty() {
        let fs = make_fs();
        let paths = paths();

        let records = read_previews(&fs, &paths, "no-such-doc").unwrap();
        assert!(records.is_empty());
    }
}
