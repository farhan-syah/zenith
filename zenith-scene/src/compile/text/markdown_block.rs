//! Single-box block-level markdown layout.
//!
//! Renders a `format="markdown"` text node as VERTICALLY STACKED, individually
//! styled blocks (headings, paragraphs, blockquotes, list items, code blocks,
//! horizontal rules) instead of one flat wrapped run. Activated only for a
//! NON-CHAINED node whose id is present in the parsed-markdown side-channel
//! ([`super::super::markdown_resolve::MdBlockMap`]); every other node falls
//! through to the historical inline path (byte-identical).
//!
//! ## Reuse, don't reimplement
//!
//! For each block this module SYNTHESIZES a temporary [`TextNode`] (a clone of
//! the real node with its style/spans/geometry overridden) and delegates to the
//! existing per-node compile ([`super::text_node::compile_text_sized`]), so all
//! shaping / wrapping / emit / bullet / measure logic is reused verbatim. The
//! synth node's geometry places it at the running vertical cursor; the compile
//! returns the consumed pixel height which advances the cursor.
//!
//! ## Cascade
//!
//! Each block resolves its role style with per-property precedence node > page >
//! document (highest specificity wins); an absent property falls back to the
//! text node's OWN base typography. Styling is author-controlled: a block with
//! no `block` decl renders in the node's base font, NOT a hardcoded heading
//! look. Only spacing has conventional defaults (overridable per role).
//!
//! ## Byte-identity
//!
//! A lone [`MdBlock::Paragraph`] with no block decls synthesizes a node equal to
//! the original (same font / x / y / w, top-aligned, `space_before` suppressed
//! for the first block) and so reduces to the historical inline path's command
//! stream.

use std::collections::BTreeMap;

use zenith_core::{
    BlockStyle, Diagnostic, Dimension, ListKind, MdBlock, PropertyValue, ResolvedToken, TextNode,
    TextSpan, Unit, dim_to_px,
};

use crate::ir::{Color, Paint, SceneCommand};

use super::super::RenderCtx;
use super::super::util::{resolve_geometry_px, resolve_property_dimension_px};
use super::ctx::{TextCompileEnv, empty_md_blocks};
use super::measure::font_size_px;
use super::shape::CODE_MONO_FAMILY;
use super::text_node::compile_text_sized;

// ── Internal layout constants ───────────────────────────────────────────────

/// Left indent (px) applied to a blockquote block. Shared with the chain block
/// flow so a chained blockquote indents identically to a single-box one.
pub(in crate::compile) const BLOCKQUOTE_INDENT_PX: f64 = 24.0;
/// Per-depth-level indent (px) applied to a list item. Shared with the chain
/// block flow so a chained list item indents identically to a single-box one.
pub(in crate::compile) const LIST_INDENT_PX: f64 = 24.0;
/// Color of a horizontal-rule fill (muted gray, #CCCCCC). Shared with the chain
/// block flow so a chained `hr` rule matches a single-box one.
pub(in crate::compile) const HR_COLOR: Color = Color::srgb(204, 204, 204, 255);
/// Background color behind a fenced code block (light neutral gray, #F5F5F5).
/// Shared with the chain block flow so a chained code block matches a single-box one.
pub(in crate::compile) const CODE_BLOCK_BG: Color = Color::srgb(245, 245, 245, 255);
/// Thickness (px) of a horizontal-rule fill. Shared with the chain block flow.
pub(in crate::compile) const HR_THICKNESS_PX: f64 = 2.0;

// ── Default spacing factors (× the block's resolved font size) ──────────────
// Applied only when the role's cascade carries NO explicit space_before /
// space_after, so prose is readable out of the box yet fully overridable.

/// Default space AFTER a paragraph.
const PARAGRAPH_SPACE_AFTER_FACTOR: f64 = 0.5;
/// Default space BEFORE a heading.
const HEADING_SPACE_BEFORE_FACTOR: f64 = 0.6;
/// Default space AFTER a heading.
const HEADING_SPACE_AFTER_FACTOR: f64 = 0.25;
/// Default space AFTER a blockquote / list item / code block.
const BLOCK_SPACE_AFTER_FACTOR: f64 = 0.4;
/// Default space BEFORE and AFTER a horizontal rule.
const HR_SPACE_FACTOR: f64 = 0.5;

