//! Dispatch logic for `zenith render`.

use std::process::ExitCode;

use zenith_core::DataContext;

use crate::cli::RenderArgs;
use crate::cli_helpers::{
    count_hard_diagnostics, parse_spread_spec, print_diagnostics_stderr, read_file, write_bytes,
};
use crate::commands;
use crate::commands::serialize_pretty;
use crate::config::CliPolicyFlags;
use crate::json_types::RenderOutput;

pub(super) fn dispatch_render(args: RenderArgs) -> ExitCode {
    // Require at least one output flag.
    if args.scene.is_none() && args.png.is_none() && args.pdf.is_none() && args.all_pages.is_none()
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

    let flags = CliPolicyFlags {
        allow: args.allow,
        warn: args.warn,
        deny: args.deny,
    };

    // --data ──────────────────────────────────────────────────────────
    // Load the data context once (if requested) and pass a reference to each
    // render entry function so `(data)"field"` refs resolve at compile time.
    let data_ctx: Option<DataContext> = match &args.data {
        Some(data_path) => match commands::render::load_data_context(data_path) {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                eprintln!("error: {}", e);
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let data: Option<&DataContext> = data_ctx.as_ref();

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
            commands::render::SpreadRenderOpts {
                locked: args.locked,
                flags: &flags,
                data,
            },
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
        match commands::render::to_scene_json(&src, args.path.parent(), args.page, &flags, data) {
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
            &flags,
            data,
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
            &flags,
            data,
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
        match commands::render::to_png_all_pages(
            &src,
            args.path.parent(),
            args.locked,
            &flags,
            data,
        ) {
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
