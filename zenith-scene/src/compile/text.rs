//! Text and code leaf-node compilation, plus the shaping/glyph and
//! syntax-highlight helpers they depend on.

use std::collections::BTreeMap;

use zenith_core::{
    CodeNode, Diagnostic, FontProvider, FontStyle, PropertyValue, ResolvedToken, ResolvedValue,
    Style, TextNode, TokenKind, builtin_color, dim_to_px, is_supported, scan, token_id_for_kind,
};
use zenith_layout::{RustybuzzEngine, ShapeRequest, TextLayoutEngine, ZenithGlyphRun};

use crate::color::parse_srgb_hex;
use crate::ir::{Color, SceneCommand, SceneGlyph};

use super::RenderCtx;
use super::chain::ChainAssignments;
use super::paint::{resolve_property_color, resolve_property_shadow};
use super::style_prop;
use super::util::{resolve_property_dimension_px, rotation_degrees, unsupported_unit_diag};

// ── Shared word-wrap structures + helpers ─────────────────────────────────────
//
// These are factored out of `compile_text`'s WRAP path so the threaded-text
// chain pre-pass ([`super::chain`]) can reuse the EXACT same shaping, greedy
// line-packing, and glyph/decoration emission. A single-box text node and a
// chain member therefore share one code path for byte-stability.

/// A re-shaped word plus the visual attributes inherited from its source span.
///
/// A word may shape to multiple font-runs (per-glyph fallback), so `runs` is a
/// Vec laid out left-to-right; `advance` is their summed width.
pub(super) struct WordToken {
    pub(super) runs: Vec<ZenithGlyphRun>,
    pub(super) advance: f64,
    pub(super) color: Color,
    pub(super) underline: bool,
    pub(super) strikethrough: bool,
}

/// One packed line: its words plus the summed content width (no trailing space).
pub(super) struct Line {
    pub(super) words: Vec<WordToken>,
    pub(super) content_w: f64,
}

/// Shared font metrics captured from the first successfully shaped word.
#[derive(Clone, Copy, Default)]
pub(super) struct WordMetrics {
    pub(super) ascent: f64,
    pub(super) line_height: f64,
    pub(super) space_advance: f64,
}

/// A span already resolved to color/decoration/weight/style, ready for the
/// per-word re-shaping the wrap + chain paths perform. Mirrors the private
/// `ShapedSpan` fields the wrap path consumes.
pub(super) struct ResolvedSpan {
    pub(super) text: String,
    pub(super) color: Color,
    pub(super) underline: bool,
    pub(super) strikethrough: bool,
    pub(super) weight: u16,
    pub(super) style: FontStyle,
}

/// Tokenise resolved spans into per-word [`WordToken`]s (one re-shape per word,
/// with per-glyph fallback) and capture the shared font metrics.
///
/// This is the SINGLE shaping routine used by both the single-box wrap path and
/// the chain distributor, so a chain member and a standalone wrapped node shape
/// identical word geometry. `node_id` is used only for diagnostics wording.
#[allow(clippy::too_many_arguments)]
pub(super) fn shape_words(
    spans: &[ResolvedSpan],
    families: &[String],
    font_size: f32,
    node_base_weight: u16,
    engine: &RustybuzzEngine,
    fonts: &dyn FontProvider,
    diagnostics: &mut Vec<Diagnostic>,
    node_id: &str,
    span: Option<zenith_core::Span>,
) -> (Vec<WordToken>, WordMetrics) {
    let mut tokens: Vec<WordToken> = Vec::new();
    let mut metrics = WordMetrics::default();
    let mut have_metrics = false;

    for shaped in spans {
        for word in shaped.text.split_whitespace() {
            let req = ShapeRequest {
                text: word,
                families,
                weight: shaped.weight,
                style: shaped.style,
                font_size,
            };
            match engine.shape_with_fallback(&req, fonts) {
                Err(e) => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.text_unshaped",
                        format!("text node '{}' could not be shaped: {}", node_id, e.message),
                        span,
                        Some(node_id.to_owned()),
                    ));
                }
                Ok(runs) => {
                    if !have_metrics && let Some(first) = runs.first() {
                        metrics.ascent = first.ascent as f64;
                        metrics.line_height = first.line_height as f64;
                        have_metrics = true;
                    }
                    let advance: f64 = runs.iter().map(|r| r.advance_width as f64).sum();
                    tokens.push(WordToken {
                        advance,
                        color: shaped.color,
                        underline: shaped.underline,
                        strikethrough: shaped.strikethrough,
                        runs,
                    });
                }
            }
        }
    }

    // Shape a single space once (node base weight/style) for inter-word gaps
    // and packing measurement.
    metrics.space_advance = {
        let req = ShapeRequest {
            text: " ",
            families,
            weight: node_base_weight,
            style: FontStyle::Normal,
            font_size,
        };
        match engine.shape(&req, fonts) {
            Ok(run) => run.advance_width as f64,
            Err(_) => 0.0,
        }
    };

    (tokens, metrics)
}

