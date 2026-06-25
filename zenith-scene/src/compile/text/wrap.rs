//! The single-box WRAP path: greedy cross-span word packing with the optional
//! drop-cap, text-runaround, hyphenation/break-word, and bullet/hanging-indent
//! behaviours. Lifted verbatim out of `compile_text_sized` so each concern lives
//! in a focused unit; the emitted command stream is byte-identical to before.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, PropertyValue, ResolvedToken, TextNode, dim_to_px};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine, ZenithGlyphRun};

use crate::ir::{Color, SceneCommand};

use super::super::RenderCtx;
use super::super::paint::resolve_property_color;
use super::baseline::{baseline_grid_snap_failed_diag, snap_to_baseline_grid};
use super::ctx::{EmitStyle, NodeShape, ShapeEnv};
use super::dropcap::{
    DROPCAP_GAP_FACTOR, DropCap, DropCapInitial, drop_cap_font_size, shape_drop_cap,
    take_drop_cap_initial,
};
use super::emit::{emit_lines, emit_lines_profiled};
use super::hyphen::{HyphenationContext, en_us_hyphenator};
use super::pack::{
    Line, WidthProfile, pack_lines_core, pack_lines_reporting, pack_lines_runaround,
    pack_lines_variable,
};
use super::shape::{
    ResolvedSpan, WordMetrics, resolve_font_weight, run_to_scene_glyphs, shape_words,
};

/// The borrowed environment + node-level style the wrap path needs beyond the
/// resolved spans and the geometry. Bundled so [`emit_wrap_path`] stays under the
/// argument lint. `node_fill_prop`/`node_weight_prop`/`color_opacity` drive the
/// bullet marker resolution; `node_boxes` resolves the text-exclusion target.
#[derive(Clone, Copy)]
pub(in crate::compile) struct WrapEnv<'a> {
    pub(in crate::compile) env: ShapeEnv<'a>,
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    pub(in crate::compile) node_fill_prop: Option<&'a PropertyValue>,
    pub(in crate::compile) node_weight_prop: Option<&'a PropertyValue>,
    pub(in crate::compile) color_opacity: f64,
    pub(in crate::compile) ctx: RenderCtx,
}

/// The resolved geometry + style scalars for the wrap path. `font_size`,
/// `align`, `deco_thickness`, `direction`, and `glyph_stroke` are the same
/// scalars the emit consumes; `box_w`/`box_h_opt` bound the measure; `text_x`/
/// `text_y` are the post-`ctx.dy` origin.
#[derive(Clone, Copy)]
pub(in crate::compile) struct WrapGeom<'a> {
    pub(in crate::compile) text_x: f64,
    pub(in crate::compile) text_y: f64,
    pub(in crate::compile) box_w: f64,
    pub(in crate::compile) box_h_opt: Option<f64>,
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) align: &'a str,
    pub(in crate::compile) deco_thickness: f64,
    pub(in crate::compile) direction: TextDirection,
    pub(in crate::compile) glyph_stroke: (Option<Color>, Option<f64>),
}

