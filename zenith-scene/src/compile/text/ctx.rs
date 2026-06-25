//! Small `Copy` context structs that bundle the over-long parameter lists the
//! text/code compile paths used to thread loosely. Each struct groups one
//! cohesive concern (shaping environment, emit-time style, compile environment,
//! placement geometry) so every function stays under the argument-count lint
//! without suppressing it. All fields are borrows or scalars held for the
//! duration of a single call, so the structs are trivially `Copy` and never own
//! heap state.

use std::collections::BTreeMap;

use zenith_core::{BlockStyle, FontProvider, ResolvedToken, Style};
use zenith_layout::{RustybuzzEngine, TextDirection};

use crate::ir::Color;

use super::super::anchor::AnchorMap;
use super::super::chain::ChainAssignments;
use super::super::markdown_resolve::MdBlockMap;
use super::WordMetrics;

/// The shaping engine + font provider, borrowed for one shape/measure call.
/// Threaded through every helper that shapes runs so the two backends are passed
/// as one unit rather than two parallel arguments.
#[derive(Clone, Copy)]
pub(in crate::compile) struct ShapeEnv<'a> {
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
}

/// The node-level shaping parameters shared by every word of a node: the base
/// font size, the node base weight, and the base writing direction.
#[derive(Clone, Copy)]
pub(in crate::compile) struct NodeShape {
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) base_weight: u16,
    pub(in crate::compile) direction: TextDirection,
}

/// Emit-time line style shared by the line emitters. `align`/`direction` drive
/// per-line anchoring; the remaining scalars are the shared decoration + glyph
/// attributes. `justify_final_line` is the last-line justify policy for THIS
/// batch (see [`super::emit::emit_lines`]).
#[derive(Clone, Copy)]
pub(in crate::compile) struct EmitStyle<'a> {
    pub(in crate::compile) align: &'a str,
    pub(in crate::compile) metrics: WordMetrics,
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) deco_thickness: f64,
    pub(in crate::compile) justify_final_line: bool,
    pub(in crate::compile) direction: TextDirection,
    pub(in crate::compile) glyph_stroke: (Option<Color>, Option<f64>),
}

/// The threaded maps/providers a `text`/`code` leaf needs to compile: token
/// resolution, the style cascade, the font backends, and the page-level pre-pass
/// maps (chains, footnote markers, node boxes, anchors). Bundled so the public
/// `compile_text`/`compile_code` edges stay under the argument lint; `compile_code`
/// reads only the subset it needs (resolved/style_map/fonts/engine/anchors).
#[derive(Clone, Copy)]
pub(in crate::compile) struct TextCompileEnv<'a> {
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    pub(in crate::compile) chains: &'a ChainAssignments,
    pub(in crate::compile) footnote_markers: &'a BTreeMap<String, String>,
    pub(in crate::compile) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    pub(in crate::compile) anchors: &'a AnchorMap,
    /// Parsed block-level markdown keyed by `text` node id. A node present here
    /// AND not chained takes the block-layout path; everything else is empty
    /// (byte-identical). Synthetic-text callers (fields, footnotes, labels) pass
    /// the shared empty map so they never activate block layout.
    pub(in crate::compile) md_blocks: &'a MdBlockMap,
    /// Page-scope block-role style declarations (cascade tier 2).
    pub(in crate::compile) page_block_styles: &'a [BlockStyle],
    /// Document-scope block-role style declarations (cascade tier 3).
    pub(in crate::compile) doc_block_styles: &'a [BlockStyle],
}

/// A process-wide empty [`MdBlockMap`] for synthetic-text compile sites (fields,
/// footnotes, shape/connector labels) that never carry markdown blocks. Lets
/// those sites populate [`TextCompileEnv::md_blocks`] without threading the real
/// map; a synthesized node id is never present, so block layout never activates.
pub(in crate::compile) fn empty_md_blocks() -> &'static MdBlockMap {
    use std::sync::OnceLock;
    static EMPTY: OnceLock<MdBlockMap> = OnceLock::new();
    EMPTY.get_or_init(MdBlockMap::new)
}

/// Placement geometry + style for a chain member's emit (see
/// [`super::chain_member::render_chain_member`]). Bundles the box-relative origin
/// and the shared scalars threaded into the emit.
#[derive(Clone, Copy)]
pub(in crate::compile) struct ChainMemberPlace {
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) text_x: f64,
    pub(in crate::compile) text_y: f64,
    pub(in crate::compile) baseline_grid: Option<f64>,
    pub(in crate::compile) glyph_stroke: (Option<Color>, Option<f64>),
}

/// Everything the tab-leader (TOC) renderer needs beyond the node, leader glyph,
/// families, and command/diagnostic sinks: the shaping env, the resolved-token
/// map, the node-level fill/weight/opacity style, the placement origin, the
/// render ctx, and the glyph stroke. Bundled so [`super::tableader::compile_tab_leader`]
/// stays under the argument lint.
#[derive(Clone, Copy)]
pub(in crate::compile) struct TabLeaderArgs<'a> {
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) node_fill_prop: Option<&'a zenith_core::PropertyValue>,
    pub(in crate::compile) node_weight_prop: Option<&'a zenith_core::PropertyValue>,
    pub(in crate::compile) node_opacity: f64,
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) env: ShapeEnv<'a>,
    pub(in crate::compile) text_x: f64,
    pub(in crate::compile) text_y: f64,
    pub(in crate::compile) ctx: super::super::RenderCtx,
    pub(in crate::compile) glyph_stroke: (Option<Color>, Option<f64>),
}

/// A line being emitted, plus the constant geometry — used internally by the
/// emitter only when a constant per-line origin/measure is required. Kept here
/// so the emitter file stays focused on the emit loop.
#[derive(Clone, Copy)]
pub(in crate::compile) struct UniformGeom {
    pub(in crate::compile) text_x: f64,
    pub(in crate::compile) box_w: f64,
}

impl UniformGeom {
    /// Resolve the constant `(origin_x, box_w)` for any line index.
    pub(in crate::compile) fn at(self, _line_index: usize) -> (f64, f64) {
        (self.text_x, self.box_w)
    }
}
