//! Pure path-builder helpers for the zenith store layout.
//!
//! [`StorePaths`] computes filesystem paths for every well-known location
//! under the zenith data directory.  It performs NO I/O — callers must pass
//! the resulting paths to an [`crate::adapter::Fs`] implementation.
//!
//! # Store layout
//!
//! ```text
//! <data_dir>/
//!   docs/
//!     <doc_id>/
//!       objects/         ← immutable object blobs (future unit)
//!       versions.jsonl   ← append-only version manifest (future unit)
//!       session/         ← mutable local session state
//! ```

use std::path::PathBuf;

/// Path-builder for the zenith local store rooted at a data directory.
///
/// All methods are pure: they compute a [`PathBuf`] via [`Path::join`] and
/// return it without touching the filesystem.
pub struct StorePaths {
    root: PathBuf,
}

impl StorePaths {
    /// Create a new `StorePaths` rooted at `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: data_dir.into(),
        }
    }

    /// Directory that contains all data for a given document.
    ///
    /// `<root>/docs/<doc_id>`
    pub fn doc_dir(&self, doc_id: &str) -> PathBuf {
        self.root.join("docs").join(doc_id)
    }

    /// Directory that holds immutable object blobs for a document.
    ///
    /// `<root>/docs/<doc_id>/objects`
    pub fn objects_dir(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("objects")
    }

    /// Append-only version manifest file for a document.
    ///
    /// `<root>/docs/<doc_id>/versions.jsonl`
    pub fn versions_file(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("versions.jsonl")
    }

    /// Mutable local session state directory for a document.
    ///
    /// `<root>/docs/<doc_id>/session`
    pub fn session_dir(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("session")
    }

    /// Persisted per-doc metadata file.
    ///
    /// `<root>/docs/<doc_id>/meta.json`
    pub fn meta_file(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("meta.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> StorePaths {
        StorePaths::new("/data")
    }

    #[test]
    fn doc_dir() {
        assert_eq!(paths().doc_dir("doc1"), PathBuf::from("/data/docs/doc1"));
    }

    #[test]
    fn objects_dir() {
        assert_eq!(
            paths().objects_dir("doc1"),
            PathBuf::from("/data/docs/doc1/objects")
        );
    }

    #[test]
    fn versions_file() {
        assert_eq!(
            paths().versions_file("doc1"),
            PathBuf::from("/data/docs/doc1/versions.jsonl")
        );
    }

    #[test]
    fn session_dir() {
        assert_eq!(
            paths().session_dir("doc1"),
            PathBuf::from("/data/docs/doc1/session")
        );
    }

    #[test]
    fn different_doc_ids_produce_different_paths() {
        let p = paths();
        assert_ne!(p.doc_dir("alpha"), p.doc_dir("beta"));
    }

    #[test]
    fn meta_file() {
        assert_eq!(
            paths().meta_file("doc1"),
            PathBuf::from("/data/docs/doc1/meta.json")
        );
    }
}
