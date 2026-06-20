//! Threaded text flow ("text chain") pre-pass.
//!
//! A *chain* is the set of `text` nodes that share the same `chain` id. A long
//! article placed in the FIRST member (source order) flows across every
//! member's box in order: each box consumes as much text as fits, and the
//! remainder continues in the next box. This enables tri-fold leaflet panels
//! where one article spans three text boxes.
//!
//! This module runs ONCE per page, BEFORE the main compile walk, producing a
//! [`ChainAssignments`] map keyed by node id. [`super::text::compile_text`]
//! consults that map: a chain member renders its ASSIGNED lines (via the shared
//! [`super::text::emit_lines`]) instead of wrapping its own spans; a non-chain
//! node is wholly unaffected (byte-identical).
//!
//! ## v0 design choices (documented)
//!
//! - **Content source.** The chain's content is the spans of the FIRST member
//!   (source order) that has non-empty spans. Later members are continuation
//!   slots and declare `chain=id` with empty spans. If more than one member
//!   carries spans, only the first member's spans are used — spans are NOT
//!   concatenated (kept simple for v0).
//! - **Shared style.** All members are assumed to share font family/size/
//!   weight/fill. The whole chain is shaped ONCE with the first member's
//!   resolved style (+ per-span overrides). Each box re-wraps to its OWN width,
//!   so line height is uniform across the chain even when boxes differ in width.
//! - **Geometry source.** A chain member must carry explicit `x`/`y`/`w`/`h`
//!   geometry resolvable to pixels. The pre-pass runs before the flow-layout
//!   geometry injection in [`super::container`], so combining `layout="flow"`
//!   box injection WITH `chain` is a documented follow-up — a flow-injected
//!   member has no explicit box at pre-pass time and is skipped from the chain.
//! - **Opacity cascade.** The pre-pass shapes colors at opacity 1.0 (no group/
//!   frame opacity cascade), so placing chain members under an opacity-cascading
//!   group is a documented follow-up.
//!
//! ## Determinism
//!
//! Members are collected in document source order (a depth-first walk, frames
//! and groups included). The result is a [`BTreeMap`] keyed by node id, and the
//! shaping reuses the deterministic engine. No `HashMap`/time/random reaches
//! output.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, FontStyle, Node, PropertyValue, ResolvedToken, ResolvedValue, Style,
    TextNode, dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::Color;

use super::paint::resolve_property_color;
use super::style_prop;
use super::text::{
    Line, ResolvedSpan, WordMetrics, pack_lines, resolve_family_with_fallback, resolve_font_weight,
    shape_words,
};
use super::util::resolve_property_dimension_px;

/// The lines a single chain member must render, already shaped + packed to that
/// member's box width, plus the shared font metrics for baseline stacking.
pub(crate) struct ChainAssignment {
    pub(super) lines: Vec<Line>,
    pub(super) metrics: WordMetrics,
}

/// Map from node id → its assigned chain lines. Empty when the page has no
/// chains. A node whose id is absent is NOT a chain member.
pub(crate) type ChainAssignments = BTreeMap<String, ChainAssignment>;

/// A collected chain member: its node id and the box width/height (px) used to
/// distribute lines. The member's actual draw geometry (x/y/align) is resolved
/// independently inside `compile_text` from the node's own AST, so only the box
/// extents needed for distribution are carried here.
struct Member {
    id: String,
    w: f64,
    h: f64,
}

/// Resolve a text node's explicit box to pixels, or `None` if any of
/// `x`/`y`/`w`/`h` is absent or uses an unsupported unit.
fn member_box(text: &TextNode) -> Option<(f64, f64, f64, f64)> {
    let (xd, yd, wd, hd) = (
        text.x.as_ref()?,
        text.y.as_ref()?,
        text.w.as_ref()?,
        text.h.as_ref()?,
    );
    Some((
        dim_to_px(xd.value, &xd.unit)?,
        dim_to_px(yd.value, &yd.unit)?,
        dim_to_px(wd.value, &wd.unit)?,
        dim_to_px(hd.value, &hd.unit)?,
    ))
}

/// Depth-first walk in source order collecting `(chain_id → ordered members)`
/// plus the first span-bearing member node per chain (the content source).
fn collect_chains<'a>(
    nodes: &'a [Node],
    members: &mut BTreeMap<String, Vec<Member>>,
    source: &mut BTreeMap<String, &'a TextNode>,
) {
    for node in nodes {
        match node {
            Node::Text(t) => {
                if let Some(chain_id) = &t.chain {
                    // First span-bearing member becomes the content source.
                    let has_spans = t.spans.iter().any(|s| !s.text.is_empty());
                    if has_spans {
                        source.entry(chain_id.clone()).or_insert(t);
                    }
                    if let Some((_x, _y, w, h)) = member_box(t) {
                        members.entry(chain_id.clone()).or_default().push(Member {
                            id: t.id.clone(),
                            w,
                            h,
                        });
                    }
                }
            }
            Node::Frame(f) => collect_chains(&f.children, members, source),
            Node::Group(g) => collect_chains(&g.children, members, source),
            _ => {}
        }
    }
}

