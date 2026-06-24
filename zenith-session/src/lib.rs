//! zenith-session: local-machine history/session state for `.zen` documents.
//!
//! Pure crate with injected fs/clock/rng adapters; never depended on by the
//! deterministic render pipeline.
//!
//! # Module layout
//!
//! - [`adapter`] — injectable trait boundaries (filesystem, clock, RNG)
//! - [`bundle`] — deterministic DEFLATE bundle/unbundle for portable transfer
//! - [`datadir`] — platform data-directory resolution
//! - [`docid`] — ULID document-identity minting
//! - [`error`] — [`SessionError`] (the single error type for this crate)
//! - [`gc`] — object garbage collection ([`gc`])
//! - [`global`] — global cross-document LRU storage cap ([`enforce_global_cap`])
//! - [`identity`] — document-identity reconciliation ([`reconcile`])
//! - [`layout`] — [`StorePaths`] pure path builders
//! - [`manifest`] — [`HistoryRecord`] schema and append-only JSONL manifest I/O
//! - [`retention`] — Time-Machine-style retention thinning for Tier-2 version history
//! - [`revspec`] — revision-spec resolver: map a human/agent revspec string to a record id
//! - [`session`] — Tier-1 ephemeral session: snapshot DAG with HEAD + redo stack
//! - [`store`] — content-addressed object store (SHA-256 + DEFLATE)
//! - [`previews`] — [`PreviewRecord`] schema and append-only preview-artifact log
//! - [`runs`] — [`RunRecord`] schema and append-only agent-run provenance log
//! - [`scratch`] — [`CandidateEntry`] schema and append-only scratch/candidate index
//! - [`tier2`] — Tier-2 durable version history: bounded flat list in `versions.jsonl`

pub mod adapter;
pub mod bundle;
pub mod datadir;
pub mod docid;
pub mod error;
pub mod gc;
pub mod global;
pub mod identity;
pub mod layout;
pub mod manifest;
pub mod previews;
pub mod retention;
pub mod revspec;
pub mod runs;
pub mod scratch;
pub mod session;
pub mod store;
pub mod tier2;

pub use bundle::{bundle, unbundle};
pub use datadir::{resolve_data_dir, resolve_data_dir_with};
pub use docid::mint_ulid;
pub use error::SessionError;
pub use gc::{GcReport, gc};
pub use global::{GlobalCapReport, enforce_global_cap};
pub use identity::{DocMeta, Outcome, Reconciled, reconcile};
pub use layout::StorePaths;
pub use manifest::{CheckpointMeta, HistoryRecord, append_record, read_records};
pub use previews::{PreviewCritique, PreviewRecord, append_preview, read_previews};
pub use retention::{
    CapReport, MaintainReport, RetentionPolicy, ThinReport, apply_caps, apply_thinning, maintain,
    thin_versions,
};
pub use revspec::{resolve_revspec, resolve_revspec_for};
pub use runs::{RunDiagnostic, RunRecord, RunStep, append_run, read_runs};
pub use scratch::{
    CandidateEntry, CandidateMeta, CandidateStatus, FinalizeReport, NewCandidate,
    finalize_candidates, get_scratch_snapshot, list_scratch, put_scratch, set_candidate_status,
};
pub use session::{
    RecordOutcome, SessionState, clear_session, current_content, record_state, redo, undo,
};
pub use store::{
    get_object, has_object, object_hash, object_size, put_object, put_object_with_hash,
};
pub use tier2::{
    VersionMeta, VersionOutcome, list_versions, record_version, resolve_version, restore_content,
    version_content,
};
