//! Argument types for `zenith library` and its subcommands.

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Arguments for `zenith library`.
///
/// The library subsystem is a set of reusable **packs** — collections of design
/// assets that you materialize into a `.zen` document.  A pack is identified by
/// a package id such as `@zenith/filters`, and each pack exports one or more
/// named **items** addressed as `<package>#<item>` (e.g.
/// `@zenith/flowchart#decision`).
///
/// Three item kinds exist:
///
/// * **token** — a filter or mask token (e.g. `@zenith/filters#sepia`).  Added
///   to the document's `tokens` block; apply with `filter=(token)"sepia"` or
///   `mask=(token)"vignette"` on any node.
/// * **component** — a reusable node group (e.g. a flowchart shape) that is
///   materialized as an `instance` on a named page.  Requires `--page <id>`.
/// * **action** — a canned transaction op sequence (e.g.
///   `@zenith/brand-kit#apply-2026`) that mutates the target document's tokens
///   or layout.  No page required.
///
/// Embedded `@zenith/*` packs are bundled in the binary; project-local packs
/// live in `<project-dir>/libraries/*.zen` and shadow the embedded ones.
///
/// WORKFLOW:
///   zenith library list                          # discover packs + items
///   zenith library show @zenith/filters#sepia    # inspect one item
///   zenith library add @zenith/filters#sepia --into poster.zen
#[derive(Debug, Args)]
#[command(
    long_about = "Manage reusable library packs (embedded @zenith/* presets + project-local packs).\n\n\
A pack exports items addressed as <package>#<item>, e.g. `@zenith/flowchart#decision`.\n\
Item kinds:\n  \
token     — filter or mask token; copy into tokens block, apply with filter=(token)\"id\"\n  \
component — reusable node group; materialized as an instance on a page (requires --page)\n  \
action    — canned tx op sequence; runs a transaction against the target document\n\n\
Embedded @zenith/* packs are built in; project packs live in libraries/*.zen and shadow them.\n\n\
WORKFLOW:\n  \
zenith library list                          # discover packs and items\n  \
zenith library show @zenith/filters#sepia    # inspect item content before adding\n  \
zenith library add @zenith/filters#sepia --into poster.zen"
)]
pub struct LibraryArgs {
    #[command(subcommand)]
    pub command: LibrarySub,
}

/// Subcommands of `zenith library`.
#[derive(Debug, Subcommand)]
pub enum LibrarySub {
    /// List all resolved library packs (project + embedded presets) and items.
    ///
    /// Lists every available pack and its exported items.  Run `zenith library
    /// show <package>#<item>` to inspect any item in detail before adding it.
    List(LibraryListArgs),

    /// Inspect a library item in detail before adding it.
    ///
    /// Shows the package, item id, and kind-specific content: filter/mask token
    /// types and ops, component node structure, or action op sequence.  Prints
    /// the exact `zenith library add` invocation to materialize the item.
    Show(LibraryShowArgs),

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

/// Arguments for `zenith library show`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLES:\n  \
zenith library show @zenith/filters#sepia       # inspect a filter token\n  \
zenith library show @zenith/flowchart#decision  # inspect a component\n  \
zenith library show @zenith/brand-kit#apply-2026 --json")]
pub struct LibraryShowArgs {
    /// The item to inspect, as `<package>#<item>`, e.g. `@zenith/filters#sepia`.
    pub spec: String,

    /// Project directory, or a `.zen` file whose parent is the project directory.
    /// Project `libraries/*.zen` packs are resolved alongside embedded presets.
    /// Defaults to the current working directory.
    pub path: Option<PathBuf>,

    /// Emit machine-readable JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}