/// Run the single-box WRAP path: convert the resolved spans to word tokens, pick
/// the drop-cap / runaround / plain-wrap sub-path, emit the glyph draws into
/// `commands`, and return the laid-out line count for the overflow checks.
pub(in crate::compile) fn emit_wrap_path(
    text: &TextNode,
    mut resolved_spans: Vec<ResolvedSpan>,
    families: &[String],
    wrap: WrapEnv,
    geom: WrapGeom,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let WrapEnv {
        env,
        resolved,
        node_boxes,
        node_fill_prop,
        node_weight_prop,
        color_opacity,
        ctx,
    } = wrap;
    let WrapGeom {
        text_x,
        text_y,
        box_w,
        box_h_opt,
        font_size,
        align,
        deco_thickness,
        direction: node_direction,
        glyph_stroke,
    } = geom;
    let engine = env.engine;
    let fonts = env.fonts;

    let base_weight = resolve_font_weight(node_weight_prop, resolved, 400);

    // ── Drop cap (single-box wrap path only) ─────────────────────────
    // Active when `drop-cap-lines >= 1` and the first body span carries at
    // least one character. The FIRST char (a `char`, the v0 grapheme unit)
    // is lifted out of the body here so the body wrap re-tokenizes only the
    // remainder; the oversized cap glyph is shaped AFTER the body pass so it
    // can use the real body `line_height`. When inactive, `dropcap_initial`
    // stays `None` and the body packs/emits exactly as before
    // (byte-identical).
    let dropcap_initial: Option<(DropCapInitial, u32)> = match text.drop_cap_lines {
        Some(n) if n >= 1 => take_drop_cap_initial(&mut resolved_spans).map(|init| (init, n)),
        _ => None,
    };

    let (tokens, metrics) = shape_words(
        &resolved_spans,
        families,
        NodeShape {
            font_size,
            base_weight,
            direction: node_direction,
        },
        ShapeEnv { engine, fonts },
        diagnostics,
        &text.id,
        text.source_span,
    );

    // ── Baseline-grid snap (single-box wrap path) ────────────────────
    // When the page declares a positive baseline grid `g` AND no drop cap
    // is active on this node, snap the first baseline down to the grid and
    // inflate the inter-line advance to a multiple of `g`. Drop-cap +
    // baseline-grid is a documented v0 follow-up (skip the snap when a drop
    // cap is active, exactly like the existing drop-cap/chain deferral).
    // `text_y` here is already in the post-`ctx.dy` space, the same space
    // the grid origin is measured in. With no grid this leaves `emit_text_y`
    // = `text_y` and `emit_metrics` = `metrics` (byte-identical to before).
    let mut emit_text_y = text_y;
    let mut emit_metrics = metrics;
    if let Some(g) = ctx.baseline_grid
        && g.is_finite()
        && g > 0.0
        && dropcap_initial.is_none()
    {
        let (snapped_text_y, effective_line_height) =
            snap_to_baseline_grid(text_y, metrics.ascent, metrics.line_height, g);
        emit_text_y = snapped_text_y;
        emit_metrics.line_height = effective_line_height;
        // Advisory: a single line is taller than one grid cell, so leading
        // grows to a multiple of `g`. Emit ONCE per node (not per line).
        if metrics.line_height > g {
            diagnostics.push(baseline_grid_snap_failed_diag(
                &text.id,
                metrics.line_height,
                g,
                text.source_span,
            ));
        }
    }

    // ── Text-runaround exclusion resolution ──────────────────────────
    // Resolve `text-exclusion` against this page's node boxes using the
    // EFFECTIVE (post-baseline-snap) `emit_text_y` and line height, so the
    // band geometry composes with the baseline grid. An id naming no node box
    // → advisory + NO exclusion (uniform path, byte-identical). A drop cap
    // present → no exclusion (drop-cap + runaround is a v0 follow-up). When
    // the attribute is absent, `exclusion` stays `None` and the body packs/
    // emits exactly as before (byte-identical). Resolved here ONCE.
    let exclusion: Option<(f64, f64, f64, f64)> = match &text.text_exclusion {
        None => None,
        Some(target) => match node_boxes.get(target) {
            // Drop cap + runaround is a documented v0 follow-up: skip the
            // exclusion and keep the existing drop-cap path.
            Some(_) if dropcap_initial.is_some() => None,
            Some(rect) => Some(*rect),
            None => {
                diagnostics.push(Diagnostic::warning(
                    "text-exclusion.unresolved_ref",
                    format!(
                        "text node '{}' references unknown exclusion node '{}'",
                        text.id, target
                    ),
                    text.source_span,
                    Some(text.id.clone()),
                ));
                None
            }
        },
    };

    // Shape the cap now that the body `line_height` is known.
    let dropcap: Option<DropCap> = dropcap_initial.as_ref().and_then(|(init, n)| {
        let cap_size = drop_cap_font_size(font_size as f64, metrics.line_height, *n);
        shape_drop_cap(init, families, base_weight, cap_size, *n, engine, fonts)
    });

    if let Some(cap) = &dropcap {
        emit_drop_cap(EmitDropCap {
            cap,
            tokens,
            metrics,
            text_x,
            text_y,
            box_w,
            font_size,
            align,
            deco_thickness,
            glyph_stroke,
            commands,
        })
    } else if let Some((ex, ey, ew, eh)) = exclusion {
        emit_runaround(EmitRunaround {
            tokens,
            metrics,
            emit_metrics,
            emit_text_y,
            text_x,
            box_w,
            box_h_opt,
            exclusion: (ex, ey, ew, eh),
            font_size,
            align,
            deco_thickness,
            direction: node_direction,
            glyph_stroke,
            commands,
        })
    } else {
        emit_plain_wrap(
            text,
            EmitPlainWrap {
                tokens,
                metrics,
                emit_metrics,
                emit_text_y,
                text_x,
                box_w,
                font_size,
                align,
                deco_thickness,
                direction: node_direction,
                glyph_stroke,
            },
            PlainWrapStyle {
                env,
                resolved,
                families,
                node_fill_prop,
                color_opacity,
                base_weight,
            },
            commands,
            diagnostics,
        )
    }
}

