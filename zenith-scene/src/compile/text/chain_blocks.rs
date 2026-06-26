//! Block-aware shaping for a CHAINED markdown text source.
//!
//! When a chain's source `text` node is a parsed-markdown node (its id is present
//! in [`super::super::markdown_resolve::MdBlockMap`]), its content flows across
//! the chain members as BLOCKS — headings styled, paragraphs spaced — rather than
//! as one flat inline run. This module shapes the source's [`MdBlock`] sequence
//! into per-block descriptors ([`BlockDescriptor`]); the chain distributor then
//! packs each block's tokens into each member's box width (members differ), tags
//! every resulting [`super::pack::Line`] with the block's per-line style + height,
//! and folds inter-block spacing into the boundary line's `height_px`.
//!
//! Reuse, don't reimplement: each block resolves its role style through the SAME
//! cascade core the single-box markdown path uses ([`super::markdown_block::resolve_block_style_core`]),
//! its spans become [`ResolvedSpan`]s with the block's resolved font/size/fill/
//! weight, and shaping goes through the shared [`super::shape::shape_words`]. The
//! single-box markdown path ([`super::markdown_block`]) and all non-markdown
//! chains are untouched.
//!
//! ## Visual parity with the single-box path
//!
//! Blockquote / list-item left indent, fenced-code-block backgrounds, and the
//! horizontal-rule fill are all carried into the chain flow via per-block
//! [`BlockDescriptor::left_indent_px`] / [`BlockDescriptor::decoration`], stamped
//! onto every packed line so the emit path reproduces the single-box look using
//! the SAME constants ([`super::markdown_block::BLOCKQUOTE_INDENT_PX`] etc.). The
//! list bullet still flows inline as a leading literal span; the indent shifts
//! the whole item (the bullet sits at the indent column, continuation lines
//! align under the text — for the chain flow the indent is applied uniformly to
//! every wrapped line of the item).

use std::collections::BTreeMap;

use zenith_core::{
    BlockStyle, Diagnostic, FontStyle, ListKind, MdBlock, ResolvedToken, TextNode, TextSpan,
};
use zenith_layout::TextDirection;

use crate::ir::Color;

use super::super::paint::resolve_property_color;
use super::ctx::{NodeShape, ShapeEnv};
use super::markdown_block::{
    BLOCKQUOTE_INDENT_PX, BlockStyleCascade, CODE_BLOCK_BG, HR_COLOR, HR_THICKNESS_PX,
    LIST_INDENT_PX, ResolvedBlockStyle, block_role, resolve_block_style_core,
};
use super::pack::{LineDecoration, LineStyle};
use super::shape::{
    LINK_COLOR, ResolvedSpan, WordMetrics, WordToken, resolve_font_family_name,
    resolve_font_weight, resolve_vertical_align, shape_words,
};

/// One shaped markdown block ready for per-member packing in the chain flow.
pub(in crate::compile) struct BlockDescriptor {
    /// The block's shaped word tokens (font/size/fill baked per word), in order.
    pub(in crate::compile) tokens: Vec<WordToken>,
    /// The block's shared font metrics (ascent / line_height / space_advance).
    pub(in crate::compile) metrics: WordMetrics,
    /// The per-line style stamped onto every [`super::pack::Line`] packed from
    /// this block, so the emit path uses the block's ascent/size/decoration even
    /// when blocks of different sizes share one galley.
    pub(in crate::compile) line_style: LineStyle,
    /// Space (px) inserted ABOVE the block (suppressed for the first block).
    pub(in crate::compile) space_before_px: f64,
    /// Space (px) inserted BELOW the block.
    pub(in crate::compile) space_after_px: f64,
    /// `true` for a `HorizontalRule`: the block has no shaped text and instead
    /// contributes a single EMPTY line in the chain flow whose `decoration` is a
    /// [`LineDecoration::Rule`]. The distributor emits one zero-word line of the
    /// block's `metrics.line_height` carrying that rule fill, so the rule renders
    /// like the single-box path.
    pub(in crate::compile) is_spacer: bool,
    /// Left indent (px) applied to every line packed from this block (blockquote
    /// and list-item blocks; `0.0` otherwise). The distributor stamps it onto each
    /// [`super::pack::Line::left_indent_px`].
    pub(in crate::compile) left_indent_px: f64,
    /// Full-width per-line decoration for this block (code-block background or
    /// horizontal-rule fill; `None` otherwise). The distributor stamps it onto
    /// each [`super::pack::Line::decoration`].
    pub(in crate::compile) decoration: Option<LineDecoration>,
}

/// The cascade tiers + token map needed to resolve each block's role style,
/// bundled so [`shape_source_blocks`] stays under the argument lint.
#[derive(Clone, Copy)]
pub(in crate::compile) struct BlockStyleEnv<'a> {
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) page_block_styles: &'a [BlockStyle],
    pub(in crate::compile) doc_block_styles: &'a [BlockStyle],
}