/// A block role's fully resolved, concrete style + spacing.
pub(in crate::compile) struct ResolvedBlockStyle {
    /// Font family as a `PropertyValue` ready to set on the synth node (token
    /// ref or literal); `None` keeps the node's own family.
    pub(in crate::compile) font_family: Option<PropertyValue>,
    /// Font size in pixels.
    pub(in crate::compile) font_size_px: f64,
    /// Font weight as a `PropertyValue` (token ref or literal); `None` keeps the
    /// node's own weight.
    pub(in crate::compile) font_weight: Option<PropertyValue>,
    /// Fill color as a `PropertyValue`; `None` keeps the node's own fill.
    pub(in crate::compile) fill: Option<PropertyValue>,
    /// Horizontal alignment override (`"left"`/`"center"`/`"right"`/`"justify"`);
    /// `None` keeps the node's own align.
    pub(in crate::compile) align: Option<String>,
    /// Italic override; `None` keeps upright.
    pub(in crate::compile) italic: Option<bool>,
    /// Space inserted above the block, in pixels.
    pub(in crate::compile) space_before_px: f64,
    /// Space inserted below the block, in pixels.
    pub(in crate::compile) space_after_px: f64,
}

/// Map an [`MdBlock`] to its block-role string (the cascade key).
pub(in crate::compile) fn block_role(block: &MdBlock) -> &'static str {
    match block {
        MdBlock::Heading { level, .. } => match level {
            1 => "h1",
            2 => "h2",
            3 => "h3",
            4 => "h4",
            5 => "h5",
            // 6 and any out-of-range value clamp to h6.
            _ => "h6",
        },
        MdBlock::Paragraph { .. } => "p",
        MdBlock::Blockquote { .. } => "blockquote",
        MdBlock::ListItem { .. } => "li",
        MdBlock::CodeBlock { .. } => "code-block",
        MdBlock::HorizontalRule => "hr",
    }
}

/// First `Some` property in cascade order: node block styles → page → document.
fn cascade_prop<'a, F>(
    role: &str,
    node_styles: &'a [BlockStyle],
    page_styles: &'a [BlockStyle],
    doc_styles: &'a [BlockStyle],
    pick: F,
) -> Option<&'a PropertyValue>
where
    F: Fn(&BlockStyle) -> Option<&PropertyValue>,
{
    for scope in [node_styles, page_styles, doc_styles] {
        if let Some(found) = scope.iter().find(|b| b.role == role).and_then(&pick) {
            return Some(found);
        }
    }
    None
}

/// First `Some` `align`/`italic`/spacing field in cascade order.
fn cascade_field<'a, T, F>(
    role: &str,
    node_styles: &'a [BlockStyle],
    page_styles: &'a [BlockStyle],
    doc_styles: &'a [BlockStyle],
    pick: F,
) -> Option<T>
where
    F: Fn(&'a BlockStyle) -> Option<T>,
{
    for scope in [node_styles, page_styles, doc_styles] {
        if let Some(found) = scope.iter().find(|b| b.role == role).and_then(&pick) {
            return Some(found);
        }
    }
    None
}

/// Default `(space_before, space_after)` factors (× font size) for a role when
/// the cascade supplies no explicit spacing.
fn default_spacing_factors(role: &str) -> (f64, f64) {
    match role {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            (HEADING_SPACE_BEFORE_FACTOR, HEADING_SPACE_AFTER_FACTOR)
        }
        "p" => (0.0, PARAGRAPH_SPACE_AFTER_FACTOR),
        "blockquote" | "li" | "code-block" => (0.0, BLOCK_SPACE_AFTER_FACTOR),
        "hr" => (HR_SPACE_FACTOR, HR_SPACE_FACTOR),
        // Any unknown role behaves like a paragraph.
        _ => (0.0, PARAGRAPH_SPACE_AFTER_FACTOR),
    }
}

/// Resolve a block role's concrete style via the node > page > document cascade,
/// falling back to the text node's own base typography per property.
fn resolve_block_style_for_role(
    role: &str,
    text: &TextNode,
    env: TextCompileEnv,
) -> ResolvedBlockStyle {
    let node_font_size = f64::from(font_size_px(text, env.resolved, env.style_map));
    resolve_block_style_core(BlockStyleCascade {
        role,
        node_styles: text.block_styles.as_slice(),
        page_styles: env.page_block_styles,
        doc_styles: env.doc_block_styles,
        resolved: env.resolved,
        node_font_size,
        node_font_family: text.font_family.as_ref(),
        node_font_weight: text.font_weight.as_ref(),
        node_fill: text.fill.as_ref(),
        node_align: text.align.as_ref(),
    })
}

