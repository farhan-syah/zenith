use std::process::ExitCode;

use clap::Parser;

use crate::cli;
use crate::cli::{Cli, Command};
use crate::cli_helpers::{read_file, scope_from_arg, targets_from_flags};
use crate::commands::serialize_pretty;
use crate::{commands, history, mcp, selfupdate};

mod library;
mod render;
mod workspace;

/// Main entry point: parse CLI arguments, dispatch to the appropriate command,
/// handle all file I/O, and return the appropriate exit code.
///
/// All business logic lives in `commands/`; this function is I/O only.
pub fn run() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::New(args) => match commands::new::run(&args.path, args.name.as_deref()) {
            Ok(result) => {
                if let Some(w) = &result.warning {
                    eprintln!("warning: {w}");
                }
                println!(
                    "created '{}' (doc-id: {})",
                    result.path.display(),
                    result.doc_id
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}", e.message);
                ExitCode::from(e.exit_code)
            }
        },

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

        Command::Render(args) => render::dispatch_render(args),

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

        Command::Library(args) => library::dispatch_library(args),

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

        Command::Variant(args) => {
            let doc_src = match read_file(&args.doc) {
                Ok(s) => s,
                Err(msg) => {
                    eprintln!("{}", msg);
                    return ExitCode::from(2);
                }
            };

            // Derive the stem from the input filename (no extension).
            let stem = args
                .doc
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("doc");
            let project_dir = args.doc.parent();

            match commands::variant::run_variant(&doc_src, project_dir, &args.out_dir, stem) {
                Ok(report) => {
                    let failed = report.failed();
                    if args.json {
                        println!(
                            "{}",
                            serialize_pretty(&commands::variant::to_json_output(&report))
                        );
                    } else {
                        println!(
                            "generated {} variant(s) to '{}'",
                            report.generated(),
                            args.out_dir.display()
                        );
                        for r in &failed {
                            eprintln!("variant {}: {}", r.id, r.failure.as_deref().unwrap_or(""));
                        }
                    }
                    if let Some(manifest_path) = &args.manifest {
                        let manifest = commands::variant::build_manifest(&doc_src, &report);
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
                    if failed.is_empty() {
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

        Command::Schema(args) => {
            let json = args.json;
            let (output, code) = match args.command {
                None => commands::schema::overview(json),
                Some(cli::SchemaSub::Nodes) => commands::schema::nodes(json),
                Some(cli::SchemaSub::Node { kind }) => commands::schema::node_detail(&kind, json),
                Some(cli::SchemaSub::Ops) => commands::schema::ops(json),
                Some(cli::SchemaSub::Op { name }) => commands::schema::op_detail(&name, json),
                Some(cli::SchemaSub::Page) => commands::schema::page(json),
                Some(cli::SchemaSub::Asset) => commands::schema::asset(json),
                Some(cli::SchemaSub::Document) => commands::schema::document(json),
                Some(cli::SchemaSub::Diagnostics) => commands::schema::diagnostics(json),
            };
            println!("{}", output);
            ExitCode::from(code)
        }

        Command::Workspace(args) => workspace::dispatch_workspace(args),
    }
}
