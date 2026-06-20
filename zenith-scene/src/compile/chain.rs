//! Threaded text flow ("text chain") pre-pass.
//!
//! A *chain* is the set of `text` nodes that share the same `chain` id. A long
//! article placed in the FIRST member (source order) flows across every
//! member's box in order: each box consumes as much text as fits, and the
//! remainder continues in the next box. This enables tri-fold leaflet panels
//! where one article spans three text boxes.
//!
//! This module runs ONCE per document (across ALL pages), BEFORE the main
//! compile walk, producing a single [`ChainAssignments`] map keyed by global
//! node id. A chain may span boxes on DIFFERENT pages: members are collected in
//! (page-order, then source-order) and the source content is poured greedily
//! across every member — box 1 fills, the remainder flows to box 2, … across
//! page boundaries. [`super::text::compile_text`] consults that map: a chain
//! member renders its ASSIGNED lines (via the shared [`super::text::emit_lines`])
//! instead of wrapping its own spans; a non-chain node is wholly unaffected
//! (byte-identical). The same document-wide map is threaded into every
//! `compile_page` call so a node on page 3 renders the slice it was assigned.
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
    HyphenationContext, Line, ResolvedSpan, WordMetrics, en_us_hyphenator, flatten_lines_to_tokens,
    pack_lines, resolve_family_with_fallback, resolve_font_weight, resolve_vertical_align,
    shape_words,
};
use super::util::resolve_property_dimension_px;

/// The lines a single chain member must render, already shaped + packed to that
/// member's box width, plus the shared font metrics for baseline stacking.
pub(crate) struct ChainAssignment {
    pub(super) lines: Vec<Line>,
    pub(super) metrics: WordMetrics,
    /// `true` only for the LAST member of the chain (document-wide). Drives the
    /// justify last-line policy: the final member leaves its last line ragged;
    /// a continuation member justifies its last line (the paragraph flows on).
    pub(super) is_last_member: bool,
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
        // Super/subscript: reduced size + baseline shift, shared with the
        // single-box wrap path so a chained article honors vertical-align too.
        let (span_font_size, baseline_dy) =
            resolve_vertical_align(span.vertical_align.as_deref(), font_size);
        spans.push(ResolvedSpan {
            text: span.text.clone(),
            color,
            underline: span.underline == Some(true),
            strikethrough: span.strikethrough == Some(true),
            weight,
            style,
            font_size: span_font_size,
            baseline_dy,
        });
    }

    (families, font_size, base_weight, spans)
}

/// Build the DOCUMENT-WIDE chain-assignment map across every page.
///
/// Chains thread across boxes on DIFFERENT pages: members are collected in
/// (page-order, then source-order) over `doc.body.pages`, each carrying its OWN
/// page's box geometry, and a chain's source content is poured greedily across
/// all members in that global order — box 1 fills, the remainder flows into
/// box 2, … across page boundaries. The returned map is keyed by global node id,
/// so `compile_page` for any page looks up the slice assigned to a box on that
/// page.
///
/// Returns an empty map when no `chain` members are present, in which case
/// `compile_text` behaves exactly as before for every node.
pub(super) fn resolve_chains_document<'a>(
    doc: &'a zenith_core::Document,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    diagnostics: &mut Vec<Diagnostic>,
) -> ChainAssignments {
    // Collect members + content sources across ALL pages in page-then-source
    // order. A `BTreeMap` per-chain member list preserves the push order, which
    // is exactly the document-wide flow order.
    let mut members: BTreeMap<String, Vec<Member>> = BTreeMap::new();
    let mut source: BTreeMap<String, &'a TextNode> = BTreeMap::new();
    for page in &doc.body.pages {
        collect_chains(&page.children, &mut members, &mut source);
    }

    distribute_chains(
        &members,
        &source,
        resolved,
        style_map,
        fonts,
        engine,
        diagnostics,
    )
}

