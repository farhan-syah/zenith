//! Argument types for `zenith render`.

use clap::Args;
use std::path::PathBuf;

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

    /// Suppress a diagnostic code (downgrade Warning/Advisory to nothing).
    ///
    /// Repeatable. Overrides the document's in-file `diagnostics` block and any
    /// global/local config policy for this code.
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
