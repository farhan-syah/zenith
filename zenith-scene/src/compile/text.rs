//! Text and code leaf-node compilation, plus the shaping/glyph and
//! syntax-highlight helpers they depend on.

use std::collections::BTreeMap;

use zenith_core::{
    CodeNode, Diagnostic, FontProvider, FontStyle, PropertyValue, ResolvedToken, ResolvedValue,
    Style, TextNode, TextSpan, TokenKind, builtin_color, dim_to_px, is_supported, scan,
    token_id_for_kind,
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
    /// Super/subscript baseline shift in pixels (negative = up; 0 = baseline).
    /// Applied per-glyph-run by [`emit_lines`] on top of the line baseline.
    pub(super) baseline_dy: f64,
    /// The exact source text this word was shaped from, plus the weight/style/
    /// size needed to RE-shape a hyphenated fragment of it. Used ONLY by the
    /// optional hyphenation path in [`pack_lines`]; the non-hyphenate path never
    /// reads it, so default-off packing is byte-identical. `paragraph` is the
    /// 0-based paragraph index this word belongs to (newline-separated source),
    /// consumed by widow/orphan control in the chain distributor.
    pub(super) src: WordSource,
}

/// The source text + shaping attributes a [`WordToken`] was produced from, so a
/// hyphenated fragment can be deterministically re-shaped with identical style.
#[derive(Clone)]
pub(super) struct WordSource {
    pub(super) text: String,
    pub(super) weight: u16,
    pub(super) style: FontStyle,
    pub(super) font_size: f32,
    /// 0-based paragraph index (each `\n` in the source starts a new paragraph).
    pub(super) paragraph: usize,
    /// When this token is a hyphenation fragment, the ORIGINAL unsplit word it
    /// came from, with `true` for the head (`fragment-`) and `false` for the
    /// tail. The chain distributor uses this to MERGE an adjacent head+tail back
    /// into the original word before re-wrapping it in the next box, so a
    /// fragment is never hyphenated twice. `None` for an ordinary word.
    pub(super) hyphen_part: Option<(String, bool)>,
}

/// One packed line: its words plus the summed content width (no trailing space).
pub(super) struct Line {
    pub(super) words: Vec<WordToken>,
    pub(super) content_w: f64,
    /// The 0-based paragraph index this line belongs to, taken from its first
    /// word. Used by widow/orphan control in the chain distributor; the
    /// single-box path ignores it. A line is always within ONE paragraph because
    /// the packer never mixes words from different paragraphs onto one line.
    pub(super) paragraph: usize,
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
    /// The span's OWN font size (reduced for super/subscript). When equal to the
    /// shared node size, shaping is byte-identical to the size-less form.
    pub(super) font_size: f32,
    /// Super/subscript baseline shift in pixels (negative = up; 0 = baseline).
    pub(super) baseline_dy: f64,
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
    // Running paragraph index. Each `\n` in the source (across spans) starts a
    // new paragraph; consecutive spans without a newline keep the same index, so
    // a multi-span paragraph stays one paragraph. Widow/orphan control reads this
    // per-line; the default-off path never inspects it.
    let mut paragraph: usize = 0;

