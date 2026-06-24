//! Execute an MCP tool call by reusing the CLI's `commands::*` logic and
//! shaping the result into a token-lean structured object.
//!
//! This is the filesystem/I/O edge for the MCP server (the same role `lib.rs`
//! plays for the CLI): it resolves the `doc` reference, reads inputs, calls the
//! pure command functions, writes any outputs, and returns a [`ToolResult`]
//! carrying a trimmed `structuredContent` object (never raw human stdout).

use std::path::Path;

use serde_json::{Value, json};

use super::doc_ref;
use super::protocol::ToolResult;
use super::resources::open_store;
use super::serialize::{compact, maybe_offload, store_link};
use crate::cli::ScratchNewArgs;
use crate::commands::{self, theme};

/// Dispatch a tool call, returning a structured result. Unknown names and
/// execution failures become `is_error` results (never JSON-RPC errors).
pub fn call(name: &str, args: &Value) -> ToolResult {
    let result = match name {
        "zenith_schema" => run_schema(args),
        "zenith_validate" => run_validate(args),
        "zenith_inspect" => run_inspect(args),
        "zenith_tokens" => run_tokens(args),
        "zenith_fmt" => run_fmt(args),
        "zenith_tx" => run_tx(args),
        "zenith_render" => run_render(args),
        "zenith_merge" => run_merge(args),
        "zenith_theme_new" => run_theme_new(args),
        "zenith_workspace_scratch" => run_workspace_scratch(args),
        "zenith_workspace_candidate" => run_workspace_candidate(args),
        "zenith_workspace_promote" => run_workspace_promote(args),
        "zenith_workspace_finalize" => run_workspace_finalize(args),
        other => Err(format!("unknown tool '{other}'")),
    };
    match result {
        Ok(value) => {
            let text = compact(&value);
            ToolResult::ok(value, text)
        }
        Err(message) => ToolResult::err(message),
    }
}

// ── Schema (progressive disclosure) ───────────────────────────────────────────

fn run_schema(args: &Value) -> Result<Value, String> {
    let surface = req_str(args, "surface")?;
    let name = opt_str(args, "name");
    let (text, code) = match surface {
        "overview" => commands::schema::overview(true),
        "nodes" => commands::schema::nodes(true),
        "node" => commands::schema::node_detail(need(name, "name (node kind)")?, true),
        "ops" => commands::schema::ops(true),
        "op" => commands::schema::op_detail(need(name, "name (op)")?, true),
        "page" => commands::schema::page(true),
        "asset" => commands::schema::asset(true),
        "document" => commands::schema::document(true),
        "diagnostics" => commands::schema::diagnostics(true),
        other => return Err(format!("unknown schema surface '{other}'")),
    };
    if code != 0 {
        return Err(text);
    }
    parse_json(&text)
}

// ── Read tools ────────────────────────────────────────────────────────────────

fn run_validate(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let src = read(&loc.path)?;
    let flags = crate::config::CliPolicyFlags::default();
    let out = commands::validate::run(&src, loc.path.parent(), true, &flags);
    let parsed = parse_json(&out.stdout)?;
    let diags = parsed
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let threshold = opt_str(args, "severity").unwrap_or("error");
    let min = severity_rank(threshold);
    let filtered: Vec<Value> = diags
        .iter()
        .filter(|d| {
            severity_rank(d.get("severity").and_then(Value::as_str).unwrap_or("error")) >= min
        })
        .map(trim_diagnostic)
        .collect();

    Ok(json!({
        "valid": parsed.get("valid").and_then(Value::as_bool).unwrap_or(false),
        "error_count": count_severity(&diags, "error"),
        "warning_count": count_severity(&diags, "warning"),
        "advisory_count": count_severity(&diags, "advisory"),
        "diagnostics": filtered,
    }))
}

fn run_inspect(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let src = read(&loc.path)?;
    let depth = opt_u64(args, "depth").unwrap_or(1) as usize;
    let detail = flag(args, "detail");
    let value = commands::inspect::summary(&src, opt_str(args, "node"), depth, detail)
        .map_err(|e| e.message)?;
    Ok(maybe_offload(
        loc.doc_id.as_deref(),
        value,
        "json",
        "inspect",
    ))
}