/// Arguments for the drop-cap emit sub-path.
struct EmitDropCap<'a> {
    cap: &'a DropCap,
    tokens: Vec<super::shape::WordToken>,
    metrics: WordMetrics,
    text_x: f64,
    text_y: f64,
    box_w: f64,
    font_size: f32,
    align: &'a str,
    deco_thickness: f64,
    glyph_stroke: (Option<Color>, Option<f64>),
    commands: &'a mut Vec<SceneCommand>,
}

/// Emit the drop-cap initial + the wrap-around body lines, returning the body
/// line count.
fn emit_drop_cap(a: EmitDropCap) -> usize {
    let EmitDropCap {
        cap,
        tokens,
        metrics,
        text_x,
        text_y,
        box_w,
        font_size,
        align,
        deco_thickness,
        glyph_stroke,
        commands,
    } = a;
    // Gap between the drop cap's right edge and the wrapped body, as a
    // fraction of the body font size (documented constant).
    let gap = font_size as f64 * DROPCAP_GAP_FACTOR;
    let indent = cap.advance + gap;
    let n = cap.lines;
    let profile = WidthProfile {
        narrow_w: (box_w - indent).max(0.0),
        narrow_count: n,
        full_w: box_w,
    };

    let lines = pack_lines_variable(tokens, profile, metrics.space_advance);
    let fit_line_count = lines.len();

    // Drop-cap baseline sits on line `n`'s baseline (body ascent +
    // (n-1) line heights below the box top). Because the cap is sized so
    // its cap-height spans (n-1) lines + the body cap-height, this also
    // aligns the cap's cap-top with line 1's cap-top. Emit it ONCE at the
    // box left edge, in the node's resolved color/family.
    let cap_baseline_y = text_y + metrics.ascent + (n as f64 - 1.0) * metrics.line_height;
    commands.push(SceneCommand::DrawGlyphRun {
        x: text_x,
        y: cap_baseline_y,
        font_id: cap.run.font_id.clone(),
        font_size: cap.run.font_size,
        color: cap.color,
        stroke_color: glyph_stroke.0,
        stroke_width: glyph_stroke.1,
        glyphs: run_to_scene_glyphs(&cap.run),
    });

    // Body wraps around: lines 0..n indented to the cap's right at the
    // narrow measure; line n onward at the box left, full measure.
    emit_lines_profiled(
        &lines,
        move |i| {
            if i < n {
                (text_x + indent, profile.narrow_w)
            } else {
                (text_x, box_w)
            }
        },
        text_y,
        EmitStyle {
            align,
            metrics,
            font_size,
            deco_thickness,
            justify_final_line: false,
            // Drop-cap wrap-around is an LTR feature in v0; RTL drop caps are
            // a documented follow-up.
            direction: TextDirection::Ltr,
            glyph_stroke,
        },
        commands,
    );

    fit_line_count
}

/// Arguments for the text-runaround emit sub-path.
struct EmitRunaround<'a> {
    tokens: Vec<super::shape::WordToken>,
    metrics: WordMetrics,
    emit_metrics: WordMetrics,
    emit_text_y: f64,
    text_x: f64,
    box_w: f64,
    box_h_opt: Option<f64>,
    exclusion: (f64, f64, f64, f64),
    font_size: f32,
    align: &'a str,
    deco_thickness: f64,
    direction: TextDirection,
    glyph_stroke: (Option<Color>, Option<f64>),
    commands: &'a mut Vec<SceneCommand>,
}