/// The inputs the block-role cascade needs, bundled into one `Copy`-ish struct so
/// both the single-box markdown path and the chain block path call the SAME
/// resolver (keeping cascade behavior byte-identical between the two). The
/// `node_*` fields are the node-level fallbacks applied per property when the
/// cascade supplies none; `node_font_size` is the already-resolved node base size
/// (px) used both as the font-size fallback and as the spacing-factor base.
pub(in crate::compile) struct BlockStyleCascade<'a> {
    pub(in crate::compile) role: &'a str,
    pub(in crate::compile) node_styles: &'a [BlockStyle],
    pub(in crate::compile) page_styles: &'a [BlockStyle],
    pub(in crate::compile) doc_styles: &'a [BlockStyle],
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) node_font_size: f64,
    pub(in crate::compile) node_font_family: Option<&'a PropertyValue>,
    pub(in crate::compile) node_font_weight: Option<&'a PropertyValue>,
    pub(in crate::compile) node_fill: Option<&'a PropertyValue>,
    pub(in crate::compile) node_align: Option<&'a String>,
}

/// Resolve a block role's concrete style + spacing via the node > page > document
/// cascade, falling back per-property to the node-level values. This is the
/// SINGLE cascade resolver shared by the single-box markdown layout and the
/// chained-markdown block flow, so both honor the same precedence.
pub(in crate::compile) fn resolve_block_style_core(c: BlockStyleCascade) -> ResolvedBlockStyle {
    let BlockStyleCascade {
        role,
        node_styles,
        page_styles,
        doc_styles,
        resolved,
        node_font_size,
        node_font_family,
        node_font_weight,
        node_fill,
        node_align,
    } = c;

    // Font family: cascade override, else the node's own family.
    let font_family = cascade_prop(role, node_styles, page_styles, doc_styles, |b| {
        b.font_family.as_ref()
    })
    .cloned()
    .or_else(|| node_font_family.cloned());

    // Font size px: cascade override resolves against the token map, else the
    // node's own resolved size.
    let font_size_px = match cascade_prop(role, node_styles, page_styles, doc_styles, |b| {
        b.font_size.as_ref()
    }) {
        Some(prop) => resolve_property_dimension_px(Some(prop), resolved, node_font_size),
        None => node_font_size,
    };

    let font_weight = cascade_prop(role, node_styles, page_styles, doc_styles, |b| {
        b.font_weight.as_ref()
    })
    .cloned()
    .or_else(|| node_font_weight.cloned());

    let fill = cascade_prop(role, node_styles, page_styles, doc_styles, |b| {
        b.fill.as_ref()
    })
    .cloned()
    .or_else(|| node_fill.cloned());

    let align = cascade_field(role, node_styles, page_styles, doc_styles, |b| {
        b.align.clone()
    })
    .or_else(|| node_align.cloned());

    let italic = cascade_field(role, node_styles, page_styles, doc_styles, |b| b.italic);

    // Spacing: explicit cascade dimension (resolved to px) else the role default
    // proportional to the resolved font size.
    let (sb_factor, sa_factor) = default_spacing_factors(role);
    let space_before_px = cascade_field(role, node_styles, page_styles, doc_styles, |b| {
        b.space_before.as_ref()
    })
    .and_then(|d| dim_to_px(d.value, &d.unit))
    .unwrap_or(font_size_px * sb_factor);
    let space_after_px = cascade_field(role, node_styles, page_styles, doc_styles, |b| {
        b.space_after.as_ref()
    })
    .and_then(|d| dim_to_px(d.value, &d.unit))
    .unwrap_or(font_size_px * sa_factor);

    ResolvedBlockStyle {
        font_family,
        font_size_px,
        font_weight,
        fill,
        align,
        italic,
        space_before_px,
        space_after_px,
    }
}