/// Greedy-pack word tokens into lines for a given box width, left-to-right and
/// deterministic. Identical algorithm to the original inline wrap packer.
pub(super) fn pack_lines(tokens: Vec<WordToken>, box_w: f64, space_advance: f64) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut cur: Vec<WordToken> = Vec::new();
    let mut line_w: f64 = 0.0;
    for tok in tokens {
        if !cur.is_empty() && line_w + space_advance + tok.advance > box_w {
            let content_w = line_w;
            lines.push(Line {
                words: std::mem::take(&mut cur),
                content_w,
            });
            line_w = 0.0;
        }
        let gap = if cur.is_empty() { 0.0 } else { space_advance };
        line_w += gap + tok.advance;
        cur.push(tok);
    }
    if !cur.is_empty() {
        lines.push(Line {
            words: cur,
            content_w: line_w,
        });
    }
    lines
}

/// Emit decoration FillRects + DrawGlyphRun commands for a sequence of packed
/// lines, stacked by `line_height`, with per-line horizontal `align`.
///
/// This is the EXACT emit body lifted out of `compile_text`'s wrap path so the
/// single-box and chain renderers produce byte-identical command streams.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_lines(
    lines: &[Line],
    text_x: f64,
    text_y: f64,
    box_w: f64,
    align: &str,
    metrics: WordMetrics,
    font_size: f32,
    deco_thickness: f64,
    commands: &mut Vec<SceneCommand>,
) {
    let ascent = metrics.ascent;
    let line_height = metrics.line_height;
    let space_advance = metrics.space_advance;
    let last_idx = lines.len().saturating_sub(1);
    for (i, line) in lines.iter().enumerate() {
        let baseline_y = text_y + ascent + (i as f64) * line_height;
        let word_count = line.words.len();

        let (base_x, gap) = match align {
            "center" => (text_x + (box_w - line.content_w) / 2.0, space_advance),
            "end" => (text_x + (box_w - line.content_w), space_advance),
            "justify" => {
                if i != last_idx && word_count > 1 {
                    let extra = (box_w - line.content_w) / (word_count as f64 - 1.0);
                    (text_x, space_advance + extra)
                } else {
                    (text_x, space_advance)
                }
            }
            _ => (text_x, space_advance),
        };

        // Precompute each word's left x along the line.
        let mut word_x: Vec<f64> = Vec::with_capacity(word_count);
        {
            let mut x = base_x;
            for (wi, word) in line.words.iter().enumerate() {
                word_x.push(x);
                x += word.advance;
                if wi + 1 < word_count {
                    x += gap;
                }
            }
        }

        // Decorations FIRST (so glyphs paint on top), one FillRect per maximal
        // contiguous same-flag run of words.
        let underline_y = baseline_y + font_size as f64 * 0.12;
        let strike_y = baseline_y - font_size as f64 * 0.30;
        for (is_underline, deco_y) in [(true, underline_y), (false, strike_y)] {
            let mut run_start: Option<(f64, Color)> = None;
            let mut run_right: f64 = base_x;
            for (wi, word) in line.words.iter().enumerate() {
                let on = if is_underline {
                    word.underline
                } else {
                    word.strikethrough
                };
                let wx = word_x.get(wi).copied().unwrap_or(base_x);
                if on {
                    if run_start.is_none() {
                        run_start = Some((wx, word.color));
                    }
                    run_right = wx + word.advance;
                } else if let Some((sx, color)) = run_start.take() {
                    commands.push(SceneCommand::FillRect {
                        x: sx,
                        y: deco_y,
                        w: run_right - sx,
                        h: deco_thickness,
                        color,
                    });
                }
            }
            if let Some((sx, color)) = run_start.take() {
                commands.push(SceneCommand::FillRect {
                    x: sx,
                    y: deco_y,
                    w: run_right - sx,
                    h: deco_thickness,
                    color,
                });
            }
        }

        // Glyphs.
        for (wi, word) in line.words.iter().enumerate() {
            let mut run_x = word_x.get(wi).copied().unwrap_or(base_x);
            for run in &word.runs {
                commands.push(SceneCommand::DrawGlyphRun {
                    x: run_x,
                    y: baseline_y,
                    font_id: run.font_id.clone(),
                    font_size: run.font_size,
                    color: word.color,
                    glyphs: run_to_scene_glyphs(run),
                });
                run_x += run.advance_width as f64;
            }
        }
    }
}

