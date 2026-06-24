//! Clap argument types for the Zenith CLI.
//!
//! This module defines the top-level [`Cli`] struct and the [`Command`]
//! subcommand enum. No business logic lives here — just argument shapes.
//!
//! Per-command-group arg structs live in submodules and are re-exported here
//! so that all existing `crate::cli::*` paths continue to resolve unchanged.
//!
//! Submodules:
//! - [`library`] — `LibraryArgs`, `LibrarySub`, and library item arg types.
//! - [`plugin`] — `PluginArgs`, `PluginSub`, `ScopeArg`, `AgentFlags`, and install/uninstall args.
//! - [`render`] — `RenderArgs`.
//! - [`schema`] — `SchemaArgs`, `SchemaSub`.
//! - [`workspace`] — `WorkspaceArgs`, `WorkspaceSub`, scratch, candidate, and promote arg types.

mod library;
mod plugin;
mod render;
mod schema;
mod workspace;

pub use library::{LibraryAddArgs, LibraryArgs, LibraryListArgs, LibraryShowArgs, LibrarySub};
pub use plugin::{
    AgentFlags, PluginArgs, PluginInstallArgs, PluginSub, PluginUninstallArgs, ScopeArg,
};
pub use render::RenderArgs;
pub use schema::{SchemaArgs, SchemaSub};
pub use workspace::{
    CandidateArgs, PromoteArgs, ScratchArgs, ScratchListArgs, ScratchNewArgs, ScratchShowArgs,
    ScratchSub, WorkspaceArgs, WorkspaceSub,
};

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Zenith — design-document toolchain.
#[derive(Debug, Parser)]
#[command(
    name = "zenith",
    version,
    about = "Author, validate, and render deterministic .zen design documents (KDL → PNG/PDF).",
    long_about = "Zenith turns a design into plain-text .zen source (KDL) you can read, diff, \
validate, edit with typed transactions, and render deterministically to pixel-exact PNG or \
print-ready PDF — the opposite of a flat AI image.\n\n\
The core loop: author/edit source → `validate` → `render` to inspect → iterate. Edits to \
existing documents should go through `tx` (typed, dry-run by default). Every command accepts \
`--json` for machine-readable output; run `zenith <command> --help` for exact flags.",
    after_help = "QUICK START:\n  \
zenith validate poster.zen --json     # check for hard diagnostics\n  \
zenith render poster.zen --png out.png # render a page to inspect\n  \
zenith theme new acme --scheme light --primary '#3b5bdb' --out acme.zen\n  \
zenith plugin install --claude         # teach your AI agent to use zenith\n\n\
Run `zenith <command> --help` for details on any command."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a new minimal, valid `.zen` document at a fresh path.
    ///
    /// Writes a minimal valid document (one page on a white background) at the
    /// given path, with a `doc-id` already minted + stamped and its first
    /// version recorded in history — a ready-to-edit "File > New" starting point.
    /// A `.zen` extension is appended when the path has none, and any missing
    /// parent directories are created. Refuses to overwrite an existing file.
    /// The id slug is derived from `--name` when given, otherwise from the
    /// path's file stem; the default name is "Untitled".
    New(NewArgs),

    /// Validate a `.zen` document and report diagnostics.
    ///
    /// Validate a .zen document and report diagnostics. Hard (Error) diagnostics
    /// block rendering — always validate and fix them before `render`. Exit code is non-zero when
    /// hard diagnostics are present.
    Validate(ValidateArgs),

    /// Format a `.zen` document in-place (idempotent).
    Fmt(FmtArgs),

    /// List all design tokens and their resolved values.
    ///
    /// List every design token and its resolved value. Visual properties must
    /// reference tokens, so this is how you discover the palette/type/spacing a document exposes
    /// before authoring or editing nodes.
    Tokens(TokensArgs),

    /// Compile and render a `.zen` document.
    ///
    /// Compile and render a .zen document to PNG, PDF, or a scene display-list.
    /// Rendering is deterministic (same source + backend → same bytes) and is blocked by hard
    /// diagnostics, so `validate` first. Use `--all-pages <DIR>` for a contact sheet and `--spread
    /// A-B` for facing pages.
    Render(RenderArgs),

    /// Apply a transaction to a `.zen` document (dry-run by default).
    ///
    /// Apply a typed transaction (a JSON edit script) to a .zen document. This is
    /// the preferred way to edit existing documents: it is dry-run by default (shows a source + scene
    /// diff), enforces id-uniqueness and referential integrity, and only writes with `--apply`.
    Tx(TxArgs),

    /// Print the node tree of a `.zen` document (read-only).
    ///
    /// Print the structure of a .zen document (read-only): the node tree plus
    /// document-level blocks such as the `recipes` provenance block. Use it to discover node ids before
    /// writing a `tx` edit, to see which recipes a document declares, or to confirm what it contains.
    Inspect(InspectArgs),

    /// Mail-merge a `.zen` template with a CSV data file, writing one PNG per row.
    ///
    /// Mail-merge a .zen template with a CSV, writing one PNG per row. Mark variable
    /// nodes with role="data.<column>" (text nodes substitute their text; image nodes substitute
    /// their asset path) where <column> matches a CSV header. Use this for localized posts,
    /// personalized graphics, certificates, badges, and campaign variants.
    Merge(MergeArgs),

    /// Discover and materialize reusable design assets (tokens, components, actions).
    ///
    /// Manages library packs: embedded `@zenith/*` presets and project-local packs
    /// in `libraries/*.zen`. Run `zenith library list` to discover, `zenith library
    /// show <pkg>#<item>` to inspect, and `zenith library add <pkg>#<item> --into
    /// <doc.zen>` to materialize a token, component, or action into a document.
    Library(LibraryArgs),

    /// List a document's version history.
    ///
    /// History is automatic: every `tx --apply` and edit is recorded in a durable
    /// per-document version store kept beside the file (content-addressed; off the
    /// render path). This lists those revisions with their version ids. The related
    /// commands operate on that same store: `undo`/`redo` step the session, `version`
    /// names the current state, `restore <rev>` rewinds to a past one, and `sync`
    /// records an out-of-band external edit. Use the version ids shown here as the
    /// `<rev>` argument to `restore`.
    History(HistoryArgs),

    /// Undo the last edit, rewriting the document in place.
    Undo(UndoArgs),

    /// Redo the last undone edit, rewriting the document in place.
    Redo(RedoArgs),

    /// Save the current document as a named version.
    Version(VersionArgs),

    /// Restore the document to a past version.
    ///
    /// Rewinds the document to a past revision and rewrites it in place. The `<rev>`
    /// argument accepts: a version id as listed by `zenith history` (e.g. `v2`);
    /// `@head` or `@head~N` (the current head, or N steps back); `@latest:<name>`
    /// (the most recent version saved under that name via `zenith version`); or a
    /// bare version name. Run `zenith history` first to see the available revisions.
    Restore(RestoreArgs),

    /// Capture the document's current on-disk state into history as an external
    /// change (e.g. after a GUI edit, hand-edit, or `git checkout`).
    Sync(SyncArgs),

    /// Generate size/format variants of a document (one design → many sizes).
    ///
    /// Expands the `variants` block: one canonical page becomes N named target sizes (square,
    /// story, banner), each written as a native `.zen` page plus a rendered PNG. Per-variant
    /// `override`s can hide/show nodes, swap text, or change a fill; source token edits propagate
    /// to every variant. This varies DIMENSIONS — distinct from `merge`, which varies CONTENT
    /// across CSV rows. Deterministic: same source → byte-identical outputs.
    Variant(VariantArgs),

    /// Update the installed `zenith` binary to a published release.
    Update(UpdateArgs),

    /// Generate design themes (token packs) from brand colours.
    Theme(ThemeArgs),

    /// Install the Zenith agent skill into AI coding tools (Claude Code, Codex, OpenCode, …).
    Plugin(PluginArgs),

    /// Run Zenith as an MCP server over stdio (for remote/CI/server agents).
    ///
    /// Run Zenith as a Model Context Protocol (MCP) server over stdio, exposing the
    /// command surface (validate, inspect, tokens, fmt, render, tx, merge, theme) as MCP tools for any
    /// MCP-aware client.
    ///
    /// For a LOCAL agent, prefer installing the CLI and the skill
    /// (`zenith plugin install`) and running commands directly. This MCP server is for
    /// environments where a local binary is not suitable (remote, CI, sandboxed, hosted agents) —
    /// and it is a first-class surface there: tools return trimmed structured results, fetch schema
    /// detail on demand (`zenith_schema`), hand back large/binary artifacts as resource links, and
    /// drive the full scratch/candidate/promote/finalize workspace loop by doc-id.
    /// Defaults to the stdio transport; pass `--http <ADDR>` for native Streamable-HTTP
    /// (requires the `http` build feature).
    Mcp(McpArgs),

    /// Describe the Zenith document schema (node kinds, attributes, tx ops, and non-node surfaces).
    ///
    /// Self-describing source of truth for agents and tooling. Reports every
    /// authorable node kind with its one-line summary and recognized attribute
    /// names, every transaction op with its summary, and the recognized
    /// attributes for the non-node authorable surfaces (page, asset, document).
    /// Attribute types, required-ness, and valid values are enforced at
    /// document-level by `zenith validate` — run that command for the full
    /// diagnostic loop.
    ///
    /// Subcommands: `nodes` (all kinds), `node <kind>` (one kind + its
    /// attributes), `ops` (all tx ops), `op <name>` (one op: summary,
    /// fields, and a working JSON example), `page`, `asset`, `document`
    /// (non-node surface attributes).
    /// Bare `zenith schema` prints a short overview with counts and drill-in hints.
    Schema(SchemaArgs),

    /// Manage workspace-level process state: scratch candidates and their lifecycle.
    ///
    /// The workspace subsystem persists design scratch candidates — point-in-time
    /// `.zen` snapshots that are evaluated and promoted or rejected — alongside
    /// the durable version history. Use `zenith workspace scratch` to record and
    /// inspect candidates; use `zenith workspace candidate` to transition their
    /// lifecycle status (draft → selected | rejected).
    Workspace(WorkspaceArgs),
}