fn run_tokens(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let src = read(&loc.path)?;
    let text = commands::tokens::list(&src, true).map_err(|(m, _)| m)?;
    let parsed = parse_json(&text)?;
    let diags = parsed
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = json!({
        "tokens": parsed.get("tokens").cloned().unwrap_or(json!([])),
        "error_count": count_severity(&diags, "error"),
    });
    if flag(args, "diagnostics") {
        insert(&mut out, "diagnostics", Value::Array(diags));
    }
    Ok(out)
}

// ── Edit tools ────────────────────────────────────────────────────────────────

fn run_fmt(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let src = read(&loc.path)?;
    let result = commands::fmt::run(&src).map_err(|e| e.message)?;
    std::fs::write(&loc.path, &result.formatted)
        .map_err(|e| format!("error writing '{}': {e}", loc.path.display()))?;
    Ok(json!({ "changed": result.changed, "hash": result.hash }))
}

fn run_tx(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let src = read(&loc.path)?;
    let tx_json = match args.get("transaction") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string(v).map_err(|e| e.to_string())?,
        None => return Err("missing 'transaction'".into()),
    };
    let outcome = commands::tx::run(&src, &tx_json).map_err(|e| e.message)?;

    if flag(args, "apply") && outcome.exit_code != 1 {
        std::fs::write(&loc.path, outcome.result.source_after.as_bytes())
            .map_err(|e| format!("error writing '{}': {e}", loc.path.display()))?;
    }

    let parsed = parse_json(&outcome.json_str)?;
    let diags = parsed
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = json!({
        "status": parsed.get("status").cloned().unwrap_or(Value::Null),
        "changed": parsed.get("changed").and_then(Value::as_bool).unwrap_or(false),
        "affected": parsed.get("affected").cloned().unwrap_or(json!([])),
        "error_count": count_severity(&diags, "error"),
    });
    if flag(args, "diff") {
        let after = outcome.result.source_after.as_bytes();
        let link = match loc.doc_id.as_deref() {
            Some(id) => store_link(id, after, "zen", "tx-after")?,
            // No identity to anchor a resource: inline the resulting source.
            None => Value::String(outcome.result.source_after.clone()),
        };
        insert(&mut out, "after_source", link);
    }
    Ok(out)
}

fn run_render(args: &Value) -> Result<Value, String> {
    let (path, doc_id) = doc_ref::ensure(req_str(args, "doc")?)?;
    let format = req_str(args, "format")?;
    let page = opt_u64(args, "page").unwrap_or(1).max(1) as usize;
    let locked = flag(args, "locked");
    let parent = path.parent();
    let src = read(&path)?;

    let (bytes, ext, mime_diags): (Vec<u8>, &str, Vec<zenith_core::Diagnostic>) = match format {
        "png" => {
            let art = commands::render::to_png_with_dir(&src, parent, page, locked)
                .map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            (art.png, "png", art.diagnostics)
        }
        "pdf" => {
            let art = commands::render::to_pdf_with_dir(&src, parent, page, locked)
                .map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            (art.pdf, "pdf", art.diagnostics)
        }
        "scene" => {
            let art = commands::render::to_scene_json(&src, parent, page).map_err(|e| e.message)?;
            blocked(&art.diagnostics)?;
            (art.json.into_bytes(), "json", art.diagnostics)
        }
        other => {
            return Err(format!(
                "invalid format '{other}' (expected png, pdf, or scene)"
            ));
        }
    };

    // Optional caller-chosen path, plus a stable per-doc preview file.
    if let Some(out) = opt_str(args, "out") {
        std::fs::write(out, &bytes).map_err(|e| format!("error writing '{out}': {e}"))?;
    }
    write_preview(&doc_id, page, ext, &bytes);

    let link = store_link(&doc_id, &bytes, ext, &format!("render-{format}"))?;
    let mut out = json!({
        "format": format,
        "resource": link,
        "blocked": false,
        "error_count": 0,
        "warning_count": count_diag_severity(&mime_diags, zenith_core::Severity::Warning),
    });
    if flag(args, "diagnostics") {
        let diags: Vec<Value> = mime_diags
            .iter()
            .map(|d| json!({ "code": d.code, "severity": severity_word(d.severity), "message": d.message }))
            .collect();
        insert(&mut out, "diagnostics", Value::Array(diags));
    }
    Ok(out)
}