/// Build the base synthetic [`TextNode`] for a block: a clone of the real node
/// with chain/anchor/format/src cleared, top vertical-align, the resolved role
/// style applied, and the x/y geometry slot installed. Callers must set `w`
/// (and optionally `bullet`) before passing the node to the compile path.
fn synth_base(text: &TextNode, style: &ResolvedBlockStyle, x: f64, y: f64) -> TextNode {
    let mut n = text.clone();
    // Avoid recursion + block re-entry: a synth node must not re-trigger block
    // layout, the chain path, anchor derivation, or markdown re-parsing.
    n.chain = None;
    n.content_format = None;
    n.src = None;
    n.anchor = None;
    n.anchor_zone = None;
    n.anchor_sibling = None;
    n.anchor_edge = None;
    n.anchor_gap = None;
    n.anchor_parent = None;
    // Block stacks from the top; height is intrinsic (let it take what it needs).
    n.v_align = Some("top".to_owned());
    n.h = None;
    // x/y geometry slot for this block. Callers must set w.
    n.x = Some(PropertyValue::Dimension(px_dim(x)));
    n.y = Some(PropertyValue::Dimension(px_dim(y)));
    n.w = None;
    // Reset hanging-indent machinery; callers set bullet geometry per block.
    n.bullet = None;
    n.bullet_gap = None;
    n.padding_left = None;
    n.text_indent = None;
    // Apply the resolved role style.
    n.font_family = style.font_family.clone();
    n.font_size = Some(PropertyValue::Dimension(px_dim(style.font_size_px)));
    n.font_weight = style.font_weight.clone();
    n.fill = style.fill.clone();
    n.align = style.align.clone();
    n
}

/// Wrap `value` into an `(px)`-unit [`Dimension`].
fn px_dim(value: f64) -> Dimension {
    Dimension {
        value,
        unit: Unit::Px,
    }
}

/// Apply the role's italic flag to every span (TextNode carries no node-level
/// italic; spans do).
fn apply_italic(spans: &mut [TextSpan], italic: Option<bool>) {
    if italic == Some(true) {
        for s in spans {
            s.italic = Some(true);
        }
    }
}

