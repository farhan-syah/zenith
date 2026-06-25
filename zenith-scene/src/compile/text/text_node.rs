//! The `text` leaf compile path: the public `compile_text` entry, the
//! `overflow="autofit"` shrink-to-fit search, and the sized layout engine
//! (`compile_text_sized`) with its fast single-line path, tab-leader/chain
//! branches, overflow checks, and effect/mask/blend/rotation brackets. The
//! multi-sub-path WRAP body lives in [`super::wrap`].

use std::collections::BTreeSet;

use zenith_core::{
    Diagnostic, Dimension, FontStyle, PropertyValue, TextNode, TextSpan, Unit, dim_to_px,
};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine, ZenithGlyphRun};

use crate::ir::{Color, Paint, SceneCommand};

use super::super::RenderCtx;
use super::super::paint::{
    NodeEffect, emit_node_with_effects, resolve_property_color, resolve_property_filter,
    resolve_property_mask, resolve_property_shadow,
};
use super::super::style_prop;
use super::super::util::{resolve_property_dimension_px, rotation_degrees, unsupported_unit_diag};
use super::chain_member::render_chain_member;
use super::ctx::{ChainMemberPlace, ShapeEnv, TabLeaderArgs, TextCompileEnv};
use super::measure::{
    MeasureEnv, font_size_px, measure_text_wrapped_height, resolve_text_families,
};
use super::shape::{
    CODE_BG, CODE_MONO_FAMILY, LINK_COLOR, ResolvedSpan, emit_glyph_missing, resolve_font_weight,
    resolve_vertical_align, run_to_scene_glyphs,
};
use super::tableader::compile_tab_leader;
use super::wrap::{WrapEnv, WrapGeom, emit_wrap_path};

/// Compile a `text` leaf node.
///
/// This is the public entry point. It is a thin BLACK-BOX wrapper around
/// [`compile_text_sized`] (which carries every layout path verbatim):
///
/// - For any node whose `overflow` is NOT `"autofit"` it is a pure pass-through
///   — it forwards every argument unchanged to [`compile_text_sized`], so the
///   emitted [`SceneCommand`] stream is BYTE-IDENTICAL to before this attribute
///   existed (the determinism gate).
/// - For `overflow="autofit"` it drives [`compile_text_sized`] at TRIAL font
///   sizes (into throwaway buffers) to find the LARGEST size in
///   `[floor, declared]` whose content fits the box height, then performs the
///   single real emit at that size. See [`compile_text_autofit`].
///
/// Returns the laid-out content height in pixels (`line_count * line_height`).
pub(in crate::compile) fn compile_text(
    text: &TextNode,
    env: TextCompileEnv,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    if text.overflow.as_deref() != Some("autofit") {
        // Pass-through: byte-identical command stream for every non-autofit node.
        return compile_text_sized(text, env, commands, diagnostics, ctx);
    }
    compile_text_autofit(text, env, commands, diagnostics, ctx)
}