// ── Authoring tools ───────────────────────────────────────────────────────────

fn run_merge(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let data = req_str(args, "data")?;
    let out_dir = req_str(args, "out_dir")?;
    let name_by = opt_str(args, "name_by");
    let doc_src = read(&loc.path)?;
    let csv_src = read(Path::new(data))?;

    let report = commands::merge::run(
        &doc_src,
        &csv_src,
        loc.path.parent(),
        Path::new(out_dir),
        name_by,
    )
    .map_err(|e| e.message)?;

    if let Some(manifest) = opt_str(args, "manifest") {
        let m = commands::merge::build_manifest(&doc_src, &csv_src, name_by, &report);
        let txt = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
        std::fs::write(manifest, txt).map_err(|e| format!("error writing '{manifest}': {e}"))?;
    }

    let failures: Vec<Value> = report
        .rows
        .iter()
        .filter_map(|r| {
            r.failure
                .as_ref()
                .map(|f| json!({ "row": r.row + 1, "error": f }))
        })
        .collect();
    let written = report.rows.iter().filter(|r| r.failure.is_none()).count();
    Ok(json!({
        "total_rows": report.rows.len(),
        "written": written,
        "failed": failures.len(),
        "failures": failures,
    }))
}

fn run_theme_new(args: &Value) -> Result<Value, String> {
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
            Ok(json!({ "written": out }))
        }
        None => Ok(json!({ "source": source })),
    }
}

// ── Workspace tools ───────────────────────────────────────────────────────────

fn run_workspace_scratch(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    match req_str(args, "op")? {
        "new" => {
            let doc_bytes = std::fs::read(&loc.path)
                .map_err(|e| format!("error reading '{}': {e}", loc.path.display()))?;
            let scratch_args = ScratchNewArgs {
                doc: loc.path.clone(),
                page: opt_str(args, "page").map(str::to_owned),
                status: opt_str(args, "status").unwrap_or("draft").to_owned(),
                notes: opt_str(args, "notes").map(str::to_owned),
                promotion_target: opt_str(args, "promotion_target").map(str::to_owned),
                cleanup_policy: opt_str(args, "cleanup_policy").map(str::to_owned),
                workspace_role: opt_str(args, "workspace_role").map(str::to_owned),
            };
            let outcome = commands::workspace::scratch_new(&doc_bytes, &loc.path, &scratch_args)?;
            let doc_id = crate::history::read_doc_id(&loc.path)?;
            let mut out = json!({
                "candidate_id": outcome.id,
                "candidate_uri": format!("zenith://doc/{doc_id}/candidate/{}", outcome.id),
            });
            if let Some(w) = outcome.warning {
                insert(&mut out, "warning", Value::String(w));
            }
            Ok(out)
        }
        "list" => {
            let text = commands::workspace::scratch_list(&loc.path, true)?;
            let parsed = parse_json(&text)?;
            let trimmed: Vec<Value> = parsed
                .as_array()
                .map(|a| a.iter().map(trim_candidate).collect())
                .unwrap_or_default();
            Ok(json!({ "candidates": trimmed }))
        }
        "show" => {
            let cand = req_str(args, "candidate_id")?;
            let text = commands::workspace::scratch_show(&loc.path, cand, true)?;
            parse_json(&text)
        }
        other => Err(format!(
            "unknown scratch op '{other}' (expected new, list, show)"
        )),
    }
}

fn run_workspace_candidate(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let cand = req_str(args, "candidate_id")?;
    let status = req_str(args, "status")?;
    commands::workspace::candidate_set_status(&loc.path, cand, status)?;
    Ok(json!({ "candidate_id": cand, "status": status }))
}