    for shaped in spans {
        // A super/subscript span carries its own reduced size; a baseline span
        // uses the shared node `font_size`. Metrics (ascent/line_height) are
        // captured ONLY from a full-size word so the line grid stays uniform.
        let is_vertical_align = shaped.baseline_dy != 0.0;
        let word_font_size = shaped.font_size;
        // Split the span text into paragraphs on `\n`; each segment after the
        // first increments the running paragraph index. `split('\n')` always
        // yields ≥1 segment, so a span without a newline keeps `paragraph`.
        for (seg_idx, segment) in shaped.text.split('\n').enumerate() {
            if seg_idx > 0 {
                paragraph += 1;
            }
            for word in segment.split_whitespace() {
                let req = ShapeRequest {
                    text: word,
                    families,
                    weight: shaped.weight,
                    style: shaped.style,
                    font_size: word_font_size,
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
                        if !have_metrics
                            && !is_vertical_align
                            && let Some(first) = runs.first()
                        {
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
                            baseline_dy: shaped.baseline_dy,
                            runs,
                            src: WordSource {
                                text: word.to_owned(),
                                weight: shaped.weight,
                                style: shaped.style,
                                font_size: word_font_size,
                                paragraph,
                                hyphen_part: None,
                            },
                        });
                    }
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

// ── Hyphenation ───────────────────────────────────────────────────────────────
//
// Opt-in Knuth–Liang hyphenation. The embedded en-US `Standard` dictionary is
// loaded exactly ONCE into a process-wide `OnceLock` (deterministic: a pure
// function of the embedded patterns, no time/random/IO beyond the embedded
// blob). A `HyphenationContext` bundles the dictionary with the shaping engine
// and node-level family so the packer can re-shape a `fragment-` head and the
// remainder of a split word with identical style.

use std::sync::OnceLock;

use hyphenation::{Hyphenator, Language, Load, Standard};

/// Process-wide cache for the embedded en-US hyphenation dictionary. Loaded at
/// most once; `None` only if the embedded blob fails to decode (it should not,
/// but we never panic). Subsequent calls reuse the same `Standard`.
static EN_US_HYPHENATOR: OnceLock<Option<Standard>> = OnceLock::new();

/// Return the cached en-US hyphenator, loading it on first use. Deterministic.
pub(super) fn en_us_hyphenator() -> Option<&'static Standard> {
    EN_US_HYPHENATOR
        .get_or_init(|| Standard::from_embedded(Language::EnglishUS).ok())
        .as_ref()
}

/// Everything the packer needs to hyphenate + re-shape a word fragment: the
/// dictionary plus the shaping engine, fonts, and node family. The per-word
/// weight/style/size come from each [`WordToken::src`], so a chain or wrapped
/// node hyphenates with that word's exact style.
pub(super) struct HyphenationContext<'a> {
    pub(super) dict: &'static Standard,
    pub(super) engine: &'a RustybuzzEngine,
    pub(super) fonts: &'a dyn FontProvider,
    pub(super) families: &'a [String],
    /// The hyphen glyph string shaped onto the head fragment.
    pub(super) hyphen: &'a str,
}

/// A word split at a hyphenation point: the head (`fragment-`, including the
/// hyphen glyph) to place on the current line, and the tail to carry to the next.
struct HyphenSplit {
    head: WordToken,
    tail: WordToken,
}

/// Re-shape `text` into a [`WordToken`] inheriting `donor`'s visual attributes,
/// at `donor`'s weight/style/size. `hyphen_part` tags the result as a
/// hyphenation head/tail (or `None` for a plain reconstruction). Returns `None`
/// on a shaping failure so the caller falls back to NOT splitting. Deterministic.
fn reshape_fragment(
    text: &str,
    donor: &WordToken,
    hyphen_part: Option<(String, bool)>,
    ctx: &HyphenationContext,
) -> Option<WordToken> {
    let req = ShapeRequest {
        text,
        families: ctx.families,
        weight: donor.src.weight,
        style: donor.src.style,
        font_size: donor.src.font_size,
    };
    let runs = ctx.engine.shape_with_fallback(&req, ctx.fonts).ok()?;
    let advance: f64 = runs.iter().map(|r| r.advance_width as f64).sum();
    Some(WordToken {
        runs,
        advance,
        color: donor.color,
        underline: donor.underline,
        strikethrough: donor.strikethrough,
        baseline_dy: donor.baseline_dy,
        src: WordSource {
            text: text.to_owned(),
            weight: donor.src.weight,
            style: donor.src.style,
            font_size: donor.src.font_size,
            paragraph: donor.src.paragraph,
            hyphen_part,
        },
    })
}

/// Attempt to hyphenate `word` so its head fragment (`fragment-`, including the
/// hyphen glyph) fits within `avail` pixels. Tries the LAST (longest) embedded
/// break point that fits; returns `None` when the word has no break point whose
/// head fits, leaving the caller to push the whole word to the next line.
///
/// Determinism: the break list is pattern-derived (deterministic), the chosen
/// point is a pure function of `avail`, and both fragments are re-shaped with the
/// same engine. Words containing non-letters (e.g. trailing punctuation) still
/// hyphenate on their letter run because the dictionary only proposes interior
/// letter breaks; the head/tail slices are taken on byte boundaries the
/// dictionary returns, which always fall between characters.
fn try_hyphenate(word: &WordToken, avail: f64, ctx: &HyphenationContext) -> Option<HyphenSplit> {
    let text = word.src.text.as_str();
    // A break shorter than this many bytes on either side is never useful.
    if text.len() < 4 {
        return None;
    }
    let breaks = ctx.dict.hyphenate(text).breaks;
    // Walk break points from LAST to FIRST: the longest head that still fits is
    // the most text we can place, minimizing wasted space.
    for &b in breaks.iter().rev() {
        // `b` is a byte offset within `text` strictly inside the word.
        let (Some(head_txt), Some(tail_txt)) = (text.get(..b), text.get(b..)) else {
            continue;
        };
        if head_txt.is_empty() || tail_txt.is_empty() {
            continue;
        }
        let head_with_hyphen = format!("{head_txt}{}", ctx.hyphen);
        let orig = text.to_owned();
        let Some(head) = reshape_fragment(&head_with_hyphen, word, Some((orig.clone(), true)), ctx)
        else {
            continue;
        };
        if head.advance > avail {
            continue;
        }
        let Some(tail) = reshape_fragment(tail_txt, word, Some((orig, false)), ctx) else {
            continue;
        };
        return Some(HyphenSplit { head, tail });
    }
    None
}

/// Flatten packed lines back into a single ordered word stream for re-wrapping in
/// the NEXT chain box, MERGING any hyphenation head+tail pair back into the
/// original unsplit word so a fragment is never carried across a box (and never
/// hyphenated twice). When `hyph` is `None` (no hyphenation was performed) the
/// words pass through unchanged. A head whose tail does not immediately follow
/// (it should always, by construction) is passed through as-is rather than lost.
pub(super) fn flatten_lines_to_tokens(
    lines: Vec<Line>,
    hyph: Option<&HyphenationContext>,
) -> Vec<WordToken> {
    let mut words: Vec<WordToken> = Vec::new();
    for line in lines {
        for w in line.words {
            words.push(w);
        }
    }
    let Some(ctx) = hyph else {
        return words;
    };
    // Merge adjacent (head, tail) pairs back into the original word.
    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut iter = words.into_iter().peekable();
    while let Some(w) = iter.next() {
        if let Some((orig, true)) = &w.src.hyphen_part {
            let is_tail_next = iter
                .peek()
                .and_then(|n| n.src.hyphen_part.as_ref())
                .is_some_and(|(o, head)| !head && o == orig);
            if is_tail_next {
                let orig = orig.clone();
                let tail = iter.next();
                match reshape_fragment(&orig, &w, None, ctx) {
                    Some(merged) => out.push(merged),
                    None => {
                        // Reshape failed: keep both fragments rather than drop text.
                        out.push(w);
                        if let Some(t) = tail {
                            out.push(t);
                        }
                    }
                }
                continue;
            }
        }
        out.push(w);
    }
    out
}

/// Greedy-pack word tokens into lines for a given box width, left-to-right and
/// deterministic. Identical algorithm to the original inline wrap packer when
/// `hyph` is `None`; passing a [`HyphenationContext`] enables word splitting at
/// embedded break points (see [`pack_lines_core`]).
pub(super) fn pack_lines(
    tokens: Vec<WordToken>,
    box_w: f64,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
) -> Vec<Line> {
    let profile = WidthProfile {
        narrow_w: box_w,
        narrow_count: 0,
        full_w: box_w,
    };
    pack_lines_core(tokens, profile, space_advance, hyph)
}

/// Per-line width profile for the drop-cap wrap-around.
///
/// The first `narrow_count` lines are packed to `narrow_w` (they sit to the
/// RIGHT of the drop-cap glyph); every line from index `narrow_count` onward is
/// packed to `full_w` (the text has cleared the cap and returns to full measure).
/// `pack_lines` is the degenerate case `narrow_count == 0`.
#[derive(Clone, Copy)]
pub(super) struct WidthProfile {
    pub(super) narrow_w: f64,
    pub(super) narrow_count: usize,
    pub(super) full_w: f64,
}

impl WidthProfile {
    /// The available width for the line at index `i` (0-based).
    fn width_for(&self, line_index: usize) -> f64 {
        if line_index < self.narrow_count {
            self.narrow_w
        } else {
            self.full_w
        }
    }
}

/// Greedy-pack word tokens into lines using a per-line [`WidthProfile`] so the
/// first `narrow_count` lines wrap to the narrow measure beside a drop cap and
/// the remainder return to the full measure. The greedy algorithm matches
/// [`pack_lines`] exactly when the profile is uniform (`narrow_count == 0` or
/// `narrow_w == full_w`); the only difference is the per-line target width,
/// re-read from the profile as each new line is opened.
pub(super) fn pack_lines_variable(
    tokens: Vec<WordToken>,
    profile: WidthProfile,
    space_advance: f64,
) -> Vec<Line> {
    // The drop-cap path does not hyphenate (a documented v0 follow-up like the
    // chain/flow drop-cap combination), so it threads `None`.
    pack_lines_core(tokens, profile, space_advance, None)
}

/// The single greedy packer shared by [`pack_lines`] and [`pack_lines_variable`].
///
/// Beyond the original greedy fill it adds two OPT-IN behaviours that leave the
/// default path byte-identical:
///
/// 1. **Paragraph breaks.** Each word carries a paragraph index
///    ([`WordSource::paragraph`]); a word whose paragraph differs from the line
///    being filled forces a line break, so a line never mixes paragraphs and
///    every [`Line::paragraph`] is well-defined. A single-paragraph document
///    (every index 0) never triggers this, so output is unchanged.
///
/// 2. **Hyphenation.** When `hyph` is `Some` and a word does NOT fit the
///    remaining space on a NON-EMPTY line, the packer tries to split it at the
///    last embedded break point whose `fragment-` head fits the remaining width
///    ([`try_hyphenate`]); the head joins the current line and the tail is
///    re-queued as the first word of the next line. If no break fits, the word
///    flows whole to the next line exactly as before. `hyph == None` skips this
///    entirely, so the default path is byte-identical.
fn pack_lines_core(
    tokens: Vec<WordToken>,
    profile: WidthProfile,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut cur: Vec<WordToken> = Vec::new();
    let mut line_w: f64 = 0.0;
    // Paragraph index of the line currently being filled (set by its first word).
    let mut cur_para: usize = 0;
    // A queue so a hyphenation tail can be re-processed as the next word without
    // restructuring the loop. Seeded with the input tokens in order.
    let mut queue: std::collections::VecDeque<WordToken> = tokens.into();

    while let Some(tok) = queue.pop_front() {
        // The width budget for the line currently being filled is the width for
        // the line this word would land on (the next line index when `cur` is
        // empty, else the current one — `lines.len()` is that index).
        let box_w = profile.width_for(lines.len());

        // Paragraph boundary: a word from a later paragraph than the line being
        // filled forces a break first (single-paragraph docs never hit this).
        let para_break = !cur.is_empty() && tok.src.paragraph != cur_para;

        let overflow = !cur.is_empty() && line_w + space_advance + tok.advance > box_w;

        if overflow && !para_break {
            // Try to hyphenate the word into the remaining space before wrapping.
            // `avail` is the width left on the current line after a space gap.
            if let Some(ctx) = hyph {
                let avail = box_w - line_w - space_advance;
                if avail > 0.0
                    && let Some(split) = try_hyphenate(&tok, avail, ctx)
                {
                    // Head + hyphen joins the current line; close the line.
                    line_w += space_advance + split.head.advance;
                    cur.push(split.head);
                    lines.push(Line {
                        words: std::mem::take(&mut cur),
                        content_w: line_w,
                        paragraph: cur_para,
                    });
                    line_w = 0.0;
                    // The tail becomes the first word of the next line.
                    queue.push_front(split.tail);
                    continue;
                }
            }
        }

        if overflow || para_break {
            let content_w = line_w;
            lines.push(Line {
                words: std::mem::take(&mut cur),
                content_w,
                paragraph: cur_para,
            });
            line_w = 0.0;
        }

        if cur.is_empty() {
            cur_para = tok.src.paragraph;
        }
        let gap = if cur.is_empty() { 0.0 } else { space_advance };
        line_w += gap + tok.advance;
        cur.push(tok);
    }
    if !cur.is_empty() {
        lines.push(Line {
            words: cur,
            content_w: line_w,
            paragraph: cur_para,
        });
    }
    lines
}

/// Emit decoration FillRects + DrawGlyphRun commands for a sequence of packed
/// lines, stacked by `line_height`, with per-line horizontal `align`.
///
/// This is the EXACT emit body lifted out of `compile_text`'s wrap path so the
/// single-box and chain renderers produce byte-identical command streams.
///
/// `justify_final_line` controls the LAST line of THIS batch under
/// `align="justify"`: `false` (the single-box wrap path and the FINAL chain
/// box) leaves it ragged per paragraph semantics; `true` (a non-final chain box
/// whose flow continues into the next box) keeps it justified, since the text
/// does not actually end at this box's last line.
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
    justify_final_line: bool,
    commands: &mut Vec<SceneCommand>,
) {
    // Uniform geometry: every line shares `text_x`/`box_w`. Delegates to the
    // profiled emit with a constant per-line geometry so the two paths are one
    // body (byte-identical to the historical single-width emit).
    emit_lines_profiled(
        lines,
        |_| (text_x, box_w),
        text_y,
        align,
        metrics,
        font_size,
        deco_thickness,
        justify_final_line,
        commands,
    );
}

