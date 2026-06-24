//! Portable, deterministic document-store bundle (`*.zenithbundle`).
//!
//! # Format
//!
//! ```text
//! [8 bytes]  magic: b"ZNBDL1\0\0"  (written raw, NOT compressed)
//! [remaining] single DEFLATE stream (flate2 ZlibEncoder/ZlibDecoder,
//!             pure-Rust miniz_oxide backend) over the following payload:
//!
//!   doc_id        : u32 LE length  +  UTF-8 bytes
//!   entry_count   : u32 LE
//!   for each entry (sorted ascending by relative path bytes):
//!     rel_path    : u32 LE length  +  UTF-8 bytes
//!                   (forward-slash separated; relative to doc_dir)
//!     content     : u64 LE length  +  raw bytes
//! ```
//!
//! All integers are little-endian. Determinism is guaranteed by collecting
//! all (relative_path, content) pairs from a depth-first walk of `doc_dir`,
//! then sorting the collected Vec by relative path bytes before serialising.
//! No timestamps or filesystem metadata are included.
//!
//! `unbundle` writes are not transactional: a partial write is possible if
//! the process is killed mid-way. A future unit may stage into a temp dir and
//! rename for atomicity.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::adapter::Fs;
use crate::error::SessionError;
use crate::layout::StorePaths;

// ── Magic constant ─────────────────────────────────────────────────────────────

/// 8-byte magic header written at the start of every bundle (raw, uncompressed).
const MAGIC: &[u8; 8] = b"ZNBDL1\0\0";

/// One bundled file: its store-relative path and raw content bytes.
type BundleEntry = (String, Vec<u8>);

// ── Public API ─────────────────────────────────────────────────────────────────