/// Resolve a text node's font size in pixels with style cascade (default 16.0).
/// Shared by the chain-member render path and mirrors `compile_text`'s inline
/// resolution.
fn font_size_px(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
) -> f32 {
    let font_size_prop = text
        .font_size
        .clone()
        .or_else(|| style_prop(&text.style, style_map, "font-size").cloned());
    resolve_property_dimension_px(&font_size_prop, resolved, 16.0) as f32
}

/// Render a chain member's PRE-ASSIGNED lines into its own box.
///
/// The lines were shaped + packed by the chain pre-pass using the chain
/// source's shared style; this function only positions them in THIS box using
/// the box's own geometry/align, with the same rotation + shadow brackets and
/// the SHARED [`emit_lines`] code the single-box wrap path uses. Returns the
/// laid-out content height (line count × line height) for flow-advance parity.
#[allow(clippy::too_many_arguments)]
fn render_chain_member(
    text: &TextNode,
    assignment: &super::chain::ChainAssignment,
    font_size: f32,
    text_x: f64,
    text_y: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    // Box width is required to position lines; height/align are optional.
    let box_w = match text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)) {
        Some(w) => w,
        None => return 0.0,
    };
    let box_h_opt: Option<f64> = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let align = text.align.as_deref().unwrap_or("start");
    let deco_thickness = (font_size as f64 / 14.0).max(1.0);

    // overflow="fit": this member's assigned content must fit its own box. For
    // a continuation/last member this catches an article that overruns even the
    // final panel. Mirrors the single-box height-overflow check.
    if text.overflow.as_deref() == Some("fit")
        && let Some(box_h) = box_h_opt
    {
        const EPSILON: f64 = 0.5;
        let content_height = assignment.lines.len() as f64 * assignment.metrics.line_height;
        if content_height > box_h + EPSILON {
            diagnostics.push(Diagnostic::error(
                "text.fit_failed",
                format!(
                    "text '{}': chain content does not fit its box (overflow=\"fit\"): \
                     needs ~{:.0}px height in {:.0}px box",
                    text.id, content_height, box_h
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

    let has_shadow = if assignment.lines.is_empty() {
        false
    } else {
        match text
            .shadow
            .as_ref()
            .and_then(|p| resolve_property_shadow(p, resolved, &text.id))
        {
            Some(shadows) => {
                commands.push(SceneCommand::BeginShadow { shadows });
                true
            }
            None => false,
        }
    };

    emit_lines(
        &assignment.lines,
        text_x,
        text_y,
        box_w,
        align,
        assignment.metrics,
        font_size,
        deco_thickness,
        commands,
    );

    if has_shadow {
        commands.push(SceneCommand::EndShadow);
    }

    if text_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    assignment.lines.len() as f64 * assignment.metrics.line_height
}

// ── Glyph conversion helper ───────────────────────────────────────────────────

/// Map a [`ZenithGlyphRun`]'s positioned glyphs into [`SceneGlyph`] records.
///
/// Used by every shaped-run emit site (Text, highlighted Code, plain Code) so
/// that the field mapping is defined in exactly one place.
fn run_to_scene_glyphs(run: &ZenithGlyphRun) -> Vec<SceneGlyph> {
    run.glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect()
}

/// Resolve an optional font-weight property to a numeric weight (100–900).
///
/// Returns `default` when the property is absent, references a non-fontWeight
/// (or unresolved) token, or carries a dimension. The idiomatic path is a token
/// ref resolving to a `FontWeight`. A bare numeric literal (e.g. `font-weight=700`)
/// is parsed directly; an unparsable literal falls back to `default`. Mirrors
/// `resolve_property_dimension_px`.
pub(super) fn resolve_font_weight(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: u16,
) -> u16 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontWeight(w) => *w as u16,
                _ => default,
            },
            None => default,
        },
        Some(PropertyValue::Literal(s)) => s.parse::<u16>().unwrap_or(default),
        // A dimension is not a weight → fall back to the default.
        Some(PropertyValue::Dimension(_)) => default,
        None => default,
    }
}

