//! Provenance block declaration AST types.
//!
//! The top-level `provenance` block records per-node origin metadata: each
//! `origin` entry records WHERE a document node came from (which library/package
//! and item). It is a sibling of the `assets`/`libraries`/`sections` blocks. The
//! engine preserves and validates these records — each references a node id AND a
//! declared library id that must exist — but does NOT act on the link state; the
//! `linked` flag is round-tripped for external tooling.

use std::collections::BTreeMap;

use super::Span;
use super::node::UnknownProperty;

/// A single provenance record within a `provenance` block — one node's origin.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceDef {
    /// This record's own unique id. Required.
    pub id: String,
    /// The id of the document node this provenance describes. Required; must
    /// reference an existing node id, a declared token id, or a declared action
    /// id.
    pub node: String,
    /// The declared library/package id this node originated from. Required; must
    /// reference a `library` declared in the `libraries` block.
    pub library: String,
    /// The item name within the library (e.g. "button"). Optional.
    pub item: Option<String>,
    /// Link state: `Some(true)` = linked (updates when the library updates),
    /// `Some(false)` = detached (frozen). `None` = unspecified (treated as linked
    /// by external tooling). The engine preserves this; it does not act on it.
    pub linked: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Forward-compat: unrecognized attributes preserved with typed values + annotations.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
