//! Filesystem adapter trait and implementations.
//!
//! [`Fs`] is the injectable boundary between zenith-session logic and the real
//! operating system.  All library code that touches the filesystem receives an
//! `&impl Fs` (or `&dyn Fs`) so that tests can substitute [`MemFs`] without
//! touching disk.
//!
//! [`MemFs`] is the in-memory test adapter.  It lives in lib (not
//! `#[cfg(test)]`) so that integration tests in later units can import it
//! directly.  It is intentionally not feature-gated.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::SessionError;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstraction over filesystem operations used by zenith-session.
///
/// Implementations must satisfy the following contract:
/// - All directory and file listings are returned **sorted** for deterministic
///   iteration across platforms.
/// - `write` must fail with an error when the parent directory does not exist
///   (to faithfully model real OS behaviour so fakes don't silently
///   false-pass tests that forget `create_dir_all`).
pub trait Fs {
    /// Create `path` and all missing ancestors, like `mkdir -p`.
    fn create_dir_all(&self, path: &Path) -> Result<(), SessionError>;

    /// Return `true` if `path` exists (file or directory).
    fn exists(&self, path: &Path) -> bool;

    /// Read the entire contents of the file at `path`.
    fn read(&self, path: &Path) -> Result<Vec<u8>, SessionError>;

    /// Write `data` to `path`, replacing any existing content.
    ///
    /// Returns an error if the parent directory does not exist.
    fn write(&self, path: &Path, data: &[u8]) -> Result<(), SessionError>;

    /// Append `data` to the file at `path`, creating it if absent. Like `write`,
    /// it errors if the parent directory does not exist.
    fn append(&self, path: &Path, data: &[u8]) -> Result<(), SessionError>;

    /// List immediate children (files and directories) of `path`, sorted for
    /// deterministic iteration.
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, SessionError>;

    /// Rename / move `from` to `to`.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), SessionError>;

    /// Remove a file or directory tree at `path`.
    ///
    /// Tries `remove_file` first; on failure tries `remove_dir_all`.  This
    /// keeps the API simple: callers do not need to know whether `path` is a
    /// file or a directory.
    fn remove(&self, path: &Path) -> Result<(), SessionError>;
}

// ── OsFs ──────────────────────────────────────────────────────────────────────

/// Real filesystem adapter that delegates to `std::fs`.
pub struct OsFs;