/// Shared distributor: shape each chain's source once and pour its words greedily
/// across the chain's ordered members. Used by [`resolve_chains_document`]; kept
/// scope-agnostic so the collection scope (one page vs. the whole document) is
/// the ONLY thing that differs between call sites.
fn distribute_chains(
    members: &BTreeMap<String, Vec<Member>>,
    source: &BTreeMap<String, &TextNode>,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    diagnostics: &mut Vec<Diagnostic>,
) -> ChainAssignments {
    let mut assignments: ChainAssignments = BTreeMap::new();

    for (chain_id, chain_members) in members {
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

        // Opt-in hyphenation for the whole chain, read from the source node.
        // Absent → `None` → packing + flattening are byte-identical to before.
        let hyph_ctx = if src.hyphenate == Some(true) {
            en_us_hyphenator().map(|dict| HyphenationContext {
                dict,
                engine,
                fonts,
                families: &families,
                hyphen: "-",
            })
        } else {
            None
        };

        // Widow/orphan minimum, read from the chain source node. `None` or a
        // value < 2 leaves the greedy height-cut unadjusted (byte-identical).
        let widow_orphan = src.widow_orphan.filter(|&n| n >= 2);

        // Distribute tokens across the members' boxes in order.
        let mut remaining = tokens;
        let last_member = chain_members.len().saturating_sub(1);
        for (mi, member) in chain_members.iter().enumerate() {
            // Greedy-wrap the REMAINING words to THIS box's width.
            let mut lines = pack_lines(
                remaining,
                member.w,
                metrics.space_advance,
                hyph_ctx.as_ref(),
            );

            if mi == last_member {
                // Last box: keep everything that remains (it may overflow; the
                // member's own overflow handling rides in compile_text). The
                // `remaining` queue is not read again after this iteration.
                assignments.insert(
                    member.id.clone(),
                    ChainAssignment {
                        lines,
                        metrics,
                        is_last_member: true,
                    },
                );
                break;
            }

            // How many leading lines fit this box height (≥1 unless the box is
            // too short for even one line, in which case 0 lines are taken so
            // the content cascades into the next box).
            let line_h = metrics.line_height.max(1.0);
            let max_lines = (member.h / line_h).floor() as usize;
            let mut take = max_lines.min(lines.len());

            // Widow/orphan adjustment: if the greedy cut splits a paragraph
            // across this boundary, pull lines DOWN into the next box so neither
            // side is left with fewer than N lines of that paragraph.
            if let Some(n) = widow_orphan {
                take = adjust_for_widow_orphan(&lines, take, n as usize);
            }

            // Lines beyond `take` carry their words into the next box. Rebuild
            // the remaining token queue from the overflow lines (flatten back
            // into a single word stream so the next box re-wraps to its width),
            // merging any hyphenation fragments back into whole words.
            let overflow_lines = lines.split_off(take);
            remaining = flatten_lines_to_tokens(overflow_lines, hyph_ctx.as_ref());

            assignments.insert(
                member.id.clone(),
                ChainAssignment {
                    lines,
                    metrics,
                    is_last_member: false,
                },
            );
        }
    }

    assignments
}