fn run_workspace_promote(args: &Value) -> Result<Value, String> {
    let loc = doc_ref::locate(req_str(args, "doc")?)?;
    let cand = req_str(args, "candidate_id")?;
    let target = req_str(args, "target_page")?;
    let suffix = opt_str(args, "id_suffix").unwrap_or(".promoted");
    commands::workspace::promote(&loc.path, cand, target, suffix)?;
    Ok(json!({ "status": "promoted", "candidate_id": cand, "target_page": target }))
}

fn run_workspace_finalize(args: &Value) -> Result<Value, String> {
    match req_str(args, "op")? {
        "finalize" => {
            let loc = doc_ref::locate(req_str(args, "doc")?)?;
            let text = commands::workspace::finalize(&loc.path, true)?;
            parse_json(&text)
        }
        "bundle" => {
            let loc = doc_ref::locate(req_str(args, "doc")?)?;
            let bundle = req_str(args, "bundle")?;
            commands::workspace::bundle_doc(&loc.path, Path::new(bundle))?;
            Ok(json!({ "bundled": true, "path": bundle }))
        }
        "unbundle" => {
            let bundle = req_str(args, "bundle")?;
            let doc_id = commands::workspace::unbundle_doc(Path::new(bundle))?;
            Ok(json!({ "doc_id": doc_id }))
        }
        other => Err(format!("unknown finalize op '{other}'")),
    }
}

// ── Result-shaping helpers ────────────────────────────────────────────────────

/// Best-effort write of a render to the per-doc workspace preview dir.
fn write_preview(doc_id: &str, page: usize, ext: &str, bytes: &[u8]) {
    let Ok(paths) = open_store() else { return };
    let dir = paths.workspace_renders_dir(doc_id);
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(format!("page-{page}.{ext}")), bytes);
    }
}

/// Trim a diagnostic JSON object to `{code, message, subject_id?}`.
fn trim_diagnostic(d: &Value) -> Value {
    let mut out = json!({
        "code": d.get("code").cloned().unwrap_or(Value::Null),
        "message": d.get("message").cloned().unwrap_or(Value::Null),
    });
    if let Some(s) = d.get("subject_id").filter(|v| !v.is_null()) {
        insert(&mut out, "subject_id", s.clone());
    }
    out
}

/// Trim a candidate JSON entry to the listing shape.
fn trim_candidate(c: &Value) -> Value {
    let mut out = json!({
        "id": c.get("id").cloned().unwrap_or(Value::Null),
        "status": c.get("status").cloned().unwrap_or(Value::Null),
        "page_id": c.get("page_id").cloned().unwrap_or(Value::Null),
    });
    for key in ["workspace_role", "notes"] {
        if let Some(v) = c.get(key).filter(|v| !v.is_null()) {
            insert(&mut out, key, v.clone());
        }
    }
    out
}

fn count_severity(diags: &[Value], sev: &str) -> usize {
    diags
        .iter()
        .filter(|d| d.get("severity").and_then(Value::as_str) == Some(sev))
        .count()
}

fn count_diag_severity(diags: &[zenith_core::Diagnostic], sev: zenith_core::Severity) -> usize {
    diags.iter().filter(|d| d.severity == sev).count()
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "advisory" => 0,
        "warning" => 1,
        "error" => 2,
        _ => 2,
    }
}

fn severity_word(s: zenith_core::Severity) -> &'static str {
    match s {
        zenith_core::Severity::Error => "error",
        zenith_core::Severity::Warning => "warning",
        zenith_core::Severity::Advisory => "advisory",
    }
}

/// Fail the call when any diagnostic is a hard (Error) diagnostic.
fn blocked(diagnostics: &[zenith_core::Diagnostic]) -> Result<(), String> {
    let hard: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.severity == zenith_core::Severity::Error)
        .map(commands::format_diagnostic_line)
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

// ── Argument helpers ──────────────────────────────────────────────────────────

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string '{key}'"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn need<'a>(value: Option<&'a str>, what: &str) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("missing required '{what}'"))
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

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("error reading '{}': {e}", path.display()))
}

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("internal: malformed JSON from command: {e}"))
}

/// Insert `key`/`value` into a JSON object, ignoring non-object values.
fn insert(target: &mut Value, key: &str, value: Value) {
    if let Some(obj) = target.as_object_mut() {
        obj.insert(key.to_owned(), value);
    }
}
