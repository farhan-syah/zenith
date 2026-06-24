//! Object garbage collection.
//!
//! Deletes content-addressed objects that are no longer referenced by the
//! Tier-1 session journal or the Tier-2 version history. An object is reachable
//! iff its hash appears as the `snapshot` of some live record in either manifest.

use std::collections::BTreeSet;

use crate::adapter::Fs;
use crate::error::SessionError;
use crate::layout::StorePaths;
use crate::manifest::read_records;
use crate::session::journal_path;

// ── Report ─────────────────────────────────────────────────────────────────────

/// Summary of a [`gc`] run.
#[derive(Debug, Clone, PartialEq)]
pub struct GcReport {
    /// Object files deleted (no longer referenced).
    pub deleted: usize,
    /// Object files kept (still referenced).
    pub kept: usize,
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Delete objects for `doc_id` that are not referenced by the Tier-1 journal or
/// the Tier-2 versions. Safe to call any time; a no-op if there is no object dir.
pub fn gc(fs: &impl Fs, paths: &StorePaths, doc_id: &str) -> Result<GcReport, SessionError> {
    // Collect the reachable object hashes from BOTH manifests.
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for r in read_records(fs, &journal_path(paths, doc_id))? {
        referenced.insert(r.snapshot);
    }
    for r in read_records(fs, &paths.versions_file(doc_id))? {
        referenced.insert(r.snapshot);
    }

    let odir = paths.objects_dir(doc_id);
    if !fs.exists(&odir) {
        return Ok(GcReport {
            deleted: 0,
            kept: 0,
        });
    }

    let mut deleted = 0usize;
    let mut kept = 0usize;
    // objects_dir/<shard>/<rest>
    for shard in fs.read_dir(&odir)? {
        // The shard's directory-name is the first 2 hex chars of the hash.
        let shard_name = match shard.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_owned(),
            None => continue, // non-utf8 dir name: not one of ours, skip
        };
        for obj in fs.read_dir(&shard)? {
            let file_name = match obj.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_owned(),
                None => continue,
            };
            let hash = format!("{shard_name}{file_name}");
            if referenced.contains(&hash) {
                kept += 1;
            } else {
                fs.remove(&obj)?;
                deleted += 1;
            }
        }
    }
    Ok(GcReport { deleted, kept })
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::adapter::{FakeClock, FakeRng, MemFs};
    use crate::tier2::VersionMeta;
    use crate::{session, store, tier2};

    fn setup() -> (MemFs, StorePaths, FakeClock, FakeRng) {
        let fs = MemFs::new();
        let paths = StorePaths::new("/data");
        let clock = FakeClock(UNIX_EPOCH + Duration::from_millis(1000));
        let rng = FakeRng(0);
        (fs, paths, clock, rng)
    }

    #[test]
    fn gc_empty_is_noop() {
        let (fs, paths, _clock, _rng) = setup();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(
            report,
            GcReport {
                deleted: 0,
                kept: 0
            }
        );
    }

    #[test]
    fn gc_keeps_version_referenced() {
        let (fs, paths, clock, _rng) = setup();
        tier2::record_version(&fs, &paths, &clock, "doc1", b"V1", VersionMeta::default()).unwrap();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, 0);
        assert!(report.kept >= 1);
        // Object must survive — version_content still works.
        let content = tier2::version_content(&fs, &paths, "doc1", "v0").unwrap();
        assert_eq!(content, b"V1");
    }

    #[test]
    fn gc_keeps_session_referenced() {
        let (fs, paths, clock, rng) = setup();
        session::record_state(&fs, &paths, &clock, &rng, "doc1", b"S1", None).unwrap();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, 0);
        assert!(report.kept >= 1);
        // Object must survive — current_content still returns b"S1".
        let content = session::current_content(&fs, &paths, "doc1").unwrap();
        assert_eq!(content, Some(b"S1".to_vec()));
    }

    #[test]
    fn gc_removes_unreferenced() {
        let (fs, paths, _clock, _rng) = setup();
        let hash = store::put_object(&fs, &paths, "doc1", b"orphan").unwrap();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(
            report,
            GcReport {
                deleted: 1,
                kept: 0
            }
        );
        // Object must be gone.
        let result = store::get_object(&fs, &paths, "doc1", &hash);
        assert!(result.is_err());
    }

    #[test]
    fn gc_mixed() {
        let (fs, paths, clock, _rng) = setup();
        // Record a version (referenced).
        tier2::record_version(&fs, &paths, &clock, "doc1", b"kept", VersionMeta::default())
            .unwrap();
        // Store an orphan object directly (unreferenced).
        store::put_object(&fs, &paths, "doc1", b"orphan").unwrap();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, 1);
        assert_eq!(report.kept, 1);
        // Version content must still be readable.
        let content = tier2::version_content(&fs, &paths, "doc1", "v0").unwrap();
        assert_eq!(content, b"kept");
        // Orphan hash must be gone.
        let orphan_hash = store::object_hash(b"orphan");
        let result = store::get_object(&fs, &paths, "doc1", &orphan_hash);
        assert!(result.is_err());
    }

    #[test]
    fn gc_keeps_object_shared_by_both_tiers() {
        let (fs, paths, clock, rng) = setup();
        // Same content → same hash → one object file shared by both tiers.
        session::record_state(&fs, &paths, &clock, &rng, "doc1", b"shared", None).unwrap();
        tier2::record_version(
            &fs,
            &paths,
            &clock,
            "doc1",
            b"shared",
            VersionMeta::default(),
        )
        .unwrap();
        let report = gc(&fs, &paths, "doc1").unwrap();
        assert_eq!(report.deleted, 0);
        assert!(report.kept >= 1);
    }
}
