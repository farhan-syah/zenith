//! Render a threaded-text chain member's PRE-ASSIGNED lines into its own box,
//! reusing the shared [`super::emit::emit_lines`] body plus the same rotation,
//! blend, effect/mask, and baseline-grid handling the single-box wrap path uses.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, ResolvedToken, TextNode, dim_to_px};
use zenith_layout::TextDirection;

use crate::ir::SceneCommand;

use super::super::paint::{
    NodeEffect, emit_node_with_effects, resolve_property_filter, resolve_property_mask,
    resolve_property_shadow,
};
use super::super::util::{blend_mode_ir, rotation_degrees};
use super::baseline::{baseline_grid_snap_failed_diag, snap_to_baseline_grid};
use super::ctx::{ChainMemberPlace, EmitStyle};
use super::emit::emit_lines;

/// Render a chain member's PRE-ASSIGNED lines into its own box.
///
/// The lines were shaped + packed by the chain pre-pass using the chain
/// source's shared style; this function only positions them in THIS box using
/// the box's own geometry/align, with the same rotation + shadow brackets and
/// the SHARED [`emit_lines`] code the single-box wrap path uses. Returns the
/// laid-out content height (line count × line height) for flow-advance parity.
pub(in crate::compile) fn render_chain_member(
    text: &TextNode,
    assignment: &super::super::chain::ChainAssignment,
    place: ChainMemberPlace,
    resolved: &BTreeMap<String, ResolvedToken>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let ChainMemberPlace {
        font_size,
        text_x,
        text_y,
        baseline_grid,
        glyph_stroke,
    } = place;

    // Box width is required to position lines; height/align are optional.
    let box_w = match text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)) {
        Some(w) => w,
        None => return 0.0,
    };
    let box_h_opt: Option<f64> = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let align = text.align.as_deref().unwrap_or("start");
    let deco_thickness = (font_size as f64 / 14.0).max(1.0);

    // ── Baseline-grid snap (chain member) ────────────────────────────────
    // Chain members share the page grid so columns align (this is what makes a
    // three-column chain on a 14px grid line up). Compute the snap from this
    // member box's own `text_y` and the shared grid `g`, with the same drop-cap
    // guard as the single-box path (drop cap + baseline-grid is a v0 follow-up).
    // With no grid this leaves `emit_text_y`/`emit_metrics` untouched
    // (byte-identical to before).
    let mut emit_text_y = text_y;
    let mut emit_metrics = assignment.metrics;
    let drop_cap_active = matches!(text.drop_cap_lines, Some(n) if n >= 1);
    if let Some(g) = baseline_grid
        && g.is_finite()
        && g > 0.0
        && !drop_cap_active
    {
        let (snapped_text_y, effective_line_height) = snap_to_baseline_grid(
            text_y,
            assignment.metrics.ascent,
            assignment.metrics.line_height,
            g,
        );
        emit_text_y = snapped_text_y;
        emit_metrics.line_height = effective_line_height;
        if assignment.metrics.line_height > g && !assignment.lines.is_empty() {
            diagnostics.push(baseline_grid_snap_failed_diag(
                &text.id,
                assignment.metrics.line_height,
                g,
                text.source_span,
            ));
        }
    }

    // overflow="fit": this member's assigned content must fit its own box. For
    // a continuation/last member this catches an article that overruns even the
    // final panel. Mirrors the single-box height-overflow check.
    if text.overflow.as_deref() == Some("fit")
        && let Some(box_h) = box_h_opt
    {
        const EPSILON: f64 = 0.5;
        // Sum the per-line heights: a chained markdown flow has heterogeneous line
        // heights (headings vs body + folded inter-block gaps), so the content
        // height is the cumulative `height_px`, not `lines × line_height`. For a
        // uniform chain every `height_px` equals `metrics.line_height`, so this is
        // identical to the prior formula (byte-identical for non-markdown chains).
        let content_height = assignment.lines.iter().map(|l| l.height_px).sum::<f64>();
        if content_height > box_h + EPSILON {
            diagnostics.push(Diagnostic::error(
                "text.fit_failed",
                format!(
                    "text '{}': chain content does not fit its box (overflow=\"fit\"): \
                     at {:.0}px font-size it needs ~{:.0}px height in a {:.0}px-tall box",
                    text.id, font_size as f64, content_height, box_h
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
        }
    }

    // Bracket order matches the non-chain path: PushTransform (rotation,
    // outermost) → BeginShadow → glyphs → EndShadow → PopTransform.
    // Rotation only when both w and h present (safe pivot center).
    let rot = rotation_degrees(text.rotate.as_ref());
    let text_rot = rot
        .zip(Some(box_w))
        .zip(box_h_opt)
        .map(|((a, bw), bh)| (a, text_x + bw / 2.0, text_y + bh / 2.0));
    if let Some((angle, cx, cy)) = text_rot {
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // BLEND-MODE layer bracket (inside rotation, outside shadow). The chain
    // pre-pass already baked the node/ctx opacity into each word color, so the
    // layer uses opacity 1.0 — it only changes the compositing operator, never
    // re-applies opacity. Absent for normal/no blend (byte-identical).
    let blend = blend_mode_ir(text.blend_mode.as_deref());
    if let Some(blend_mode) = blend {
        commands.push(SceneCommand::PushLayer {
            opacity: 1.0,
            blend_mode: Some(blend_mode),
        });
    }

    // BLUR / SHADOW / FILTER effect (innermost). Blur > shadow > filter; at most
    // one is chosen. The winning effect plus the optional mask bracket the
    // member's glyph draws via `emit_node_with_effects` below. An empty member
    // (no assigned lines) carries no effect (matching the prior guard).
    let blur_sigma = text
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let effect: Option<NodeEffect> = if assignment.lines.is_empty() {
        None
    } else if let Some(sigma) = blur_sigma {
        Some(NodeEffect::Blur(sigma))
    } else if let Some(shadows) = text
        .shadow
        .as_ref()
        .and_then(|p| resolve_property_shadow(p, resolved, &text.id))
    {
        Some(NodeEffect::Shadow(shadows))
    } else {
        text.filter
            .as_ref()
            .and_then(|p| resolve_property_filter(p, resolved, &text.id))
            .map(NodeEffect::Filter)
    };

    // Resolve the optional node mask against the member's box. The box height
    // falls back to the laid-out content height when `h` is absent.
    let mask = text.mask.as_ref().and_then(|p| {
        let mask_h = box_h_opt.unwrap_or(assignment.lines.len() as f64 * emit_metrics.line_height);
        resolve_property_mask(p, resolved, (text_x, text_y, box_w, mask_h))
    });

    // Collect the member's glyph draws into a local buffer so the helper can
    // bracket them with the effect and/or mask (byte-identical when neither set).
    let mut draws: Vec<SceneCommand> = Vec::new();

    // Honor the node's direction for line layout. The chain pre-pass shapes the
    // source's spans with the source direction (see [`super::super::chain`]); here
    // the member's own `direction` drives line ordering/alignment. RTL chains are
    // feasible because shaping + this emit both consult direction.
    let chain_direction = match text.direction.as_deref() {
        Some("rtl") => TextDirection::Rtl,
        _ => TextDirection::Ltr,
    };

    emit_lines(
        &assignment.lines,
        text_x,
        // Baseline-grid snap (no-op when no grid is active): the first baseline
        // lands on the shared page grid so columns align across members.
        emit_text_y,
        box_w,
        EmitStyle {
            align,
            metrics: emit_metrics,
            font_size,
            deco_thickness,
            // Only the FINAL chain member leaves its last line ragged under
            // justify; a continuation box justifies its last line because the
            // paragraph flows on into the next box.
            justify_final_line: !assignment.is_last_member,
            direction: chain_direction,
            glyph_stroke,
        },
        &mut draws,
    );

    // Emit the collected glyph draws, bracketed by the winning effect and/or
    // mask. No effect + no mask → draws appended verbatim (byte-identical).
    emit_node_with_effects(commands, draws, effect, mask);

    if blend.is_some() {
        commands.push(SceneCommand::PopLayer);
    }

    if text_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    // Laid-out content height = sum of per-line heights (heterogeneous for a
    // chained markdown flow; identical to `lines × line_height` for a uniform
    // chain, so byte-identical flow-advance for non-markdown chains).
    assignment.lines.iter().map(|l| l.height_px).sum::<f64>()
}
