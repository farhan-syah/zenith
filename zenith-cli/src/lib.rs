//! Command-line interface library for Zenith.
//!
//! Owns all command dispatch, argument parsing (via clap), JSON I/O shaping,
//! and human-readable stdout/stderr formatting.
//!
//! `src/main.rs` is kept thin — it only calls [`run`].
//! `zenith-layout` is reached transitively through `zenith-scene`; the CLI
//! never constructs layout types directly.
//!
//! # Module layout
//!
//! - `cli` — clap `#[derive(Parser)]` types.
//! - `commands/` — one module per subcommand; all business logic is here,
//!   operating on in-memory bytes, never touching the FS.
//! - `json_types` — serialisable DTOs for JSON output.
//! - `lib.rs` — this file: wiring + `run()` dispatcher + file I/O edge.

pub mod cli;
pub mod commands;
pub mod json_types;

use std::io::Write as _;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::commands::serialize_pretty;
use crate::json_types::RenderOutput;

/// Main entry point: parse CLI arguments, dispatch to the appropriate command,
/// handle all file I/O, and return the appropriate exit code.
///
/// All business logic lives in `commands/`; this function is I/O only.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Validate(args) => {
            let src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };
            let out = commands::validate::run(&src, args.path.parent(), args.json);
            println!("{}", out.stdout);
            ExitCode::from(out.exit_code)
        }

        Command::Fmt(args) => {
            let src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };
            match commands::fmt::run(&src) {
                Ok(result) => {
                    // Write formatted content back to disk.
                    if let Err(e) = std::fs::write(&args.path, &result.formatted) {
                        eprintln!("error writing '{}': {}", args.path.display(), e);
                        return ExitCode::from(2);
                    }
                    println!("{}", commands::fmt::render_stdout(&result, args.json));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.message);
                    ExitCode::from(e.exit_code)
                }
            }
        }

        Command::Tokens(args) => {
            let src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };
            match commands::tokens::list(&src, args.json) {
                Ok(out) => {
                    println!("{}", out);
                    ExitCode::SUCCESS
                }
                Err((msg, code)) => {
                    eprintln!("{}", msg);
                    ExitCode::from(code)
                }
            }
        }

        Command::Render(args) => {
            // Require at least one output flag.
            if args.scene.is_none() && args.png.is_none() && args.all_pages.is_none() {
                eprintln!(
                    "error: at least one of --scene <OUT>, --png <OUT>, or --all-pages <DIR> is required"
                );
                return ExitCode::from(2);
            }

            let src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            // --scene ─────────────────────────────────────────────────────────
            if let Some(scene_out) = &args.scene {
                match commands::render::to_scene_json(&src, args.path.parent(), args.page) {
                    Ok(artifact) => {
                        // Block on hard (Error-severity) compile diagnostics.
                        let n_hard = count_hard_diagnostics(&artifact.diagnostics);
                        if n_hard > 0 {
                            print_diagnostics_stderr(&artifact.diagnostics);
                            eprintln!("render blocked by {} hard diagnostic(s)", n_hard);
                            return ExitCode::from(2);
                        }
                        if let Err(e) = std::fs::write(scene_out, artifact.json.as_bytes()) {
                            eprintln!("error writing scene to '{}': {}", scene_out.display(), e);
                            return ExitCode::from(2);
                        }
                        if args.json {
                            let out = RenderOutput {
                                schema: "zenith-render-v1",
                                diagnostics: artifact.diagnostics.iter().map(Into::into).collect(),
                            };
                            println!("{}", serialize_pretty(&out));
                        } else {
                            println!("scene written to '{}'", scene_out.display());
                            print_diagnostics_stderr(&artifact.diagnostics);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.message);
                        return ExitCode::from(e.exit_code);
                    }
                }
            }

            // --png ───────────────────────────────────────────────────────────
            if let Some(png_out) = &args.png {
                // Source image asset bytes relative to the .zen file's parent
                // directory so `image` nodes render their raster.
                match commands::render::to_png_with_dir(
                    &src,
                    args.path.parent(),
                    args.page,
                    args.locked,
                ) {
                    Ok(artifact) => {
                        // Block on hard (Error-severity) compile diagnostics.
                        let n_hard = count_hard_diagnostics(&artifact.diagnostics);
                        if n_hard > 0 {
                            print_diagnostics_stderr(&artifact.diagnostics);
                            eprintln!("render blocked by {} hard diagnostic(s)", n_hard);
                            return ExitCode::from(2);
                        }
                        if let Err(e) = write_bytes(png_out, &artifact.png) {
                            eprintln!("error writing PNG to '{}': {}", png_out.display(), e);
                            return ExitCode::from(2);
                        }
                        if args.json {
                            let out = RenderOutput {
                                schema: "zenith-render-v1",
                                diagnostics: artifact.diagnostics.iter().map(Into::into).collect(),
                            };
                            println!("{}", serialize_pretty(&out));
                        } else {
                            println!("PNG written to '{}'", png_out.display());
                            print_diagnostics_stderr(&artifact.diagnostics);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.message);
                        return ExitCode::from(e.exit_code);
                    }
                }
            }

            // --all-pages ─────────────────────────────────────────────────────
            if let Some(dir) = &args.all_pages {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    eprintln!("error creating directory '{}': {}", dir.display(), e);
                    return ExitCode::from(2);
                }
                match commands::render::to_png_all_pages(&src, args.path.parent(), args.locked) {
                    Ok(artifacts) => {
                        // Collect all diagnostics first; block if any are hard errors
                        // before writing any page to disk.
                        let all_diagnostics: Vec<&zenith_core::Diagnostic> = artifacts
                            .iter()
                            .flat_map(|a| a.diagnostics.iter())
                            .collect();
                        let n_hard = all_diagnostics
                            .iter()
                            .filter(|d| d.severity == zenith_core::Severity::Error)
                            .count();
                        if n_hard > 0 {
                            for d in &all_diagnostics {
                                eprintln!("{}", commands::format_diagnostic_line(d));
                            }
                            eprintln!("render blocked by {} hard diagnostic(s)", n_hard);
                            return ExitCode::from(2);
                        }
                        for (i, artifact) in artifacts.iter().enumerate() {
                            let page_path = dir.join(format!("page-{}.png", i + 1));
                            if let Err(e) = write_bytes(&page_path, &artifact.png) {
                                eprintln!("error writing PNG to '{}': {}", page_path.display(), e);
                                return ExitCode::from(2);
                            }
                        }
                        if args.json {
                            let out = RenderOutput {
                                schema: "zenith-render-v1",
                                diagnostics: all_diagnostics.iter().map(|d| (*d).into()).collect(),
                            };
                            println!("{}", serialize_pretty(&out));
                        } else {
                            println!("{} page(s) written to '{}'", artifacts.len(), dir.display());
                            for d in all_diagnostics {
                                eprintln!("{}", commands::format_diagnostic_line(d));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.message);
                        return ExitCode::from(e.exit_code);
                    }
                }
            }

            ExitCode::SUCCESS
        }

        Command::Inspect(args) => {
            let src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };
            match commands::inspect::run(&src, args.node.as_deref(), args.json) {
                Ok(out) => {
                    println!("{}", out);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.message);
                    ExitCode::from(e.exit_code)
                }
            }
        }

        Command::Tx(args) => {
            // Read document source.
            let doc_src = match read_file(&args.path) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            // Read transaction JSON.
            let tx_json = match read_file(&args.tx_file) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            // Run the pure transaction logic.
            let outcome = match commands::tx::run(&doc_src, &tx_json) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("{}", e.message);
                    return ExitCode::from(e.exit_code);
                }
            };

            // Print output.
            if args.json {
                println!("{}", outcome.json_str);
            } else {
                println!("{}", outcome.human);
            }

            // Apply: persist source_after if requested and not rejected.
            if args.apply
                && outcome.exit_code != 1
                && let Err(e) = std::fs::write(&args.path, outcome.result.source_after.as_bytes())
            {
                eprintln!("error writing '{}': {}", args.path.display(), e);
                return ExitCode::from(2);
            }

            ExitCode::from(outcome.exit_code)
        }
    }
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

/// Print compile-stage diagnostics (advisories/warnings) to stderr, one per
/// line, so they are surfaced without polluting the stdout success message.
/// Does nothing when there are no diagnostics.
fn print_diagnostics_stderr(diagnostics: &[zenith_core::Diagnostic]) {
    for d in diagnostics {
        eprintln!("{}", commands::format_diagnostic_line(d));
    }
}

/// Count diagnostics with [`Severity::Error`].
fn count_hard_diagnostics(diagnostics: &[zenith_core::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.severity == zenith_core::Severity::Error)
        .count()
}

// ── I/O helpers ───────────────────────────────────────────────────────────────

/// Read a file to a UTF-8 string.
///
/// Returns a human-readable error message on failure (never panics).
fn read_file(path: &std::path::Path) -> Result<String, String> {
    std::fs::read(path)
        .map_err(|e| format!("error reading '{}': {}", path.display(), e))
        .and_then(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| format!("error: '{}' is not valid UTF-8", path.display()))
        })
}

/// Write raw bytes to a file.
///
/// Returns a `std::io::Error` on failure.
fn write_bytes(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)
}
