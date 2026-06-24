//! Shared diagnostic types used across all Zenith validation passes.
//!
//! Diagnostics are collected without hard-failing — callers push them into a
//! `Vec<Diagnostic>` and continue resolving what they can. `lib.rs` re-exports
//! the most-used symbols at the crate root.

use crate::ast::Span;

/// The severity level of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A definite problem that prevents correct output.
    Error,
    /// A potential problem or version-relative issue; output may still be
    /// produced.
    Warning,
    /// Informational note; does not block output.
    Advisory,
}

/// A single structured diagnostic produced during validation or resolution.
///
/// Diagnostics carry a stable `code` that agents and tooling can key on, a
/// human-readable `message`, an optional source `span`, and an optional
/// `subject_id` naming the token (or future: node/style) the diagnostic is
/// about.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Stable dot-separated code, e.g. `"token.cyclic_reference"`.
    pub code: String,
    /// Severity classification.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
    /// Source location, when available.
    pub span: Option<Span>,
    /// The token ID (or future: node/style ID) the diagnostic concerns.
    pub subject_id: Option<String>,
}

impl Diagnostic {
    /// Construct a new diagnostic with all fields explicit.
    pub fn new(
        code: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        span: Option<Span>,
        subject_id: Option<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            span,
            subject_id,
        }
    }

    /// Shorthand for an [`Severity::Error`]-level diagnostic.
    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        span: Option<Span>,
        subject_id: Option<String>,
    ) -> Self {
        Self::new(code, Severity::Error, message, span, subject_id)
    }

    /// Shorthand for a [`Severity::Warning`]-level diagnostic.
    pub fn warning(
        code: impl Into<String>,
        message: impl Into<String>,
        span: Option<Span>,
        subject_id: Option<String>,
    ) -> Self {
        Self::new(code, Severity::Warning, message, span, subject_id)
    }

    /// Shorthand for an [`Severity::Advisory`]-level diagnostic.
    pub fn advisory(
        code: impl Into<String>,
        message: impl Into<String>,
        span: Option<Span>,
        subject_id: Option<String>,
    ) -> Self {
        Self::new(code, Severity::Advisory, message, span, subject_id)
    }
}