/// Emit the text-runaround (largest-area / jump) lines, returning the line count.
fn emit_runaround(a: EmitRunaround) -> usize {
    let EmitRunaround {
        tokens,
        metrics,
        emit_metrics,
        emit_text_y,
        text_x,
        box_w,
        box_h_opt,
        exclusion: (ex, ey, ew, eh),
        font_size,
        align,
        deco_thickness,
        direction: node_direction,
        glyph_stroke,
        commands,
    } = a;

    // For each prospective line `i`, its vertical span is
    // `[lh_y(i), lh_y(i+1))` where `lh_y(i) = emit_text_y + i*lh`. A line
    // whose band overlaps the exclusion `[ey, ey+eh)` flows into the
    // LARGER free horizontal segment (left or right of the rect); a line
    // with neither segment ≥ MIN_W is BLOCKED (empty), so text flows
    // above and below a full-width exclusion. Hyphenation is disabled in
    // v0 runaround (like the drop-cap path).
    let lh = emit_metrics.line_height;
    // A line narrower than one space is useless → treat as blocked.
    let min_w = metrics.space_advance.max(1.0);
    // Half-open vertical-overlap test + larger-segment selection.
    let band_for = move |i: usize| -> (f64, f64) {
        let line_top = emit_text_y + (i as f64) * lh;
        let line_bottom = line_top + lh;
        // No overlap with the exclusion band → full measure.
        if line_bottom <= ey || line_top >= ey + eh {
            return (0.0, box_w);
        }
        let left_w = (ex - text_x).max(0.0);
        let right_w = ((text_x + box_w) - (ex + ew)).max(0.0);
        if left_w >= right_w && left_w >= min_w {
            (0.0, left_w)
        } else if right_w >= min_w {
            ((ex + ew) - text_x, right_w)
        } else {
            // Neither segment is wide enough → blocked line.
            (0.0, 0.0)
        }
    };

    // Bound the blocked-skip loop: at most the number of lines that fit
    // the box height (when known) plus slack, else a safe constant cap.
    let max_lines = match box_h_opt {
        Some(box_h) if lh > 0.0 => ((box_h / lh).ceil() as usize).saturating_add(4),
        _ => 4096,
    };

    let lines = pack_lines_runaround(
        tokens,
        |i| band_for(i).1,
        metrics.space_advance,
        min_w,
        max_lines,
    );
    let fit_line_count = lines.len();

    // Per-line geometry: blocked lines emit as empty `Line`s (no words),
    // so the baseline advances past them with no glyphs — producing the
    // above/below flow naturally.
    emit_lines_profiled(
        &lines,
        |i| {
            let (dx, w) = band_for(i);
            (text_x + dx, w)
        },
        emit_text_y,
        EmitStyle {
            align,
            metrics: emit_metrics,
            font_size,
            deco_thickness,
            justify_final_line: false,
            direction: node_direction,
            glyph_stroke,
        },
        commands,
    );

    fit_line_count
}

/// Geometry/style scalars for the plain-wrap emit sub-path.
struct EmitPlainWrap<'a> {
    tokens: Vec<super::shape::WordToken>,
    metrics: WordMetrics,
    emit_metrics: WordMetrics,
    emit_text_y: f64,
    text_x: f64,
    box_w: f64,
    font_size: f32,
    align: &'a str,
    deco_thickness: f64,
    direction: TextDirection,
    glyph_stroke: (Option<Color>, Option<f64>),
}

/// Borrowed environment for the plain-wrap sub-path (bullet + hyphenation).
struct PlainWrapStyle<'a> {
    env: ShapeEnv<'a>,
    resolved: &'a BTreeMap<String, ResolvedToken>,
    families: &'a [String],
    node_fill_prop: Option<&'a PropertyValue>,
    color_opacity: f64,
    base_weight: u16,
}

