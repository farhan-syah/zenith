//! Content-addressed object store.
//!
//! Objects are addressed by the lowercase hex SHA-256 of their UNCOMPRESSED
//! content and persisted DEFLATE-compressed (pure-Rust `flate2`/`miniz_oxide`
//! backend — the workspace stays free of C dependencies). Compression is an
//! internal detail behind [`put_object`]/[`get_object`]; because the address is
//! the hash of the *plaintext*, the codec can be swapped (e.g. to zstd) without
//! changing object identity or breaking dedup.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::PathBuf;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use sha2::{Digest, Sha256};

use crate::adapter::Fs;
use crate::error::SessionError;
use crate::layout::StorePaths;

// ── Private helpers ────────────────────────────────────────────────────────────

/// `<objects_dir>/<hash[0..2]>/<hash[2..]>`. Errors if `hash` is too short to shard.
fn object_path(paths: &StorePaths, doc_id: &str, hash: &str) -> Result<PathBuf, SessionError> {
    let shard = hash
        .get(0..2)
        .ok_or_else(|| SessionError::new(format!("invalid object hash (too short): {hash:?}")))?;
    let rest = hash
        .get(2..)
        .ok_or_else(|| SessionError::new(format!("invalid object hash (too short): {hash:?}")))?;
    Ok(paths.objects_dir(doc_id).join(shard).join(rest))
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Compute the lowercase-hex SHA-256 address of `content`. Pure; no IO.
pub fn object_hash(content: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(content);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// True if an object with `hash` already exists for `doc_id`.
pub fn has_object(fs: &impl Fs, paths: &StorePaths, doc_id: &str, hash: &str) -> bool {
    match object_path(paths, doc_id, hash) {
        Ok(p) => fs.exists(&p),
        Err(_) => false,
    }
}

/// Store `content`, returning its hash address. Idempotent / dedup'd:
/// if the object already exists it is NOT rewritten.
pub fn put_object(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    content: &[u8],
) -> Result<String, SessionError> {
    let hash = object_hash(content);
    put_object_with_hash(fs, paths, doc_id, content, &hash)?;
    Ok(hash)
}

/// Store `content` at the already-computed address `hash`. Idempotent / dedup'd:
/// if the object already exists it is NOT rewritten. Callers that have already
/// hashed `content` (e.g. for a dedup check) use this to avoid hashing twice;
/// `hash` MUST equal `object_hash(content)`.
pub fn put_object_with_hash(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    content: &[u8],
    hash: &str,
) -> Result<(), SessionError> {
    if has_object(fs, paths, doc_id, hash) {
        return Ok(());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(content).map_err(SessionError::from)?;
    let compressed = encoder.finish().map_err(SessionError::from)?;
    let path = object_path(paths, doc_id, hash)?;
    let shard_dir = path
        .parent()
        .ok_or_else(|| SessionError::new("object path has no parent directory"))?;
    fs.create_dir_all(shard_dir)?;
    fs.write(&path, &compressed)?;
    Ok(())
}

/// The on-disk (compressed) byte size of the object addressed by `hash`.
/// Errors if the object does not exist.
pub fn object_size(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    hash: &str,
) -> Result<u64, SessionError> {
    let path = object_path(paths, doc_id, hash)?;
    let bytes = fs.read(&path)?;
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

/// Load and decompress the object addressed by `hash` for `doc_id`.
pub fn get_object(
    fs: &impl Fs,
    paths: &StorePaths,
    doc_id: &str,
    hash: &str,
) -> Result<Vec<u8>, SessionError> {
    let path = object_path(paths, doc_id, hash)?;
    let compressed = fs.read(&path)?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(SessionError::from)?;
    // Integrity: a content-addressed store must never hand back bytes that do
    // not match the requested address. A mismatch means on-disk corruption.
    let actual = object_hash(&out);
    if actual != hash {
        return Err(SessionError::new(format!(
            "object integrity check failed for {hash}: decompressed content hashes to {actual}"
        )));
    }
    Ok(out)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MemFs;

    fn setup() -> (MemFs, StorePaths) {
        (MemFs::new(), StorePaths::new("/data"))
    }

    #[test]
    fn object_hash_known_vector() {
        let hash = object_hash(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn put_get_roundtrip() {
        let (fs, paths) = setup();
        let hash = put_object(&fs, &paths, "doc1", b"some content").unwrap();
        let got = get_object(&fs, &paths, "doc1", &hash).unwrap();
        assert_eq!(got, b"some content");
    }

    #[test]
    fn put_with_hash_matches_put_object() {
        let (fs, paths) = setup();
        let content = b"precomputed-hash content";
        let hash = object_hash(content);
        put_object_with_hash(&fs, &paths, "doc1", content, &hash).unwrap();
        // Same address, same readback as the hashing put_object would produce.
        assert!(has_object(&fs, &paths, "doc1", &hash));
        assert_eq!(get_object(&fs, &paths, "doc1", &hash).unwrap(), content);
        // Idempotent: a second call is a no-op and still succeeds.
        put_object_with_hash(&fs, &paths, "doc1", content, &hash).unwrap();
        assert_eq!(get_object(&fs, &paths, "doc1", &hash).unwrap(), content);
    }

    #[test]
    fn put_is_deterministic_and_dedup() {
        let (fs, paths) = setup();
        let hash1 = put_object(&fs, &paths, "doc1", b"repeated content").unwrap();
        let hash2 = put_object(&fs, &paths, "doc1", b"repeated content").unwrap();
        assert_eq!(hash1, hash2);
        assert!(has_object(&fs, &paths, "doc1", &hash1));
    }

    #[test]
    fn put_different_content_different_hash() {
        let (fs, paths) = setup();
        let hash_a = put_object(&fs, &paths, "doc1", b"content A").unwrap();
        let hash_b = put_object(&fs, &paths, "doc1", b"content B").unwrap();
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn get_missing_errors() {
        let (fs, paths) = setup();
        let missing_hash = object_hash(b"never stored");
        let result = get_object(&fs, &paths, "doc1", &missing_hash);
        assert!(result.is_err());
    }

    #[test]
    fn malformed_hash_errors() {
        let (fs, paths) = setup();
        let result = get_object(&fs, &paths, "doc1", "x");
        assert!(result.is_err());
    }

    #[test]
    fn large_content_roundtrips() {
        let (fs, paths) = setup();
        let content: Vec<u8> = vec![0xABu8; 100_000];
        let hash = put_object(&fs, &paths, "doc1", &content).unwrap();
        let got = get_object(&fs, &paths, "doc1", &hash).unwrap();
        assert_eq!(got, content);
    }

    #[test]
    fn corrupted_object_fails_integrity_check() {
        let (fs, paths) = setup();
        // Store real content, then move its blob under a DIFFERENT (claimed)
        // address so that what we read decompresses to bytes whose hash does
        // not match the requested address — i.e. simulated on-disk corruption.
        let real_hash = put_object(&fs, &paths, "doc1", b"the real bytes").unwrap();
        let real_path = paths
            .objects_dir("doc1")
            .join(&real_hash[..2])
            .join(&real_hash[2..]);
        let blob = fs.read(&real_path).unwrap();

        let claimed_hash = object_hash(b"a different thing entirely");
        let claimed_path = paths
            .objects_dir("doc1")
            .join(&claimed_hash[..2])
            .join(&claimed_hash[2..]);
        fs.create_dir_all(claimed_path.parent().unwrap()).unwrap();
        fs.write(&claimed_path, &blob).unwrap();

        let result = get_object(&fs, &paths, "doc1", &claimed_hash);
        assert!(
            result.is_err(),
            "integrity check must reject content that does not hash to the requested address"
        );
    }

    #[test]
    fn compression_actually_shrinks() {
        let (fs, paths) = setup();
        let content = vec![0u8; 10_000];
        let hash = put_object(&fs, &paths, "doc1", &content).unwrap();
        // Read the raw stored bytes at the sharded path.
        let stored_path = paths.objects_dir("doc1").join(&hash[..2]).join(&hash[2..]);
        let raw = fs.read(&stored_path).unwrap();
        assert!(
            raw.len() < 10_000,
            "expected compressed size ({}) to be smaller than 10000",
            raw.len()
        );
    }

    #[test]
    fn object_size_returns_stored_length() {
        let (fs, paths) = setup();
        let content = b"some stored content";
        let hash = put_object(&fs, &paths, "doc1", content).unwrap();

        let stored_path = paths.objects_dir("doc1").join(&hash[..2]).join(&hash[2..]);
        let raw = fs.read(&stored_path).unwrap();
        let expected = u64::try_from(raw.len()).unwrap();

        let size = object_size(&fs, &paths, "doc1", &hash).unwrap();
        assert!(size > 0, "stored size must be nonzero");
        assert_eq!(
            size, expected,
            "object_size must match the raw stored file length"
        );
    }

    #[test]
    fn object_size_missing_hash_errors() {
        let (fs, paths) = setup();
        let missing = object_hash(b"never stored");
        let result = object_size(&fs, &paths, "doc1", &missing);
        assert!(
            result.is_err(),
            "object_size for a missing hash must return Err"
        );
    }
}
