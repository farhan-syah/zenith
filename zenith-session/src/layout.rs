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
//!       runs.jsonl       ← append-only agent-runs log
//!       previews.jsonl   ← append-only preview-artifacts log
//!       scratch/
//!         index.jsonl    ← scratch/candidate index
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

    /// The root directory holding all per-document history: `<root>/docs`.
    pub fn docs_root(&self) -> PathBuf {
        self.root.join("docs")
    }

    /// Directory that contains all data for a given document.
    ///
    /// `<root>/docs/<doc_id>`
    pub fn doc_dir(&self, doc_id: &str) -> PathBuf {
        self.docs_root().join(doc_id)
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

    /// Append-only agent-runs log: `<root>/docs/<doc_id>/runs.jsonl`.
    pub fn runs_file(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("runs.jsonl")
    }

    /// Append-only preview-artifacts log: `<root>/docs/<doc_id>/previews.jsonl`.
    pub fn previews_file(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("previews.jsonl")
    }

    /// Scratch/candidate directory: `<root>/docs/<doc_id>/scratch`.
    pub fn scratch_dir(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("scratch")
    }

    /// Scratch/candidate index: `<root>/docs/<doc_id>/scratch/index.jsonl`.
    pub fn scratch_index(&self, doc_id: &str) -> PathBuf {
        self.scratch_dir(doc_id).join("index.jsonl")
    }

    /// Per-document workspace directory for ephemeral working artifacts that are
    /// NOT part of the deliverable `.zen`: `<root>/docs/<doc_id>/workspace`.
    pub fn workspace_dir(&self, doc_id: &str) -> PathBuf {
        self.doc_dir(doc_id).join("workspace")
    }

    /// Predictable scratch area for rendered previews produced via the agent
    /// (MCP) surface: `<root>/docs/<doc_id>/workspace/renders`.
    ///
    /// Keeping previews here means the `.zen` holds only final content while the
    /// agent still has one stable, per-document place to find its render output.
    pub fn workspace_renders_dir(&self, doc_id: &str) -> PathBuf {
        self.workspace_dir(doc_id).join("renders")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> StorePaths {
        StorePaths::new("/data")
    }

    #[test]
    fn docs_root() {
        assert_eq!(paths().docs_root(), PathBuf::from("/data/docs"));
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

    #[test]
    fn runs_file() {
        assert_eq!(
            paths().runs_file("doc1"),
            PathBuf::from("/data/docs/doc1/runs.jsonl")
        );
    }

    #[test]
    fn previews_file() {
        assert_eq!(
            paths().previews_file("doc1"),
            PathBuf::from("/data/docs/doc1/previews.jsonl")
        );
    }

    #[test]
    fn scratch_dir() {
        assert_eq!(
            paths().scratch_dir("doc1"),
            PathBuf::from("/data/docs/doc1/scratch")
        );
    }

    #[test]
    fn scratch_index() {
        assert_eq!(
            paths().scratch_index("doc1"),
            PathBuf::from("/data/docs/doc1/scratch/index.jsonl")
        );
    }

    #[test]
    fn workspace_dir() {
        assert_eq!(
            paths().workspace_dir("doc1"),
            PathBuf::from("/data/docs/doc1/workspace")
        );
    }

    #[test]
    fn workspace_renders_dir() {
        assert_eq!(
            paths().workspace_renders_dir("doc1"),
            PathBuf::from("/data/docs/doc1/workspace/renders")
        );
    }
}
