//! Text and code leaf-node compilation, plus the shaping/glyph and
//! syntax-highlight helpers they depend on.
//!
//! This module is wiring only: it declares the focused submodules and re-exports
//! the surface other `compile` modules import. The implementation lives in the
//! submodules, grouped by concern:
//!
//! - [`ctx`] — the `Copy` context structs that bundle the threaded parameters.
//! - [`shape`] — word shaping, resolved-span carriers, metrics, font resolvers.
//! - [`hyphen`] — opt-in hyphenation + break-word fragment splitting.
//! - [`pack`] — greedy line packing (uniform / variable / runaround).
//! - [`emit`] — line emission (decorations + glyph runs).
//! - [`baseline`] — baseline-grid snapping.
//! - [`dropcap`] — drop-cap initial lifting + shaping.
//! - [`tableader`] — tab-leader (table-of-contents) rendering.
//! - [`measure`] — natural-width / wrapped-height measurement.
//! - [`chain_member`] — threaded-text chain member rendering.
//! - [`wrap`] — the single-box wrap path (drop-cap / runaround / plain).
//! - [`text_node`] — the `compile_text` entry + sized layout engine.
//! - [`code_node`] — the `compile_code` entry.

mod baseline;
mod chain_blocks;
mod chain_member;
mod code_node;
mod ctx;
mod dropcap;
mod emit;
mod hyphen;
mod markdown_block;
mod measure;
mod pack;
mod shape;
mod tableader;
mod text_node;
mod wrap;

// ── Public surface re-exported for sibling `compile` modules ────────────────

pub(in crate::compile) use chain_blocks::{BlockStyleEnv, ChainSourceShape, shape_source_blocks};
pub(in crate::compile) use code_node::compile_code;
pub(in crate::compile) use ctx::{NodeShape, ShapeEnv, TextCompileEnv, empty_md_blocks};
pub(in crate::compile) use hyphen::{
    HyphenationContext, en_us_hyphenator, flatten_lines_to_tokens,
};
pub(in crate::compile) use measure::{
    MeasureEnv, measure_text_natural, measure_text_wrapped_height, resolve_text_families,
};
pub(in crate::compile) use pack::{Line, LineDecoration, LineStyle, pack_lines};
pub(in crate::compile) use shape::{
    LINK_COLOR, ResolvedSpan, WordMetrics, WordToken, resolve_family_with_fallback,
    resolve_font_family_name, resolve_font_weight, resolve_vertical_align, run_to_scene_glyphs,
    shape_words,
};
pub(in crate::compile) use text_node::compile_text;