/// The chain-source node parameters threaded into block shaping, bundled so
/// [`shape_source_blocks`] stays under the argument lint.
#[derive(Clone, Copy)]
pub(in crate::compile) struct ChainSourceShape<'a> {
    pub(in crate::compile) families: &'a [String],
    /// The source node's resolved base font size (px), the cascade fallback.
    pub(in crate::compile) node_font_size: f32,
    pub(in crate::compile) base_weight: u16,
    pub(in crate::compile) direction: TextDirection,
}

/// Shape the chain source's markdown blocks into per-block descriptors.
///
/// Each block is shaped independently so a heading and body paragraph keep their
/// own metrics; the chain distributor packs them per member width.
pub(in crate::compile) fn shape_source_blocks(
    src: &TextNode,
    blocks: &[MdBlock],
    shape: ChainSourceShape,
    style_env: BlockStyleEnv,
    shape_env: ShapeEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<BlockDescriptor> {
    let mut descriptors: Vec<BlockDescriptor> = Vec::with_capacity(blocks.len());

    for block in blocks {
        let role = block_role(block);
        let style = resolve_block_style_core(BlockStyleCascade {
            role,
            node_styles: src.block_styles.as_slice(),
            page_styles: style_env.page_block_styles,
            doc_styles: style_env.doc_block_styles,
            resolved: style_env.resolved,
            node_font_size: f64::from(shape.node_font_size),
            node_font_family: src.font_family.as_ref(),
            node_font_weight: src.font_weight.as_ref(),
            node_fill: src.fill.as_ref(),
            node_align: src.align.as_ref(),
        });

        let block_font_size = style.font_size_px as f32;
        // Decoration thickness mirrors the single-box emit's derivation so a
        // chained heading underline matches a single-box one at the same size.
        let deco_thickness = (block_font_size as f64 / 14.0).max(1.0);

        // The block's resolved family overrides the chain family when the cascade
        // (or the node) supplies one; otherwise the chain families slice is used
        // directly (no copy — only allocate when the cascade supplies an override).
        let override_families: Option<Vec<String>> = style.font_family.as_ref().map(|fp| {
            vec![resolve_font_family_name(
                Some(fp),
                style_env.resolved,
                "Noto Sans",
            )]
        });
        let block_families: &[String] = override_families.as_deref().unwrap_or(shape.families);

        // Block weight/fill resolved once from the role style; per-span overrides
        // still win in `block_spans`.
        let block_weight = resolve_font_weight(
            style.font_weight.as_ref(),
            style_env.resolved,
            shape.base_weight,
        );
        let block_fill: Option<Color> = style
            .fill
            .as_ref()
            .and_then(|fp| resolve_property_color(fp, style_env.resolved, diagnostics, &src.id));

        // A horizontal rule has no shaped text: it becomes a single empty line
        // whose height carries the rule's surrounding gap and whose decoration is
        // the rule fill (centered in the band by the emit path), matching the
        // single-box look.
        if matches!(block, MdBlock::HorizontalRule) {
            let gap = style.space_before_px + style.space_after_px;
            descriptors.push(BlockDescriptor {
                tokens: Vec::new(),
                metrics: WordMetrics {
                    ascent: 0.0,
                    line_height: gap,
                    space_advance: 0.0,
                },
                line_style: LineStyle {
                    ascent: 0.0,
                    space_advance: 0.0,
                    font_size: block_font_size,
                    deco_thickness,
                },
                // Spacing folded into the spacer height, so no extra gap around it.
                space_before_px: 0.0,
                space_after_px: 0.0,
                is_spacer: true,
                left_indent_px: 0.0,
                decoration: Some(LineDecoration::Rule {
                    color: HR_COLOR,
                    thickness: HR_THICKNESS_PX,
                }),
            });
            continue;
        }

        let bctx = BlockShapeCtx {
            style: &style,
            font_size: block_font_size,
            weight: block_weight,
            fill: block_fill,
            resolved: style_env.resolved,
            node_id: &src.id,
        };
        let spans = block_spans(block, bctx, diagnostics);
        let (tokens, metrics) = shape_words(
            &spans,
            block_families,
            NodeShape {
                font_size: block_font_size,
                base_weight: block_weight,
                direction: shape.direction,
            },
            shape_env,
            diagnostics,
            &src.id,
            src.source_span,
        );

        // Left indent + full-width decoration recovered from the block kind, so a
        // chained blockquote/list/code block matches the single-box look. Indent
        // shifts the whole item right (and the emit path shrinks the usable width
        // to keep wrapped text inside the box); the code-block background fills
        // each line's band. EXHAUSTIVE over the text-bearing `MdBlock` variants
        // (HorizontalRule returned above).
        let (left_indent_px, decoration) = match block {
            MdBlock::Blockquote { .. } => (BLOCKQUOTE_INDENT_PX, None),
            MdBlock::ListItem { depth, .. } => ((*depth as f64) * LIST_INDENT_PX, None),
            MdBlock::CodeBlock { .. } => (0.0, Some(LineDecoration::Background(CODE_BLOCK_BG))),
            MdBlock::Heading { .. } | MdBlock::Paragraph { .. } => (0.0, None),
            // Returned earlier as a spacer; unreachable here but matched
            // exhaustively (no `_`) so a new variant forces a decision.
            MdBlock::HorizontalRule => (0.0, None),
        };

        descriptors.push(BlockDescriptor {
            tokens,
            metrics,
            line_style: LineStyle {
                ascent: metrics.ascent,
                space_advance: metrics.space_advance,
                font_size: block_font_size,
                deco_thickness,
            },
            space_before_px: style.space_before_px,
            space_after_px: style.space_after_px,
            is_spacer: false,
            left_indent_px,
            decoration,
        });
    }

    descriptors
}

/// The per-block resolved style + context threaded into span construction.
#[derive(Clone, Copy)]
struct BlockShapeCtx<'a> {
    style: &'a ResolvedBlockStyle,
    font_size: f32,
    weight: u16,
    fill: Option<Color>,
    resolved: &'a BTreeMap<String, ResolvedToken>,
    node_id: &'a str,
}