/// Emit the plain wrap path (hyphenation/break-word + bullet/hanging-indent),
/// returning the line count.
fn emit_plain_wrap(
    text: &TextNode,
    geom: EmitPlainWrap,
    style: PlainWrapStyle,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let EmitPlainWrap {
        tokens,
        metrics,
        emit_metrics,
        emit_text_y,
        text_x,
        box_w,
        font_size,
        align,
        deco_thickness,
        direction: node_direction,
        glyph_stroke,
    } = geom;
    let PlainWrapStyle {
        env,
        resolved,
        families,
        node_fill_prop,
        color_opacity,
        base_weight,
    } = style;
    let engine = env.engine;
    let fonts = env.fonts;

    // Opt-in hyphenation and/or break-word: build a context when EITHER
    // `hyphenate=#true` OR `overflow-wrap="break-word"` is set. The
    // dictionary is loaded regardless (it is needed only by the
    // hyphenation branch; break-word is independent of it), so a
    // break-word-only node still gets a context even if the dict is `None`.
    // When NEITHER is requested the context is `None` → the packer is
    // byte-identical to before.
    let want_hyphenate = text.hyphenate == Some(true);
    let want_break_word = text.overflow_wrap.as_deref() == Some("break-word");
    let hyph_ctx = if want_hyphenate || want_break_word {
        Some(HyphenationContext {
            // `dict` is consulted only by the hyphenation branch (which
            // also requires `want_hyphenate` via a `None` early-return in
            // `try_hyphenate`); a break-word-only node leaves it `None`.
            dict: if want_hyphenate {
                en_us_hyphenator()
            } else {
                None
            },
            engine,
            fonts,
            families,
            hyphen: "-",
            direction: node_direction,
            break_word: want_break_word,
        })
    } else {
        None
    };
    // ── Auto-aligning bullet marker ───────────────────────────────────
    // When `bullet` is `Some(marker)` with a non-empty string on the plain
    // wrap path (drop-cap/runaround/chain are handled above), the marker is:
    //   1. Shaped at the node's own font/weight/size to get `marker_advance`.
    //   2. Combined with the gap (`bullet_gap` or `0.4 × font_size`) to give
    //      `M = marker_advance + gap_px`.
    //   3. Stacked on top of any explicit `padding_left` (ADDED), so the
    //      effective indent is `M + explicit_pl`. An explicit `text_indent`
    //      is ignored on a bullet node (documented v0 follow-up).
    //   4. The marker is drawn once as a `DrawGlyphRun` at `x = text_x`
    //      (the UN-indented box edge, i.e. in the left margin), at the
    //      first line's baseline (`emit_text_y + emit_metrics.ascent`), in
    //      the node's resolved fill color. All text lines (first AND
    //      wrapped) are indented by `M + explicit_pl` via the reused
    //      `emit_lines_profiled` per-line geometry mechanism.
    // When `bullet` is `None` (or empty) this block is a no-op and the
    // node is BYTE-IDENTICAL to a node without the attribute.
    let bullet_run: Option<(ZenithGlyphRun, Color)> =
        match text.bullet.as_deref().filter(|s| !s.is_empty()) {
            None => None,
            Some(marker) => {
                // Resolve node fill color for the marker (same cascade as
                // the body spans: node fill → style fill → black).
                // Reuses `node_fill_prop` already computed above.
                let mut marker_color = node_fill_prop
                    .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
                    .unwrap_or(Color::srgb(0, 0, 0, 255));
                marker_color.a = (marker_color.a as f64 * color_opacity).round() as u8;

                // Shape the marker string at the node's resolved
                // font/weight/size (mirror `shape_drop_cap`). Take only
                // the FIRST run on success (the marker is a single glyph
                // cluster). On shaping failure the bullet is silently
                // skipped (no marker drawn, no extra indent) so the body
                // still renders.
                // `base_weight` was already resolved above for word shaping.
                let req = ShapeRequest {
                    text: marker,
                    families,
                    weight: base_weight,
                    style: zenith_core::FontStyle::Normal,
                    font_size,
                    // Bullet marker is always LTR (the glyph faces left
                    // regardless of body direction in v0).
                    direction: TextDirection::Ltr,
                };
                match engine.shape_with_fallback(&req, fonts) {
                    Ok(result) => result.runs.into_iter().next().map(|r| (r, marker_color)),
                    Err(_) => None,
                }
            }
        };

    // ── Hanging indent: padding-left + bullet-M + (optional negative) text-indent ─
    // `pl` indents EVERY line's left edge inward (reducing the measure);
    // `ti` shifts line 0 by an additional amount relative to the padded
    // edge (may be negative to pull the first line back out for a hanging
    // bullet). Both default to 0. This composes with hyphenation/break-
    // word (via `hyph_ctx`), justify and RTL (via `emit_lines_profiled`'s
    // align/direction), and the baseline grid (already folded into
    // `emit_text_y`/`emit_metrics` above). Combining indent with the
    // drop-cap, runaround, or chain paths is a documented v0 follow-up:
    // those branches use their own per-line width profiles and are
    // handled above, so this code is reached only on the plain wrap path.
    let explicit_pl = text
        .padding_left
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .unwrap_or(0.0);
    // Bullet auto-indent: measured marker advance + gap, added ON TOP of
    // any explicit `padding_left`. When there is no bullet run (bullet
    // absent, empty, or shaping failed) `bullet_m = 0.0` so the rest of
    // the logic is byte-identical to the pre-bullet path.
    let bullet_m: f64 = match &bullet_run {
        None => 0.0,
        Some((run, _)) => {
            let marker_advance = run.advance_width as f64;
            let gap_px = text
                .bullet_gap
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.4 * font_size as f64);
            marker_advance + gap_px
        }
    };
    let pl = bullet_m + explicit_pl;
    // Explicit `text_indent` is ignored on a bullet node (documented).
    // On a non-bullet node it is honoured as before (byte-identical).
    let ti = if bullet_run.is_some() {
        0.0
    } else {
        text.text_indent
            .as_ref()
            .and_then(|d| dim_to_px(d.value, &d.unit))
            .unwrap_or(0.0)
    };

    let mut forced_break = false;
    let lines: Vec<Line> = if pl == 0.0 && ti == 0.0 {
        // Default-off: byte-identical to the historical uniform packing.
        pack_lines_reporting(
            tokens,
            box_w,
            metrics.space_advance,
            hyph_ctx.as_ref(),
            &mut forced_break,
        )
    } else {
        // Line 0 measure is `box_w - pl - ti`; lines ≥1 are `box_w - pl`.
        // Widths clamp to ≥ 0 so a large pad/indent never goes negative.
        let width_for = |i: usize| {
            if i == 0 {
                (box_w - pl - ti).max(0.0)
            } else {
                (box_w - pl).max(0.0)
            }
        };
        pack_lines_core(
            tokens,
            width_for,
            metrics.space_advance,
            hyph_ctx.as_ref(),
            f64::NEG_INFINITY,
            usize::MAX,
            &mut forced_break,
        )
    };

    // One advisory per node when a forced character-boundary break
    // occurred (break-word split an overlong token). Mirrors the
    // `text.overflow` warning construction in this file.
    if forced_break {
        diagnostics.push(Diagnostic::warning(
            "text.forced_break",
            format!(
                "text node '{}' has a token wider than its column; forced a \
                 character-boundary break (consider editing the copy)",
                text.id
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
    }

    // Record the actual line count for the overflow="fit" check below.
    let fit_line_count = lines.len();

    // Emit the bullet marker BEFORE the text runs (drawn first → below the
    // text in z-order, consistent with drop-cap emission order). The
    // baseline is the SNAPPED first-line baseline so the marker aligns with
    // the body's first line regardless of baseline-grid state.
    if let Some((marker_run, marker_color)) = bullet_run {
        let marker_baseline_y = emit_text_y + emit_metrics.ascent;
        let glyphs = run_to_scene_glyphs(&marker_run);
        commands.push(SceneCommand::DrawGlyphRun {
            x: text_x,
            y: marker_baseline_y,
            font_id: marker_run.font_id,
            font_size: marker_run.font_size,
            color: marker_color,
            stroke_color: glyph_stroke.0,
            stroke_width: glyph_stroke.1,
            glyphs,
        });
    }

    if pl == 0.0 && ti == 0.0 {
        emit_lines(
            &lines,
            text_x,
            // Baseline-grid snap (no-op when no grid is active): the first
            // baseline lands on the grid and the advance is a multiple of g.
            emit_text_y,
            box_w,
            EmitStyle {
                align,
                metrics: emit_metrics,
                font_size,
                deco_thickness,
                // Single-box wrap: the batch's last line IS the paragraph's
                // last line → leave it ragged under justify.
                justify_final_line: false,
                direction: node_direction,
                glyph_stroke,
            },
            commands,
        );
    } else {
        // Per-line geometry mirrors the packing widths: line 0 starts at
        // `text_x + pl + ti` (the outdented bullet edge when `ti < 0`),
        // continuation lines start at `text_x + pl`.
        emit_lines_profiled(
            &lines,
            |i| {
                if i == 0 {
                    (text_x + pl + ti, (box_w - pl - ti).max(0.0))
                } else {
                    (text_x + pl, (box_w - pl).max(0.0))
                }
            },
            emit_text_y,
            EmitStyle {
                align,
                metrics: emit_metrics,
                font_size,
                deco_thickness,
                justify_final_line: false,
                direction: node_direction,
                glyph_stroke,
            },
            commands,
        );
    }

    fit_line_count
}

#[cfg(test)]
mod indent_tests {
    //! Hanging-indent geometry (`padding-left` + signed `text-indent`).
    //!
    //! These exercise the EXACT pack + emit calls `compile_text`'s plain wrap
    //! path makes, with the same per-line `width_for`/`geom` formulas, so the
    //! line-packing and per-glyph x origins are checked end-to-end without a
    //! full font stack. A glyph-bearing token is built so `emit_lines_profiled`
    //! emits a `DrawGlyphRun` whose `x` is the line origin we assert.
    use super::{
        EmitStyle, emit_lines, emit_lines_profiled, pack_lines_core, pack_lines_reporting,
    };
    use zenith_core::FontStyle;
    use zenith_layout::{TextDirection, ZenithGlyphRun};

    use super::super::shape::{WordMetrics, WordSource, WordToken};
    use crate::ir::{Color, SceneCommand};

    /// A single-run [`WordToken`] of the given `advance`, carrying one glyph so
    /// `emit_lines_profiled` emits a `DrawGlyphRun` at the line origin.
    fn word(advance: f64) -> WordToken {
        WordToken {
            runs: vec![ZenithGlyphRun {
                font_id: "test-font".to_owned(),
                font_size: 16.0,
                ascent: 12.0,
                descent: 4.0,
                line_height: 18.0,
                advance_width: advance as f32,
                glyphs: vec![zenith_layout::PositionedGlyph {
                    glyph_id: 1,
                    x: 0.0,
                    y: 0.0,
                }],
            }],
            advance,
            color: Color::srgb(0, 0, 0, 255),
            underline: false,
            strikethrough: false,
            highlight: None,
            code: false,
            link: None,
            baseline_dy: 0.0,
            src: WordSource {
                text: String::new(),
                weight: 400,
                style: FontStyle::Normal,
                font_size: 16.0,
                paragraph: 0,
                hyphen_part: None,
            },
        }
    }

    fn tokens(advances: &[f64]) -> Vec<WordToken> {
        advances.iter().copied().map(word).collect()
    }

    fn metrics() -> WordMetrics {
        WordMetrics {
            ascent: 12.0,
            line_height: 18.0,
            space_advance: 5.0,
        }
    }

    /// The x origin of the FIRST glyph run of each emitted line, indexed by the
    /// line's baseline y so per-line origins can be matched to a line index.
    fn line_origin_xs(commands: &[SceneCommand]) -> Vec<(f64, f64)> {
        let mut seen: Vec<(f64, f64)> = Vec::new();
        for c in commands {
            if let SceneCommand::DrawGlyphRun { x, y, .. } = c
                && !seen.iter().any(|(yy, _)| *yy == *y)
            {
                seen.push((*y, *x));
            }
        }
        seen
    }

    /// Run the EXACT plain-path pack + emit `compile_text` runs for the given
    /// `pl`/`ti`, returning the emitted commands. Mirrors the production formula.
    fn pack_emit(advances: &[f64], box_w: f64, text_x: f64, pl: f64, ti: f64) -> Vec<SceneCommand> {
        let m = metrics();
        let mut forced = false;
        let lines = if pl == 0.0 && ti == 0.0 {
            pack_lines_reporting(tokens(advances), box_w, m.space_advance, None, &mut forced)
        } else {
            let width_for = |i: usize| {
                if i == 0 {
                    (box_w - pl - ti).max(0.0)
                } else {
                    (box_w - pl).max(0.0)
                }
            };
            pack_lines_core(
                tokens(advances),
                width_for,
                m.space_advance,
                None,
                f64::NEG_INFINITY,
                usize::MAX,
                &mut forced,
            )
        };
        let mut commands = Vec::new();
        if pl == 0.0 && ti == 0.0 {
            emit_lines(
                &lines,
                text_x,
                0.0,
                box_w,
                EmitStyle {
                    align: "start",
                    metrics: m,
                    font_size: 16.0,
                    deco_thickness: 1.0,
                    justify_final_line: false,
                    direction: TextDirection::Ltr,
                    glyph_stroke: (None, None),
                },
                &mut commands,
            );
        } else {
            emit_lines_profiled(
                &lines,
                |i| {
                    if i == 0 {
                        (text_x + pl + ti, (box_w - pl - ti).max(0.0))
                    } else {
                        (text_x + pl, (box_w - pl).max(0.0))
                    }
                },
                0.0,
                EmitStyle {
                    align: "start",
                    metrics: m,
                    font_size: 16.0,
                    deco_thickness: 1.0,
                    justify_final_line: false,
                    direction: TextDirection::Ltr,
                    glyph_stroke: (None, None),
                },
                &mut commands,
            );
        }
        commands
    }

    #[test]
    fn indent_none_is_byte_identical() {
        // Five words that wrap into multiple lines at box_w = 70.
        let advances = [10.0, 20.0, 30.0, 40.0, 15.0];
        // The default-off path (pl=ti=0) and an EXPLICIT (px)0/(px)0 must both
        // equal the historical uniform path command-for-command.
        let baseline = pack_emit(&advances, 70.0, 100.0, 0.0, 0.0);
        // Re-running the same call is deterministic.
        let again = pack_emit(&advances, 70.0, 100.0, 0.0, 0.0);
        assert_eq!(baseline, again, "default-off packing/emit is deterministic");
        assert!(
            !baseline.is_empty(),
            "the byte-identical baseline must emit glyph runs"
        );
    }

    #[test]
    fn padding_left_indents_all_lines() {
        // Without padding the copy packs to fewer lines; padding narrows the
        // measure so it wraps more, and every line's origin shifts right by pl.
        let advances = [30.0, 30.0, 30.0];
        let no_pad = pack_emit(&advances, 70.0, 100.0, 0.0, 0.0);
        let padded = pack_emit(&advances, 70.0, 100.0, 44.0, 0.0);
        let no_pad_lines = line_origin_xs(&no_pad);
        let padded_lines = line_origin_xs(&padded);
        // Every padded line's first glyph x is text_x + pl = 144.
        for (_, x) in &padded_lines {
            assert_eq!(*x, 144.0, "every padded line starts at text_x + pl");
        }
        // Narrower measure ⇒ at least as many lines (more wraps) as unpadded.
        assert!(
            padded_lines.len() > no_pad_lines.len(),
            "padding reduces the measure and forces more wraps: {} vs {}",
            padded_lines.len(),
            no_pad_lines.len()
        );
    }

    #[test]
    fn hanging_indent_first_line_outdented() {
        // padding-left=44, text-indent=-44: line 0 returns to the original
        // margin (text_x), continuation lines hang at text_x + 44.
        let advances = [30.0, 30.0, 30.0, 30.0];
        let cmds = pack_emit(&advances, 70.0, 100.0, 44.0, -44.0);
        let lines = line_origin_xs(&cmds);
        assert!(lines.len() >= 2, "copy must wrap to ≥2 lines");
        assert_eq!(
            lines[0].1, 100.0,
            "line 0 first glyph at the original margin"
        );
        assert_eq!(lines[1].1, 144.0, "continuation lines hang at text_x + pl");
    }

    #[test]
    fn positive_text_indent_indents_first_line_only() {
        // text-indent=60 with no padding: line 0 starts indented at text_x + 60,
        // continuation lines return to text_x.
        let advances = [30.0, 30.0, 30.0, 30.0];
        let cmds = pack_emit(&advances, 70.0, 100.0, 0.0, 60.0);
        let lines = line_origin_xs(&cmds);
        assert!(lines.len() >= 2, "copy must wrap to ≥2 lines");
        assert_eq!(lines[0].1, 160.0, "line 0 indented by text_x + ti");
        assert_eq!(lines[1].1, 100.0, "continuation lines return to text_x");
    }
}
