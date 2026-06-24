//! Result-shaping helpers shared by the MCP exec handlers: compact JSON
//! stringification and the link-vs-inline offload decision.
//!
//! Small results are inlined directly into `structuredContent`. Results over
//! [`INLINE_LIMIT`] are written to the content-addressed store and replaced with
//! a resource link, so the model context never carries large payloads. Binary
//! artifacts (renders) are always stored and linked, never inlined.

use serde_json::{Value, json};
use zenith_session::adapter::OsFs;
use zenith_session::put_object;

use super::resources::{self, ResourceMeta};

/// Inline budget (bytes of compact JSON). At/above this, a text result is
/// offloaded to a store object and returned as a link instead.
pub const INLINE_LIMIT: usize = 8 * 1024;

/// Serialise a value to compact (whitespace-free) JSON — the token-minimal mirror.
pub fn compact(value: &Value) -> String {
    crate::commands::serialize_compact(value)
}

/// Store `bytes` as an immutable object for `doc_id` and return a resource-link
/// value `{uri, mimeType, name, size}`, registering it for `resources/list`.
///
/// Used for binary render artifacts (always linked) and for offloaded large text.
pub fn store_link(doc_id: &str, bytes: &[u8], ext: &str, name: &str) -> Result<Value, String> {
    let paths = resources::open_store()?;
    let fs = OsFs;
    let sha = put_object(&fs, &paths, doc_id, bytes).map_err(|e| e.message)?;
    let uri = resources::blob_uri(doc_id, &sha, ext);
    let (mime, _) = resources::mime_for_ext(ext);
    let size = bytes.len() as u64;
    resources::register(
        &uri,
        ResourceMeta {
            mime: mime.to_owned(),
            name: name.to_owned(),
            size,
        },
    );
    Ok(json!({ "uri": uri, "mimeType": mime, "name": name, "size": size }))
}

/// Return `value` inline when small (or when no `doc_id` is available to offload
/// to); otherwise store it as a `<ext>` object and return `{resource, offloaded}`.
///
/// Read-only tools pass the document's `doc_id` only when it already exists —
/// they never mint identity merely to offload, so an unidentified document's
/// large result is simply inlined.
pub fn maybe_offload(doc_id: Option<&str>, value: Value, ext: &str, name: &str) -> Value {
    let text = compact(&value);
    match doc_id {
        Some(id) if text.len() >= INLINE_LIMIT => {
            match store_link(id, text.as_bytes(), ext, name) {
                Ok(link) => json!({ "resource": link, "offloaded": true }),
                // On any store failure, fall back to inlining — correctness over thrift.
                Err(_) => value,
            }
        }
        _ => value,
    }
}
