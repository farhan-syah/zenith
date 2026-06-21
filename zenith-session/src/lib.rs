//! zenith-session: local-machine history/session state for `.zen` documents.
//!
//! Pure crate with injected fs/clock/rng adapters; never depended on by the
//! deterministic render pipeline.
//!
//! # Module layout
//!
//! - [`adapter`] — injectable trait boundaries (filesystem, clock, RNG)
//! - [`datadir`] — platform data-directory resolution
//! - [`docid`] — ULID document-identity minting
//! - [`error`] — [`SessionError`] (the single error type for this crate)
//! - [`identity`] — document-identity reconciliation ([`reconcile`])
//! - [`layout`] — [`StorePaths`] pure path builders
//! - [`manifest`] — [`HistoryRecord`] schema and append-only JSONL manifest I/O
//! - [`store`] — content-addressed object store (SHA-256 + DEFLATE)

pub mod adapter;
pub mod datadir;
pub mod docid;
pub mod error;
pub mod identity;
pub mod layout;
pub mod manifest;
pub mod store;

pub use datadir::{resolve_data_dir, resolve_data_dir_with};
pub use docid::mint_ulid;
pub use error::SessionError;
pub use identity::{DocMeta, Outcome, Reconciled, reconcile};
pub use layout::StorePaths;
pub use manifest::{HistoryRecord, append_record, read_records};
pub use store::{get_object, has_object, object_hash, put_object};
