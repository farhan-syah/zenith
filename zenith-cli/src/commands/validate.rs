//! Pure logic for `zenith validate`.
//!
//! The public entry point [`run`] operates entirely on in-memory source text;
//! the caller is responsible for all filesystem I/O.

use std::path::Path;

use zenith_core::{KdlAdapter, KdlSource, Severity, validate};

use crate::commands::render::{
    collect_image_dimension_diagnostics, collect_missing_asset_diagnostics,
};
use crate::commands::serialize_pretty;
use crate::json_types::{DiagnosticJson, ValidateOutput};

// ── Result type ───────────────────────────────────────────────────────────────

/// The outcome of a validate run.
#[derive(Debug)]
pub struct CmdOutput {
    /// Text to write to stdout.
    pub stdout: String,
    /// Exit code: 0 = no errors, 1 = validation errors, 2 = parse/io error.
    pub exit_code: u8,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Validate `src` and return formatted output.
///
/// When `project_dir` is `Some` (the `.zen` file's parent directory), each
/// declared asset's file is checked for existence and a hard `asset.missing`
/// Error diagnostic is added for any that are absent. When `None`, no asset
/// files are checked.
///
/// - Parse errors produce `exit_code = 2`.
/// - Documents with at least one error-severity diagnostic produce
///   `exit_code = 1`.
/// - Clean documents produce `exit_code = 0`.
pub fn run(src: &str, project_dir: Option<&Path>, json: bool) -> CmdOutput {
    // Parse ─────────────────────────────────────────────────────────────────
    let doc = match KdlAdapter.parse(src.as_bytes()) {
        Ok(d) => d,
        Err(e) => {
            let msg = if json {
                let out = ValidateOutput {
                    schema: "zenith-validate-v1",
                    valid: false,
                    diagnostics: vec![DiagnosticJson {
                        code: "parse.error".to_owned(),
                        severity: "error".to_owned(),
                        message: e.message.clone(),
                        subject_id: None,
                    }],
                };
                serialize_pretty(&out)
            } else {
                format!("error[parse.error]: {}", e.message)
            };
            return CmdOutput {
                stdout: msg,
                exit_code: 2,
            };
        }
    };

    // Validate ───────────────────────────────────────────────────────────────
    let mut diagnostics = validate(&doc).diagnostics;
    if let Some(dir) = project_dir {
        diagnostics.extend(collect_missing_asset_diagnostics(&doc, dir));
        diagnostics.extend(collect_image_dimension_diagnostics(&doc, dir));
    }
    let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);

    let stdout = if json {
        let out = ValidateOutput {
            schema: "zenith-validate-v1",
            valid: !has_errors,
            diagnostics: diagnostics.iter().map(DiagnosticJson::from).collect(),
        };
        serialize_pretty(&out)
    } else {
        format_human(&diagnostics)
    };

    CmdOutput {
        stdout,
        exit_code: if has_errors { 1 } else { 0 },
    }
}

// ── Human-readable formatter ──────────────────────────────────────────────────

fn format_human(diagnostics: &[zenith_core::Diagnostic]) -> String {
    if diagnostics.is_empty() {
        return "ok — no diagnostics".to_owned();
    }
    diagnostics
        .iter()
        .map(crate::commands::format_diagnostic_line)
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_DOC: &str = r##"zenith version=1 {
  project id="proj.v" name="Validate Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.accent" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.v" title="Validate Test" {
    page id="page.v" w=(px)320 h=(px)200 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)320 h=(px)200 fill=(token)"color.bg"
      rect id="rect.accent" x=(px)40 y=(px)40 w=(px)240 h=(px)120 fill=(token)"color.accent"
    }
  }
}
"##;

    const DUP_ID_DOC: &str = r##"zenith version=1 {
  project id="proj.d" name="Dup"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
    token id="color.bg" type="color" value="#000000"
  }
  styles {}
  document id="doc.d" title="Dup" {
    page id="page.d" w=(px)100 h=(px)100 {
      rect id="rect.d" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.bg"
    }
  }
}
"##;

    #[test]
    fn valid_doc_exits_zero() {
        let out = run(VALID_DOC, None, false);
        assert_eq!(out.exit_code, 0, "stdout: {}", out.stdout);
    }

    #[test]
    fn valid_doc_human_output_is_ok() {
        let out = run(VALID_DOC, None, false);
        assert!(
            out.stdout.contains("ok"),
            "expected 'ok' in human output; got: {}",
            out.stdout
        );
    }

    #[test]
    fn duplicate_id_exits_one() {
        let out = run(DUP_ID_DOC, None, false);
        assert_eq!(out.exit_code, 1, "stdout: {}", out.stdout);
    }

    #[test]
    fn duplicate_id_reports_id_duplicate_code() {
        let out = run(DUP_ID_DOC, None, false);
        assert!(
            out.stdout.contains("id.duplicate") || out.stdout.contains("token.duplicate_id"),
            "expected duplicate diagnostic code; got: {}",
            out.stdout
        );
    }

    #[test]
    fn valid_doc_json_has_schema_field() {
        let out = run(VALID_DOC, None, true);
        assert!(
            out.stdout.contains("zenith-validate-v1"),
            "JSON must contain schema field; got: {}",
            out.stdout
        );
    }

    #[test]
    fn valid_doc_json_valid_true() {
        let out = run(VALID_DOC, None, true);
        assert!(
            out.stdout.contains(r#""valid": true"#),
            "valid doc JSON must have valid=true; got: {}",
            out.stdout
        );
    }

    #[test]
    fn parse_error_exits_two() {
        let out = run("not kdl !!!{{{", None, false);
        assert_eq!(out.exit_code, 2, "stdout: {}", out.stdout);
    }
}
