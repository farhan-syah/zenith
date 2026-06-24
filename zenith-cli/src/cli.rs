//! Clap argument types for the Zenith CLI.
//!
//! This module defines the top-level [`Cli`] struct and the [`Command`]
//! subcommand enum. No business logic lives here — just argument shapes.

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
    /// This is for remote, CI, or server contexts. For a LOCAL agent, prefer
    /// installing the CLI and the skill (`zenith plugin install`) and running commands directly — it is
    /// faster and cheaper on tokens than going through MCP.
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
    /// attributes), `ops` (all tx ops), `op <name>` (one op summary),
    /// `page`, `asset`, `document` (non-node surface attributes).
    /// Bare `zenith schema` prints a short overview with counts and drill-in hints.
    Schema(SchemaArgs),
}

/// Arguments for `zenith mcp`.
#[derive(Debug, Args)]
#[command(
    after_help = "Configure your MCP client to launch `zenith mcp` (command: \"zenith\", args: \
[\"mcp\"]). Logs go to stderr; stdout carries the JSON-RPC protocol."
)]
pub struct McpArgs {}

/// Arguments for `zenith plugin`.
#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub command: PluginSub,
}

/// Subcommands of `zenith plugin`.
#[derive(Debug, Subcommand)]
pub enum PluginSub {
    /// Install the skill for the given agents (auto-detects when none are named).
    ///
    /// Install the Zenith agent skill so AI coding tools know how to drive the
    /// `zenith` CLI. Claude Code, Codex, and OpenCode receive the full folder skill (SKILL.md plus
    /// reference packs, templates, and themes); other agents receive a single self-contained rule
    /// file that points back at this self-documenting CLI. Writes are idempotent. With no agent flag,
    /// the present agents are auto-detected.
    Install(PluginInstallArgs),

    /// Remove a previously installed skill for the given agents.
    Uninstall(PluginUninstallArgs),

    /// Show where the Zenith skill is installed, per agent and scope.
    List,
}

/// Installation scope for `zenith plugin`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ScopeArg {
    /// Install for the current user (e.g. `~/.claude/skills/…`).
    User,
    /// Install into the current project (e.g. `./.claude/skills/…`).
    Project,
}

/// Per-agent selection flags, shared by install and uninstall.
///
/// With no flag set, the command auto-detects which agents are present. `--all`
/// selects every supported agent.
#[derive(Debug, Args)]
pub struct AgentFlags {
    /// Target Claude Code (folder skill).
    #[arg(long)]
    pub claude: bool,
    /// Target Codex (folder skill).
    #[arg(long)]
    pub codex: bool,
    /// Target OpenCode (folder skill).
    #[arg(long)]
    pub opencode: bool,
    /// Target Cursor (project rule).
    #[arg(long)]
    pub cursor: bool,
    /// Target Windsurf (project rule).
    #[arg(long)]
    pub windsurf: bool,
    /// Target Aider (rule file).
    #[arg(long)]
    pub aider: bool,
    /// Target Zed (rule file).
    #[arg(long)]
    pub zed: bool,
    /// Target Gemini CLI (rule file).
    #[arg(long)]
    pub gemini: bool,
    /// Target GitHub Copilot (rule file).
    #[arg(long)]
    pub copilot: bool,
    /// Target Continue (rule file).
    #[arg(long = "continue")]
    pub continue_dev: bool,
    /// Target Kiro (steering rule).
    #[arg(long)]
    pub kiro: bool,
    /// Target Antigravity (rule file).
    #[arg(long)]
    pub antigravity: bool,
    /// Target every supported agent.
    #[arg(long)]
    pub all: bool,
}

/// Arguments for `zenith plugin install`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLES:\n  \
zenith plugin install                       # auto-detect and install for the user\n  \
zenith plugin install --claude --codex      # specific agents\n  \
zenith plugin install --all --scope project # everything, into ./\n  \
zenith plugin install --claude --dry-run    # preview without writing")]
pub struct PluginInstallArgs {
    #[command(flatten)]
    pub agents: AgentFlags,

