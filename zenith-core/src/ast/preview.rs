//! Preview-artifacts block declaration AST types.
//!
//! The top-level `previews` block records preview/critique artifacts for
//! candidate pages. Each `preview` entry captures a required `candidate` page
//! id, optional content-hash and output-path fields, and a sequence of
//! `critique` children that record visual-critique notes. It is a sibling of
//! the `variants`/`recipes`/`document` blocks. The engine round-trips these
//! records but does NOT act on them; auditability and diffability are the sole
//! purpose.

use std::collections::BTreeMap;

use super::Span;
use super::node::UnknownProperty;

/// A preview/critique artifact recorded for a candidate page. Non-rendering
/// audit metadata; never consulted by the render/compile path.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewArtifact {
    /// Id of the page this preview is OF (a candidate/export page). Required.
    pub candidate: String,
    /// Content hash of the source state the preview was rendered from.
    pub source_hash: Option<String>,
    /// Output file path of the rendered preview (free-form; escaped on emit).
    pub output: Option<String>,
    /// Content hash of the rendered output.
    pub output_hash: Option<String>,
    /// Id/label of the parent revision this preview descends from.
    pub parent_revision: Option<String>,
    /// Visual-critique notes recorded against this preview.
    pub critiques: Vec<PreviewCritique>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Forward-compat: unrecognized attributes preserved with typed values and
    /// annotations.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}

/// A single visual-critique note on a preview. Fixed-schema leaf (no
/// unknown_props).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewCritique {
    /// Severity string (e.g. `"warn"`, `"error"`). Required.
    pub severity: String,
    /// Machine-readable critique code. Required.
    pub code: String,
    /// Human-readable critique message. Required.
    pub message: String,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}
