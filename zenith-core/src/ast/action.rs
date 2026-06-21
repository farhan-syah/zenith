//! Action block declaration AST types.
//!
//! The top-level `actions` block is an automation manifest: each entry
//! declares a named transaction script the host can invoke. It is a sibling
//! of the `assets`/`tokens`/`libraries` blocks. The engine round-trips the
//! `tx` payload verbatim as an opaque string; it does NOT parse or validate
//! the JSON content.

use std::collections::BTreeMap;

use super::Span;
use super::node::UnknownProperty;

/// A single action declaration within an `actions` block — one named
/// transaction script.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionDef {
    /// The action identifier, e.g. "apply-brand-kit". Required.
    pub id: String,
    /// Human-readable label, e.g. "Apply Brand Kit". Optional.
    pub label: Option<String>,
    /// Declared version string, e.g. "1.0.0". Optional.
    pub version: Option<String>,
    /// The transaction script payload as an opaque JSON string. Required.
    /// The engine round-trips this verbatim without parsing it.
    pub tx_json: String,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Forward-compat: unrecognized attributes preserved with typed values +
    /// annotations.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