    /// Install for the user (default) or the current project.
    #[arg(long, value_enum, default_value_t = ScopeArg::User)]
    pub scope: ScopeArg,

    /// Overwrite existing files whose content differs.
    #[arg(long)]
    pub force: bool,

    /// Show what would be written without touching the filesystem.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `zenith plugin uninstall`.
#[derive(Debug, Args)]
pub struct PluginUninstallArgs {
    #[command(flatten)]
    pub agents: AgentFlags,

    /// Uninstall from the user (default) or the current project.
    #[arg(long, value_enum, default_value_t = ScopeArg::User)]
    pub scope: ScopeArg,

    /// Show what would be removed without touching the filesystem.
    #[arg(long)]
    pub dry_run: bool,
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
zenith theme new acme --scheme light --primary '#3b5bdb' --accent '#f76707' --out acme.zen")]
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
#[command(after_help = "EXAMPLE:\n  zenith validate poster.zen --json")]
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
#[command(after_help = "EXAMPLE:\n  \
zenith tx poster.zen edits.json            # preview the diff\n  \
zenith tx poster.zen edits.json --apply    # write the change")]
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

/// Arguments for `zenith render`.
#[derive(Debug, Args)]
#[command(
    after_help = "At least one of --scene, --png, --pdf, or --all-pages is required.\n\n\
EXAMPLES:\n  \
zenith render poster.zen --png out.png\n  \
zenith render book.zen --all-pages sheet/      # one PNG per page\n  \
zenith render book.zen --pdf book.pdf          # print-ready vector PDF"
)]
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

/// Arguments for `zenith schema`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLES:\n  \
zenith schema                       # overview: counts + drill-in hints\n  \
zenith schema nodes                 # list all node kinds with summaries\n  \
zenith schema node pattern          # attributes for one node kind\n  \
zenith schema ops                   # list all transaction ops\n  \
zenith schema op set_fill           # summary for one op\n  \
zenith schema page                  # attributes for a page declaration\n  \
zenith schema asset                 # attributes for an asset declaration\n  \
zenith schema document              # attributes for the document root\n  \
zenith schema nodes --json          # machine-readable JSON")]
pub struct SchemaArgs {
    #[command(subcommand)]
    pub command: Option<SchemaSub>,

    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,
}

/// Subcommands of `zenith schema`.
#[derive(Debug, Subcommand)]
pub enum SchemaSub {
    /// List all authorable node kinds with their one-line summaries.
    Nodes,

    /// Show the summary and recognized attributes for one node kind.
    Node {
        /// The node kind to look up (e.g. `rect`, `text`, `pattern`).
        kind: String,
    },

    /// List all transaction ops with their one-line summaries.
    Ops,

    /// Show the summary for one transaction op.
    Op {
        /// The op name to look up (e.g. `set_fill`, `add_node`).
        name: String,
    },

    /// Show the recognized attributes for a `page` declaration.
    ///
    /// Lists every attribute the parser recognises on a `page` node:
    /// geometry (w, h), margins, bleed, baseline-grid, line-jumps, parity,
    /// master, and workflow-metadata (workspace-role, candidate-status, …).
    Page,

    /// Show the recognized attributes for an `asset` declaration.
    ///
    /// Lists every attribute the parser recognises on an `asset` node inside
    /// the `assets { … }` block: id, kind, src, sha256, and the full suite of
    /// AI-provenance fields (ai-prompt, ai-model, ai-provider, …).
    Asset,

    /// Show the recognized attributes for the document root (`zenith` node).
    ///
    /// Lists every attribute the parser recognises on the top-level `zenith`
    /// node and the `document { … }` child block: version, colorspace, doc-id,
    /// mirror-margins, page-progression, spread-gutter, margin-*, and more.
    Document,
}