/// Arguments for `zenith mcp`.
#[derive(Debug, Args)]
#[command(
    after_help = "Configure your MCP client to launch `zenith mcp` (command: \"zenith\", args: \
[\"mcp\"]). Logs go to stderr; stdout carries the JSON-RPC protocol."
)]
pub struct McpArgs {
    /// Serve over native Streamable-HTTP at this address (e.g. 127.0.0.1:8080)
    /// instead of stdio. Requires a build with the `http` feature.
    #[arg(long, value_name = "ADDR")]
    pub http: Option<String>,
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
    ///
    /// Synthesize a complete theme pack (a token-only .zen) from brand colours.
    /// Surfaces are tinted toward the primary; each role gets an APCA-correct `.content` pairing for
    /// WCAG 3 contrast. Captures radius, border, spacing, type, and optional depth/noise — not just
    /// colour. The output validates clean and can be merged into a document or used as a starting palette.
    New(ThemeNewArgs),
}

/// Arguments for `zenith theme new`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  \
zenith theme new acme --scheme light --primary '#3b5bdb' --accent '#f76707' --out acme.zen\n\n\
NOTE: quote every hex value — a bare # starts a comment in most shells, so an\n  \
unquoted --primary #3b5bdb is silently dropped and reads as a missing value.")]
pub struct ThemeNewArgs {
    /// Theme name (used in ids and the preview title), e.g. `acme`.
    pub name: String,

