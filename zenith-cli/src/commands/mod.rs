//! Command implementations for the Zenith CLI.
//!
//! Each submodule exposes a pure function whose core logic operates on
//! in-memory source bytes/strings — never touching the filesystem.  File I/O
//! (reading the document, writing formatted source or rendered output) is the
//! responsibility of the dispatcher in `lib.rs`.

pub mod fmt;
pub mod inspect;
pub mod library;
pub mod merge;
pub mod plugin;
pub mod render;
pub mod schema;
pub mod theme;
pub mod tokens;
pub mod tx;
pub mod validate;
pub mod variant;
pub mod workspace;

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Serialise `value` to pretty-printed JSON, falling back to the error
/// message string if serialisation itself fails (which cannot happen for
/// these well-typed DTOs, but is kept as a safe fallback).
pub(crate) fn serialize_pretty<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| e.to_string())
}

/// Format a single diagnostic as a human-readable line:
/// `severity[code] (subject_id): message` (the subject is omitted when absent).
pub(crate) fn format_diagnostic_line(d: &zenith_core::Diagnostic) -> String {
    let sev = crate::json_types::severity_str(&d.severity);
    let subject = d
        .subject_id
        .as_deref()
        .map(|s| format!(" ({})", s))
        .unwrap_or_default();
    format!("{}[{}]{}: {}", sev, d.code, subject, d.message)
}

/// Format a hard (Error-severity) diagnostic as `error[code]: message`.
///
/// Used in "filter for Error, then format" pipelines in merge, variant, and
/// render.  Centralised here so the string shape has exactly one definition.
pub(crate) fn format_error_diag(d: &zenith_core::Diagnostic) -> String {
    format!("error[{}]: {}", d.code, d.message)
}
