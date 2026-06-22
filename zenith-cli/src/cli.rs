//! Clap argument types for the Zenith CLI.
//!
//! This module defines the top-level [`Cli`] struct and the [`Command`]
//! subcommand enum. No business logic lives here — just argument shapes.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Zenith — design-document toolchain.
#[derive(Debug, Parser)]
#[command(name = "zenith", version, about = "Zenith design-document toolchain")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate a `.zen` document and report diagnostics.
    Validate(ValidateArgs),

    /// Format a `.zen` document in-place (idempotent).
    Fmt(FmtArgs),

    /// List all design tokens and their resolved values.
    Tokens(TokensArgs),

    /// Compile and render a `.zen` document.
    Render(RenderArgs),

    /// Apply a transaction to a `.zen` document (dry-run by default).
    Tx(TxArgs),

    /// Print the node tree of a `.zen` document (read-only).
    Inspect(InspectArgs),

    /// Mail-merge a `.zen` template with a CSV data file, writing one PNG per row.
    Merge(MergeArgs),

    /// Inspect the library subsystem (preset + project packs).
    Library(LibraryArgs),

    /// List a document's version history.
    History(HistoryArgs),

    /// Undo the last edit, rewriting the document in place.
    Undo(UndoArgs),

    /// Redo the last undone edit, rewriting the document in place.
    Redo(RedoArgs),

    /// Save the current document as a named version.
    Version(VersionArgs),

    /// Restore the document to a past version.
    Restore(RestoreArgs),

    /// Capture the document's current on-disk state into history as an external
    /// change (e.g. after a GUI edit, hand-edit, or `git checkout`).
    Sync(SyncArgs),

    /// Update the installed `zenith` binary to a published release.
    Update(UpdateArgs),

    /// Generate design themes (token packs) from brand colours.
    Theme(ThemeArgs),
}

/// Arguments for `zenith theme`.
#[derive(Debug, Args)]
pub struct ThemeArgs {
    #[command(subcommand)]
    pub command: ThemeSub,
}

/// Subcommands of `zenith theme`.
#[derive(Debug, Subcommand)]
pub enum ThemeSub {
    /// Synthesize a complete theme pack from a primary colour (+ optional roles).
    New(ThemeNewArgs),
}

/// Arguments for `zenith theme new`.
#[derive(Debug, Args)]
pub struct ThemeNewArgs {
    /// Theme name (used in ids and the preview title), e.g. `acme`.
    pub name: String,

    /// Base scheme: `light` or `dark`.
    #[arg(long, value_name = "light|dark")]
    pub scheme: String,

    /// Primary brand colour as `#rrggbb`.
    #[arg(long, value_name = "HEX")]
    pub primary: String,

    /// Secondary colour (default: same as primary).
    #[arg(long, value_name = "HEX")]
    pub secondary: Option<String>,

    /// Accent colour (default: same as secondary).
    #[arg(long, value_name = "HEX")]
    pub accent: Option<String>,

    /// Neutral colour (default: a tinted grey).
    #[arg(long, value_name = "HEX")]
    pub neutral: Option<String>,

    /// Override the info status colour.
    #[arg(long, value_name = "HEX")]
    pub info: Option<String>,

    /// Override the success status colour.
    #[arg(long, value_name = "HEX")]
    pub success: Option<String>,

    /// Override the warning status colour.
    #[arg(long, value_name = "HEX")]
    pub warning: Option<String>,

    /// Override the error status colour.
    #[arg(long, value_name = "HEX")]
    pub error: Option<String>,

    /// Box/card corner radius in px (default 16).
    #[arg(long, value_name = "PX", default_value_t = 16.0)]
    pub radius_box: f64,

    /// Field/button corner radius in px (default 8).
    #[arg(long, value_name = "PX", default_value_t = 8.0)]
    pub radius_field: f64,

    /// Selector/badge corner radius in px (default 8).
    #[arg(long, value_name = "PX", default_value_t = 8.0)]
    pub radius_selector: f64,

    /// Default border width in px (default 1).
    #[arg(long, value_name = "PX", default_value_t = 1.0)]
    pub border: f64,

    /// Emit a `shadow.depth` elevation token (raised look).
    #[arg(long)]
    pub depth: bool,

    /// Mark the theme as wanting a grain overlay (recorded in the header).
    #[arg(long)]
    pub noise: bool,

    /// Write to this path instead of stdout.
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

/// Arguments for `zenith update`.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Install the latest prerelease instead of the latest stable release.
    #[arg(long)]
    pub pre: bool,

    /// Install a specific version (e.g. `v0.1.0` or `0.1.0`) instead of the latest.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,
}

/// Arguments for `zenith library`.
#[derive(Debug, Args)]
pub struct LibraryArgs {
    #[command(subcommand)]
    pub command: LibrarySub,
}

/// Subcommands of `zenith library`.
#[derive(Debug, Subcommand)]
pub enum LibrarySub {
    /// List all resolved library packs (project + embedded presets) and items.
    List(LibraryListArgs),

    /// Materialize a library item into a target `.zen` document.
    Add(LibraryAddArgs),
}