impl Fs for OsFs {
    fn create_dir_all(&self, path: &Path) -> Result<(), SessionError> {
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, SessionError> {
        let bytes = std::fs::read(path)?;
        Ok(bytes)
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), SessionError> {
        std::fs::write(path, data)?;
        Ok(())
    }

    fn append(&self, path: &Path, data: &[u8]) -> Result<(), SessionError> {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        f.write_all(data)?;
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, SessionError> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        Ok(entries)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), SessionError> {
        std::fs::rename(from, to)?;
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<(), SessionError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            // Only fall through to a recursive directory removal when `path` is
            // actually a directory. Any other failure (e.g. permission denied)
            // is propagated as-is rather than masked by a misleading dir error.
            Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => {
                std::fs::remove_dir_all(path)?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}

// ── MemFs ─────────────────────────────────────────────────────────────────────

/// In-memory filesystem for tests.
///
/// Backed by a [`BTreeMap`] (files) and [`BTreeSet`] (directories) for
/// deterministic, sorted output.  Interior mutability via [`RefCell`] lets
/// callers hold `&MemFs` (shared reference) while still mutating state.
///
/// **Fidelity contract**: `write` returns an error when the parent directory
/// has not been registered via `create_dir_all`, faithfully modelling real OS
/// behaviour so tests cannot accidentally skip the parent-creation step.
///
/// This type is available in non-test builds so integration tests in later
/// units can import it without feature flags.
pub struct MemFs {
    inner: RefCell<MemFsInner>,
}

struct MemFsInner {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
}

impl MemFs {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(MemFsInner {
                files: BTreeMap::new(),
                dirs: BTreeSet::new(),
            }),
        }
    }
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl Fs for MemFs {
    fn create_dir_all(&self, path: &Path) -> Result<(), SessionError> {
        let mut inner = self.inner.borrow_mut();
        // Insert the path itself and every ancestor.
        let mut current = path.to_path_buf();
        loop {
            let next = current
                .parent()
                .filter(|&p| p != current.as_path())
                .map(|p| p.to_path_buf());
            inner.dirs.insert(current);
            match next {
                Some(p) => current = p,
                None => break,
            }
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        let inner = self.inner.borrow();
        inner.files.contains_key(path) || inner.dirs.contains(path)
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, SessionError> {
        let inner = self.inner.borrow();
        inner
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| SessionError::new(format!("file not found: {}", path.display())))
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), SessionError> {
        let mut inner = self.inner.borrow_mut();
        // Enforce: parent must have been created first.
        let parent = path
            .parent()
            .ok_or_else(|| SessionError::new("path has no parent directory"))?;
        if !inner.dirs.contains(parent) {
            return Err(SessionError::new(format!(
                "parent directory does not exist: {}",
                parent.display()
            )));
        }
        inner.files.insert(path.to_path_buf(), data.to_vec());
        Ok(())
    }

    fn append(&self, path: &Path, data: &[u8]) -> Result<(), SessionError> {
        let mut inner = self.inner.borrow_mut();
        let parent = path
            .parent()
            .ok_or_else(|| SessionError::new("path has no parent directory"))?;
        if !inner.dirs.contains(parent) {
            return Err(SessionError::new(format!(
                "parent directory does not exist: {}",
                parent.display()
            )));
        }
        inner
            .files
            .entry(path.to_path_buf())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, SessionError> {
        let inner = self.inner.borrow();
        if !inner.dirs.contains(path) {
            return Err(SessionError::new(format!(
                "directory not found: {}",
                path.display()
            )));
        }
        // Collect immediate children from both file and dir sets.
        let mut children: BTreeSet<PathBuf> = BTreeSet::new();
        for file_path in inner.files.keys() {
            if file_path.parent() == Some(path) {
                children.insert(file_path.clone());
            }
        }
        for dir_path in inner.dirs.iter() {
            if dir_path.parent() == Some(path) && dir_path != path {
                children.insert(dir_path.clone());
            }
        }
        // BTreeSet is already sorted.
        Ok(children.into_iter().collect())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), SessionError> {
        let mut inner = self.inner.borrow_mut();
        let data = inner
            .files
            .remove(from)
            .ok_or_else(|| SessionError::new(format!("file not found: {}", from.display())))?;
        inner.files.insert(to.to_path_buf(), data);
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<(), SessionError> {
        let mut inner = self.inner.borrow_mut();
        if inner.files.remove(path).is_some() {
            return Ok(());
        }
        if inner.dirs.contains(path) {
            // Remove all files and dirs under this subtree.
            inner.files.retain(|k, _| !k.starts_with(path));
            inner.dirs.retain(|k| !k.starts_with(path));
            return Ok(());
        }
        Err(SessionError::new(format!(
            "path not found: {}",
            path.display()
        )))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── MemFs tests ───────────────────────────────────────────────────────────

    #[test]
    fn memfs_write_read_roundtrip() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/data");
        let file = dir.join("hello.txt");
        fs.create_dir_all(&dir).unwrap();
        fs.write(&file, b"hello world").unwrap();
        assert_eq!(fs.read(&file).unwrap(), b"hello world");
    }

    #[test]
    fn memfs_write_without_parent_errors() {
        let fs = MemFs::new();
        let file = PathBuf::from("/missing/dir/file.txt");
        let result = fs.write(&file, b"data");
        assert!(result.is_err(), "expected error when parent dir is absent");
    }

    #[test]
    fn memfs_read_dir_sorted() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/root");
        fs.create_dir_all(&dir).unwrap();
        // Write in reverse order to confirm sorting is by path, not insert order.
        fs.write(&dir.join("c.txt"), b"c").unwrap();
        fs.write(&dir.join("a.txt"), b"a").unwrap();
        fs.write(&dir.join("b.txt"), b"b").unwrap();
        let entries = fs.read_dir(&dir).unwrap();
        assert_eq!(
            entries,
            vec![dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt")]
        );
    }

    #[test]
    fn memfs_read_dir_missing_errors() {
        let fs = MemFs::new();
        let result = fs.read_dir(Path::new("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn memfs_rename_moves_file() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/d");
        fs.create_dir_all(&dir).unwrap();
        let from = dir.join("old.txt");
        let to = dir.join("new.txt");
        fs.write(&from, b"payload").unwrap();
        fs.rename(&from, &to).unwrap();
        assert!(!fs.exists(&from));
        assert!(fs.exists(&to));
        assert_eq!(fs.read(&to).unwrap(), b"payload");
    }

    #[test]
    fn memfs_rename_missing_errors() {
        let fs = MemFs::new();
        let result = fs.rename(Path::new("/a"), Path::new("/b"));
        assert!(result.is_err());
    }

    #[test]
    fn memfs_remove_file() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/r");
        fs.create_dir_all(&dir).unwrap();
        let file = dir.join("f.txt");
        fs.write(&file, b"x").unwrap();
        fs.remove(&file).unwrap();
        assert!(!fs.exists(&file));
    }

    #[test]
    fn memfs_remove_dir_subtree() {
        let fs = MemFs::new();
        let parent = PathBuf::from("/p");
        let child_dir = parent.join("sub");
        fs.create_dir_all(&child_dir).unwrap();
        fs.write(&child_dir.join("f.txt"), b"data").unwrap();
        fs.remove(&parent).unwrap();
        assert!(!fs.exists(&parent));
        assert!(!fs.exists(&child_dir));
        assert!(!fs.exists(&child_dir.join("f.txt")));
    }

    #[test]
    fn memfs_append_creates_then_accumulates() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/data");
        let file = dir.join("journal.txt");
        fs.create_dir_all(&dir).unwrap();
        fs.append(&file, b"hello ").unwrap();
        fs.append(&file, b"world").unwrap();
        assert_eq!(fs.read(&file).unwrap(), b"hello world");
    }

    #[test]
    fn memfs_append_without_parent_errors() {
        let fs = MemFs::new();
        let file = PathBuf::from("/missing/dir/journal.txt");
        let result = fs.append(&file, b"data");
        assert!(result.is_err(), "expected error when parent dir is absent");
    }

    #[test]
    fn memfs_append_then_read_roundtrip() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/logs");
        let file = dir.join("log.txt");
        fs.create_dir_all(&dir).unwrap();
        fs.append(&file, b"line one\n").unwrap();
        fs.append(&file, b"line two\n").unwrap();
        assert_eq!(fs.read(&file).unwrap(), b"line one\nline two\n");
    }

    #[test]
    fn memfs_exists_reflects_state() {
        let fs = MemFs::new();
        let dir = PathBuf::from("/e");
        let file = dir.join("x.txt");
        assert!(!fs.exists(&dir));
        fs.create_dir_all(&dir).unwrap();
        assert!(fs.exists(&dir));
        assert!(!fs.exists(&file));
        fs.write(&file, b"").unwrap();
        assert!(fs.exists(&file));
    }

    // ── OsFs integration test (uses tempfile dev-dependency) ─────────────────

    #[test]
    fn osfs_write_read_read_dir_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fs = OsFs;
        let dir = tmp.path().join("subdir");
        fs.create_dir_all(&dir).unwrap();
        let file_a = dir.join("a.txt");
        let file_b = dir.join("b.txt");
        fs.write(&file_a, b"aaa").unwrap();
        fs.write(&file_b, b"bbb").unwrap();
        assert_eq!(fs.read(&file_a).unwrap(), b"aaa");
        assert_eq!(fs.read(&file_b).unwrap(), b"bbb");
        let entries = fs.read_dir(&dir).unwrap();
        assert_eq!(entries, vec![file_a.clone(), file_b.clone()]);
    }
}
