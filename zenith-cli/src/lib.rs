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
pub mod history;
pub mod json_types;
pub mod library;
pub mod mcp;
pub mod selfupdate;

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
            if args.scene.is_none()
                && args.png.is_none()
                && args.pdf.is_none()
                && args.all_pages.is_none()
            {
                eprintln!(
                    "error: at least one of --scene <OUT>, --png <OUT>, --pdf <OUT>, or --all-pages <DIR> is required"
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

            // --spread ────────────────────────────────────────────────────────
            // When set, parse the "A-B" page pair up front and render a single
            // composited PNG to the --png target. --spread requires --png; it is
            // mutually exclusive with --pdf (deferred) and takes over the --png
            // target (the normal single-page --png render is skipped below).
            if let Some(spread_spec) = &args.spread {
                let png_out = match &args.png {
                    Some(p) => p,
                    None => {
                        eprintln!("error: --spread requires --png <OUT>");
                        return ExitCode::from(2);
                    }
                };
                if args.pdf.is_some() {
                    eprintln!("error: --spread cannot be combined with --pdf (not supported)");
                    return ExitCode::from(2);
                }
                let (page_a, page_b) = match parse_spread_spec(spread_spec) {
                    Ok(pair) => pair,
                    Err(msg) => {
                        eprintln!("{}", msg);
                        return ExitCode::from(2);
                    }
                };
                match commands::render::to_png_spread(
                    &src,
                    args.path.parent(),
                    page_a,
                    page_b,
                    args.gutter,
                    args.locked,
                ) {
                    Ok(artifact) => {
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
                            println!("spread PNG written to '{}'", png_out.display());
                            print_diagnostics_stderr(&artifact.diagnostics);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.message);
                        return ExitCode::from(e.exit_code);
                    }
                }
            }

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
            // Skipped when --spread is active: the spread branch above already
            // wrote the composited image to the --png target.
            if let (Some(png_out), None) = (&args.png, &args.spread) {
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

            // --pdf ───────────────────────────────────────────────────────────
            if let Some(pdf_out) = &args.pdf {
                match commands::render::to_pdf_with_dir(
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
                        if let Err(e) = write_bytes(pdf_out, &artifact.pdf) {
                            eprintln!("error writing PDF to '{}': {}", pdf_out.display(), e);
                            return ExitCode::from(2);
                        }
                        if args.json {
                            let out = RenderOutput {
                                schema: "zenith-render-v1",
                                diagnostics: artifact.diagnostics.iter().map(Into::into).collect(),
                            };
                            println!("{}", serialize_pretty(&out));
                        } else {
                            println!("PDF written to '{}'", pdf_out.display());
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

        Command::Merge(args) => {
            // Read the template document.
            let doc_src = match read_file(&args.doc) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            // Read the CSV file.
            let csv_src = match read_file(&args.data) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            let project_dir = args.doc.parent();

            match commands::merge::run(
                &doc_src,
                &csv_src,
                project_dir,
                &args.out_dir,
                args.name_by.as_deref(),
            ) {
                Ok(report) => {
                    if args.json {
                        println!(
                            "{}",
                            serialize_pretty(&commands::merge::to_json_output(&report))
                        );
                    } else {
                        let n_written = report.rows.iter().filter(|r| r.failure.is_none()).count();
                        println!(
                            "wrote {} file(s) to '{}'",
                            n_written,
                            args.out_dir.display()
                        );
                        for r in report.failed() {
                            eprintln!("row {}: {}", r.row + 1, r.failure.as_deref().unwrap_or(""));
                        }
                    }
                    if let Some(manifest_path) = &args.manifest {
                        let manifest = commands::merge::build_manifest(
                            &doc_src,
                            &csv_src,
                            args.name_by.as_deref(),
                            &report,
                        );
                        let manifest_json = serialize_pretty(&manifest);
                        if let Some(parent) = manifest_path.parent()
                            && !parent.as_os_str().is_empty()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            eprintln!(
                                "error creating manifest directory '{}': {}",
                                parent.display(),
                                e
                            );
                            return ExitCode::from(2);
                        }
                        if let Err(e) = std::fs::write(manifest_path, manifest_json.as_bytes()) {
                            eprintln!(
                                "error writing manifest '{}': {}",
                                manifest_path.display(),
                                e
                            );
                            return ExitCode::from(2);
                        }
                    }
                    let n_failed = report.rows.iter().filter(|r| r.failure.is_some()).count();
                    if n_failed == 0 {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1u8)
                    }
                }
                Err(e) => {
                    eprintln!("{}", e.message);
                    ExitCode::from(e.exit_code)
                }
            }
        }

        Command::Library(args) => match args.command {
            cli::LibrarySub::List(list_args) => {
                // Resolve the project directory: if `path` names an existing
                // file (e.g. a `.zen`), use its parent; if it names a directory,
                // use it directly; if omitted, use the current working directory.
                let project_dir = resolve_project_dir(list_args.path.as_deref());
                let packs = library::resolve_packs(project_dir.as_deref());
                println!("{}", commands::library::list(&packs, list_args.json));
                ExitCode::SUCCESS
            }

            cli::LibrarySub::Add(add_args) => {
                // Parse the optional `--at "X,Y"` origin up front.
                let at = match parse_at_spec(add_args.at.as_deref()) {
                    Ok(pair) => pair,
                    Err(msg) => {
                        eprintln!("{}", msg);
                        return ExitCode::from(2);
                    }
                };

                let target_src = match read_file(&add_args.into) {
                    Ok(s) => s,
                    Err(msg) => {
                        eprintln!("{}", msg);
                        return ExitCode::from(2);
                    }
                };

                // The project dir is the --into file's parent directory.
                let project_dir = add_args
                    .into
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(std::path::Path::to_path_buf);

                match commands::library::add(
                    &target_src,
                    &add_args.spec,
                    project_dir.as_deref(),
                    add_args.page.as_deref(),
                    at,
                    add_args.id.as_deref(),
                ) {
                    Ok(result) => {
                        if add_args.dry_run {
                            // Print the resulting source WITHOUT writing.
                            match String::from_utf8(result.formatted) {
                                Ok(s) => print!("{}", s),
                                Err(_) => {
                                    eprintln!("error: formatted output is not valid UTF-8");
                                    return ExitCode::from(2);
                                }
                            }
                        } else {
                            let recorded = history::record_edit(
                                &result.formatted,
                                &add_args.into,
                                "library.add",
                            );
                            if let Some(w) = &recorded.warning {
                                eprintln!("warning: {w}");
                            }
                            if let Err(e) = std::fs::write(&add_args.into, &recorded.bytes) {
                                eprintln!("error writing '{}': {}", add_args.into.display(), e);
                                return ExitCode::from(2);
                            }
                            println!("{}", result.summary);
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{}", e.message);
                        ExitCode::from(e.exit_code)
                    }
                }
            }
        },

        Command::History(args) => {
            match history::history_view(&args.path) {
                Ok(view) => {
                    if args.json {
                        let versions_json: Vec<serde_json::Value> = view
                            .versions
                            .iter()
                            .map(|v| {
                                serde_json::json!({
                                    "id": v.id,
                                    "seq": v.seq,
                                    "label": v.label,
                                    "op_kind": v.op_kind,
                                    "timestamp_ms": v.timestamp_ms,
                                })
                            })
                            .collect();
                        let obj = serde_json::json!({
                            "doc_id": view.doc_id,
                            "has_session": view.has_session,
                            "versions": versions_json,
                        });
                        match serde_json::to_string_pretty(&obj) {
                            Ok(s) => println!("{}", s),
                            Err(_) => {
                                // Fallback to text if JSON serialisation fails.
                                println!("doc-id: {}", view.doc_id);
                                for v in &view.versions {
                                    let label = v.label.as_deref().unwrap_or("");
                                    let op = v.op_kind.as_deref().unwrap_or("");
                                    println!("{:>4}  {}  {} {}", v.seq, v.id, op, label);
                                }
                            }
                        }
                    } else {
                        println!("doc-id: {}", view.doc_id);
                        if view.versions.is_empty() {
                            println!("(no versions recorded yet)");
                        } else {
                            for v in &view.versions {
                                let label = v.label.as_deref().unwrap_or("");
                                let op = v.op_kind.as_deref().unwrap_or("");
                                println!("{:>4}  {}  {} {}", v.seq, v.id, op, label);
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(msg) => {
                    eprintln!("{}", msg);
                    ExitCode::from(2)
                }
            }
        }

        Command::Undo(args) => match history::undo_edit(&args.path) {
            Ok(history::NavOutcome::Moved) => {
                println!("undid last edit to '{}'", args.path.display());
                ExitCode::SUCCESS
            }
            Ok(history::NavOutcome::NothingToDo) => {
                println!("nothing to undo");
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("{}", msg);
                ExitCode::from(2)
            }
        },

        Command::Redo(args) => match history::redo_edit(&args.path) {
            Ok(history::NavOutcome::Moved) => {
                println!("redid last undone edit to '{}'", args.path.display());
                ExitCode::SUCCESS
            }
            Ok(history::NavOutcome::NothingToDo) => {
                println!("nothing to redo");
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("{}", msg);
                ExitCode::from(2)
            }
        },

        Command::Version(args) => match history::name_version(&args.path, &args.name) {
            Ok(id) => {
                println!("saved version '{}' as {}", args.name, id);
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::from(2)
            }
        },

        Command::Restore(args) => match history::restore(&args.path, &args.rev) {
            Ok(outcome) => {
                if let Some(w) = &outcome.warning {
                    eprintln!("warning: {w}");
                }
                println!(
                    "restored '{}' to {}",
                    args.path.display(),
                    outcome.version_id
                );
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::from(2)
            }
        },

        Command::Sync(args) => match history::sync_external(&args.path) {
            Ok(history::SyncOutcome::Captured { id }) => {
                println!("captured external change as {id}");
                ExitCode::SUCCESS
            }
            Ok(history::SyncOutcome::AlreadyInSync) => {
                println!("already in sync");
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::from(2)
            }
        },

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
            if args.apply && outcome.exit_code != 1 {
                let recorded = history::record_edit(
                    outcome.result.source_after.as_bytes(),
                    &args.path,
                    "tx.apply",
                );
                if let Some(w) = &recorded.warning {
                    eprintln!("warning: {w}");
                }
                if let Err(e) = std::fs::write(&args.path, &recorded.bytes) {
                    eprintln!("error writing '{}': {}", args.path.display(), e);
                    return ExitCode::from(2);
                }
            }

            ExitCode::from(outcome.exit_code)
        }

        Command::Update(args) => match selfupdate::run(args.pre, args.version.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("error: {msg}");
                ExitCode::from(2)
            }
        },

        Command::Theme(args) => match args.command {
            cli::ThemeSub::New(a) => {
                let scheme = match a.scheme.as_str() {
                    "light" => zenith_core::theme::Scheme::Light,
                    "dark" => zenith_core::theme::Scheme::Dark,
                    other => {
                        eprintln!("error: --scheme must be 'light' or 'dark', got '{other}'");
                        return ExitCode::from(2);
                    }
                };
                let input = commands::theme::ThemeInput {
                    name: &a.name,
                    scheme,
                    primary: &a.primary,
                    secondary: a.secondary.as_deref(),
                    accent: a.accent.as_deref(),
                    neutral: a.neutral.as_deref(),
                    info: a.info.as_deref(),
                    success: a.success.as_deref(),
                    warning: a.warning.as_deref(),
                    error: a.error.as_deref(),
                    shape: commands::theme::Shape {
                        radius_box: a.radius_box,
                        radius_field: a.radius_field,
                        radius_selector: a.radius_selector,
                        border: a.border,
                        depth: a.depth,
                        noise: a.noise,
                    },
                };
                match commands::theme::new(&input) {
                    Ok(source) => {
                        if let Some(path) = &a.out {
                            if let Err(e) = std::fs::write(path, &source) {
                                eprintln!("error writing '{}': {}", path.display(), e);
                                return ExitCode::from(2);
                            }
                            println!("wrote {}", path.display());
                        } else {
                            print!("{source}");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("error: {}", e.message);
                        ExitCode::from(e.exit_code)
                    }
                }
            }
        },

        Command::Plugin(args) => {
            let project_root = std::path::Path::new(".");
            match args.command {
                cli::PluginSub::Install(a) => {
                    let targets = targets_from_flags(&a.agents);
                    let code = commands::plugin::run_install(
                        project_root,
                        targets,
                        scope_from_arg(a.scope),
                        a.force,
                        a.dry_run,
                    );
                    ExitCode::from(code)
                }
                cli::PluginSub::Uninstall(a) => {
                    let targets = targets_from_flags(&a.agents);
                    let code = commands::plugin::run_uninstall(
                        project_root,
                        targets,
                        scope_from_arg(a.scope),
                        a.dry_run,
                    );
                    ExitCode::from(code)
                }
                cli::PluginSub::List => ExitCode::from(commands::plugin::run_list(project_root)),
            }
        }

        Command::Mcp(_) => ExitCode::from(mcp::run()),
    }
}

/// Map the CLI scope flag to the plugin module's [`Scope`](commands::plugin::Scope).
fn scope_from_arg(scope: cli::ScopeArg) -> commands::plugin::Scope {
    match scope {
        cli::ScopeArg::User => commands::plugin::Scope::User,
        cli::ScopeArg::Project => commands::plugin::Scope::Project,
    }
}

/// Translate the per-agent boolean flags into a [`Targets`](commands::plugin::Targets)
/// selection. `--all` wins; no flag set means auto-detect.
fn targets_from_flags(f: &cli::AgentFlags) -> commands::plugin::Targets {
    use commands::plugin::{Agent, Targets};
    if f.all {
        return Targets::All;
    }
    let mut agents = Vec::new();
    let mut push = |on: bool, a: Agent| {
        if on {
            agents.push(a);
        }
    };
    push(f.claude, Agent::ClaudeCode);
    push(f.codex, Agent::Codex);
    push(f.opencode, Agent::OpenCode);
    push(f.cursor, Agent::Cursor);
    push(f.windsurf, Agent::Windsurf);
    push(f.aider, Agent::Aider);
    push(f.zed, Agent::Zed);
    push(f.gemini, Agent::Gemini);
    push(f.copilot, Agent::Copilot);
    push(f.continue_dev, Agent::Continue);
    push(f.kiro, Agent::Kiro);
    push(f.antigravity, Agent::Antigravity);
    if agents.is_empty() {
        Targets::Auto
    } else {
        Targets::Agents(agents)
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

/// Parse a `--spread` spec of the form `"A-B"` (two 1-based page numbers) into
/// `(a, b)`.
///
/// Returns a human-readable error message (never panics) when the spec is not
/// exactly two dash-separated positive integers.
fn parse_spread_spec(spec: &str) -> Result<(usize, usize), String> {
    let err = || {
        format!(
            "error: invalid --spread value {:?} (expected two 1-based page \
             numbers like \"10-11\")",
            spec
        )
    };
    let (a_str, b_str) = spec.split_once('-').ok_or_else(err)?;
    let a: usize = a_str.trim().parse().map_err(|_| err())?;
    let b: usize = b_str.trim().parse().map_err(|_| err())?;
    if a == 0 || b == 0 {
        return Err(err());
    }
    Ok((a, b))
}

/// Parse an `--at` spec of the form `"X,Y"` (two finite floats) into `(x, y)`.
///
/// `None` (the flag was omitted) defaults to `(0.0, 0.0)`. Returns a human-
/// readable error (never panics) when the value is not exactly two
/// comma-separated finite numbers.
fn parse_at_spec(spec: Option<&str>) -> Result<(f64, f64), String> {
    let spec = match spec {
        None => return Ok((0.0, 0.0)),
        Some(s) => s,
    };
    let err = || {
        format!(
            "error: invalid --at value {:?} (expected two comma-separated \
             numbers like \"120,80\")",
            spec
        )
    };
    let (x_str, y_str) = spec.split_once(',').ok_or_else(err)?;
    let x: f64 = x_str.trim().parse().map_err(|_| err())?;
    let y: f64 = y_str.trim().parse().map_err(|_| err())?;
    if !x.is_finite() || !y.is_finite() {
        return Err(err());
    }
    Ok((x, y))
}

/// Resolve the project directory for the library subsystem from an optional
/// `--path` argument.
///
/// - `None` → the current working directory (`.`).
/// - a path to an existing FILE (e.g. a `.zen`) → its parent directory.
/// - a path to an existing DIRECTORY → that directory.
/// - a non-existent path → its parent if it has one, else the path itself
///   (so a bare name like `proj.zen` still resolves to `.`).
///
/// Never panics; returns `None` only when no usable directory can be derived.
fn resolve_project_dir(path: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    use std::path::Path;
    match path {
        None => Some(std::path::PathBuf::from(".")),
        Some(p) if p.is_dir() => Some(p.to_path_buf()),
        // An existing file (e.g. a `.zen`) or a bare/non-existent name: use the
        // parent directory, falling back to `.` when there is none.
        Some(p) => Some(
            p.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
        ),
    }
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
