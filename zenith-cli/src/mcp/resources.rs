//! MCP `resources` capability: a content-addressed handle scheme over the
//! `zenith-session` store, plus an in-process registry of resources minted this
//! session.
//!
//! Large or binary tool outputs (rendered PNG/PDF, big scene/inspect JSON,
//! transaction diffs, candidate snapshots) are written to the immutable store
//! and surfaced to the agent as a resource *link* — a `{uri, mimeType, name,
//! size}` handle — instead of being inlined into the model context. The agent
//! reads them with `resources/read` only when it actually needs the bytes.
//!
//! ## URI scheme
//!
//! - `zenith://doc/<doc_id>/blob/<sha256>.<ext>` — an immutable store object.
//!   Content-addressed, so the link survives `workspace finalize` cleanup.
//! - `zenith://doc/<doc_id>/candidate/<candidate_id>` — alias resolving to a
//!   scratch candidate's snapshot object.
//!
//! `resources/list` returns the session registry (deterministic, sorted by URI
//! via [`BTreeMap`]); `resources/read` resolves a URI to its bytes on demand.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use serde_json::{Value, json};
use zenith_session::adapter::OsFs;
use zenith_session::{StorePaths, get_object, list_scratch, resolve_data_dir};

use super::base64;

/// Metadata recorded for one minted resource.
#[derive(Clone)]
pub struct ResourceMeta {
    pub mime: String,
    pub name: String,
    pub size: u64,
}

/// Session-scoped registry of resources produced by tool calls. Keyed by URI so
/// `resources/list` is deterministic. A poisoned lock degrades to "no
/// resources" rather than panicking (lib code never unwraps).
static REGISTRY: LazyLock<Mutex<BTreeMap<String, ResourceMeta>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Record a minted resource so it appears in `resources/list`.
pub fn register(uri: &str, meta: ResourceMeta) {
    if let Ok(mut guard) = REGISTRY.lock() {
        guard.insert(uri.to_owned(), meta);
    }
}

/// Build a `zenith://` blob URI for a stored object.
pub fn blob_uri(doc_id: &str, sha: &str, ext: &str) -> String {
    format!("zenith://doc/{doc_id}/blob/{sha}.{ext}")
}

/// The `resources/list` result payload (sorted by URI; session-scoped).
pub fn list_payload() -> Value {
    let mut resources = Vec::new();
    if let Ok(guard) = REGISTRY.lock() {
        for (uri, meta) in guard.iter() {
            resources.push(json!({
                "uri": uri,
                "name": meta.name,
                "mimeType": meta.mime,
                "size": meta.size,
            }));
        }
    }
    json!({ "resources": resources })
}

/// The `resources/read` result payload for `uri`, or a human-readable error.
pub fn read_payload(uri: &str) -> Result<Value, String> {
    let parsed = parse_uri(uri)?;
    let paths = open_store()?;
    let fs = OsFs;

    let (bytes, mime, is_text) = match parsed {
        ParsedUri::Blob {
            doc_id, sha, ext, ..
        } => {
            let bytes = get_object(&fs, &paths, &doc_id, &sha).map_err(|e| e.message)?;
            let (mime, is_text) = mime_for_ext(&ext);
            (bytes, mime.to_owned(), is_text)
        }
        ParsedUri::Candidate { doc_id, candidate } => {
            let entries = list_scratch(&fs, &paths, &doc_id).map_err(|e| e.message)?;
            let entry = entries
                .iter()
                .find(|e| e.id == candidate)
                .ok_or_else(|| format!("candidate not found: {candidate}"))?;
            let bytes =
                get_object(&fs, &paths, &doc_id, &entry.snapshot_hash).map_err(|e| e.message)?;
            (bytes, "text/plain".to_owned(), true)
        }
    };

    let content = if is_text {
        match String::from_utf8(bytes) {
            Ok(text) => json!({ "uri": uri, "mimeType": mime, "text": text }),
            // Declared text but not valid UTF-8: fall back to a binary blob.
            Err(e) => json!({
                "uri": uri,
                "mimeType": "application/octet-stream",
                "blob": base64::encode(e.as_bytes()),
            }),
        }
    } else {
        json!({ "uri": uri, "mimeType": mime, "blob": base64::encode(&bytes) })
    };

    Ok(json!({ "contents": [content] }))
}

// ── URI parsing ─────────────────────────────────────────────────────────────

/// A parsed `zenith://` resource URI. Exhaustive over the supported families.
enum ParsedUri {
    Blob {
        doc_id: String,
        sha: String,
        ext: String,
    },
    Candidate {
        doc_id: String,
        candidate: String,
    },
}

/// Parse a `zenith://doc/<doc_id>/<family>/<rest>` URI.
fn parse_uri(uri: &str) -> Result<ParsedUri, String> {
    let rest = uri
        .strip_prefix("zenith://doc/")
        .ok_or_else(|| format!("unsupported resource URI: {uri}"))?;
    let mut parts = rest.splitn(3, '/');
    let doc_id = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("malformed resource URI (no doc-id): {uri}"))?;
    let family = parts
        .next()
        .ok_or_else(|| format!("malformed resource URI (no family): {uri}"))?;
    let tail = parts
        .next()
        .ok_or_else(|| format!("malformed resource URI (no target): {uri}"))?;

    match family {
        "blob" => {
            let (sha, ext) = tail
                .rsplit_once('.')
                .ok_or_else(|| format!("malformed blob URI (no extension): {uri}"))?;
            Ok(ParsedUri::Blob {
                doc_id: doc_id.to_owned(),
                sha: sha.to_owned(),
                ext: ext.to_owned(),
            })
        }
        "candidate" => Ok(ParsedUri::Candidate {
            doc_id: doc_id.to_owned(),
            candidate: tail.to_owned(),
        }),
        other => Err(format!("unknown resource family '{other}' in {uri}")),
    }
}

/// Map a blob extension to its MIME type and whether it is UTF-8 text.
pub fn mime_for_ext(ext: &str) -> (&'static str, bool) {
    match ext {
        "png" => ("image/png", false),
        "pdf" => ("application/pdf", false),
        "json" => ("application/json", true),
        "zen" => ("text/plain", true),
        "diff" => ("text/plain", true),
        "txt" => ("text/plain", true),
        _ => ("application/octet-stream", false),
    }
}

/// Resolve the data directory into [`StorePaths`].
pub fn open_store() -> Result<StorePaths, String> {
    resolve_data_dir()
        .map(StorePaths::new)
        .map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blob_uri() {
        match parse_uri("zenith://doc/D1/blob/abc123.png") {
            Ok(ParsedUri::Blob { doc_id, sha, ext }) => {
                assert_eq!(doc_id, "D1");
                assert_eq!(sha, "abc123");
                assert_eq!(ext, "png");
            }
            _ => panic!("expected blob"),
        }
    }

    #[test]
    fn parses_candidate_uri() {
        match parse_uri("zenith://doc/D1/candidate/cand0") {
            Ok(ParsedUri::Candidate { doc_id, candidate }) => {
                assert_eq!(doc_id, "D1");
                assert_eq!(candidate, "cand0");
            }
            _ => panic!("expected candidate"),
        }
    }

    #[test]
    fn rejects_foreign_scheme() {
        assert!(parse_uri("file:///etc/passwd").is_err());
        assert!(parse_uri("zenith://doc/D1/bogus/x").is_err());
    }

    #[test]
    fn mime_table() {
        assert_eq!(mime_for_ext("png"), ("image/png", false));
        assert_eq!(mime_for_ext("json"), ("application/json", true));
        assert_eq!(mime_for_ext("zip").0, "application/octet-stream");
    }
}
