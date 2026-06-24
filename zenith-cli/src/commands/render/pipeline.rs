//! Shared parse/validate, page-resolution, and hash-verification helpers.

use std::path::Path;

use sha2::{Digest, Sha256};

use zenith_core::{Document, KdlAdapter, KdlSource, validate_with_policy};

use crate::config::{CliPolicyFlags, load_global_and_local, merge_policy};

use super::entry::RenderCmdErr;

/// Verify that `bytes` match the `sha256` field declared on an asset.
///
/// `id` is the asset identifier (for error messages); `kind` is a short noun
/// used in error messages (`"asset"` or `"font asset"`).
///
/// Returns `Err` (exit code 2) when:
/// - `sha256` is `None` (no hash declared).
/// - The computed SHA-256 hex digest does not match `sha256` (case-insensitive,
///   trimmed).
pub(super) fn verify_locked_sha256(
    id: &str,
    kind: &str,
    sha256: Option<&str>,
    bytes: &[u8],
) -> Result<(), RenderCmdErr> {
    let declared = sha256.ok_or_else(|| {
        RenderCmdErr::new(format!("--locked: {kind} '{id}' has no declared sha256"), 2)
    })?;
    let hex = format!("{:x}", Sha256::digest(bytes));
    if declared.trim().to_lowercase() != hex {
        return Err(RenderCmdErr::new(
            format!("--locked: {kind} '{id}' sha256 mismatch (declared {declared}, actual {hex})"),
            2,
        ));
    }
    Ok(())
}

/// Parse → validate with the merged diagnostic policy, returning the parsed
/// [`Document`].
///
/// The effective policy is `merge_policy(global, local, in_file, flags)`,
/// mirroring the `validate` command exactly:
/// - Global config is always consulted.
/// - Local config is walked up from `start_dir` when `Some`.
/// - In-file policy comes from the parsed document.
/// - CLI flags layer on top.
///
/// A config-load error returns exit code 2. Parse errors return exit code 2.
/// Validation errors (at least one Error-severity diagnostic after policy
/// application) return exit code 1. With no config files and no flags the
/// merged policy equals the in-file policy, so the result is byte-identical
/// to the old behaviour.
pub(super) fn parse_validate(
    src: &str,
    start_dir: Option<&Path>,
    flags: &CliPolicyFlags,
) -> Result<Document, RenderCmdErr> {
    // Resolve config policy ───────────────────────────────────────────────────
    let (global, local) = load_global_and_local(start_dir)
        .map_err(|msg| RenderCmdErr::new(format!("error[config.error]: {msg}"), 2))?;

    // Parse ─────────────────────────────────────────────────────────────────
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| RenderCmdErr::new(format!("error[parse.error]: {}", e.message), 2))?;

    // Validate ───────────────────────────────────────────────────────────────
    let merged = merge_policy(&global, &local, &doc.diagnostic_policy, flags);
    let report = validate_with_policy(&doc, &merged);
    if report.has_errors() {
        let msgs: Vec<String> = report
            .diagnostics
            .iter()
            .filter(|d| d.severity == zenith_core::Severity::Error)
            .map(crate::commands::format_error_diag)
            .collect();
        return Err(RenderCmdErr::new(msgs.join("\n"), 1));
    }

    Ok(doc)
}

/// Resolve a 1-based `page` number to a 0-based page index within `doc`.
///
/// Returns `Err` (exit code 2) when the document has no pages or when `page`
/// is outside `1..=pages.len()`.
pub(super) fn resolve_page_index(doc: &Document, page: usize) -> Result<usize, RenderCmdErr> {
    let n = doc.body.pages.len();
    if doc.body.pages.is_empty() || page < 1 || page > n {
        return Err(RenderCmdErr::new(
            format!("page {page} out of range; document has {n} page(s)"),
            2,
        ));
    }
    Ok(page - 1)
}
