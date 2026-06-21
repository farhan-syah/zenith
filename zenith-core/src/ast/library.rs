//! Library block declaration AST types.
//!
//! The top-level `libraries` block is an imported-package manifest: each entry
//! declares an external library dependency the document draws on. It is a
//! sibling of the `assets`/`tokens`/`sections` blocks. The engine preserves and
//! validates these declarations but does NOT fetch or verify package content;
//! the optional `hash` lock string is round-tripped for an external tool.

use std::collections::BTreeMap;

use super::Span;
use super::node::UnknownProperty;

/// A single library declaration within a `libraries` block — one imported
/// external package.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryDef {
    /// The imported package identifier, e.g. "@acme/brand-kit". Required.
    pub id: String,
    /// Declared package version, e.g. "1.4.0". Optional (a lockfile/external tool may fill it).
    pub version: Option<String>,
    /// Content hash / lock string, e.g. "sha256-...". Optional; preserved for external
    /// integrity checking (the engine round-trips it but does not fetch/verify package content).
    pub hash: Option<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Forward-compat: unrecognized attributes preserved with typed values + annotations.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