/// Arguments for `zenith library add`.
#[derive(Debug, Args)]
pub struct LibraryAddArgs {
    /// The item to add, as `<package>#<item>`, e.g. `@zenith/flowchart#decision`.
    pub spec: String,

    /// Target `.zen` document to materialize the item into (written in-place,
    /// unless `--dry-run`). Its parent directory is the project dir whose
    /// `libraries/*.zen` packs are resolved alongside the embedded presets.
    #[arg(long, value_name = "FILE")]
    pub into: PathBuf,

    /// Id of the page in the target document to place the instance on.
    ///
    /// Required only for COMPONENT items; TOKEN items (filter tokens) ignore it.
    #[arg(long, value_name = "ID")]
    pub page: Option<String>,

    /// Instance origin as `X,Y` in pixels (default `0,0`).
    #[arg(long, value_name = "X,Y")]
    pub at: Option<String>,

    /// Override the generated instance id base (default: the item name).
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    /// Print the resulting source to stdout WITHOUT writing the file.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `zenith library list`.
#[derive(Debug, Args)]
pub struct LibraryListArgs {
    /// Project directory, or a `.zen` file whose parent is the project directory.
    /// Project `libraries/*.zen` packs are scanned alongside embedded presets.
    /// Defaults to the current working directory.
    pub path: Option<PathBuf>,

    /// Emit machine-readable JSON instead of a human-readable listing.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith validate`.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith fmt`.
#[derive(Debug, Args)]
pub struct FmtArgs {
    /// Path to the `.zen` document (written in-place).
    pub path: PathBuf,

    /// Emit machine-readable JSON reporting `changed` and `hash`.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith tokens`.
#[derive(Debug, Args)]
pub struct TokensArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith tx`.
#[derive(Debug, Args)]
pub struct TxArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Path to the transaction JSON file.
    pub tx_file: PathBuf,

    /// Apply the result back to disk (dry-run by default).
    #[arg(long)]
    pub apply: bool,

    /// Emit machine-readable JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Inspect only the subtree rooted at this node id.
    #[arg(long, value_name = "ID")]
    pub node: Option<String>,

    /// Emit machine-readable JSON instead of a human-readable tree.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith merge`.
#[derive(Debug, Args)]
pub struct MergeArgs {
    /// Template `.zen` document with role="data.<column>" text nodes.
    pub doc: PathBuf,

    /// CSV data file; header row names the columns.
    pub data: PathBuf,

    /// Directory to write one PNG per row into.
    #[arg(long, value_name = "DIR")]
    pub out_dir: PathBuf,

    /// CSV column to name each output file by (default: row-NNNN.png).
    #[arg(long, value_name = "COL")]
    pub name_by: Option<String>,

    /// Emit a machine-readable JSON batch report (per-row provenance).
    #[arg(long)]
    pub json: bool,

    /// Write a deterministic generation manifest (JSON) to this path for CI
    /// reproducibility. Independent of --json.
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,
}

/// Arguments for `zenith render`.
#[derive(Debug, Args)]
#[command(after_help = "At least one of --scene or --png is required.")]
pub struct RenderArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Write the compiled scene display-list JSON to this path.
    #[arg(long, value_name = "OUT")]
    pub scene: Option<PathBuf>,

    /// Write the rendered PNG to this path.
    #[arg(long, value_name = "OUT")]
    pub png: Option<PathBuf>,

    /// Write a vector PDF (with print boxes + DeviceCMYK) to this path.
    #[arg(long, value_name = "OUT")]
    pub pdf: Option<PathBuf>,

    /// 1-based page number to render (default: 1).
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub page: usize,

    /// Render every page to `<DIR>/page-<N>.png` (1-based) instead of a single page.
    #[arg(long, value_name = "DIR")]
    pub all_pages: Option<PathBuf>,

    /// Render two facing pages side by side as a single PNG, e.g. `--spread 10-11`
    /// (1-based page numbers; A on the left, B on the right). Requires `--png`.
    #[arg(long, value_name = "A-B")]
    pub spread: Option<String>,

    /// Override the spread gutter in pixels (default: the document's spread-gutter, or 0).
    /// Only used when `--spread` is set.
    #[arg(long, value_name = "PX")]
    pub gutter: Option<u32>,

    /// Verify each image asset's bytes against its declared `sha256` and fail on mismatch.
    #[arg(long)]
    pub locked: bool,

    /// Emit machine-readable JSON (diagnostics + output path) to stdout.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith history`.
#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable listing.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith undo`.
#[derive(Debug, Args)]
pub struct UndoArgs {
    /// Path to the `.zen` document (rewritten in place).
    pub path: PathBuf,
}

/// Arguments for `zenith redo`.
#[derive(Debug, Args)]
pub struct RedoArgs {
    /// Path to the `.zen` document (rewritten in place).
    pub path: PathBuf,
}

/// Arguments for `zenith version`.
#[derive(Debug, Args)]
pub struct VersionArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,
    /// Name for this version (a named version is retained indefinitely).
    pub name: String,
}

/// Arguments for `zenith restore`.
#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,
    /// Revision spec (e.g. a version id `v2`, `@head~1`, `@latest:named`, or a name).
    pub rev: String,
}

/// Arguments for `zenith sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,
}
