//! Argument types for `zenith schema` and its subcommands.

use clap::{Args, Subcommand};

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

    /// Show the summary, JSON fields, and a working example for one transaction op.
    Op {
        /// The op name to look up (e.g. `set_fill`, `add_node`).
        name: String,
    },

    /// Show the recognized attributes for a `page` declaration.
    ///
    /// Lists every attribute the parser recognises on a `page` node:
    /// geometry (w, h), margins, bleed, baseline-grid, line-jumps, parity,
    /// and master.
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