/// Per-line geometry resolver: maps a 0-based line index to its
/// `(line_origin_x, line_box_width)`. The drop-cap path returns an indented
/// origin + narrow width for the first `n` lines and `(text_x, full_w)` after;
/// the uniform path returns the same `(text_x, box_w)` for every line.
///
/// This is the SINGLE emit body; [`emit_lines`] is the uniform special case.
/// Alignment, decoration, and glyph emission are identical — only the per-line
/// horizontal origin/measure are read from `geom`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_lines_profiled<F>(
    lines: &[Line],
    geom: F,
    text_y: f64,
    align: &str,
    metrics: WordMetrics,
    font_size: f32,
    deco_thickness: f64,
    justify_final_line: bool,
    commands: &mut Vec<SceneCommand>,
) where
    F: Fn(usize) -> (f64, f64),
{
    let ascent = metrics.ascent;
    let line_height = metrics.line_height;
    let space_advance = metrics.space_advance;
    let last_idx = lines.len().saturating_sub(1);
    for (i, line) in lines.iter().enumerate() {
        let (text_x, box_w) = geom(i);
        let baseline_y = text_y + ascent + (i as f64) * line_height;
        let word_count = line.words.len();

        let (base_x, gap) = match align {
            "center" => (text_x + (box_w - line.content_w) / 2.0, space_advance),
            "end" => (text_x + (box_w - line.content_w), space_advance),
            "justify" => {
                // Justify stretches inter-word gaps so a non-final, multi-word
                // line fills the box. The final line and single-word lines stay
                // at the start offset (paragraph semantics). `extra` is clamped
                // ≥ 0 so an overlong line (content_w > box_w) never SHRINKS gaps
                // below the normal space; `word_count > 1` guards the divisor.
                // A continuation chain box (`justify_final_line`) justifies its
                // own last line too, since the paragraph flows on past it.
                let is_final_line = i == last_idx && !justify_final_line;
                if !is_final_line && word_count > 1 {
                    let extra = (box_w - line.content_w).max(0.0) / (word_count as f64 - 1.0);
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

        // Glyphs. A super/subscript word carries a non-zero `baseline_dy`,
        // shifting its runs off the shared line baseline (negative = up); a
        // baseline word has dy 0 and is byte-identical to before.
        for (wi, word) in line.words.iter().enumerate() {
            let mut run_x = word_x.get(wi).copied().unwrap_or(base_x);
            let word_baseline_y = baseline_y + word.baseline_dy;
            for run in &word.runs {
                commands.push(SceneCommand::DrawGlyphRun {
                    x: run_x,
                    y: word_baseline_y,
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
        // Only the FINAL chain member leaves its last line ragged under
        // justify; a continuation box justifies its last line because the
        // paragraph flows on into the next box.
        !assignment.is_last_member,
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

/// Resolve a font-family [`PropertyValue`] to a raw family name string.
///
/// Priority: `TokenRef → FontFamily` value → else `default`; `Literal` → that
/// string; `Dimension` → `default` (not a family name); absent → `default`.
/// This extraction step is shared by [`compile_text`] and [`super::chain`]'s
/// style resolver so the two code paths stay byte-identical.
pub(super) fn resolve_font_family_name(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: &str,
) -> String {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontFamily(name) => name.clone(),
                _ => default.to_owned(),
            },
            None => default.to_owned(),
        },
        Some(PropertyValue::Literal(name)) => name.clone(),
        Some(PropertyValue::Dimension(_)) | None => default.to_owned(),
    }
}

/// Superscript/subscript font-size scale factor applied to a span's resolved
/// size (deterministic). A `vertical-align="super"`/`"sub"` span is typeset at
/// `0.65 ×` the full font size.
const VALIGN_SCALE: f64 = 0.65;
/// Superscript baseline shift as a fraction of the FULL font size: the baseline
/// is raised (negative = up) by `0.34 × full_font_size`.
const VALIGN_SUPER_SHIFT: f64 = 0.34;
/// Subscript baseline shift as a fraction of the FULL font size: the baseline is
/// lowered by `0.16 × full_font_size`.
const VALIGN_SUB_SHIFT: f64 = 0.16;

/// Resolve a span's `vertical-align` into `(span_font_size, baseline_dy)`.
///
/// `node_font_size` is the full (node-resolved) size. For `"super"`/`"sub"` the
/// span size is reduced by [`VALIGN_SCALE`] and the baseline is shifted by a
/// fraction of the FULL size ([`VALIGN_SUPER_SHIFT`] up / [`VALIGN_SUB_SHIFT`]
/// down). Any other / absent value returns the full size and a zero shift, so a
/// span without vertical-align is byte-identical to before.
pub(super) fn resolve_vertical_align(
    vertical_align: Option<&str>,
    node_font_size: f32,
) -> (f32, f64) {
    let full = node_font_size as f64;
    match vertical_align {
        Some("super") => ((full * VALIGN_SCALE) as f32, -(full * VALIGN_SUPER_SHIFT)),
        Some("sub") => ((full * VALIGN_SCALE) as f32, full * VALIGN_SUB_SHIFT),
        _ => (node_font_size, 0.0),
    }
}

/// Horizontal gap between the drop-cap glyph's right edge and the wrapped body
/// text, as a fraction of the BODY font size. `0.25 ×` body size is a compact,
/// deterministic default.
const DROPCAP_GAP_FACTOR: f64 = 0.25;

/// A shaped drop-cap initial ready for emission.
struct DropCap {
    /// The oversized shaped glyph run for the initial.
    run: ZenithGlyphRun,
    /// Ascent of the oversized run (cap top → baseline), in pixels.
    ascent: f64,
    /// Pen advance of the oversized run (used as the body indent base).
    advance: f64,
    /// Resolved node color the cap paints with.
    color: Color,
    /// Number of body lines the cap spans (the narrow-line count).
    lines: usize,
}

/// The initial lifted out of the body for a drop cap, plus the donor span's
/// visual attributes, BEFORE the oversized glyph is shaped.
struct DropCapInitial {
    ch: char,
    color: Color,
    style: FontStyle,
}

/// Lift the first character out of the body spans for a drop cap.
///
/// The first NON-EMPTY resolved span donates its leading `char` (the v0 grapheme
/// unit — combining sequences are a documented follow-up); that span's text is
/// rewritten WITHOUT the initial so the body wrap re-tokenizes only the
/// remainder. Returns `None` (leaving the spans untouched) when no span carries
/// a character, so an empty-text node with the attribute never panics and draws
/// no cap. The donor color/style are captured for the cap glyph.
fn take_drop_cap_initial(spans: &mut [ResolvedSpan]) -> Option<DropCapInitial> {
    let donor = spans.iter_mut().find(|s| !s.text.is_empty())?;
    let first = donor.text.chars().next()?;
    // Strip the initial from the body (it is now drawn by the cap).
    donor.text = donor.text.chars().skip(1).collect();
    Some(DropCapInitial {
        ch: first,
        color: donor.color,
        style: donor.style,
    })
}

/// Shape a lifted [`DropCapInitial`] as an oversized glyph spanning `n` lines.
///
/// The cap is shaped at `n * line_height` (the real body line height, captured
/// from the body shaping pass) so the glyph optically fills ~`n` body lines (its
/// ascent is ≈ `0.75 ×` that). It paints in the donor span's color and the node
/// family. `None` on a shaping failure → no cap, body unchanged.
fn shape_drop_cap(
    initial: &DropCapInitial,
    families: &[String],
    weight: u16,
    line_height: f64,
    n: u32,
    engine: &RustybuzzEngine,
    fonts: &dyn FontProvider,
) -> Option<DropCap> {
    let cap_size = (n as f64 * line_height) as f32;
    let glyph = initial.ch.to_string();
    let req = ShapeRequest {
        text: &glyph,
        families,
        weight,
        style: initial.style,
        font_size: cap_size,
    };
    let run = engine
        .shape_with_fallback(&req, fonts)
        .ok()?
        .into_iter()
        .next()?;
    let ascent = run.ascent as f64;
    let advance = run.advance_width as f64;
    Some(DropCap {
        run,
        ascent,
        advance,
        color: initial.color,
        lines: n as usize,
    })
}

/// Horizontal breathing space, as a fraction of the leader glyph's advance,
/// left between the LEFT segment's right edge (and before the RIGHT segment's
/// left edge) and the run of leader glyphs. A deterministic, compact default.
const TAB_LEADER_GAP_FACTOR: f64 = 1.0;

/// A row's left/right text shaped into glyph runs, plus the summed advances.
struct TabLeaderRow {
    /// LEFT-segment runs (placed at the box left edge), left-to-right.
    left_runs: Vec<ZenithGlyphRun>,
    left_advance: f64,
    /// RIGHT-segment runs (right-aligned to the box right edge), left-to-right.
    /// Empty when the row carried no `\t` (left-aligned row, no leader).
    right_runs: Vec<ZenithGlyphRun>,
    right_advance: f64,
    /// Whether this row carried a tab (so a leader run + right segment apply).
    has_tab: bool,
}

/// Shape one TOC row's LEFT and RIGHT segments with the node font/size/weight.
///
/// The row is split on its FIRST `\t`; everything before is the LEFT segment,
/// everything after is the RIGHT segment. A row without a tab yields only a
/// LEFT segment (`has_tab == false`). Empty segments shape to zero runs/advance
/// safely. Deterministic: the same engine/fonts always produce the same runs.
fn shape_tab_leader_row(
    row: &str,
    families: &[String],
    font_size: f32,
    weight: u16,
    engine: &RustybuzzEngine,
    fonts: &dyn FontProvider,
) -> TabLeaderRow {
    let (left_text, right_text, has_tab) = match row.split_once('\t') {
        Some((l, r)) => (l, r, true),
        None => (row, "", false),
    };

    let shape_seg = |seg: &str| -> (Vec<ZenithGlyphRun>, f64) {
        if seg.is_empty() {
            return (Vec::new(), 0.0);
        }
        let req = ShapeRequest {
            text: seg,
            families,
            weight,
            style: FontStyle::Normal,
            font_size,
        };
        match engine.shape_with_fallback(&req, fonts) {
            Ok(runs) => {
                let adv: f64 = runs.iter().map(|r| r.advance_width as f64).sum();
                (runs, adv)
            }
            Err(_) => (Vec::new(), 0.0),
        }
    };

    let (left_runs, left_advance) = shape_seg(left_text);
    let (right_runs, right_advance) = shape_seg(right_text);

    TabLeaderRow {
        left_runs,
        left_advance,
        right_runs,
        right_advance,
        has_tab,
    }
}

/// Emit a sequence of glyph runs starting at pen `start_x` on baseline `y`, in
/// the node's resolved `color`. Runs are positioned left-to-right by their own
/// advances. Shared by the LEFT, RIGHT, and leader emission so the field mapping
/// lives in one place.
fn emit_tab_leader_runs(
    runs: &[ZenithGlyphRun],
    start_x: f64,
    y: f64,
    color: Color,
    commands: &mut Vec<SceneCommand>,
) {
    let mut x = start_x;
    for run in runs {
        commands.push(SceneCommand::DrawGlyphRun {
            x,
            y,
            font_id: run.font_id.clone(),
            font_size: run.font_size,
            color,
            glyphs: run_to_scene_glyphs(run),
        });
        x += run.advance_width as f64;
    }
}

/// Render a text node in TAB-LEADER (table-of-contents) mode.
///
/// The combined span text is split into rows on `\n`. Each row is stacked by
/// `line_height` (baseline = `text_y + ascent + i*line_height`) like the
/// multi-line text/code paths. Within a row the FIRST `\t` splits a LEFT and a
/// RIGHT segment: LEFT is placed at the box left edge (`text_x`); RIGHT is
/// right-aligned so its right edge sits at the box right edge
/// (`text_x + box_w - right_advance`). The gap between LEFT's right edge (plus a
/// small breathing gap) and RIGHT's left edge (minus the same gap) is filled
/// with the leader glyph repeated: ONE leader glyph is shaped, the integer count
/// is `floor(gap / leader_advance)`, and the leaders are placed left-to-right
/// starting just after LEFT, spaced by `leader_advance` (documented choice). A
/// non-positive gap (or a zero leader advance) emits no leaders and an advisory
/// `text.overflow` warning, leaving LEFT and RIGHT to abut/overlap. A row with no
/// tab renders left-aligned with no leader.
///
/// Determinism: rows are processed in source order; the leader count is an
/// integer from a deterministic float division; runs are emitted in a fixed
/// order — so two runs are byte-identical. Returns the laid-out content height
/// (`row_count * line_height`).
#[allow(clippy::too_many_arguments)]
fn compile_tab_leader(
    text: &TextNode,
    leader: &str,
    families: &[String],
    font_size: f32,
    node_fill_prop: Option<&PropertyValue>,
    node_weight_prop: Option<&PropertyValue>,
    node_opacity: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    engine: &RustybuzzEngine,
    fonts: &dyn FontProvider,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    text_x: f64,
    text_y: f64,
    ctx: RenderCtx,
) -> f64 {
    // Combined source text across all spans (tab-leader mode treats the node as
    // one verbatim block, like a code node, so `\t`/`\n` keep their meaning).
    let combined: String = text.spans.iter().map(|s| s.text.as_str()).collect();
    if combined.is_empty() {
        return 0.0;
    }

    // Resolved node color (the whole TOC block shares one fill) and weight.
    let mut color = node_fill_prop
        .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &text.id))
        .unwrap_or(Color::srgb(0, 0, 0, 255));
    color.a = (color.a as f64 * node_opacity * ctx.opacity).round() as u8;
    let weight = resolve_font_weight(node_weight_prop, resolved, 400);

    // Box width is required to right-align the page number. Without it, a TOC
    // row cannot be laid out (no box edge to flush to) — surface an advisory and
    // render nothing rather than guessing a width.
    let Some(box_w) = text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)) else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "text node '{}' uses tab-leader but has no box width; skipped",
                text.id
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
        return 0.0;
    };

    // Shape ONE leader glyph once; its advance drives the fill count + spacing.
    let leader_req = ShapeRequest {
        text: leader,
        families,
        weight,
        style: FontStyle::Normal,
        font_size,
    };
    let leader_run = match engine.shape_with_fallback(&leader_req, fonts) {
        Ok(runs) => runs.into_iter().next(),
        Err(_) => None,
    };
    let leader_advance = leader_run
        .as_ref()
        .map(|r| r.advance_width as f64)
        .unwrap_or(0.0);

    // Shape every row first so the shared ascent/line-height (from the first
    // non-empty run) define a uniform line grid for the whole block.
    let rows: Vec<TabLeaderRow> = combined
        .split('\n')
        .map(|row| shape_tab_leader_row(row, families, font_size, weight, engine, fonts))
        .collect();

    // Shared metrics from the first available run (left, right, or the leader).
    let (ascent, line_height) = rows
        .iter()
        .flat_map(|r| r.left_runs.iter().chain(r.right_runs.iter()))
        .next()
        .or(leader_run.as_ref())
        .map(|r| (r.ascent as f64, r.line_height as f64))
        .unwrap_or((0.0, 0.0));

    // Rotation bracket: only when both w and h are present (safe pivot center),
    // mirroring the main text path so rotated TOC pages behave consistently.
    let box_h_opt: Option<f64> = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
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

    let gap_pad = leader_advance * TAB_LEADER_GAP_FACTOR;
    let box_right = text_x + box_w;

    for (i, row) in rows.iter().enumerate() {
        let baseline_y = text_y + ascent + (i as f64) * line_height;

        // LEFT segment at the box left edge.
        emit_tab_leader_runs(&row.left_runs, text_x, baseline_y, color, commands);

        if !row.has_tab {
            // No tab → left-aligned row, no right segment, no leader.
            continue;
        }

        // RIGHT segment right-aligned: its right edge = box right edge.
        let right_x = box_right - row.right_advance;
        emit_tab_leader_runs(&row.right_runs, right_x, baseline_y, color, commands);

        // Fill the gap between LEFT and RIGHT with leader glyphs. The usable gap
        // leaves a small breathing pad on each side.
        let leader_start = text_x + row.left_advance + gap_pad;
        let leader_end = right_x - gap_pad;
        let gap = leader_end - leader_start;

        if leader_advance <= 0.0 || gap <= 0.0 {
            // Row too long (or no leader glyph): emit no leaders. Surface a
            // non-fatal warning so the author knows this row overflowed.
            diagnostics.push(Diagnostic::warning(
                "text.overflow",
                format!(
                    "text '{}': tab-leader row {} is too long to fit a leader \
                     in its box; no leader emitted",
                    text.id,
                    i + 1
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
            continue;
        }

        let count = (gap / leader_advance).floor() as usize;
        if let Some(run) = leader_run.as_ref() {
            let mut x = leader_start;
            for _ in 0..count {
                commands.push(SceneCommand::DrawGlyphRun {
                    x,
                    y: baseline_y,
                    font_id: run.font_id.clone(),
                    font_size: run.font_size,
                    color,
                    glyphs: run_to_scene_glyphs(run),
                });
                x += leader_advance;
            }
        }
    }

    if text_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    rows.len() as f64 * line_height
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
    footnote_markers: &BTreeMap<String, String>,
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

    // Resolve font family with style cascade.
    // Priority: node-local font_family → style font-family → default "Noto Sans".
    let font_family_prop = text
        .font_family
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "font-family"));
    let raw_family_name = resolve_font_family_name(font_family_prop, resolved, "Noto Sans");
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

    // ── Tab-leader mode (table-of-contents rows) ──────────────────────────
    // A self-contained render branch taken ONLY when `tab-leader` is set to a
    // non-empty string. The normal fast/wrap paths below are untouched (and so
    // remain byte-identical) when `tab_leader` is None/empty.
    if let Some(leader) = text.tab_leader.as_deref().filter(|s| !s.is_empty()) {
        return compile_tab_leader(
            text,
            leader,
            &families,
            font_size,
            node_fill_prop,
            node_weight_prop,
            node_opacity,
            resolved,
            engine,
            fonts,
            commands,
            diagnostics,
            text_x,
            text_y,
            ctx,
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
        text: String,
        weight: u16,
        style: FontStyle,
        font_size: f32,
        baseline_dy: f64,
        vertical_align: bool,
    }

    // ── Pass 1: shape ────────────────────────────────────────────────
    let mut shaped_spans: Vec<ShapedSpan> = Vec::new();
    let mut total_advance: f64 = 0.0;
    // Shared FULL-size ascent, captured from the first full-size (non
    // super/subscript) run. Super/subscript spans position against this shared
    // baseline + their `baseline_dy` so their reduced ascent does not move them
    // off the run's baseline. `None` until a full-size span is shaped.
    let mut node_ascent: Option<f64> = None;

    for span in &effective_spans {
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

        // Per-span super/subscript: a reduced size + a baseline shift relative
        // to the node's FULL font size. Absent → full size, zero shift.
        let (span_font_size, baseline_dy) =
            resolve_vertical_align(span.vertical_align.as_deref(), font_size);
        let is_vertical_align = baseline_dy != 0.0;

        let req = ShapeRequest {
            text: &span.text,
            families: &families,
            weight,
            style,
            font_size: span_font_size,
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
                        underline: span.underline == Some(true),
                        strikethrough: span.strikethrough == Some(true),
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
                    color: shaped.color,
                });
            }
            if shaped.strikethrough {
                commands.push(SceneCommand::FillRect {
                    x: x_cursor,
                    y: baseline_y - shaped.font_size as f64 * 0.30,
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
        let mut resolved_spans: Vec<ResolvedSpan> = shaped_spans
            .iter()
            .map(|s| ResolvedSpan {
                text: s.text.clone(),
                color: s.color,
                underline: s.underline,
                strikethrough: s.strikethrough,
                weight: s.weight,
                style: s.style,
                font_size: s.font_size,
                baseline_dy: s.baseline_dy,
            })
            .collect();

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
            &families,
            font_size,
            base_weight,
            engine,
            fonts,
            diagnostics,
            &text.id,
            text.source_span,
        );

        // Shape the cap now that the body `line_height` is known.
        let dropcap: Option<DropCap> = dropcap_initial.as_ref().and_then(|(init, n)| {
            shape_drop_cap(
                init,
                &families,
                base_weight,
                metrics.line_height,
                *n,
                engine,
                fonts,
            )
        });

        if let Some(cap) = &dropcap {
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
            fit_line_count = lines.len();

            // Drop-cap baseline: the cap's top sits at the box top, so its
            // baseline is one cap-ascent below `text_y`. Emit it ONCE at the
            // box left edge, in the node's resolved color/family.
            commands.push(SceneCommand::DrawGlyphRun {
                x: text_x,
                y: text_y + cap.ascent,
                font_id: cap.run.font_id.clone(),
                font_size: cap.run.font_size,
                color: cap.color,
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
                align,
                metrics,
                font_size,
                deco_thickness,
                false,
                commands,
            );
        } else {
            // Opt-in hyphenation: build a context only when `hyphenate=#true`
            // AND the embedded dictionary is available. Absent → `None` → the
            // packer is byte-identical to before.
            let hyph_ctx = if text.hyphenate == Some(true) {
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
            let lines = pack_lines(tokens, box_w, metrics.space_advance, hyph_ctx.as_ref());

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
                // Single-box wrap: the batch's last line IS the paragraph's last
                // line → leave it ragged under justify.
                false,
                commands,
            );
        }
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
                     (overflow=\"clip\"): needs ~{:.0}px height in {:.0}px box",
                    text.id, content_height, box_h
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
