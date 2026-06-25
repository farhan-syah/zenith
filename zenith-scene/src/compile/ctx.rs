//! Shared immutable compile context threaded into [`compile_node`] and the
//! container / table / text / field compilers.
//!
//! [`NodeCtx`] bundles the read-only lookups every `compile_*` function
//! forwards down the node recursion. Bundling them into one `Copy` struct keeps
//! each signature short — the mutable `commands` / `diagnostics` sinks and the
//! per-subtree [`RenderCtx`](super::RenderCtx) cascade stay as explicit
//! parameters because they are not shared-immutable.
//!
//! Every field is a reference or a `Copy` value, so the struct is itself `Copy`
//! and forwards freely without clones.
//!
//! [`compile_node`]: super::compile_node

use std::collections::BTreeMap;

use zenith_core::{BlockStyle, FontProvider, ResolvedToken, Style};
use zenith_layout::RustybuzzEngine;

use super::ComponentMap;
use super::anchor::AnchorMap;
use super::chain::ChainAssignments;
use super::field::FieldCtx;
use super::markdown_resolve::MdBlockMap;
use super::table_flow::TableFlowAssignments;

/// Immutable, shared borrows threaded through every node compiler.
///
/// The container compilers (`compile_frame`, `compile_group`,
/// `compile_instance` and their flow/grid helpers), table emission, and the
/// node dispatcher all forward the same read-only lookups down the
/// recursion.
#[derive(Clone, Copy)]
pub(in crate::compile) struct NodeCtx<'a> {
    /// Resolved token table for dimension / color property resolution.
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    /// Style id → style lookup for the style cascade.
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    /// Component definitions, for `instance` expansion.
    pub(in crate::compile) components: &'a ComponentMap<'a>,
    /// Font provider used to shape text descendants.
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    /// Shaping engine used to shape text descendants.
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    /// Text-chain assignments cascaded into text descendants.
    pub(in crate::compile) chains: &'a ChainAssignments,
    /// Table-flow assignments cascaded into table descendants.
    pub(in crate::compile) flows: &'a TableFlowAssignments,
    /// Anchor map for `anchor`/`anchor_zone` derived placement.
    pub(in crate::compile) anchors: &'a AnchorMap,
    /// Per-page field context (page index, live area, footnote markers, …).
    pub(in crate::compile) field_ctx: &'a FieldCtx<'a>,
    /// Parsed block-level markdown, keyed by `text` node id. Empty when the
    /// document has no `format="markdown"` text nodes (byte-identical path).
    pub(in crate::compile) md_blocks: &'a MdBlockMap,
    /// Page-scope `block role="…"` style declarations (cascade tier 2).
    pub(in crate::compile) page_block_styles: &'a [BlockStyle],
    /// Document-body-scope `block role="…"` style declarations (cascade tier 3).
    pub(in crate::compile) doc_block_styles: &'a [BlockStyle],
}
