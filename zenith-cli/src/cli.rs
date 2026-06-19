//! Clap argument types for the Zenith CLI.
//!
//! This module defines the top-level [`Cli`] struct and the [`Command`]
//! subcommand enum. No business logic lives here — just argument shapes.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Zenith — design-document toolchain.
#[derive(Debug, Parser)]
#[command(name = "zenith", about = "Zenith design-document toolchain")]
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

    /// 1-based page number to render (default: 1).
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub page: usize,

    /// Emit machine-readable JSON (diagnostics + output path) to stdout.
    #[arg(long)]
    pub json: bool,
}