/// PowerPoint-style shrink-to-fit search for an `overflow="autofit"` text node.
///
/// Drives [`compile_text_sized`] at trial integer-px font sizes (into throwaway
/// command/diagnostic buffers) to find the LARGEST size in `[floor, declared]`
/// whose content fits the box height, then performs ONE real emit at that size.
///
/// - The declared node font size (px) is the search ceiling; `font-size-min`
///   (token → dimension) is the floor. When `font-size-min` is absent the floor
///   defaults to `(declared * 0.5).max(8.0)`.
/// - Both `box_w` and `box_h` must resolve; if either is missing autofit cannot
///   measure, so it falls back to a single [`compile_text_sized`] call with the
///   node's `overflow` left as-is (no crash, no silent skip).
/// - A trial at size `fs` FITS iff its throwaway diagnostics contain NO
///   `text.fit_failed` whose subject is this node id (the trial sets
///   `overflow="fit"` so the inner height-overflow check reports exactly that).
/// - The search is a DOWNWARD linear scan from `declared` to `floor` over
///   integer px, breaking on the first fit (deterministic: same inputs → same
///   `fs`).
/// - If some size fits, the real emit uses that size with `overflow="clip"` so
///   the fitted text renders clip-safe and emits NO `fit_failed`. If NONE fits
///   (even at the floor) the real emit uses the floor with `overflow="fit"`, so
///   the genuine `text.fit_failed` is emitted at the floor (PowerPoint gives up
///   too).
///
/// v0 limitation: a span carrying its OWN explicit `font-size` does not scale —
/// only the node-level font size drives inheriting spans (the typical single-
/// span title inherits, so it scales).
fn compile_text_autofit(
    text: &TextNode,
    env: TextCompileEnv,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    // Require both box dimensions to measure fit; otherwise fall back to a
    // single sized compile with overflow untouched (documented; no crash).
    let box_w = text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let box_h = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let (Some(_bw), Some(_bh)) = (box_w, box_h) else {
        return compile_text_sized(text, env, commands, diagnostics, ctx);
    };

    // Resolve the declared node font size (px) — the search ceiling — and the
    // floor from `font-size-min`, defaulting to `(declared * 0.5).max(8.0)`.
    let declared = f64::from(font_size_px(text, env.resolved, env.style_map));
    let floor = resolve_property_dimension_px(
        text.font_size_min.as_ref(),
        env.resolved,
        (declared * 0.5).max(8.0),
    );
    // Integer-px search bounds. Clamp the floor at/below the ceiling.
    let ceil_px = declared.floor().max(1.0) as i64;
    let floor_px = floor.floor().max(1.0).min(declared.floor().max(1.0)) as i64;

    // Build a trial/real clone at size `fs` with the given overflow.
    let clone_sized = |fs: f64, ov: &str| -> TextNode {
        let mut t = text.clone();
        t.font_size = Some(PropertyValue::Dimension(Dimension {
            value: fs,
            unit: Unit::Px,
        }));
        t.overflow = Some(ov.to_owned());
        t
    };

    // Does a trial at `fs` fit? Compile into throwaway buffers under
    // overflow="fit" and check for a `text.fit_failed` naming THIS node.
    let fits = |fs: f64| -> bool {
        let trial = clone_sized(fs, "fit");
        let mut throwaway_cmds: Vec<SceneCommand> = Vec::new();
        let mut throwaway_diags: Vec<Diagnostic> = Vec::new();
        compile_text_sized(&trial, env, &mut throwaway_cmds, &mut throwaway_diags, ctx);
        !throwaway_diags.iter().any(|d| {
            d.code == "text.fit_failed" && d.subject_id.as_deref() == Some(text.id.as_str())
        })
    };

    // Downward linear scan from the ceiling to the floor; break on first fit.
    let mut fitted: Option<i64> = None;
    let mut fs = ceil_px;
    while fs >= floor_px {
        if fits(fs as f64) {
            fitted = Some(fs);
            break;
        }
        fs -= 1;
    }

    // Real emit: the fitted size clipped-safe, or the floor with overflow="fit"
    // so the genuine fit_failed surfaces at the floor.
    let (real_fs, real_ov) = match fitted {
        Some(fs) => (fs as f64, "clip"),
        None => (floor_px as f64, "fit"),
    };
    let real = clone_sized(real_fs, real_ov);
    compile_text_sized(&real, env, commands, diagnostics, ctx)
}