/// Resolve a requested font family against the provider, falling back to the
/// bundled default when the requested family is unregistered.
///
/// Returns `(family_to_use, fell_back)`: if the requested family resolves it is
/// returned unchanged with `false`; otherwise `default_family` is returned with
/// `true` so the caller can emit a `font.unresolved` advisory (worded for its
/// own node kind) and shaping proceeds with the bundled face instead of
/// silently dropping text. The probe weight/style match the shaping request.
pub(super) fn resolve_family_with_fallback(
    fonts: &dyn FontProvider,
    requested: &str,
    default_family: &str,
    weight: u16,
    style: FontStyle,
) -> (String, bool) {
    // Fast path: requested == default → always available, no check needed.
    if requested.eq_ignore_ascii_case(default_family) {
        return (requested.to_owned(), false);
    }
    if fonts
        .resolve(&[requested.to_owned()], weight, style)
        .is_some()
    {
        (requested.to_owned(), false)
    } else {
        (default_family.to_owned(), true)
    }
}

/// Compile a `text` leaf node.
///
/// Returns the laid-out content height in pixels (`line_count * line_height`),
/// which the flow-layout path in [`super::container`] uses to advance its
/// vertical cursor past a text child that declares no explicit `h`. Early
/// returns (invisible, missing/bad geometry, empty spans) yield `0.0`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_text(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    ctx: RenderCtx,
) -> f64 {
    // Skip invisible text nodes.
    if text.visible == Some(false) {
        return 0.0;
    }

    // Resolve geometry — x and y are required; skip if absent or bad unit.
    let (Some(x_dim), Some(y_dim)) = (&text.x, &text.y) else {
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

    let Some(text_x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "text node",
            &text.id,
            "x",
            text.source_span,
        ));
        return 0.0;
    };
    let Some(text_y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "text node",
            &text.id,
            "y",
            text.source_span,
        ));
        return 0.0;
    };

    // Apply group translation offset.
    let text_x = text_x_raw + ctx.dx;
    let text_y = text_y_raw + ctx.dy;

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
            fs,
            text_x,
            text_y,
            resolved,
            commands,
            diagnostics,
        );
    }

    // Skip silently if every span is empty (nothing to draw).
    if text.spans.iter().all(|s| s.text.is_empty()) {
        return 0.0;
    }

    // Resolve font family with style cascade.
    // Priority: node-local font_family → style font-family → default "Noto Sans".
    let font_family_prop = text
        .font_family
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "font-family"));
    let raw_family_name: String = match font_family_prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontFamily(name) => name.clone(),
                _ => "Noto Sans".to_owned(),
            },
            None => "Noto Sans".to_owned(),
        },
        Some(PropertyValue::Literal(name)) => name.clone(),
        // A dimension is not a family name → fall back to the default.
        Some(PropertyValue::Dimension(_)) => "Noto Sans".to_owned(),
        None => "Noto Sans".to_owned(),
    };
    // Probe the provider with the node-level defaults (weight 400, Normal
    // style) — sufficient to confirm family availability.  The advisory
    // fires at most once per text node, before any per-span shaping.
    let (family_name, fell_back) =
        resolve_family_with_fallback(fonts, &raw_family_name, "Noto Sans", 400, FontStyle::Normal);
    if fell_back {
        diagnostics.push(Diagnostic::advisory(
            "font.unresolved",
            format!(
                "text node '{}': font family '{}' not available, falling back to 'Noto Sans'",
                text.id, raw_family_name
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
    }
    let families = vec![family_name];

    // Resolve font size in pixels with style cascade; default to 16.0 if absent.
    let font_size_prop = text
        .font_size
        .clone()
        .or_else(|| style_prop(&text.style, style_map, "font-size").cloned());
    let font_size: f32 = resolve_property_dimension_px(&font_size_prop, resolved, 16.0) as f32;

    // Node opacity, applied once and cascaded with ctx.opacity onto
    // every span's alpha below.
    let node_opacity = text.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

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
    struct ShapedSpan {
        run: ZenithGlyphRun,
        color: Color,
        underline: bool,
        strikethrough: bool,
        text: String,
        weight: u16,
        style: FontStyle,
    }

    // ── Pass 1: shape ────────────────────────────────────────────────
    let mut shaped_spans: Vec<ShapedSpan> = Vec::new();
    let mut total_advance: f64 = 0.0;

    for span in &text.spans {
        if span.text.is_empty() {
            continue;
        }

        // Per-span fill: span.fill overrides node fill; default black.
        let fill_prop = span.fill.as_ref().or(node_fill_prop);
        let mut color = fill_prop
            .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
            .unwrap_or(Color::srgb(0, 0, 0, 255));
        color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

        // Per-span weight: span.font_weight overrides node weight; 400.
        let weight_prop = span.font_weight.as_ref().or(node_weight_prop);
        let weight = resolve_font_weight(weight_prop, resolved, 400);

        // Per-span italic selects the italic face; otherwise upright.
        let style = if span.italic == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        let req = ShapeRequest {
            text: &span.text,
            families: &families,
            weight,
            style,
            font_size,
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
            Ok(runs) => {
                for (i, run) in runs.into_iter().enumerate() {
                    total_advance += run.advance_width as f64;
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
                        underline: span.underline == Some(true),
                        strikethrough: span.strikethrough == Some(true),
                        text: run_text,
                        weight,
                        style,
                    });
                }
            }
        }
    }

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
    let needs_wrap = match box_w_opt {
        Some(box_w) => total_advance > box_w,
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

    // SHADOW bracket (behind the glyph runs + decorations). Opened only
    // when there is ink to draw (at least one shaped span); EndShadow
    // rides at the arm's single tail, before any PopTransform.
    let has_shadow = if shaped_spans.is_empty() {
        false
    } else {
        match text
            .shadow
            .as_ref()
            .and_then(|p| resolve_property_shadow(p, resolved, &text.id))
        {
            Some(shadows) => {
                commands.push(SceneCommand::BeginShadow { shadows });
                true
            }
            None => false,
        }
    };

    // Tracks actual line count after emit; set by whichever path runs.
    // Used solely by the overflow="fit" check below.
    let mut fit_line_count: usize = 1;

    if !needs_wrap {
        // ── FAST PATH (fits / no box): single-line two-pass emit ──────
        let x_offset: f64 = match box_w_opt {
            None => 0.0, // no box width → always start-anchor
            Some(box_w) => match align {
                "center" => (box_w - total_advance) / 2.0,
                "end" => box_w - total_advance,
                // "start"/"justify"/unknown → no offset. Justify on a
                // single line that already fits is start-aligned.
                _ => 0.0,
            },
        };

        // ── Pass 2: emit ─────────────────────────────────────────────
        let mut x_cursor = text_x + x_offset;

        for shaped in shaped_spans {
            let run_advance = shaped.run.advance_width as f64;
            let baseline_y = text_y + shaped.run.ascent as f64;
            let glyphs = run_to_scene_glyphs(&shaped.run);

            // Per-span decorations: a thin filled rule in the span's own
            // color, spanning the run's advance. Position/thickness are
            // derived from the font size (the shaped run does not expose the
            // font's underline metrics) — a deterministic v0 approximation.
            // Emitted before the glyphs so the text sits on top of any overlap.
            if shaped.underline {
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: baseline_y + font_size as f64 * 0.12,
                    w: run_advance,
                    h: deco_thickness,
                    color: shaped.color,
                });
            }
            if shaped.strikethrough {
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: baseline_y - font_size as f64 * 0.30,
                    w: run_advance,
                    h: deco_thickness,
                    color: shaped.color,
                });
            }

            commands.push(SceneCommand::DrawGlyphRun {
                x: x_cursor,
                y: baseline_y,
                font_id: shaped.run.font_id,
                font_size: shaped.run.font_size,
                color: shaped.color,
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
        let base_weight = resolve_font_weight(node_weight_prop, resolved, 400);
        let resolved_spans: Vec<ResolvedSpan> = shaped_spans
            .iter()
            .map(|s| ResolvedSpan {
                text: s.text.clone(),
                color: s.color,
                underline: s.underline,
                strikethrough: s.strikethrough,
                weight: s.weight,
                style: s.style,
            })
            .collect();

        let (tokens, metrics) = shape_words(
            &resolved_spans,
            &families,
            font_size,
            base_weight,
            engine,
            fonts,
            diagnostics,
            &text.id,
            text.source_span,
        );

        let lines = pack_lines(tokens, box_w, metrics.space_advance);

        // Record the actual line count for the overflow="fit" check below.
        fit_line_count = lines.len();

        emit_lines(
            &lines,
            text_x,
            text_y,
            box_w,
            align,
            metrics,
            font_size,
            deco_thickness,
            commands,
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
                         needs ~{:.0}px height in {:.0}px box (or a word wider than {:.0}px wide box)",
                        text.id, content_height, box_h, box_w
                    ),
                    text.source_span,
                    Some(text.id.clone()),
                ));
        }
    }

    if has_shadow {
        commands.push(SceneCommand::EndShadow);
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

/// Compile a `code` leaf node.
///
/// Returns the laid-out content height in pixels (`line_count * line_height`,
/// where `line_count` counts every physical source line including blanks),
/// which the flow-layout path in [`super::container`] uses to advance its
/// vertical cursor past a code child that declares no explicit `h`. Early
/// returns (invisible, missing/bad geometry) yield `0.0`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_code(
    code: &CodeNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) -> f64 {
    // Skip invisible code nodes.
    if code.visible == Some(false) {
        return 0.0;
    }

    // Resolve geometry — x and y are required; skip if absent or bad unit.
    let (Some(x_dim), Some(y_dim)) = (&code.x, &code.y) else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "code node '{}' is missing x or y geometry; skipped",
                code.id
            ),
            code.source_span,
            Some(code.id.clone()),
        ));
        return 0.0;
    };

    let Some(code_x_raw) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "code node",
            &code.id,
            "x",
            code.source_span,
        ));
        return 0.0;
    };
    let Some(code_y_raw) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "code node",
            &code.id,
            "y",
            code.source_span,
        ));
        return 0.0;
    };

    // Width/height are OPTIONAL; they bound the clip rectangle when
    // present. A bad unit yields None (no clip), not a hard skip.
    let code_w: Option<f64> = code.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let code_h: Option<f64> = code.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));

    // Apply group translation offset.
    let code_x = code_x_raw + ctx.dx;
    let code_y = code_y_raw + ctx.dy;

    // Resolve font family with style cascade.
    // Priority: node-local font_family → style font-family → default
    // "Noto Sans Mono" (the monospace default for code).
    let font_family_prop = code
        .font_family
        .as_ref()
        .or_else(|| style_prop(&code.style, style_map, "font-family"));
    let raw_family_name: String = match font_family_prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontFamily(name) => name.clone(),
                _ => "Noto Sans Mono".to_owned(),
            },
            None => "Noto Sans Mono".to_owned(),
        },
        Some(PropertyValue::Literal(name)) => name.clone(),
        // A dimension is not a family name → fall back to the default.
        Some(PropertyValue::Dimension(_)) => "Noto Sans Mono".to_owned(),
        None => "Noto Sans Mono".to_owned(),
    };
    // Probe the provider before shaping to avoid silently dropping lines
    // when the requested mono family is unregistered.
    let (family_name, fell_back) = resolve_family_with_fallback(
        fonts,
        &raw_family_name,
        "Noto Sans Mono",
        400,
        FontStyle::Normal,
    );
    if fell_back {
        diagnostics.push(Diagnostic::advisory(
            "font.unresolved",
            format!(
                "code node '{}': font family '{}' not available, falling back to 'Noto Sans Mono'",
                code.id, raw_family_name
            ),
            code.source_span,
            Some(code.id.clone()),
        ));
    }
    let families = vec![family_name];

    // Resolve font size in pixels with style cascade; default to 14.0.
    let font_size_prop = code
        .font_size
        .clone()
        .or_else(|| style_prop(&code.style, style_map, "font-size").cloned());
    let font_size: f32 = resolve_property_dimension_px(&font_size_prop, resolved, 14.0) as f32;

    // Resolve font weight with style cascade; default to 400.
    // A weight of 700 causes the provider to select Noto Sans Mono Bold
    // instead of Noto Sans Mono Regular (unchanged → byte-identical for 400).
    let font_weight_prop = code
        .font_weight
        .as_ref()
        .or_else(|| style_prop(&code.style, style_map, "font-weight"));
    let weight = resolve_font_weight(font_weight_prop, resolved, 400);

    // Resolve fill color with style cascade; default to opaque black.
    let fill_prop = code
        .fill
        .as_ref()
        .or_else(|| style_prop(&code.style, style_map, "fill"));
    let mut color = fill_prop
        .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &code.id))
        .unwrap_or(Color::srgb(0, 0, 0, 255));

    // Apply node opacity then cascade ctx.opacity on top.
    let node_opacity = code.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;

    // Tab expansion: replace each literal tab with `tab_width` spaces
    // (default 4). A `tab_width` of 0 makes tabs vanish — acceptable.
    let tab_width = code.tab_width.unwrap_or(4) as usize;
    let expanded = code.content.replace('\t', &" ".repeat(tab_width));

    // Rotation bracket (outermost — wraps the clip). Only when both w and h
    // are present (needed for a safe pivot center). Unrotated code nodes
    // emit no PushTransform → byte-identical to before.
    let rot = rotation_degrees(code.rotate.as_ref());
    let code_rot = rot
        .zip(code_w)
        .zip(code_h)
        .map(|((a, cw), ch)| (a, code_x + cw / 2.0, code_y + ch / 2.0));
    if let Some((angle, cx, cy)) = code_rot {
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Overflow clip: clipping is the default; only `overflow="visible"`
    // disables it. The clip is applied only when enabled AND both w and
    // h resolved (the clip rectangle is fully determined). Resolve the
    // decision BEFORE the PushClip so push/pop stay balanced across the
    // emission loop (mirrors compile_frame/compile_image discipline).
    let clip_enabled = code.overflow.as_deref() != Some("visible");
    let clip_box = match (clip_enabled, code_w, code_h) {
        (true, Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    if let Some((w, h)) = clip_box {
        commands.push(SceneCommand::PushClip {
            x: code_x,
            y: code_y,
            w,
            h,
        });
    }

    // Resolve syntax-highlighting settings before the per-line loop.
    // `theme` drives builtin fallback colors; `hl_lang` is `Some` only
    // when the node declares a language that oxidoc-highlight supports.
    // When `hl_lang` is `None` the existing single-run path is used
    // unchanged, guaranteeing byte-identical output for all non-highlighted
    // documents.
    let theme = code.syntax_theme.unwrap_or_default();
    let hl_lang: Option<&str> = code.language.as_deref().filter(|l| is_supported(l));

    // Helper: resolve a TokenKind to a Color, consulting doc tokens first
    // and falling back to the builtin palette. Opacity is baked in.
    let syntax_color = |kind: TokenKind| -> Color {
        let hex: &str = resolved
            .get(token_id_for_kind(kind))
            .and_then(|rt| rt.value.as_color_hex())
            .unwrap_or_else(|| builtin_color(theme, kind));
        let mut c = parse_srgb_hex(hex).unwrap_or(Color::srgb(0, 0, 0, 255));
        c.a = (c.a as f64 * node_opacity * ctx.opacity).round() as u8;
        c
    };

    // Multi-line emission: each physical line becomes its own glyph run,
    // stacked by `line_height`. Blank lines emit no run but the index `i`
    // still advances, preserving their vertical space. All non-blank
    // lines share identical ascent/line_height (same font + size), so the
    // per-line metrics give consistent stacking.
    //
    // `measured_line_height` captures the shared per-line height from the
    // first successfully shaped run; combined with the physical line count
    // it gives the laid-out content height returned for flow layout.
    let mut measured_line_height: f64 = 0.0;
    // Largest index of a non-empty (ink-bearing) line; `(last + 1)` is the
    // line count spanned by the block, ignoring trailing blank lines.
    let mut last_inked_line: Option<usize> = None;
    for (i, line) in expanded.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        last_inked_line = Some(i);

        if let Some(lang) = hl_lang {
            // ── Highlighted path: per-token colored segments ──────────
            // Tokenise the line, walk gaps between tokens, collect
            // (text_slice, color) pairs, shape each, then emit.
            let plain_color = syntax_color(TokenKind::Plain);
            let tokens = scan(line, lang);

            // Build segment list: (slice, color)
            let mut segments: Vec<(&str, Color)> = Vec::new();
            let mut pos: usize = 0;
            for tok in &tokens {
                // Gap before this token → plain color.
                if tok.start > pos
                    && let Some(gap) = line.get(pos..tok.start)
                    && !gap.is_empty()
                {
                    segments.push((gap, plain_color));
                }
                if let Some(slice) = line.get(tok.start..tok.end)
                    && !slice.is_empty()
                {
                    segments.push((slice, syntax_color(tok.kind)));
                }
                pos = tok.end;
            }
            // Trailing gap after last token.
            if pos < line.len()
                && let Some(tail) = line.get(pos..)
                && !tail.is_empty()
            {
                segments.push((tail, plain_color));
            }

            // Shape all segments; collect (run, color) pairs so metrics
            // can be read from the first successful run before emitting.
            let mut shaped = Vec::new();
            for (seg_text, seg_color) in segments {
                let req = ShapeRequest {
                    text: seg_text,
                    families: &families,
                    weight,
                    style: FontStyle::Normal,
                    font_size,
                };
                match engine.shape(&req, fonts) {
                    Err(e) => {
                        diagnostics.push(Diagnostic::advisory(
                            "scene.text_unshaped",
                            format!("code node '{}' could not be shaped: {}", code.id, e.message),
                            code.source_span,
                            Some(code.id.clone()),
                        ));
                        // Skip this segment; cursor does not advance.
                    }
                    Ok(run) => {
                        shaped.push((run, seg_color));
                    }
                }
            }

            // Emit: metrics are font-constant, read from first shaped run.
            if let Some((first_run, _)) = shaped.first() {
                if measured_line_height == 0.0 {
                    measured_line_height = first_run.line_height as f64;
                }
                let baseline_y =
                    code_y + first_run.ascent as f64 + (i as f64) * first_run.line_height as f64;
                let mut x_cursor = code_x;
                for (run, seg_color) in shaped {
                    let advance = run.advance_width as f64;
                    let glyphs = run_to_scene_glyphs(&run);
                    commands.push(SceneCommand::DrawGlyphRun {
                        x: x_cursor,
                        y: baseline_y,
                        font_id: run.font_id,
                        font_size: run.font_size,
                        color: seg_color,
                        glyphs,
                    });
                    x_cursor += advance;
                }
            }
        } else {
            // ── Plain path (no highlighting): single run per line ──────
            // weight defaults to 400 (from resolve_font_weight), so code
            // nodes without font-weight remain byte-identical to before.
            let req = ShapeRequest {
                text: line,
                families: &families,
                weight,
                style: FontStyle::Normal,
                font_size,
            };

            match engine.shape(&req, fonts) {
                Err(e) => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.text_unshaped",
                        format!("code node '{}' could not be shaped: {}", code.id, e.message),
                        code.source_span,
                        Some(code.id.clone()),
                    ));
                    continue;
                }
                Ok(run) => {
                    if measured_line_height == 0.0 {
                        measured_line_height = run.line_height as f64;
                    }
                    let baseline_y =
                        code_y + run.ascent as f64 + (i as f64) * run.line_height as f64;
                    let glyphs = run_to_scene_glyphs(&run);

                    commands.push(SceneCommand::DrawGlyphRun {
                        x: code_x,
                        y: baseline_y,
                        font_id: run.font_id,
                        font_size: run.font_size,
                        color,
                        glyphs,
                    });
                }
            }
        }
    }

    if clip_box.is_some() {
        commands.push(SceneCommand::PopClip);
    }

    if code_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    // Laid-out content height: number of lines spanned (index of the last
    // ink-bearing line + 1, so trailing blank lines do not inflate the box)
    // times the shared per-line height. Zero when nothing was shaped.
    match last_inked_line {
        Some(last) => (last + 1) as f64 * measured_line_height,
        None => 0.0,
    }
}