/// Compile a markdown text node as stacked blocks. Returns the total consumed
/// height in pixels (the same shape [`compile_text_sized`] returns), so a
/// flow-layout parent advances past the whole block stack.
pub(in crate::compile) fn compile_markdown_blocks(
    text: &TextNode,
    blocks: &[MdBlock],
    env: TextCompileEnv,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    if text.visible == Some(false) {
        return 0.0;
    }

    // Resolve the box origin/width once (anchor derivation when x/y absent).
    let anchor_xy = env.anchors.get(&text.id).copied();
    let Some(box_x) =
        resolve_geometry_px(text.x.as_ref(), env.resolved).or(anchor_xy.map(|(ax, _)| ax))
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "text node '{}' is missing x or y geometry; skipped",
                text.id
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
        return 0.0;
    };
    let Some(box_y) =
        resolve_geometry_px(text.y.as_ref(), env.resolved).or(anchor_xy.map(|(_, ay)| ay))
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "text node '{}' is missing x or y geometry; skipped",
                text.id
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
        return 0.0;
    };
    // Box width: declared, else 0 (a width-less markdown box is unusual but the
    // synth path handles a no-width node by single-lining, mirroring plain text).
    let box_w = resolve_geometry_px(text.w.as_ref(), env.resolved);

    // The synth compiles must NOT see the block side-channel (would recurse), so
    // run them against an env whose md_blocks is empty.
    let mut synth_env = env;
    synth_env.md_blocks = empty_md_blocks();

    // Absolute origin of the block stack (group translation handled by the synth
    // compile via `ctx`, so the synth node geometry stays in authored space).
    let mut y_cursor = box_y;

    for (i, block) in blocks.iter().enumerate() {
        let role = block_role(block);
        let style = resolve_block_style_for_role(role, text, env);
        // The very first block has no gap above it.
        let space_before = if i == 0 { 0.0 } else { style.space_before_px };
        let block_top = y_cursor + space_before;

        let block_height = match block {
            MdBlock::Heading { spans, .. }
            | MdBlock::Paragraph { spans }
            | MdBlock::Blockquote { spans } => {
                let indent = if matches!(block, MdBlock::Blockquote { .. }) {
                    BLOCKQUOTE_INDENT_PX
                } else {
                    0.0
                };
                let slot_w = box_w.map(|w| (w - indent).max(0.0));
                let mut synth = synth_base(text, &style, box_x + indent, block_top);
                synth.w = slot_w.map(|v| PropertyValue::Dimension(px_dim(v)));
                synth.spans = spans.clone();
                apply_italic(&mut synth.spans, style.italic);
                compile_text_sized(&synth, synth_env, commands, diagnostics, ctx)
            }
            MdBlock::ListItem {
                kind,
                depth,
                ordinal,
                spans,
            } => {
                let indent = (*depth as f64) * LIST_INDENT_PX;
                let slot_w = box_w.map(|w| (w - indent).max(0.0));
                let mut synth = synth_base(text, &style, box_x + indent, block_top);
                synth.w = slot_w.map(|v| PropertyValue::Dimension(px_dim(v)));
                // Reuse the node's bullet machinery: the marker is drawn in the
                // gutter and continuation lines auto-align to the text column.
                let marker = match kind {
                    ListKind::Unordered => "•".to_owned(),
                    ListKind::Ordered => format!("{}.", ordinal.unwrap_or(1)),
                };
                synth.bullet = Some(marker);
                synth.spans = spans.clone();
                apply_italic(&mut synth.spans, style.italic);
                compile_text_sized(&synth, synth_env, commands, diagnostics, ctx)
            }
            MdBlock::CodeBlock { content, .. } => {
                // Synth a mono node with the raw content as a single literal span
                // (no inline parsing). Emit a background rect behind it first.
                let mut synth = synth_base(text, &style, box_x, block_top);
                synth.font_family = Some(PropertyValue::Literal(CODE_MONO_FAMILY.to_owned()));
                synth.w = box_w.map(|v| PropertyValue::Dimension(px_dim(v)));
                synth.spans = vec![literal_span(content.clone())];

                // Compile glyphs into a scratch buffer first so the background
                // rect can be sized from the returned height before the glyphs
                // are emitted — avoids a second shaping pass.
                let draw_start = commands.len();
                let code_h = compile_text_sized(&synth, synth_env, commands, diagnostics, ctx);
                // Split off the glyph commands; they will be re-appended after
                // the background rect so the rect renders beneath the text.
                let glyph_cmds = commands.split_off(draw_start);
                if let Some(w) = box_w {
                    commands.push(SceneCommand::FillRect {
                        x: box_x + ctx.dx,
                        y: block_top + ctx.dy,
                        w,
                        h: code_h,
                        paint: Paint::solid(CODE_BLOCK_BG),
                    });
                }
                commands.extend(glyph_cmds);
                code_h
            }
            MdBlock::HorizontalRule => {
                // A thin muted rule centered in its space; advance is the rule
                // thickness only (the surrounding space is space_before/after).
                if let Some(w) = box_w {
                    commands.push(SceneCommand::FillRect {
                        x: box_x + ctx.dx,
                        y: block_top + ctx.dy,
                        w,
                        h: HR_THICKNESS_PX,
                        paint: Paint::solid(HR_COLOR),
                    });
                }
                HR_THICKNESS_PX
            }
        };

        y_cursor = block_top + block_height + style.space_after_px;
    }

    // Total consumed height (matches `compile_text_sized`'s f64 height return).
    let total_height = (y_cursor - box_y).max(0.0);

    // ── Overflow warning ─────────────────────────────────────────────────────
    // When the stacked blocks are taller than the declared box, emit a warning
    // so the author knows to enlarge the box, reduce sizing, or chain a
    // continuation box. This fires regardless of the node's `overflow` value
    // (including `overflow="visible"`) — the markdown path always warns on
    // excess height. When `h` is absent there is no box constraint to check.
    if let Some(box_h) = resolve_geometry_px(text.h.as_ref(), env.resolved) {
        const EPSILON: f64 = 0.5;
        if total_height > box_h + EPSILON {
            let delta = total_height - box_h;
            diagnostics.push(Diagnostic::warning(
                "text.overflow",
                format!(
                    "text '{}': markdown content ({:.0}px) exceeds the box height ({:.0}px) \
                     by {:.0}px; enlarge the box height, reduce font-size/spacing, \
                     or add a chained continuation box (chain=\"{}\") on another page",
                    text.id, total_height, box_h, delta, text.id
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
        }
    }

    total_height
}

/// A plain literal span carrying `text` and no styling.
fn literal_span(text: String) -> TextSpan {
    TextSpan {
        text,
        fill: None,
        font_weight: None,
        italic: None,
        underline: None,
        strikethrough: None,
        vertical_align: None,
        footnote_ref: None,
        data_ref: None,
        data_format: None,
        highlight: None,
        code: None,
        link: None,
    }
}