/// Pack a document's entire store directory into one portable, deterministic
/// byte blob.
///
/// The directory bundled is `<root>/docs/<doc_id>/` (objects/, versions.jsonl,
/// runs.jsonl, previews.jsonl, scratch/, meta.json, …). Returns an error if
/// `doc_dir` does not exist, since bundling a non-existent document is a user
/// mistake and an empty bundle would silently succeed with no useful data.
pub fn bundle(fs: &impl Fs, paths: &StorePaths, doc_id: &str) -> Result<Vec<u8>, SessionError> {
    let doc_dir = paths.doc_dir(doc_id);
    if !fs.exists(&doc_dir) {
        return Err(SessionError::new(format!(
            "bundle: document directory does not exist: {}",
            doc_dir.display()
        )));
    }

    // Collect all files recursively, recording relative paths.
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    collect_files(fs, &doc_dir, &doc_dir, &mut entries)?;

    // Sort by relative path bytes for determinism.
    entries.sort_by(|(a, _), (b, _)| a.as_bytes().cmp(b.as_bytes()));

    // Serialise payload into a DEFLATE stream.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    write_payload(&mut encoder, doc_id, &entries)?;
    let compressed = encoder.finish().map_err(SessionError::from)?;

    // Prepend magic.
    let mut out = Vec::with_capacity(MAGIC.len() + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// Reconstruct a document's store directory from a bundle blob.
///
/// Returns the `doc_id` recorded in the bundle. Writes every entry under
/// `<root>/docs/<doc_id>/` using the injected `Fs`. The `paths` root
/// determines where files land; pass a different `StorePaths` root than the
/// source to restore into a separate store.
pub fn unbundle(fs: &impl Fs, paths: &StorePaths, data: &[u8]) -> Result<String, SessionError> {
    // Check magic.
    let magic = data
        .get(..MAGIC.len())
        .ok_or_else(|| SessionError::new("unbundle: data too short to contain magic header"))?;
    if magic != MAGIC {
        return Err(SessionError::new(format!(
            "unbundle: bad magic header — expected {:?}, got {:?}",
            MAGIC, magic
        )));
    }

    // Decompress the remainder.
    let compressed = &data[MAGIC.len()..];
    let mut decoder = ZlibDecoder::new(compressed);
    let mut payload = Vec::new();
    decoder
        .read_to_end(&mut payload)
        .map_err(|e| SessionError::new(format!("unbundle: decompression failed: {e}")))?;

    // Parse the payload.
    let (doc_id, entries) = parse_payload(&payload)?;

    // Write every entry under doc_dir.
    let doc_dir = paths.doc_dir(&doc_id);
    for (rel_path, content) in &entries {
        let abs_path = join_relative(&doc_dir, rel_path)?;
        let parent = abs_path.parent().ok_or_else(|| {
            SessionError::new(format!("unbundle: entry path has no parent: {rel_path}"))
        })?;
        fs.create_dir_all(parent)?;
        fs.write(&abs_path, content)?;
    }

    Ok(doc_id)
}

// ── Private helpers ────────────────────────────────────────────────────────────

/// Recursively walk `dir` and append (relative_path, content) pairs to `out`.
/// `base` is the root doc_dir; relative paths are computed from it.
fn collect_files(
    fs: &impl Fs,
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), SessionError> {
    let children = fs.read_dir(dir)?;
    for child in children {
        let rel = relative_path(base, &child)?;
        // Distinguish a directory from a file by trying read_dir on the child.
        // The Fs contract: read_dir succeeds on directories and errors on files.
        match fs.read_dir(&child) {
            Ok(_) => {
                // It's a directory — recurse.
                collect_files(fs, base, &child, out)?;
            }
            Err(_) => {
                // It's a file — read and record.
                let content = fs.read(&child)?;
                out.push((rel, content));
            }
        }
    }
    Ok(())
}

/// Compute the relative path (forward-slash separated) of `path` with respect
/// to `base`. Errors if `path` is not under `base`.
fn relative_path(base: &Path, path: &Path) -> Result<String, SessionError> {
    let rel = path.strip_prefix(base).map_err(|_| {
        SessionError::new(format!(
            "bundle: path '{}' is not under base '{}'",
            path.display(),
            base.display()
        ))
    })?;
    // Convert to forward-slash string.
    let mut parts = Vec::new();
    for component in rel.components() {
        parts.push(
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| SessionError::new("bundle: non-UTF-8 path component"))?
                .to_owned(),
        );
    }
    Ok(parts.join("/"))
}

/// Re-join a forward-slash relative path onto an absolute base, rejecting any
/// `..`, `.`, or empty component to prevent path traversal.
fn join_relative(base: &Path, rel_path: &str) -> Result<PathBuf, SessionError> {
    let mut result = base.to_path_buf();
    for component in rel_path.split('/') {
        if component == ".." || component == "." || component.is_empty() {
            return Err(SessionError::new(format!(
                "unbundle: invalid path component in entry: {rel_path:?}"
            )));
        }
        result.push(component);
    }
    Ok(result)
}

/// Serialise the bundle payload into a writer.
fn write_payload(
    w: &mut impl Write,
    doc_id: &str,
    entries: &[(String, Vec<u8>)],
) -> Result<(), SessionError> {
    // doc_id
    let id_bytes = doc_id.as_bytes();
    let id_len = u32::try_from(id_bytes.len())
        .map_err(|_| SessionError::new("bundle: doc_id is too long to encode"))?;
    w.write_all(&id_len.to_le_bytes())
        .map_err(SessionError::from)?;
    w.write_all(id_bytes).map_err(SessionError::from)?;

    // entry count
    let count = u32::try_from(entries.len())
        .map_err(|_| SessionError::new("bundle: too many entries to encode"))?;
    w.write_all(&count.to_le_bytes())
        .map_err(SessionError::from)?;

    // entries
    for (rel_path, content) in entries {
        let path_bytes = rel_path.as_bytes();
        let path_len = u32::try_from(path_bytes.len()).map_err(|_| {
            SessionError::new(format!("bundle: relative path too long: {rel_path}"))
        })?;
        w.write_all(&path_len.to_le_bytes())
            .map_err(SessionError::from)?;
        w.write_all(path_bytes).map_err(SessionError::from)?;

        let content_len = u64::try_from(content.len()).map_err(|_| {
            SessionError::new(format!("bundle: content too large for entry: {rel_path}"))
        })?;
        w.write_all(&content_len.to_le_bytes())
            .map_err(SessionError::from)?;
        w.write_all(content).map_err(SessionError::from)?;
    }
    Ok(())
}

/// Parse the decompressed bundle payload; return (doc_id, entries).
///
/// All slice indexing is checked via `.get(..).ok_or(...)` — a truncated or
/// corrupt payload returns a clean `SessionError` rather than panicking.
fn parse_payload(payload: &[u8]) -> Result<(String, Vec<BundleEntry>), SessionError> {
    let mut pos = 0usize;

    // doc_id length
    let id_len = usize::try_from(read_u32_le(payload, &mut pos, "doc_id length")?)
        .map_err(|_| SessionError::new("unbundle: doc_id length exceeds platform usize"))?;

    // doc_id bytes
    let id_bytes = payload
        .get(pos..pos + id_len)
        .ok_or_else(|| SessionError::new("unbundle: truncated payload reading doc_id"))?;
    let doc_id = std::str::from_utf8(id_bytes)
        .map_err(|_| SessionError::new("unbundle: doc_id is not valid UTF-8"))?
        .to_owned();
    pos += id_len;

    // entry count
    let count = usize::try_from(read_u32_le(payload, &mut pos, "entry count")?)
        .map_err(|_| SessionError::new("unbundle: entry count exceeds platform usize"))?;

    // Guard against a maliciously large count field by capping the pre-allocation
    // to what the remaining payload could possibly contain (each entry is at
    // minimum 12 bytes: 4 for path_len + 8 for content_len).
    let max_entries = payload.len().saturating_sub(pos) / 12;
    let mut entries = Vec::with_capacity(count.min(max_entries));
    for i in 0..count {
        // rel_path length
        let path_len = usize::try_from(read_u32_le(
            payload,
            &mut pos,
            &format!("path length for entry {i}"),
        )?)
        .map_err(|_| {
            SessionError::new(format!(
                "unbundle: path length for entry {i} exceeds platform usize"
            ))
        })?;

        // rel_path bytes
        let path_bytes = payload.get(pos..pos + path_len).ok_or_else(|| {
            SessionError::new(format!(
                "unbundle: truncated payload reading path for entry {i}"
            ))
        })?;
        let rel_path = std::str::from_utf8(path_bytes)
            .map_err(|_| {
                SessionError::new(format!("unbundle: path for entry {i} is not valid UTF-8"))
            })?
            .to_owned();
        pos += path_len;

        // content length
        let content_len = usize::try_from(read_u64_le(
            payload,
            &mut pos,
            &format!("content length for entry {i}"),
        )?)
        .map_err(|_| {
            SessionError::new(format!(
                "unbundle: content length for entry {i} exceeds platform usize"
            ))
        })?;

        // content bytes
        let content = payload
            .get(pos..pos + content_len)
            .ok_or_else(|| {
                SessionError::new(format!(
                    "unbundle: truncated payload reading content for entry {i}"
                ))
            })?
            .to_vec();
        pos += content_len;

        entries.push((rel_path, content));
    }

    Ok((doc_id, entries))
}

/// Read a u32 little-endian from `data` at `*pos`, advance `pos` by 4.
fn read_u32_le(data: &[u8], pos: &mut usize, field: &str) -> Result<u32, SessionError> {
    let bytes = data
        .get(*pos..*pos + 4)
        .ok_or_else(|| SessionError::new(format!("unbundle: truncated payload reading {field}")))?;
    let arr: [u8; 4] = bytes.try_into().map_err(|_| {
        SessionError::new(format!("unbundle: internal slice error reading {field}"))
    })?;
    *pos += 4;
    Ok(u32::from_le_bytes(arr))
}

/// Read a u64 little-endian from `data` at `*pos`, advance `pos` by 8.
fn read_u64_le(data: &[u8], pos: &mut usize, field: &str) -> Result<u64, SessionError> {
    let bytes = data
        .get(*pos..*pos + 8)
        .ok_or_else(|| SessionError::new(format!("unbundle: truncated payload reading {field}")))?;
    let arr: [u8; 8] = bytes.try_into().map_err(|_| {
        SessionError::new(format!("unbundle: internal slice error reading {field}"))
    })?;
    *pos += 8;
    Ok(u64::from_le_bytes(arr))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MemFs;

    fn make_store(doc_id: &str) -> (MemFs, StorePaths) {
        let fs = MemFs::new();
        let paths = StorePaths::new("/data");
        let doc_dir = paths.doc_dir(doc_id);

        // versions.jsonl
        fs.create_dir_all(&doc_dir).unwrap();
        fs.write(&doc_dir.join("versions.jsonl"), b"{\"id\":\"v0\"}\n")
            .unwrap();

        // runs.jsonl
        fs.write(&doc_dir.join("runs.jsonl"), b"{\"run\":1}\n")
            .unwrap();

        // objects/ab/cdef...
        let obj_shard = doc_dir.join("objects").join("ab");
        fs.create_dir_all(&obj_shard).unwrap();
        fs.write(&obj_shard.join("cdef1234"), b"object-bytes")
            .unwrap();

        // scratch/index.jsonl
        let scratch_dir = doc_dir.join("scratch");
        fs.create_dir_all(&scratch_dir).unwrap();
        fs.write(&scratch_dir.join("index.jsonl"), b"{\"cand\":\"c0\"}\n")
            .unwrap();

        (fs, paths)
    }

    #[test]
    fn bundle_unbundle_roundtrip() {
        let doc_id = "test-doc-001";
        let (fs, paths) = make_store(doc_id);

        let blob = bundle(&fs, &paths, doc_id).unwrap();

        // Unbundle into a FRESH store root.
        let fs2 = MemFs::new();
        let paths2 = StorePaths::new("/data2");
        let returned_id = unbundle(&fs2, &paths2, &blob).unwrap();

        assert_eq!(returned_id, doc_id, "returned doc_id must match");

        let doc_dir = paths.doc_dir(doc_id);
        let doc_dir2 = paths2.doc_dir(doc_id);

        // Check every original file is present and identical in the new store.
        let check = |rel: &str| {
            let orig = fs.read(&doc_dir.join(rel)).unwrap();
            let copy = fs2.read(&doc_dir2.join(rel)).unwrap();
            assert_eq!(orig, copy, "content mismatch for {rel}");
        };
        check("versions.jsonl");
        check("runs.jsonl");
        check("objects/ab/cdef1234");
        check("scratch/index.jsonl");
    }

    #[test]
    fn bundle_is_deterministic() {
        let doc_id = "det-doc";
        let (fs, paths) = make_store(doc_id);
        let blob1 = bundle(&fs, &paths, doc_id).unwrap();
        let blob2 = bundle(&fs, &paths, doc_id).unwrap();
        assert_eq!(
            blob1, blob2,
            "two bundles of the same store must be byte-identical"
        );
    }

    #[test]
    fn unbundle_bad_magic_errors() {
        // Pure garbage.
        let result = unbundle(&MemFs::new(), &StorePaths::new("/x"), b"not-a-bundle");
        assert!(result.is_err(), "garbage input must return Err");
        let msg = result.unwrap_err().message;
        assert!(
            msg.contains("magic"),
            "error must mention 'magic'; got: {msg}"
        );

        // Wrong magic prefix, correct length.
        let mut bad = b"BADMAGIC".to_vec();
        bad.extend_from_slice(b"\x78\x9c"); // zlib header (meaningless here)
        let result2 = unbundle(&MemFs::new(), &StorePaths::new("/x"), &bad);
        assert!(result2.is_err(), "wrong-magic input must return Err");
    }

    #[test]
    fn unbundle_truncated_errors() {
        let doc_id = "trunc-doc";
        let (fs, paths) = make_store(doc_id);
        let blob = bundle(&fs, &paths, doc_id).unwrap();

        // Truncate to just past the magic — valid magic but truncated/empty deflate.
        let truncated = &blob[..MAGIC.len() + 2];
        let result = unbundle(&MemFs::new(), &StorePaths::new("/x"), truncated);
        assert!(result.is_err(), "truncated bundle must return Err");
    }

    #[test]
    fn bundle_missing_doc_errors() {
        let fs = MemFs::new();
        let paths = StorePaths::new("/data");
        // "ghost-doc" directory was never created.
        let result = bundle(&fs, &paths, "ghost-doc");
        assert!(result.is_err(), "bundling a missing doc must return Err");
        let msg = result.unwrap_err().message;
        assert!(
            msg.contains("ghost-doc"),
            "error must mention the doc_id; got: {msg}"
        );
    }
}