    /// Base scheme: `light` or `dark`.
    #[arg(long, value_name = "light|dark")]
    pub scheme: String,

    /// Primary brand colour as `#rrggbb`. Quote it: most shells treat a bare
    /// `#` as the start of a comment, so write `--primary '#3b5bdb'` (or
    /// `"#3b5bdb"`) — an unquoted `#3b5bdb` is dropped and this flag will
    /// appear to have no value.
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

/// Arguments for `zenith variant`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  \
zenith variant poster.zen --out-dir out/ --manifest run.json\n\n\
The document must contain a `variants { variant id=\"square\" source=\"page.main\" w=(px)1080 \
h=(px)1080 { … } }` block.")]
pub struct VariantArgs {
    /// Input `.zen` document containing a `variants` block.
    pub doc: PathBuf,

    /// Directory to write one `.zen` + one `.png` per generated variant into.
    #[arg(long, value_name = "DIR")]
    pub out_dir: PathBuf,

    /// Emit a machine-readable JSON batch report (per-variant provenance).
    #[arg(long)]
    pub json: bool,

    /// Write a deterministic generation manifest (JSON) to this path for CI
    /// reproducibility.  Independent of --json.
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,
}

/// Arguments for `zenith new`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  zenith new poster.zen --name \"Launch Poster\"")]
pub struct NewArgs {
    /// Path to create the new document at (must not already exist). A `.zen`
    /// extension is appended if absent, and missing parent directories are created.
    pub path: PathBuf,

