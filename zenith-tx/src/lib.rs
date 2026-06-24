//! Transaction engine and audit contract for Zenith.
//!
//! Owns the v0 transaction op set and the apply/dry-run engine, referential-
//! integrity and ID-uniqueness enforcement, the transaction-result contract
//! (status, diagnostics, source diff, scene diff, affected node IDs, audit
//! preview and audit record), and audit record production. Both the CLI and
//! the future MCP surface sit on top of this crate; neither reimplements it.

pub mod engine;
pub mod merge;
pub mod op;
pub mod result;
pub mod schema;

// Curated flat re-exports.
pub use engine::run_transaction;
pub use merge::merge_candidate_page;
pub use op::{Op, OpPoint, OpSpan, Permissions, Position, Transaction};
pub use result::{TxError, TxResult, TxStatus};