/// Build the [`ResolvedSpan`]s for one block, applying the block's resolved
/// font/size/fill/weight while still honoring per-span overrides (fill, weight,
/// italic, decorations, links, vertical-align). EXHAUSTIVE over [`MdBlock`].
fn block_spans(
    block: &MdBlock,
    ctx: BlockShapeCtx,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ResolvedSpan> {
    match block {
        MdBlock::Heading { spans, .. }
        | MdBlock::Paragraph { spans }
        | MdBlock::Blockquote { spans } => text_spans_to_resolved(spans, None, ctx, diagnostics),
        MdBlock::ListItem {
            kind,
            ordinal,
            spans,
            ..
        } => {
            // Prepend the list marker as a leading literal span in the block's
            // style (v1: no hanging-indent gutter; the bullet flows inline).
            let marker = match kind {
                ListKind::Unordered => "• ".to_owned(),
                ListKind::Ordered => format!("{}. ", ordinal.unwrap_or(1)),
            };
            text_spans_to_resolved(spans, Some(marker), ctx, diagnostics)
        }
        MdBlock::CodeBlock { content, .. } => {
            // Raw content as ONE literal span; the shaper switches it to the
            // bundled mono family because `code` is set. No inline parsing.
            // The full-width band background is delivered via the block's
            // `decoration: LineDecoration::Background(CODE_BLOCK_BG)` stamped
            // onto every packed line by the distributor (not here in spans).
            vec![ResolvedSpan {
                text: content.clone(),
                color: ctx.fill.unwrap_or(Color::srgb(0, 0, 0, 255)),
                underline: false,
                strikethrough: false,
                highlight: None,
                code: true,
                link: None,
                weight: ctx.weight,
                style: FontStyle::Normal,
                font_size: ctx.font_size,
                baseline_dy: 0.0,
            }]
        }
        // A horizontal rule contributes no shaped text: the distributor emits a
        // single empty spacer line with `LineDecoration::Rule` for it (drawn
        // centered in its band by the emit path), so no spans are needed here.
        MdBlock::HorizontalRule => Vec::new(),
    }
}

/// Convert a block's [`TextSpan`]s into [`ResolvedSpan`]s with the block's
/// resolved style as the per-span default, optionally prepending a leading
/// literal `marker` (list bullet) in the block style.
fn text_spans_to_resolved(
    spans: &[TextSpan],
    marker: Option<String>,
    ctx: BlockShapeCtx,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ResolvedSpan> {
    let resolved = ctx.resolved;
    let node_id = ctx.node_id;
    let block_italic = ctx.style.italic == Some(true);
    let mut out: Vec<ResolvedSpan> = Vec::new();

    if let Some(m) = marker {
        out.push(ResolvedSpan {
            text: m,
            color: ctx.fill.unwrap_or(Color::srgb(0, 0, 0, 255)),
            underline: false,
            strikethrough: false,
            highlight: None,
            code: false,
            link: None,
            weight: ctx.weight,
            style: if block_italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            font_size: ctx.font_size,
            baseline_dy: 0.0,
        });
    }

    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        // Per-span fill precedence: span fill > link color > block fill > black.
        let is_link = span.link.is_some();
        let color = span
            .fill
            .as_ref()
            .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, node_id))
            .or(is_link.then_some(LINK_COLOR))
            .or(ctx.fill)
            .unwrap_or(Color::srgb(0, 0, 0, 255));
        let highlight: Option<Color> = span
            .highlight
            .as_ref()
            .and_then(|hp| resolve_property_color(hp, resolved, diagnostics, node_id));
        let code = span.code == Some(true);
        let link = span.link.clone();
        // Weight: span override else the block weight.
        let weight = resolve_font_weight(span.font_weight.as_ref(), resolved, ctx.weight);
        // Italic: span override else the block's role italic.
        let font_style = if span.italic == Some(true) || block_italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        let (span_font_size, baseline_dy) =
            resolve_vertical_align(span.vertical_align.as_deref(), ctx.font_size);
        out.push(ResolvedSpan {
            text: span.text.clone(),
            color,
            underline: span.underline == Some(true) || is_link,
            strikethrough: span.strikethrough == Some(true),
            highlight,
            code,
            link,
            weight,
            style: font_style,
            font_size: span_font_size,
            baseline_dy,
        });
    }

    out
}
