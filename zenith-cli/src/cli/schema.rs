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
zenith schema tokens                # list all token types with summaries\n  \
zenith schema token gradient        # value form + children + example for one token type\n  \
zenith schema page                  # attributes for a page declaration\n  \
zenith schema asset                 # attributes for an asset declaration\n  \
zenith schema document              # attributes for the document root\n  \
zenith schema diagnostics           # diagnostic-policy verbs + governable codes\n  \
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

    /// List all authorable token types with their one-line summaries.
    ///
    /// Shows every recognized `type=` value for a token node (color, dimension,
    /// fontFamily, fontWeight, gradient, shadow, filter, mask, number) with a
    /// one-line description. Use `zenith schema token <type>` for the full
    /// value form, child-node structure, and a working example.
    Tokens,

    /// Show the value form, child-node structure, and a working example for one token type.
    ///
    /// Describes exactly what to write for a given `type=` value: whether the
    /// token takes an inline `value=` literal (color, dimension, number,
    /// fontFamily, fontWeight) or is defined by child nodes (gradient, shadow,
    /// filter, mask), including the exact syntax for each.
    Token {
        /// The token type to look up (e.g. `color`, `gradient`, `shadow`).
        #[arg(value_name = "TYPE")]
        ty: String,
    },

    /// Show the in-file diagnostic-policy verbs and the governable diagnostic codes.
    ///
    /// Lists the three policy verbs (`allow`, `deny`, `warn`) usable inside a
    /// root `diagnostics { … }` block, what each does, the precedence note
    /// (in-file policy now; CLI flags/config resolve in a later unit), and the
    /// full list of governable diagnostic codes (code · severity · summary).
    /// Integrity Errors are listed as non-suppressible.
    Diagnostics,
}