/// Resolve the chain source's shared render style into `families`, `font_size`,
/// the node base weight, and the per-span [`ResolvedSpan`] carriers used for
/// shaping. Mirrors `compile_text`'s resolution at opacity 1.0 (v0: no cascade).
fn resolve_chain_style(
    source: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<String>, f32, u16, Vec<ResolvedSpan>) {
    // Font family with style cascade → default "Noto Sans".
    let font_family_prop = source
        .font_family
        .as_ref()
        .or_else(|| style_prop(&source.style, style_map, "font-family"));
    let raw_family_name: String = match font_family_prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontFamily(name) => name.clone(),
                _ => "Noto Sans".to_owned(),
            },
            None => "Noto Sans".to_owned(),
        },
        Some(PropertyValue::Literal(name)) => name.clone(),
        Some(PropertyValue::Dimension(_)) => "Noto Sans".to_owned(),
        None => "Noto Sans".to_owned(),
    };
    let (family_name, fell_back) =
        resolve_family_with_fallback(fonts, &raw_family_name, "Noto Sans", 400, FontStyle::Normal);
    if fell_back {
        diagnostics.push(Diagnostic::advisory(
            "font.unresolved",
            format!(
                "text node '{}': font family '{}' not available, falling back to 'Noto Sans'",
                source.id, raw_family_name
            ),
            source.source_span,
            Some(source.id.clone()),
        ));
    }
    let families = vec![family_name];

    // Font size with style cascade → 16.0.
    let font_size_prop = source
        .font_size
        .clone()
        .or_else(|| style_prop(&source.style, style_map, "font-size").cloned());
    let font_size: f32 = resolve_property_dimension_px(&font_size_prop, resolved, 16.0) as f32;

    // Node-level fill/weight fallbacks (span override → node → style → default).
    let node_fill_prop: Option<&PropertyValue> = source
        .fill
        .as_ref()
        .or_else(|| style_prop(&source.style, style_map, "fill"));
    let node_weight_prop: Option<&PropertyValue> = source
        .font_weight
        .as_ref()
        .or_else(|| style_prop(&source.style, style_map, "font-weight"));
    let base_weight = resolve_font_weight(node_weight_prop, resolved, 400);

    let mut spans: Vec<ResolvedSpan> = Vec::new();
    for span in &source.spans {
        if span.text.is_empty() {
            continue;
        }
        // Per-span fill: span.fill overrides node fill; default black.
        let fill_prop = span.fill.as_ref().or(node_fill_prop);
        let color = fill_prop
            .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &source.id))
            .unwrap_or(Color::srgb(0, 0, 0, 255));
        let weight_prop = span.font_weight.as_ref().or(node_weight_prop);
        let weight = resolve_font_weight(weight_prop, resolved, 400);
        let style = if span.italic == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        spans.push(ResolvedSpan {
            text: span.text.clone(),
            color,
            underline: span.underline == Some(true),
            strikethrough: span.strikethrough == Some(true),
            weight,
            style,
        });
    }

    (families, font_size, base_weight, spans)
}

/// Build the chain-assignment map for a page's node tree.
///
/// Returns an empty map when no `chain` members are present, in which case
/// `compile_text` behaves exactly as before for every node.
pub(super) fn resolve_chains(
    nodes: &[Node],
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    diagnostics: &mut Vec<Diagnostic>,
) -> ChainAssignments {
    let mut members: BTreeMap<String, Vec<Member>> = BTreeMap::new();
    let mut source: BTreeMap<String, &TextNode> = BTreeMap::new();
    collect_chains(nodes, &mut members, &mut source);

    let mut assignments: ChainAssignments = BTreeMap::new();

    for (chain_id, chain_members) in &members {
        // A chain with no span-bearing source emits nothing.
        let Some(src) = source.get(chain_id) else {
            continue;
        };

        // Shape the source spans ONCE into word tokens with the shared style.
        let (families, font_size, base_weight, spans) =
            resolve_chain_style(src, resolved, style_map, fonts, diagnostics);
        let (tokens, metrics) = shape_words(
            &spans,
            &families,
            font_size,
            base_weight,
            engine,
            fonts,
            diagnostics,
            &src.id,
            src.source_span,
        );

        // Distribute tokens across the members' boxes in order.
        let mut remaining = tokens;
        let last_member = chain_members.len().saturating_sub(1);
        for (mi, member) in chain_members.iter().enumerate() {
            // Greedy-wrap the REMAINING words to THIS box's width.
            let mut lines = pack_lines(remaining, member.w, metrics.space_advance);

            if mi == last_member {
                // Last box: keep everything that remains (it may overflow; the
                // member's own overflow handling rides in compile_text). The
                // `remaining` queue is not read again after this iteration.
                assignments.insert(member.id.clone(), ChainAssignment { lines, metrics });
                break;
            }

            // How many leading lines fit this box height (≥1 unless the box is
            // too short for even one line, in which case 0 lines are taken so
            // the content cascades into the next box).
            let line_h = metrics.line_height.max(1.0);
            let max_lines = (member.h / line_h).floor() as usize;
            let take = max_lines.min(lines.len());

            // Lines beyond `take` carry their words into the next box. Rebuild
            // the remaining token queue from the overflow lines (flatten back
            // into a single word stream so the next box re-wraps to its width).
            let overflow_lines = lines.split_off(take);
            let mut next_tokens = Vec::new();
            for line in overflow_lines {
                next_tokens.extend(line.words);
            }
            remaining = next_tokens;

            assignments.insert(member.id.clone(), ChainAssignment { lines, metrics });
        }
    }

    assignments
}
