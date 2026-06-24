//! Execute an MCP tool call by reusing the CLI's `commands::*` logic.
//!
//! This is the filesystem/I/O edge for the MCP server (the same role `lib.rs`
//! plays for the CLI): it reads inputs, calls the pure command functions, writes
//! any outputs, and returns a text result.

use std::path::Path;

use serde_json::Value;
use zenith_core::Severity;

use crate::commands::{self, format_diagnostic_line, theme};

/// The text result of a tool call and whether it represents an error.
pub struct CallResult {
    pub text: String,
    pub is_error: bool,
}

impl CallResult {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }
    fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// Dispatch a tool call. Unknown names return an error result.
pub fn call(name: &str, args: &Value) -> CallResult {
    let result = match name {
        "zenith_validate" => run_validate(args),
        "zenith_inspect" => run_inspect(args),
        "zenith_tokens" => run_tokens(args),
        "zenith_fmt" => run_fmt(args),
        "zenith_render" => run_render(args),
        "zenith_tx" => run_tx(args),
        "zenith_merge" => run_merge(args),
        "zenith_theme_new" => run_theme_new(args),
        other => Err(format!("unknown tool '{other}'")),
    };
    match result {
        Ok(text) => CallResult::ok(text),
        Err(text) => CallResult::err(text),
    }
}

// ── Tools ───────────────────────────────────────────────────────────────────

fn run_validate(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let src = read(path)?;
    // MCP has no policy flags; config files are still resolved by `run`.
    let flags = crate::config::CliPolicyFlags::default();
    let out = commands::validate::run(&src, Path::new(path).parent(), flag(args, "json"), &flags);
    Ok(out.stdout)
}

fn run_inspect(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let src = read(path)?;
    commands::inspect::run(&src, opt_str(args, "node"), flag(args, "json")).map_err(|e| e.message)
}

fn run_tokens(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let src = read(path)?;
    commands::tokens::list(&src, flag(args, "json")).map_err(|(m, _)| m)
}

fn run_fmt(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let src = read(path)?;
    let result = commands::fmt::run(&src).map_err(|e| e.message)?;
    std::fs::write(path, &result.formatted).map_err(|e| format!("error writing '{path}': {e}"))?;
    Ok(commands::fmt::render_stdout(&result, false))
}

fn run_render(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let out = req_str(args, "out")?;
    let format = req_str(args, "format")?;
    let page = opt_u64(args, "page").unwrap_or(1).max(1) as usize;
    let locked = flag(args, "locked");
    let parent = Path::new(path).parent();
    let src = read(path)?;
    // MCP has no policy flags; config files (global + local) are still resolved
    // by parse_validate using the document's parent directory.
    let flags = crate::config::CliPolicyFlags::default();

    match format {
        "png" => {
            let art = commands::render::to_png_with_dir(&src, parent, page, locked, &flags)
                .map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            std::fs::write(out, &art.png).map_err(|e| format!("error writing '{out}': {e}"))?;
            Ok(report_write("PNG", out, &art.diagnostics))
        }
        "pdf" => {
            let art = commands::render::to_pdf_with_dir(&src, parent, page, locked, &flags)
                .map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            std::fs::write(out, &art.pdf).map_err(|e| format!("error writing '{out}': {e}"))?;
            Ok(report_write("PDF", out, &art.diagnostics))
        }
        "scene" => {
            let art = commands::render::to_scene_json(&src, parent, page, &flags)
                .map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            std::fs::write(out, art.json.as_bytes())
                .map_err(|e| format!("error writing '{out}': {e}"))?;
            Ok(report_write("scene", out, &art.diagnostics))
        }
        other => Err(format!(
            "invalid format '{other}' (expected png, pdf, or scene)"
        )),
    }
}

fn run_tx(args: &Value) -> Result<String, String> {
    let path = req_str(args, "path")?;
    let src = read(path)?;
    let tx_json = match args.get("transaction") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string(v).map_err(|e| e.to_string())?,
        None => return Err("missing 'transaction'".into()),
    };
    let outcome = commands::tx::run(&src, &tx_json).map_err(|e| e.message)?;

    if flag(args, "apply") && outcome.exit_code != 1 {
        std::fs::write(path, outcome.result.source_after.as_bytes())
            .map_err(|e| format!("error writing '{path}': {e}"))?;
    }
    if flag(args, "json") {
        Ok(outcome.json_str)
    } else {
        Ok(outcome.human)
    }
}