    /// Display name for the document (used in ids and the title). Defaults to
    /// "Untitled"; the id slug is derived from this, else from the file stem.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
}

/// Arguments for `zenith validate`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLE:\n  zenith validate poster.zen --json")]
pub struct ValidateArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,

    /// Suppress a diagnostic code (downgrade Warning/Advisory to nothing).
    ///
    /// Repeatable. Overrides the document's in-file `diagnostics` block and any
    /// global/local config policy for this code. Error-severity diagnostics are
    /// immutable and never suppressed.
    #[arg(long = "allow", value_name = "CODE", action = clap::ArgAction::Append)]
    pub allow: Vec<String>,

    /// Force a diagnostic code to Warning severity.
    ///
    /// Repeatable. Overrides the document's in-file `diagnostics` block and any
    /// global/local config policy for this code.
    #[arg(long = "warn", value_name = "CODE", action = clap::ArgAction::Append)]
    pub warn: Vec<String>,

    /// Elevate a diagnostic code to a blocking Error (CI gate).
    ///
    /// Repeatable. Overrides the document's in-file `diagnostics` block and any
    /// global/local config policy for this code.
    #[arg(long = "deny", value_name = "CODE", action = clap::ArgAction::Append)]
    pub deny: Vec<String>,
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
#[command(after_help = "EXAMPLE:\n  zenith tokens poster.zen --json")]
pub struct TokensArgs {
    /// Path to the `.zen` document.
    pub path: PathBuf,

    /// Emit machine-readable JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `zenith tx`.
#[derive(Debug, Args)]
#[command(after_help = "TRANSACTION FILE FORMAT:\n  \
A tx file is a JSON object with a single \"ops\" array; ops are applied in order:\n\n  \
    {\"ops\":[\n      \
{\"op\":\"set_text_align\",\"node\":\"text.hello\",\"align\":\"center\"},\n      \
{\"op\":\"set_fill\",\"node\":\"hero\",\"fill\":\"color.brand\"}\n    \
]}\n\n\
DISCOVERING OPS:\n  \
zenith schema op set_fill          # fields, types, and a working example\n  \
zenith schema op add_node          # how to insert a new node from .zen source\n  \
zenith schema ops                  # list all 40 available ops with summaries\n  \
See examples/*.tx.json for runnable samples.\n\n\
EXAMPLES:\n  \
zenith tx poster.zen edits.json            # preview the diff (dry-run)\n  \
zenith tx poster.zen edits.json --apply    # write the change to disk")]
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
#[command(after_help = "EXAMPLE:\n  zenith inspect poster.zen --node hero --json")]
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
#[command(after_help = "EXAMPLE:\n  \
zenith merge card.zen people.csv --out-dir out/ --name-by name --manifest run.json")]
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