/// Compile a `text` leaf node at its resolved font size (the unchanged layout
/// engine: wrap/fast/drop-cap/runaround/chain paths + overflow handling).
///
/// Returns the laid-out content height in pixels (`line_count * line_height`),
/// which the flow-layout path in [`super::super::container`] uses to advance its
/// vertical cursor past a text child that declares no explicit `h`. Early
/// returns (invisible, missing/bad geometry, empty spans) yield `0.0`.
pub(in crate::compile) fn compile_text_sized(
    text: &TextNode,
    env: TextCompileEnv,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    let resolved = env.resolved;
    let style_map = env.style_map;
    let fonts = env.fonts;
    let engine = env.engine;
    let chains = env.chains;
    let footnote_markers = env.footnote_markers;
    let node_boxes = env.node_boxes;
    let anchors = env.anchors;

    // Skip invisible text nodes.
    if text.visible == Some(false) {
        return 0.0;
    }

    // Anchor-derived (x, y): look up the pre-pass map when x or y is absent.
    let anchor_xy = anchors.get(&text.id).copied();

    // Resolve x — use authored value when present, anchor derivation when absent.
    let text_x_raw = match &text.x {
        Some(x_dim) => {
            let Some(v) = dim_to_px(x_dim.value, &x_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "text node",
                    &text.id,
                    "x",
                    text.source_span,
                ));
                return 0.0;
            };
            v
        }
        None => {
            if let Some((ax, _)) = anchor_xy {
                ax
            } else {
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
            }
        }
    };

    // Resolve y — same pattern.
    let text_y_raw = match &text.y {
        Some(y_dim) => {
            let Some(v) = dim_to_px(y_dim.value, &y_dim.unit) else {
                diagnostics.push(unsupported_unit_diag(
                    "text node",
                    &text.id,
                    "y",
                    text.source_span,
                ));
                return 0.0;
            };
            v
        }
        None => {
            if let Some((_, ay)) = anchor_xy {
                ay
            } else {
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
            }
        }
    };

    // Apply group translation offset.
    let text_x = text_x_raw + ctx.dx;
    let mut text_y = text_y_raw + ctx.dy;

    // Resolve glyph stroke early (before chain early-return) so it can be
    // threaded to render_chain_member as well. Both fields are None when the
    // node carries no stroke/stroke-width → byte-identical to before.
    let early_stroke_color: Option<Color> = text
        .stroke
        .as_ref()
        .and_then(|p| resolve_property_color(p, resolved, diagnostics, &text.id));
    let early_stroke_width: Option<f64> = {
        let w = resolve_property_dimension_px(text.stroke_width.as_ref(), resolved, -1.0);
        if w > 0.0 { Some(w) } else { None }
    };
    let early_glyph_stroke: (Option<Color>, Option<f64>) = (early_stroke_color, early_stroke_width);

    // ── Threaded-text chain member ───────────────────────────────────
    // If this node belongs to a text chain, its content was shaped and
    // distributed once by the page-level chain pre-pass; render the lines
    // ASSIGNED to this box instead of wrapping the node's own spans. A
    // continuation member (empty spans) renders here too. This branch must
    // precede the empty-spans early return below.
    if text.chain.is_some()
        && let Some(assignment) = chains.get(&text.id)
    {
        let fs = font_size_px(text, resolved, style_map);
        return render_chain_member(
            text,
            assignment,
            ChainMemberPlace {
                font_size: fs,
                text_x,
                text_y,
                baseline_grid: ctx.baseline_grid,
                glyph_stroke: early_glyph_stroke,
            },
            resolved,
            commands,
            diagnostics,
        );
    }

    // ── Markdown block-layout branch ─────────────────────────────────────
    // A NON-CHAINED `text` node whose id is present in the parsed-markdown
    // side-channel renders as vertically stacked, individually-styled blocks
    // (headings, paragraphs, lists, code blocks, rules) instead of one flat
    // wrapped run. Placed AFTER the chain early-exit so chained markdown nodes
    // keep their current single-run behavior untouched. A node absent from the
    // map (every non-markdown node, and synthetic-text callers) falls straight
    // through to the historical path below — byte-identical.
    if text.chain.is_none()
        && let Some(blocks) = env.md_blocks.get(&text.id)
    {
        return super::markdown_block::compile_markdown_blocks(
            text,
            blocks,
            env,
            commands,
            diagnostics,
            ctx,
        );
    }

    // Skip silently if every span is empty (nothing to draw).
    if text.spans.iter().all(|s| s.text.is_empty()) {
        return 0.0;
    }

    // ── Footnote inline markers ───────────────────────────────────────────
    // Expand the node's spans into the EFFECTIVE span list: a span carrying a
    // `footnote_ref` keeps its text, then is IMMEDIATELY followed by a synthetic
    // SUPERSCRIPT marker span (the referenced footnote's marker string), reusing
    // the vertical-align="super" path (reduced size + raised baseline). A ref that
    // names no footnote on this page → advisory `footnote.unresolved_ref` + no
    // marker. When no span carries a ref the effective list equals `text.spans`
    // (byte-identical to before). The synthetic marker inherits the ref span's
    // fill so it matches the marked word's color.
    let effective_spans: Vec<TextSpan> = if text.spans.iter().any(|s| s.footnote_ref.is_some()) {
        let mut out: Vec<TextSpan> = Vec::with_capacity(text.spans.len());
        for span in &text.spans {
            out.push(span.clone());
            if let Some(fref) = &span.footnote_ref {
                match footnote_markers.get(fref) {
                    Some(marker) => out.push(TextSpan {
                        text: marker.clone(),
                        fill: span.fill.clone(),
                        font_weight: None,
                        italic: None,
                        underline: None,
                        strikethrough: None,
                        vertical_align: Some("super".to_owned()),
                        footnote_ref: None,
                        data_ref: None,
                        data_format: None,
                        highlight: None,
                        code: None,
                        link: None,
                    }),
                    None => diagnostics.push(Diagnostic::advisory(
                        "footnote.unresolved_ref",
                        format!(
                            "text node '{}': span footnote-ref '{}' matches no footnote \
                             on this page; no marker emitted",
                            text.id, fref
                        ),
                        text.source_span,
                        Some(text.id.clone()),
                    )),
                }
            }
        }
        out
    } else {
        text.spans.clone()
    };

    // Resolve font family with style cascade (shared with the table measurer so
    // the resolution logic lives in ONE place).
    let families = resolve_text_families(text, resolved, style_map, fonts, diagnostics);

    // Resolve font size in pixels with style cascade; default to 16.0 if absent.
    let font_size: f32 = font_size_px(text, resolved, style_map);

    // ── Vertical alignment pre-offset ─────────────────────────────────────
    // When `v_align` is `"middle"` or `"bottom"` AND the box height is
    // available, pre-offset `text_y` by the measured text-block height relative
    // to the box height. Default (absent / "top" / unrecognized) applies 0
    // offset — byte-identical to today's behavior.
    //
    // Mirrors the shape node's label v-align exactly: measure wrapped height via
    // `measure_text_wrapped_height` then apply `(box_h - wrapped_h) / 2` or
    // `box_h - wrapped_h` before any other emit path sees `text_y`.
    //
    // Chain members use a pre-distributed layout and are unaffected: v-align on
    // a chain source is a documented no-op for chain continuations (the
    // distribution already positions each member's lines).
    if matches!(text.v_align.as_deref(), Some("middle") | Some("bottom")) {
        if let Some(box_h) = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)) {
            if let Some(box_w) = text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)) {
                let wrapped_h = measure_text_wrapped_height(
                    text,
                    box_w,
                    &families,
                    MeasureEnv {
                        resolved,
                        style_map,
                        fonts,
                        engine,
                    },
                    diagnostics,
                )
                .unwrap_or(0.0);
                let v_offset = match text.v_align.as_deref() {
                    Some("bottom") => (box_h - wrapped_h).max(0.0),
                    // "middle" and any other matched arm center vertically.
                    _ => ((box_h - wrapped_h) / 2.0).max(0.0),
                };
                text_y += v_offset;
            }
        }
    }

    // Node opacity, applied once and cascaded with ctx.opacity onto
    // every span's alpha below.
    let node_opacity = text.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

    // Blend-mode layer (see compile_rect). When a non-normal blend is active the
    // full opacity cascade rides on the PushLayer and the glyph colors are
    // emitted at full alpha (`color_opacity == 1.0`); otherwise `color_opacity`
    // keeps the prior `node_opacity * ctx.opacity`, so the non-blend command
    // stream is byte-identical. `layer_op` is the alpha the layer composites at.
    let blend = super::super::util::blend_mode_ir(text.blend_mode.as_deref());
    let layer_op = node_opacity * ctx.opacity;
    let color_opacity = if blend.is_some() {
        1.0
    } else {
        node_opacity * ctx.opacity
    };

    // Node-level fill/weight props with style cascade — these are the
    // per-span fallbacks (span override → node → style → default).
    let node_fill_prop: Option<&PropertyValue> = text
        .fill
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "fill"));
    let node_weight_prop: Option<&PropertyValue> = text
        .font_weight
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "font-weight"));

    // Glyph stroke (outline). Resolved earlier (before chain early-return) and
    // re-bound here for use in the text emit paths below.
    let glyph_stroke = early_glyph_stroke;

    // ── Tab-leader mode (table-of-contents rows) ──────────────────────────
    // A self-contained render branch taken ONLY when `tab-leader` is set to a
    // non-empty string. The normal fast/wrap paths below are untouched (and so
    // remain byte-identical) when `tab_leader` is None/empty.
    if let Some(leader) = text.tab_leader.as_deref().filter(|s| !s.is_empty()) {
        // Under a non-normal blend, the leader rows draw into a compositing
        // layer that carries the opacity cascade; the inner emit then runs at
        // full alpha (node_opacity 1.0 + a ctx with opacity 1.0, dx/dy/grid
        // preserved). With no blend this is the prior call, byte-identical.
        if let Some(blend_mode) = blend {
            commands.push(SceneCommand::PushLayer {
                opacity: layer_op,
                blend_mode: Some(blend_mode),
            });
            let mut inner_ctx = ctx;
            inner_ctx.opacity = 1.0;
            let h = compile_tab_leader(
                text,
                leader,
                &families,
                TabLeaderArgs {
                    font_size,
                    node_fill_prop,
                    node_weight_prop,
                    node_opacity: 1.0,
                    resolved,
                    env: ShapeEnv { engine, fonts },
                    text_x,
                    text_y,
                    ctx: inner_ctx,
                    glyph_stroke,
                },
                commands,
                diagnostics,
            );
            commands.push(SceneCommand::PopLayer);
            return h;
        }
        return compile_tab_leader(
            text,
            leader,
            &families,
            TabLeaderArgs {
                font_size,
                node_fill_prop,
                node_weight_prop,
                node_opacity,
                resolved,
                env: ShapeEnv { engine, fonts },
                text_x,
                text_y,
                ctx,
                glyph_stroke,
            },
            commands,
            diagnostics,
        );
    }

    // Shape EACH span as its own run, positioning runs left-to-right.
    // Per-span fill and font-weight are honored; family and size are
    // shared (v0 has no per-span family/size override). Cross-span
    // kerning is lost relative to a single concatenated run — accepted
    // for v0.
    //
    // Two-pass layout to support horizontal alignment:
    //   Pass 1 — shape every non-empty span; accumulate total_advance.
    //   Compute x_offset from the alignment and box width.
    //   Pass 2 — emit decoration FillRects + DrawGlyphRun commands at
    //             (text_x + x_offset) + per-span cursor.
    //
    // When align is absent or "start", x_offset == 0.0 and the emitted
    // commands are byte-for-byte identical to the previous single-pass.

    // Per-shaped-span record: (run, color, underline, strikethrough).
    // `text`/`weight`/`style` are retained so the wrap path can re-shape
    // individual words without re-running color/weight/style resolution.
    //
    // `font_size` is the span's OWN resolved size (reduced for super/subscript,
    // equal to the node size otherwise); `baseline_dy` is the super/subscript
    // baseline shift in pixels (negative = up). `vertical_align` flags a
    // super/subscript span so the emit path positions it against the SHARED
    // full-size baseline + `baseline_dy` instead of its own reduced ascent —
    // keeping plain spans byte-identical to before.
    struct ShapedSpan {
        run: ZenithGlyphRun,
        color: Color,
        underline: bool,
        strikethrough: bool,
        /// Per-span highlight background color (`None` = no highlight).
        highlight: Option<Color>,
        /// `true` when this span was authored with `code=#true`.
        code: bool,
        /// Retained hyperlink URL from `link="…"`. `None` = no link.
        link: Option<String>,
        text: String,
        weight: u16,
        style: FontStyle,
        font_size: f32,
        baseline_dy: f64,
        vertical_align: bool,
    }

    // Node base writing direction. `direction="rtl"` shapes RTL (correct Arabic/
    // Hebrew joining + visual glyph order) AND flips line layout below; any other
    // value (including absent) is LTR, byte-identical to before.
    let node_direction = match text.direction.as_deref() {
        Some("rtl") => TextDirection::Rtl,
        _ => TextDirection::Ltr,
    };

    // ── Pass 1: shape ────────────────────────────────────────────────
    let mut shaped_spans: Vec<ShapedSpan> = Vec::new();
    let mut total_advance: f64 = 0.0;
    // Shared FULL-size ascent, captured from the first full-size (non
    // super/subscript) run. Super/subscript spans position against this shared
    // baseline + their `baseline_dy` so their reduced ascent does not move them
    // off the run's baseline. `None` until a full-size span is shaped.
    let mut node_ascent: Option<f64> = None;
    // Accumulate chars with no glyph in any registered face across ALL spans of
    // this node. Emitted as a single diagnostic after the span loop.
    let mut node_missing: BTreeSet<char> = BTreeSet::new();

    for span in &effective_spans {
        if span.text.is_empty() {
            continue;
        }

        // Per-span fill precedence: a span-level `fill` wins; then a `link` span
        // uses the internal LINK_COLOR; then the inherited node fill; then black.
        // A link's conventional color thus overrides an inherited node fill but
        // not a fill the author set directly on the span. Non-link spans keep the
        // prior `span.fill else node.fill else black` resolution (byte-identical).
        let is_link = span.link.is_some();
        let raw_color = span
            .fill
            .as_ref()
            .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
            .or(is_link.then_some(LINK_COLOR))
            .or_else(|| {
                node_fill_prop
                    .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
            })
            .unwrap_or(Color::srgb(0, 0, 0, 255));
        let mut color = raw_color;
        color.a = (color.a as f64 * color_opacity).round() as u8;

        // Per-span highlight background color. Absent → `None` (no highlight,
        // byte-identical to a span without the attribute).
        let highlight: Option<Color> = span
            .highlight
            .as_ref()
            .and_then(|hp| resolve_property_color(hp, resolved, diagnostics, &text.id));

        // `code` and `link` flags for this span.
        let code = span.code == Some(true);
        let link = span.link.clone();

        // Per-span weight: span.font_weight overrides node weight; 400.
        let weight_prop = span.font_weight.as_ref().or(node_weight_prop);
        let weight = resolve_font_weight(weight_prop, resolved, 400);

        // Per-span italic selects the italic face; otherwise upright.
        let style = if span.italic == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        // Per-span super/subscript: a reduced size + a baseline shift relative
        // to the node's FULL font size. Absent → full size, zero shift.
        let (span_font_size, baseline_dy) =
            resolve_vertical_align(span.vertical_align.as_deref(), font_size);
        let is_vertical_align = baseline_dy != 0.0;

        // `code` spans use the bundled mono family instead of the node family.
        // Allocate the override slice only when needed; non-code spans are
        // byte-identical (they reuse the node-level `families` slice directly).
        let mono_families_buf = if code {
            vec![CODE_MONO_FAMILY.to_owned()]
        } else {
            vec![]
        };
        let span_families: &[String] = if code { &mono_families_buf } else { &families };

        let req = ShapeRequest {
            text: &span.text,
            families: span_families,
            weight,
            style,
            font_size: span_font_size,
            direction: node_direction,
        };

        // Shape with per-glyph font fallback: a span whose characters are all
        // covered by the primary face yields exactly one run (byte-identical to
        // the old single-run path); a mixed-script span (e.g. Latin + emoji +
        // CJK) yields one run per contiguous same-face sub-run, each emitted as
        // its own DrawGlyphRun by the downstream machinery. All sub-runs inherit
        // the span's color/decoration/weight/style.
        match engine.shape_with_fallback(&req, fonts) {
            Err(e) => {
                diagnostics.push(Diagnostic::advisory(
                    "scene.text_unshaped",
                    format!("text node '{}' could not be shaped: {}", text.id, e.message),
                    text.source_span,
                    Some(text.id.clone()),
                ));
                // Skip this span; cursor does not advance.
            }
            Ok(result) => {
                node_missing.extend(result.missing_chars);
                for (i, run) in result.runs.into_iter().enumerate() {
                    total_advance += run.advance_width as f64;
                    // Capture the first FULL-size run's ascent as the shared
                    // baseline reference for super/subscript spans.
                    if !is_vertical_align && node_ascent.is_none() {
                        node_ascent = Some(run.ascent as f64);
                    }
                    // The WRAP path re-tokenizes whole words and re-shapes them
                    // (with per-glyph fallback) from `text`. A word can straddle
                    // a sub-run boundary, so to keep word boundaries intact the
                    // FULL span text is carried on the FIRST sub-run and the rest
                    // carry an empty marker (no extra words). The fast path
                    // ignores `text` and positions runs by `advance_width`.
                    let run_text = if i == 0 {
                        span.text.clone()
                    } else {
                        String::new()
                    };
                    shaped_spans.push(ShapedSpan {
                        run,
                        color,
                        // `link` spans are underlined by default (in addition to
                        // any explicit underline). Explicit `underline=#true` is
                        // already honored; here we OR-in the link-driven underline.
                        underline: span.underline == Some(true) || is_link,
                        strikethrough: span.strikethrough == Some(true),
                        highlight,
                        code,
                        link: link.clone(),
                        text: run_text,
                        weight,
                        style,
                        font_size: span_font_size,
                        baseline_dy,
                        vertical_align: is_vertical_align,
                    });
                }
            }
        }
    }

    // Emit one warning per node listing every character that had no glyph in
    // any registered face (would silently render as .notdef / tofu).
    emit_glyph_missing(diagnostics, &text.id, text.source_span, &node_missing);

    // ── Alignment offset ─────────────────────────────────────────────
    // Resolve the node's box width to pixels (same dim_to_px path as x/y).
    // If w is absent or uses an unsupported unit, alignment is a no-op.
    let box_w_opt: Option<f64> = text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    // Resolve the node's box height for rotation center and fit-check.
    let box_h_opt: Option<f64> = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));

    // ── overflow="fit" pre-measurement ───────────────────────────────
    // Extract line_height from the first successfully shaped span (shared
    // across all spans because font + size are fixed). Used by the fit
    // check after both emit paths.
    let first_line_height: f64 = shaped_spans
        .first()
        .map(|s| s.run.line_height as f64)
        .unwrap_or(0.0);

    let align = text.align.as_deref().unwrap_or("start");
    let deco_thickness = (font_size as f64 / 14.0).max(1.0);

    // Decide single-line (fast path) vs. wrapping path. The fast path is
    // taken when there is no box width OR the whole-span layout already
    // fits within it. Shaping a span whole differs glyph-for-glyph from
    // shaping its words separately, so the fast path is preserved exactly
    // to keep every fitting example byte-identical.
    //
    // EXCEPTION: a `bullet`, `padding-left`, or `text-indent` lives ONLY on the
    // wrapping path (it draws the marker and applies the per-line hanging-indent
    // geometry there). A node carrying any of them must take the wrapping path
    // even when its text fits one line, else a single-line bullet/indented node
    // would render with no marker and no indent. None present → unchanged
    // (byte-identical fast path for every node without these attributes).
    let has_hanging = text.bullet.as_deref().is_some_and(|s| !s.is_empty())
        || text.padding_left.is_some()
        || text.text_indent.is_some();
    let needs_wrap = match box_w_opt {
        Some(box_w) => total_advance > box_w || has_hanging,
        None => false,
    };

    // Rotation bracket: only when both w and h are present (safe pivot).
    // Unrotated text (or text with no box) emits no PushTransform → byte-identical.
    let rot = rotation_degrees(text.rotate.as_ref());
    let text_rot = rot
        .zip(box_w_opt)
        .zip(box_h_opt)
        .map(|((a, bw), bh)| (a, text_x + bw / 2.0, text_y + bh / 2.0));
    if let Some((angle, cx, cy)) = text_rot {
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // BLEND-MODE layer bracket (inside rotation, outside shadow). The glyph
    // colors above were emitted at full alpha when blend is active; the layer
    // carries the opacity cascade. Absent for normal/no blend (byte-identical).
    if let Some(blend_mode) = blend {
        commands.push(SceneCommand::PushLayer {
            opacity: layer_op,
            blend_mode: Some(blend_mode),
        });
    }

    // BLUR / SHADOW / FILTER effect. Blur > shadow > filter; at most one is
    // chosen. The winning effect plus the optional mask bracket the node's glyph
    // draws via `emit_node_with_effects` below; the draws themselves are emitted
    // unchanged into `commands` (then split off into a local buffer at the end of
    // the draw region), so an unmasked, uneffected text node is byte-identical.
    // An empty node (no shaped spans) carries no effect (matching the prior guard).
    let blur_sigma = text
        .blur
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&s| s > 0.0);
    let effect: Option<NodeEffect> = if shaped_spans.is_empty() {
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

    // Resolve the optional node mask against the text box. Width/height fall back
    // to the laid-out extents when the box dimensions are absent.
    let mask = text.mask.as_ref().and_then(|p| {
        let mask_w = box_w_opt.unwrap_or(total_advance);
        let mask_h = box_h_opt.unwrap_or(first_line_height);
        resolve_property_mask(p, resolved, (text_x, text_y, mask_w, mask_h))
    });

    // Mark where the node's glyph draws begin in `commands`; they are split off
    // into a local buffer after the draw region and re-emitted through the
    // effect/mask helper.
    let draw_start = commands.len();

    // Tracks actual line count after emit; set by whichever path runs.
    // Used solely by the overflow="fit" check below.
    let mut fit_line_count: usize = 1;

    if !needs_wrap {
        // ── FAST PATH (fits / no box): single-line two-pass emit ──────
        // Alignment → x_offset. LTR is unchanged. Under RTL the anchor flips:
        // `start` right-anchors (line right edge at box right), `end`
        // left-anchors, `center` is symmetric (unchanged). Each span's run is
        // already in visual RTL order from the shaper, so reversing the SPAN
        // sequence (below) puts the first logical span rightmost.
        let is_rtl = node_direction == TextDirection::Rtl;
        let x_offset: f64 = match box_w_opt {
            None => 0.0, // no box width → always start-anchor (origin)
            Some(box_w) => {
                if is_rtl {
                    match align {
                        "center" => (box_w - total_advance) / 2.0,
                        // RTL `end` → left edge at box left (no offset).
                        "end" => 0.0,
                        // RTL `start`/`justify`/unknown → right-anchor.
                        _ => box_w - total_advance,
                    }
                } else {
                    match align {
                        "center" => (box_w - total_advance) / 2.0,
                        "end" => box_w - total_advance,
                        // "start"/"justify"/unknown → no offset. Justify on a
                        // single line that already fits is start-aligned.
                        _ => 0.0,
                    }
                }
            }
        };

        // ── Pass 2: emit ─────────────────────────────────────────────
        let mut x_cursor = text_x + x_offset;

        // RTL: emit spans in reverse logical order so the first logical span
        // sits rightmost (each run is internally visual-ordered already).
        if is_rtl {
            shaped_spans.reverse();
        }
        for shaped in shaped_spans {
            let run_advance = shaped.run.advance_width as f64;
            // A super/subscript span sits on the SHARED full-size baseline plus
            // its baseline shift; a plain span keeps its own run ascent (so
            // documents without vertical-align are byte-identical). When no
            // full-size span was shaped, fall back to the span's own ascent.
            let baseline_y = if shaped.vertical_align {
                text_y + node_ascent.unwrap_or(shaped.run.ascent as f64) + shaped.baseline_dy
            } else {
                text_y + shaped.run.ascent as f64
            };
            let glyphs = run_to_scene_glyphs(&shaped.run);

            // Per-span background rects: highlight (author-chosen color) and/or
            // code (internal CODE_BG). Emitted FIRST so glyphs and decorations
            // paint on top. Absent → no rect, byte-identical.
            if let Some(hl_color) = shaped.highlight {
                let hl_y = baseline_y - shaped.run.ascent as f64;
                let hl_h = (shaped.run.ascent + shaped.run.descent) as f64;
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: hl_y,
                    w: run_advance,
                    h: hl_h,
                    paint: Paint::solid(hl_color),
                });
            }
            if shaped.code {
                let bg_y = baseline_y - shaped.run.ascent as f64;
                let bg_h = (shaped.run.ascent + shaped.run.descent) as f64;
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: bg_y,
                    w: run_advance,
                    h: bg_h,
                    paint: Paint::solid(CODE_BG),
                });
            }

            // Per-span decorations: a thin filled rule in the span's own
            // color, spanning the run's advance. Position/thickness are
            // derived from the SPAN's font size (reduced for super/subscript) —
            // a deterministic v0 approximation.
            // Emitted before the glyphs so the text sits on top of any overlap.
            if shaped.underline {
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: baseline_y + shaped.font_size as f64 * 0.12,
                    w: run_advance,
                    h: deco_thickness,
                    paint: Paint::solid(shaped.color),
                });
            }
            if shaped.strikethrough {
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: baseline_y - shaped.font_size as f64 * 0.30,
                    w: run_advance,
                    h: deco_thickness,
                    paint: Paint::solid(shaped.color),
                });
            }

            commands.push(SceneCommand::DrawGlyphRun {
                x: x_cursor,
                y: baseline_y,
                font_id: shaped.run.font_id,
                font_size: shaped.run.font_size,
                color: shaped.color,
                stroke_color: glyph_stroke.0,
                stroke_width: glyph_stroke.1,
                glyphs,
            });

            // Advance the cursor past this run for the next span.
            x_cursor += run_advance;
        }
    } else if let Some(box_w) = box_w_opt {
        // ── WRAP PATH (overflow): greedy cross-span word packing ──────
        // Reuses the SHARED shaping/packing/emit helpers (also used by the
        // threaded-text chain distributor) so a wrapped node and a chain
        // member produce byte-identical command streams. Convert the resolved
        // `shaped_spans` to `ResolvedSpan` carriers, then shape → pack → emit.
        let resolved_spans: Vec<ResolvedSpan> = shaped_spans
            .iter()
            .map(|s| ResolvedSpan {
                text: s.text.clone(),
                color: s.color,
                underline: s.underline,
                strikethrough: s.strikethrough,
                highlight: s.highlight,
                code: s.code,
                link: s.link.clone(),
                weight: s.weight,
                style: s.style,
                font_size: s.font_size,
                baseline_dy: s.baseline_dy,
            })
            .collect();

        fit_line_count = emit_wrap_path(
            text,
            resolved_spans,
            &families,
            WrapEnv {
                env: ShapeEnv { engine, fonts },
                resolved,
                node_boxes,
                node_fill_prop,
                node_weight_prop,
                color_opacity,
                ctx,
            },
            WrapGeom {
                text_x,
                text_y,
                box_w,
                box_h_opt,
                font_size,
                align,
                deco_thickness,
                direction: node_direction,
                glyph_stroke,
            },
            commands,
            diagnostics,
        );
    }

    // ── overflow="fit" check ──────────────────────────────────────────
    // Hard-fail when the text content does not fit the declared box.
    // Only evaluated when BOTH box_w and box_h are present — without a
    // complete box we cannot determine fit and silently skip the check.
    // Glyph runs are STILL emitted above; this diagnostic rides alongside.
    if text.overflow.as_deref() == Some("fit")
        && let (Some(box_w), Some(box_h)) = (box_w_opt, box_h_opt)
    {
        const EPSILON: f64 = 0.5;
        let content_height = fit_line_count as f64 * first_line_height;

        // Height overflow: wrapped text is taller than the box.
        let height_overflow = content_height > box_h + EPSILON;

        // Word-wider-than-box: a single word in a single-word line
        // exceeds box_w (wrapping cannot help). In the wrap path, any
        // line with one word whose content_w > box_w is unwrappable.
        // In the single-line path (needs_wrap=false), total_advance ≤
        // box_w by definition, so no word can be wider.
        let word_overflow = if needs_wrap {
            // Re-check each token's advance against box_w. Any token
            // wider than box_w is unwrappable.  We use total_advance
            // as a fast proxy: if total_advance > box_w AND there is
            // exactly one shaped span whose run.advance_width > box_w
            // the single word is wider than the box. More precisely,
            // we need to check the per-word tokens; those were consumed
            // inside the wrap block, so we detect this via the fact
            // that any line with content_w > box_w must contain a lone
            // word wider than box_w (the greedy packer would have split
            // it if it could). The wrap path set fit_line_count from
            // lines.len(), so checking content_height already catches
            // the height dimension; the word-wider check is an
            // additional width dimension. A simpler heuristic: if
            // total_advance > box_w AND fit_line_count==1 the whole
            // text landed on one line only because no word break was
            // possible — meaning one word >= box_w width.
            fit_line_count == 1 && total_advance > box_w + EPSILON
        } else {
            false // fast path: total_advance ≤ box_w by definition
        };

        if height_overflow || word_overflow {
            diagnostics.push(Diagnostic::error(
                "text.fit_failed",
                format!(
                    "text '{}': content does not fit its box (overflow=\"fit\"): \
                         at {:.0}px font-size it needs ~{:.0}px height in a {:.0}px-tall box \
                         (or a word wider than the {:.0}px box width)",
                    text.id, font_size as f64, content_height, box_h, box_w
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
        }
    }

    // ── overflow="clip" warning ────────────────────────────────────────
    // Clip mode (the default when `overflow` is absent) silently truncates
    // ink at the box edge, which can hide content. Surface a non-fatal
    // warning so the author knows text was clipped — mirrors the fit check
    // but advisory, never a hard fail. `overflow="visible"` opts out (the
    // overflow is intentional) and `overflow="fit"` is handled above.
    if matches!(text.overflow.as_deref(), None | Some("clip"))
        && let (Some(box_w), Some(box_h)) = (box_w_opt, box_h_opt)
    {
        const EPSILON: f64 = 0.5;
        let content_height = fit_line_count as f64 * first_line_height;
        let height_overflow = content_height > box_h + EPSILON;
        let word_overflow = needs_wrap && fit_line_count == 1 && total_advance > box_w + EPSILON;
        if height_overflow || word_overflow {
            diagnostics.push(Diagnostic::warning(
                "text.overflow",
                format!(
                    "text '{}': content is clipped at the box edge \
                     (overflow=\"clip\"): at {:.0}px font-size it needs ~{:.0}px height in a {:.0}px-tall box",
                    text.id, font_size as f64, content_height, box_h
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
        }
    }

    // Split off the node's glyph draws (appended since `draw_start`) and
    // re-emit them through the effect/mask helper. No effect + no mask → the
    // draws are appended back verbatim in the same order (byte-identical).
    let draws = commands.split_off(draw_start);
    emit_node_with_effects(commands, draws, effect, mask);

    if blend.is_some() {
        commands.push(SceneCommand::PopLayer);
    }

    if text_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    // Laid-out content height: line count (1 on the fast path, the wrapped
    // line count otherwise) times the shared per-line height. Reuses exactly
    // the quantities the overflow="fit" check measures above, so flow-layout
    // advance and fit-detection agree by construction.
    fit_line_count as f64 * first_line_height
}