/// Adjust a greedy height-cut `take` (number of lines kept in THIS box, out of
/// `lines`) to honor a widow/orphan minimum of `n` lines per paragraph across
/// the box boundary. Returns the possibly-reduced `take`; lines are only ever
/// moved DOWN into the next box (the greedy flow cannot push lines up).
///
/// The boundary splits a paragraph when the last kept line and the first
/// overflow line share a paragraph index. In that case:
/// - `top_count` = trailing lines of that paragraph kept in THIS box;
/// - `bottom_count` = leading lines of that paragraph in the NEXT box.
///
/// To satisfy the WIDOW rule the next box must start with ≥ `n` lines of the
/// paragraph, so if `bottom_count < n` we move `n - bottom_count` lines down. To
/// satisfy the ORPHAN rule this box must keep ≥ `n` lines of the paragraph, so if
/// the move would leave `top_count < n` we instead move the WHOLE top chunk of
/// the paragraph down (the paragraph then starts cleanly in the next box).
///
/// Degenerate cases (documented): if the adjustment would empty THIS box
/// (`take` → 0) the cut is LEFT as-is, since an empty box is worse than a
/// widow/orphan; likewise a paragraph shorter than `2n` lines cannot satisfy the
/// rule on both sides and falls back to being moved whole (or left, if that
/// empties the box).
fn adjust_for_widow_orphan(lines: &[Line], take: usize, n: usize) -> usize {
    // No straddle when nothing is kept, nothing overflows, or the boundary lines
    // belong to different paragraphs.
    if take == 0 || take >= lines.len() {
        return take;
    }
    let (Some(last_kept), Some(first_over)) = (lines.get(take - 1), lines.get(take)) else {
        return take;
    };
    if last_kept.paragraph != first_over.paragraph {
        return take;
    }
    let para = last_kept.paragraph;

    // Trailing lines of `para` kept in this box.
    let top_count = lines[..take]
        .iter()
        .rev()
        .take_while(|l| l.paragraph == para)
        .count();
    // Leading lines of `para` in the next box.
    let bottom_count = lines[take..]
        .iter()
        .take_while(|l| l.paragraph == para)
        .count();

    let mut new_take = take;
    if bottom_count < n {
        let need = n - bottom_count;
        new_take = take.saturating_sub(need);
    }
    // If the (possible) move still leaves the top side with < n lines of the
    // paragraph, move the whole top chunk down so the paragraph starts fresh.
    let top_after = top_count.saturating_sub(take - new_take);
    if top_after < n {
        new_take = take.saturating_sub(top_count);
    }

    // Never empty this box; if the rule cannot be honored without doing so,
    // leave the greedy cut unchanged (degenerate case).
    if new_take >= 1 { new_take } else { take }
}

#[cfg(test)]
mod widow_orphan_tests {
    use super::*;

    /// Build a line list from per-line paragraph indices (words/width are not
    /// read by `adjust_for_widow_orphan`).
    fn lines_with_paragraphs(paras: &[usize]) -> Vec<Line> {
        paras
            .iter()
            .map(|&p| Line {
                words: Vec::new(),
                content_w: 0.0,
                paragraph: p,
            })
            .collect()
    }

    /// No straddle (the boundary lines belong to different paragraphs) → the cut
    /// is left exactly where the greedy fit put it.
    #[test]
    fn no_straddle_keeps_take() {
        // take=3: line 2 is paragraph 0, line 3 is paragraph 1 → no straddle.
        let lines = lines_with_paragraphs(&[0, 0, 0, 1, 1, 1]);
        assert_eq!(adjust_for_widow_orphan(&lines, 3, 2), 3);
    }

    /// Orphan: box 1 would keep a lone FIRST line of paragraph 1 (take=4 keeps
    /// [0,0,0,1]). With N=2 that single line is pulled down → take=3.
    #[test]
    fn orphan_single_first_line_pulled_down() {
        let lines = lines_with_paragraphs(&[0, 0, 0, 1, 1, 1]);
        assert_eq!(adjust_for_widow_orphan(&lines, 4, 2), 3);
    }

    /// Widow: the next box would start with a lone LAST line of paragraph 0
    /// (take=5 keeps [0,0,0,0,0], overflow [0,1,...] starts with 1 line of P0).
    /// With N=2 one line is pulled down so the next box starts with 2 lines of P0.
    #[test]
    fn widow_single_last_line_pulled_down() {
        let lines = lines_with_paragraphs(&[0, 0, 0, 0, 0, 0, 1, 1]);
        // take=5 → bottom_count(P0)=1 (line index 5), top_count=5. Pull 1 down.
        assert_eq!(adjust_for_widow_orphan(&lines, 5, 2), 4);
    }

    /// Both sides already satisfy N → no change.
    #[test]
    fn satisfied_both_sides_unchanged() {
        let lines = lines_with_paragraphs(&[0, 0, 0, 0]);
        // take=2: top=2 lines of P0, bottom=2 lines of P0 → fine.
        assert_eq!(adjust_for_widow_orphan(&lines, 2, 2), 2);
    }

    /// Degenerate: honoring the rule would empty the box → leave the cut as-is.
    #[test]
    fn degenerate_would_empty_box_left_as_is() {
        // Whole box is the tail of paragraph 1 (single line), next box continues
        // it. Pulling down would empty the box → unchanged.
        let lines = lines_with_paragraphs(&[1, 1, 1]);
        assert_eq!(adjust_for_widow_orphan(&lines, 1, 2), 1);
    }
}