fn run_merge(args: &Value) -> Result<String, String> {
    let doc = req_str(args, "doc")?;
    let data = req_str(args, "data")?;
    let out_dir = req_str(args, "out_dir")?;
    let name_by = opt_str(args, "name_by");
    let doc_src = read(doc)?;
    let csv_src = read(data)?;

    let report = commands::merge::run(
        &doc_src,
        &csv_src,
        Path::new(doc).parent(),
        Path::new(out_dir),
        name_by,
    )
    .map_err(|e| e.message)?;

    if let Some(manifest) = opt_str(args, "manifest") {
        let m = commands::merge::build_manifest(&doc_src, &csv_src, name_by, &report);
        let json = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
        std::fs::write(manifest, json).map_err(|e| format!("error writing '{manifest}': {e}"))?;
    }

    let written = report.rows.iter().filter(|r| r.failure.is_none()).count();
    let failed: Vec<String> = report
        .rows
        .iter()
        .filter(|r| r.failure.is_some())
        .map(|r| format!("row {}: {}", r.row + 1, r.failure.as_deref().unwrap_or("")))
        .collect();
    let mut msg = format!("wrote {written} file(s) to '{out_dir}'");
    if !failed.is_empty() {
        msg.push('\n');
        msg.push_str(&failed.join("\n"));
    }
    Ok(msg)
}

fn run_theme_new(args: &Value) -> Result<String, String> {
    let name = req_str(args, "name")?;
    let primary = req_str(args, "primary")?;
    let scheme = match req_str(args, "scheme")? {
        "light" => zenith_core::theme::Scheme::Light,
        "dark" => zenith_core::theme::Scheme::Dark,
        other => return Err(format!("scheme must be 'light' or 'dark', got '{other}'")),
    };
    let input = theme::ThemeInput {
        name,
        scheme,
        primary,
        secondary: opt_str(args, "secondary"),
        accent: opt_str(args, "accent"),
        neutral: opt_str(args, "neutral"),
        info: opt_str(args, "info"),
        success: opt_str(args, "success"),
        warning: opt_str(args, "warning"),
        error: opt_str(args, "error"),
        shape: theme::Shape {
            radius_box: opt_f64(args, "radius_box").unwrap_or(16.0),
            radius_field: opt_f64(args, "radius_field").unwrap_or(8.0),
            radius_selector: opt_f64(args, "radius_selector").unwrap_or(8.0),
            border: opt_f64(args, "border").unwrap_or(1.0),
            depth: flag(args, "depth"),
            noise: flag(args, "noise"),
        },
    };
    let source = theme::new(&input).map_err(|e| e.message)?;
    match opt_str(args, "out") {
        Some(out) => {
            std::fs::write(out, &source).map_err(|e| format!("error writing '{out}': {e}"))?;
            Ok(format!("wrote {out}"))
        }
        None => Ok(source),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string '{key}'"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn opt_f64(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("error reading '{path}': {e}"))
}

/// Fail the call when any diagnostic is a hard (Error) diagnostic.
fn blocked(diagnostics: &[zenith_core::Diagnostic]) -> Result<(), String> {
    let hard: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(format_diagnostic_line)
        .collect();
    if hard.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "render blocked by {} hard diagnostic(s):\n{}",
            hard.len(),
            hard.join("\n")
        ))
    }
}

/// Build a success message listing the output path and any soft diagnostics.
fn report_write(kind: &str, out: &str, diagnostics: &[zenith_core::Diagnostic]) -> String {
    let mut msg = format!("{kind} written to '{out}'");
    let soft: Vec<String> = diagnostics.iter().map(format_diagnostic_line).collect();
    if !soft.is_empty() {
        msg.push('\n');
        msg.push_str(&soft.join("\n"));
    }
    msg
}
