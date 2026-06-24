//! Argument types for `zenith workspace` and its subcommands.

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Arguments for `zenith workspace`.
#[derive(Debug, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceSub,
}

/// Subcommands of `zenith workspace`.
#[derive(Debug, Subcommand)]
pub enum WorkspaceSub {
    /// Record, list, and inspect scratch design candidates for a document.
    ///
    /// Scratch candidates are content-addressed `.zen` snapshots stored in the
    /// session data directory alongside the durable version history. Use `new`
    /// to record a candidate, `list` to enumerate them, and `show` to inspect
    /// a specific one.
    Scratch(ScratchArgs),

    /// Transition a scratch candidate's lifecycle status (draft → selected | rejected).
    Candidate(CandidateArgs),

    /// Promote a selected candidate into a target page of the deliverable document.
    ///
    /// Fetches the candidate's stored `.zen` snapshot, deep-copies the source page's
    /// content into the named target page (suffixing all ids), validates the result,
    /// and writes the mutated document back in place. The promote is recorded in
    /// version history. The candidate must have status `selected`; use
    /// `zenith workspace candidate` to transition it first.
    Promote(PromoteArgs),

    /// Clean up rejected scratch candidates according to their cleanup policy.
    ///
    /// Candidates with `status = rejected` and `cleanup_policy = delete` are
    /// removed from the scratch index. Their snapshot objects are left in the
    /// object store for a future GC pass. All other candidates are preserved.
    Finalize(FinalizeArgs),

    /// Pack a document's entire session store into a portable `.zenithbundle` file.
    ///
    /// The bundle is a deterministic, C-free DEFLATE archive containing every
    /// object, version record, run log, scratch candidate, and metadata file
    /// for the document. Same store bytes in → same bundle bytes out.
    Bundle(BundleArgs),

    /// Restore a document's session store from a `.zenithbundle` file.
    ///
    /// Extracts the bundle into the default store directory and prints the
    /// restored doc-id.
    Unbundle(UnbundleArgs),
}

/// Arguments for `zenith workspace bundle`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  zenith workspace bundle poster.zen --out poster.zenithbundle")]
pub struct BundleArgs {
    /// Path to the `.zen` document whose store to bundle.
    pub doc: PathBuf,

    /// Output path for the `.zenithbundle` file.
    #[arg(long)]
    pub out: PathBuf,
}

/// Arguments for `zenith workspace unbundle`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  zenith workspace unbundle poster.zenithbundle")]
pub struct UnbundleArgs {
    /// Path to the `.zenithbundle` file to restore.
    pub bundle: PathBuf,
}

/// Arguments for `zenith workspace scratch`.
#[derive(Debug, Args)]
pub struct ScratchArgs {
    #[command(subcommand)]
    pub command: ScratchSub,
}

/// Subcommands of `zenith workspace scratch`.
#[derive(Debug, Subcommand)]
pub enum ScratchSub {
    /// Record the current `.zen` file as a scratch candidate.
    New(ScratchNewArgs),
    /// List all scratch candidates for a document.
    List(ScratchListArgs),
    /// Show detail for one scratch candidate.
    Show(ScratchShowArgs),
}

/// Arguments for `zenith workspace scratch new`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  \
zenith workspace scratch new poster.zen --page main --status draft --notes \"first pass\"")]
pub struct ScratchNewArgs {
    /// Path to the `.zen` document to snapshot.
    pub doc: PathBuf,

    /// Page id this candidate captures (default: `*` for the whole document).
    #[arg(long, value_name = "ID")]
    pub page: Option<String>,

    /// Initial lifecycle status: `draft`, `selected`, or `rejected` (default: `draft`).
    #[arg(long, default_value = "draft", value_name = "STATUS")]
    pub status: String,

    /// Free-text notes about this candidate.
    #[arg(long, value_name = "TEXT")]
    pub notes: Option<String>,

    /// Target slot or branch to promote this candidate into.
    #[arg(long, value_name = "TARGET")]
    pub promotion_target: Option<String>,

    /// Policy tag controlling when this candidate may be cleaned up.
    #[arg(long, value_name = "POLICY")]
    pub cleanup_policy: Option<String>,

    /// Workflow role label for this candidate (e.g. `hero`, `fallback`).
    #[arg(long, value_name = "ROLE")]
    pub workspace_role: Option<String>,
}

/// Arguments for `zenith workspace scratch list`.
#[derive(Debug, Args)]
pub struct ScratchListArgs {
    /// Path to the `.zen` document.
    pub doc: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable listing.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith workspace scratch show`.
#[derive(Debug, Args)]
pub struct ScratchShowArgs {
    /// Path to the `.zen` document.
    pub doc: PathBuf,

    /// The candidate id to show (e.g. `cand0`).
    pub candidate: String,

    /// Emit machine-readable JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith workspace promote`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  \
zenith workspace promote poster.zen cand0 --into page.export\n  \
zenith workspace promote poster.zen cand0 --into page.export --id-suffix .v2")]
pub struct PromoteArgs {
    /// Path to the deliverable `.zen` document (written in-place).
    pub doc: PathBuf,

    /// The candidate id to promote (must have status `selected`).
    pub candidate: String,

    /// Id of the target page in the deliverable document to merge content into.
    #[arg(long, value_name = "PAGE_ID")]
    pub into: String,

    /// Suffix appended to every cloned node id to keep them unique (default: `.promoted`).
    #[arg(long, default_value = ".promoted", value_name = "SUFFIX")]
    pub id_suffix: String,
}

/// Arguments for `zenith workspace finalize`.
#[derive(Debug, Args)]
#[command(
    after_help = "EXAMPLE:\n  zenith workspace finalize poster.zen\n  zenith workspace finalize poster.zen --json"
)]
pub struct FinalizeArgs {
    /// Path to the `.zen` document whose scratch store to finalize.
    pub doc: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable report.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith workspace candidate`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  zenith workspace candidate poster.zen cand0 selected")]
pub struct CandidateArgs {
    /// Path to the `.zen` document.
    pub doc: PathBuf,

    /// The candidate id to transition (e.g. `cand0`).
    pub candidate: String,

    /// New lifecycle status: `draft`, `selected`, or `rejected`.
    pub status: String,
}
