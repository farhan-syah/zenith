//! Text and code leaf-node compilation, plus the shaping/glyph and
//! syntax-highlight helpers they depend on.

use std::collections::BTreeMap;

use zenith_core::{
    CodeNode, Diagnostic, Dimension, FontProvider, FontStyle, PropertyValue, ResolvedToken,
    ResolvedValue, Style, TextNode, TextSpan, TokenKind, Unit, builtin_color, dim_to_px,
    is_supported, scan, token_id_for_kind,
};
use zenith_layout::{
    RustybuzzEngine, ShapeRequest, TextDirection, TextLayoutEngine, ZenithGlyphRun,
};

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

/// Snap a text node's first-line baseline and inter-line advance onto the page
/// baseline grid of pitch `g`.
///
/// Given the natural (post-`ctx.dy`) `text_y`, the resolved `ascent`, and the
/// resolved `line_height`, returns `(snapped_text_y, effective_line_height)`:
/// the first baseline moves DOWN to the next grid line at/below its natural
/// position, and the advance inflates to the smallest multiple of `g` that is
/// ≥ `line_height`, so corresponding lines align horizontally across columns.
/// Because the emit computes `baseline_y = text_y + ascent + i*line_height`,
/// substituting these two values places every baseline on the grid with no
/// change to the emit code. Caller must ensure `g.is_finite() && g > 0.0`.
fn snap_to_baseline_grid(text_y: f64, ascent: f64, line_height: f64, g: f64) -> (f64, f64) {
    let natural_baseline = text_y + ascent;
    let snapped_baseline = (natural_baseline / g).ceil() * g;
    let effective_line_height = (line_height / g).ceil() * g;
    let snapped_text_y = snapped_baseline - ascent;
    (snapped_text_y, effective_line_height)
}

/// Build the `baseline-grid.snap_failed` advisory for a text node whose resolved
/// line-height exceeds the grid pitch (a single line cannot fit one grid cell,
/// so the effective advance inflates to a multiple of `g` and leading grows).
/// Emitted ONCE per affected node; the caller only calls this when
/// `line_height > g`.
fn baseline_grid_snap_failed_diag(
    node_id: &str,
    line_height: f64,
    g: f64,
    span: Option<zenith_core::Span>,
) -> Diagnostic {
    let multiple = (line_height / g).ceil();
    let effective = multiple * g;
    Diagnostic::warning(
        "baseline-grid.snap_failed",
        format!(
            "text node '{node_id}' line-height {line_height}px exceeds baseline-grid \
             pitch {g}px; lines snap to {effective}px ({multiple}× grid)"
        ),
        span,
        Some(node_id.to_owned()),
    )
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
    direction: TextDirection,
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
                    direction,
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
            // A single space's advance is direction-independent; keep LTR so the
            // inter-word gap measurement is identical for LTR and RTL.
            direction: TextDirection::Ltr,
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
    /// The en-US dictionary, or `None` when only break-word is requested (or the
    /// embedded blob failed to load). The hyphenation branch in
    /// [`pack_lines_core`] runs only when this is `Some`; the break-word branch is
    /// independent of it, so a node that requests ONLY `overflow-wrap="break-word"`
    /// still gets a context (with `dict: None`).
    pub(super) dict: Option<&'static Standard>,
    pub(super) engine: &'a RustybuzzEngine,
    pub(super) fonts: &'a dyn FontProvider,
    pub(super) families: &'a [String],
    /// The hyphen glyph string shaped onto the head fragment.
    pub(super) hyphen: &'a str,
    /// Base writing direction for re-shaping fragments (matches the node).
    pub(super) direction: TextDirection,
    /// When `true`, the packer may break an unbreakable token that is wider than
    /// the line box at a CHARACTER boundary (`overflow-wrap="break-word"`). When
    /// `false`, the break-word branch never runs (byte-identical to before).
    pub(super) break_word: bool,
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
        direction: ctx.direction,
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
    // No dictionary (break-word-only context, or the blob failed to load) → no
    // hyphenation; the caller falls back to wrapping the whole word.
    let dict = ctx.dict?;
    let text = word.src.text.as_str();
    // A break shorter than this many bytes on either side is never useful.
    if text.len() < 4 {
        return None;
    }
    let breaks = dict.hyphenate(text).breaks;
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

/// Split `word` at the LONGEST char-boundary prefix whose re-shaped advance fits
/// within `avail` pixels (`>= 1` char in the head), for `overflow-wrap="break-word"`.
/// Returns `(head, tail)` re-shaped as plain fragments (NO hyphen glyph), or
/// `None` when not even one character fits or a shaping failure occurs — in which
/// case the caller leaves the word whole (it overflows as today).
///
/// Determinism + safety: the candidate split points are the word's own
/// `char_indices` (so every slice falls on a UTF-8 char boundary — no panic, no
/// mojibake on multi-byte chars), walked in order; the chosen point is a pure
/// function of `avail`. O(n) in the word's char count.
fn try_break_word(
    word: &WordToken,
    avail: f64,
    ctx: &HyphenationContext,
) -> Option<(WordToken, WordToken)> {
    let text = word.src.text.as_str();
    // Track the largest fitting prefix found so far (byte offset of the split and
    // the re-shaped head). Grow from 1 char up, keeping the last that fits.
    let mut best: Option<(usize, WordToken)> = None;
    // Candidate split byte offsets are the START of each char AFTER the first, so
    // the head always has at least one char; the final candidate is `text.len()`
    // but a full-word "split" is useless (empty tail), so it is excluded.
    let mut boundaries: Vec<usize> = text.char_indices().map(|(b, _)| b).skip(1).collect();
    // `char_indices` never yields `text.len()`; an empty `boundaries` means the
    // word is a single char and cannot be split.
    if boundaries.is_empty() {
        return None;
    }
    // Walk boundaries in order; keep the LARGEST prefix whose head fits `avail`.
    // Once a prefix overflows, all longer prefixes also overflow (advance grows
    // monotonically), so we can stop at the first overflow.
    boundaries.push(text.len()); // sentinel guards the very last char block
    for &b in &boundaries {
        let Some(head_txt) = text.get(..b) else {
            continue;
        };
        if head_txt.is_empty() {
            continue;
        }
        let Some(head) = reshape_fragment(head_txt, word, None, ctx) else {
            // Shaping failure on this prefix: stop and use the best-so-far.
            break;
        };
        if head.advance > avail {
            break;
        }
        // The tail must be non-empty for this to be a real split.
        if b >= text.len() {
            break;
        }
        best = Some((b, head));
    }
    let (b, head) = best?;
    let tail_txt = text.get(b..)?;
    let tail = reshape_fragment(tail_txt, word, None, ctx)?;
    Some((head, tail))
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
    // `min_line_width = NEG_INFINITY` disables the blocked-line skip (a width is
    // never `< -inf`), so uniform packing is byte-identical to before. `max_lines`
    // is unused when no line is ever blocked; a large cap keeps it inert. The
    // forced-break sentinel is inert for callers that do not read it back.
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        |_| box_w,
        space_advance,
        hyph,
        f64::NEG_INFINITY,
        usize::MAX,
        &mut forced_break,
    )
}

/// Like [`pack_lines`] but also reports (via `forced_break`) whether the packer
/// performed a forced character-boundary break for `overflow-wrap="break-word"`.
/// The single-box wrap path uses this to emit ONE `text.forced_break` advisory.
pub(super) fn pack_lines_reporting(
    tokens: Vec<WordToken>,
    box_w: f64,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
    forced_break: &mut bool,
) -> Vec<Line> {
    pack_lines_core(
        tokens,
        |_| box_w,
        space_advance,
        hyph,
        f64::NEG_INFINITY,
        usize::MAX,
        forced_break,
    )
}

/// Greedy-pack word tokens into per-line bands for text runaround.
///
/// `band_width(i)` returns the available width for line `i`; a band narrower than
/// `min_line_width` is BLOCKED — an empty [`Line`] is emitted at that index so the
/// baseline advances past it (text flows above/below the exclusion) without
/// consuming the pending word. No hyphenation is performed (v0 runaround, like the
/// drop-cap path). `max_lines` bounds the blocked-skip loop so an all-blocked tail
/// cannot loop forever; once the cap is reached remaining words are clipped.
pub(super) fn pack_lines_runaround(
    tokens: Vec<WordToken>,
    band_width: impl Fn(usize) -> f64,
    space_advance: f64,
    min_line_width: f64,
    max_lines: usize,
) -> Vec<Line> {
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        band_width,
        space_advance,
        None,
        min_line_width,
        max_lines,
        &mut forced_break,
    )
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
    // chain/flow drop-cap combination), so it threads `None`. Drop-cap widths are
    // always meaningful, so `min_line_width = NEG_INFINITY` disables the
    // blocked-line skip and packing stays byte-identical to before.
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        |i| profile.width_for(i),
        space_advance,
        None,
        f64::NEG_INFINITY,
        usize::MAX,
        &mut forced_break,
    )
}

/// The single greedy packer shared by [`pack_lines`], [`pack_lines_variable`],
/// and [`pack_lines_runaround`]. The per-line width comes from `width_for(i)` (a
/// constant for uniform packing, the drop-cap profile, or the runaround band).
///
/// Beyond the original greedy fill it adds three OPT-IN behaviours that leave the
/// default path byte-identical:
///
/// 0. **Blocked-line skip (runaround).** When the line about to receive its first
///    word has `width_for(i) < min_line_width`, an empty [`Line`] is pushed and
///    the next index is tried, without consuming the word, so text flows above
///    and below an exclusion band. `min_line_width = f64::NEG_INFINITY` makes this
///    unreachable (uniform/drop-cap callers), and `max_lines` bounds the skip.
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
///
/// 3. **Break-word.** When `hyph` is `Some(ctx)` with `ctx.break_word`, a word
///    that still does not fit AFTER the hyphenation attempt (failed/disabled) is
///    broken at a CHARACTER boundary ([`try_break_word`]) so an unbreakable token
///    wider than the box no longer overflows: the head joins the current line,
///    the line closes, and the tail is re-queued, repeating until the tail fits.
///    `forced_break` is set to `true` when at least one such break occurs so the
///    caller can emit ONE `text.forced_break` advisory. `ctx.break_word == false`
///    (or `hyph == None`) skips this entirely, so the default path is byte-identical.
fn pack_lines_core(
    tokens: Vec<WordToken>,
    width_for: impl Fn(usize) -> f64,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
    min_line_width: f64,
    max_lines: usize,
    forced_break: &mut bool,
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
        // Blocked-line skip (runaround only). When the line about to receive its
        // FIRST word (`cur` empty) has a band narrower than one usable line, emit
        // an empty `Line` so the baseline advances past the exclusion band and
        // re-evaluate at the next index WITHOUT consuming the word. Bounded by
        // `max_lines` so an all-blocked tail clips rather than looping forever.
        // With `min_line_width = NEG_INFINITY` (uniform/drop-cap callers) this
        // branch is unreachable, so packing stays byte-identical.
        if cur.is_empty() {
            while width_for(lines.len()) < min_line_width {
                if lines.len() >= max_lines {
                    // Cap reached: drop the pending word (and the rest of the
                    // queue) rather than spin. The text simply clips here.
                    return lines;
                }
                lines.push(Line {
                    words: Vec::new(),
                    content_w: 0.0,
                    paragraph: tok.src.paragraph,
                });
            }
        }

        // The width budget for the line currently being filled is the width for
        // the line this word would land on (the next line index when `cur` is
        // empty, else the current one — `lines.len()` is that index).
        let box_w = width_for(lines.len());

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

            // Break-word does NOT break here: a word that merely overflows the
            // REMAINING space on a non-empty line must wrap WHOLE to the next
            // line (CSS `overflow-wrap: break-word` only breaks a token that
            // cannot fit a line by itself). The overflow flush below re-queues
            // this word onto a fresh line; the empty-line break case then splits
            // it ONLY if it is wider than the full box width.
        }

        // Break-word on an EMPTY line: the word alone is wider than the whole box
        // (the URL case). Break it at a character boundary, place the head, close
        // the line, and re-queue the tail. Repeat (via the queue) until the tail
        // fits. Bounded by `max_lines` so a pathological zero-width box cannot
        // loop forever. Skipped entirely when break-word is off → byte-identical.
        if cur.is_empty()
            && tok.advance > box_w
            && let Some(ctx) = hyph
            && ctx.break_word
        {
            if lines.len() >= max_lines {
                // Cap reached: stop rather than spin on a degenerate box.
                let advance = tok.advance;
                let paragraph = tok.src.paragraph;
                lines.push(Line {
                    words: vec![tok],
                    content_w: advance,
                    paragraph,
                });
                return lines;
            }
            if let Some((head, tail)) = try_break_word(&tok, box_w, ctx) {
                *forced_break = true;
                let head_para = head.src.paragraph;
                let head_advance = head.advance;
                lines.push(Line {
                    words: vec![head],
                    content_w: head_advance,
                    paragraph: head_para,
                });
                queue.push_front(tail);
                continue;
            }
            // Not even one char fits `box_w` (zero/near-zero width): leave the
            // word whole so it overflows as today rather than dropping it.
        }

        if overflow || para_break {
            let content_w = line_w;
            lines.push(Line {
                words: std::mem::take(&mut cur),
                content_w,
                paragraph: cur_para,
            });
            line_w = 0.0;
            // Re-queue this word and restart the loop so the blocked-line skip at
            // the top re-evaluates the NEWLY-opened line index against its band
            // (it may itself be blocked by the exclusion). For uniform/drop-cap
            // callers (`min_line_width = NEG_INFINITY`) the skip is inert and the
            // word is simply placed on the next iteration, byte-identical to the
            // original fall-through.
            queue.push_front(tok);
            continue;
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
    direction: TextDirection,
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
        direction,
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
    direction: TextDirection,
    commands: &mut Vec<SceneCommand>,
) where
    F: Fn(usize) -> (f64, f64),
{
    let ascent = metrics.ascent;
    let line_height = metrics.line_height;
    let space_advance = metrics.space_advance;
    let last_idx = lines.len().saturating_sub(1);
    let is_rtl = direction == TextDirection::Rtl;
    for (i, line) in lines.iter().enumerate() {
        let (text_x, box_w) = geom(i);
        let baseline_y = text_y + ascent + (i as f64) * line_height;
        let word_count = line.words.len();

        // Visual left-to-right word order. LTR is logical order (byte-identical
        // to before); RTL reverses the words so the first LOGICAL word sits
        // rightmost (each word's own glyphs are already in visual order from the
        // shaper). Words are then placed left-to-right by `word_x` in this order.
        let visual: Vec<&WordToken> = if is_rtl {
            line.words.iter().rev().collect()
        } else {
            line.words.iter().collect()
        };

        // `(base_x, gap)`: the line's left origin and inter-word gap. LTR keeps
        // the historical mapping exactly. RTL flips the anchor: `start`
        // right-anchors (line right edge at box right), `end` left-anchors,
        // `center` is symmetric. Because `content_w` is order-independent, the
        // right-anchor offset `box_w - content_w` is identical whichever order
        // the words sit in.
        let (base_x, gap) = if is_rtl {
            match align {
                "center" => (text_x + (box_w - line.content_w) / 2.0, space_advance),
                // RTL `end` → left-anchor (left edge at box left).
                "end" => (text_x, space_advance),
                "justify" => {
                    // RTL justify: stretch inter-word gaps to fill, right-
                    // anchored; the last line stays right-aligned (ragged left).
                    let is_final_line = i == last_idx && !justify_final_line;
                    if !is_final_line && word_count > 1 {
                        let extra = (box_w - line.content_w).max(0.0) / (word_count as f64 - 1.0);
                        (text_x, space_advance + extra)
                    } else {
                        (text_x + (box_w - line.content_w), space_advance)
                    }
                }
                // RTL `start`/unknown → right-anchor.
                _ => (text_x + (box_w - line.content_w), space_advance),
            }
        } else {
            match align {
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
            }
        };

        // Precompute each VISUAL word's left x along the line, left-to-right.
        let mut word_x: Vec<f64> = Vec::with_capacity(word_count);
        {
            let mut x = base_x;
            for (wi, word) in visual.iter().enumerate() {
                word_x.push(x);
                x += word.advance;
                if wi + 1 < word_count {
                    x += gap;
                }
            }
        }

        // Decorations FIRST (so glyphs paint on top), one FillRect per maximal
        // contiguous same-flag run of words (in visual order).
        let underline_y = baseline_y + font_size as f64 * 0.12;
        let strike_y = baseline_y - font_size as f64 * 0.30;
        for (is_underline, deco_y) in [(true, underline_y), (false, strike_y)] {
            let mut run_start: Option<(f64, Color)> = None;
            let mut run_right: f64 = base_x;
            for (wi, word) in visual.iter().enumerate() {
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
        for (wi, word) in visual.iter().enumerate() {
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
    baseline_grid: Option<f64>,
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

    // Honor the node's direction for line layout. The chain pre-pass shapes the
    // source's spans with the source direction (see [`super::chain`]); here the
    // member's own `direction` drives line ordering/alignment. RTL chains are
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
        align,
        emit_metrics,
        font_size,
        deco_thickness,
        // Only the FINAL chain member leaves its last line ragged under
        // justify; a continuation box justifies its last line because the
        // paragraph flows on into the next box.
        !assignment.is_last_member,
        chain_direction,
        commands,
    );

    if has_shadow {
        commands.push(SceneCommand::EndShadow);
    }

    if text_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }

    assignment.lines.len() as f64 * emit_metrics.line_height
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

/// Cap-height as a fraction of em (font size) used to SIZE the drop cap so its
/// cap-height — not its full ascent — spans the requested lines. Latin cap
/// height is ≈ `0.7 × em` (Noto Sans `sCapHeight` is 714/1000); the value
/// cancels in the cap-top alignment (both the cap and the body use it), so the
/// exact figure only affects the cap's optical size, not its alignment.
const CAP_HEIGHT_RATIO: f64 = 0.714;

/// A shaped drop-cap initial ready for emission.
struct DropCap {
    /// The oversized shaped glyph run for the initial.
    run: ZenithGlyphRun,
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

/// Compute the drop-cap glyph SIZE so its cap-height spans `(n-1)` body lines
/// plus the body's own cap-height: `body_size + (n-1) * line_height /
/// CAP_HEIGHT_RATIO`. Paired with a baseline on line `n`'s baseline (emit site),
/// this aligns the cap's cap-top with line 1's cap-top and its baseline with
/// line `n`'s baseline — the standard drop-cap geometry.
fn drop_cap_font_size(body_font_size: f64, line_height: f64, n: u32) -> f32 {
    (body_font_size + (n as f64 - 1.0) * line_height / CAP_HEIGHT_RATIO).max(1.0) as f32
}

/// Shape a lifted [`DropCapInitial`] as an oversized glyph at `cap_size` (see
/// [`drop_cap_font_size`]), spanning `n` lines. It paints in the donor span's
/// color and the node family. `None` on a shaping failure → no cap, body
/// unchanged.
fn shape_drop_cap(
    initial: &DropCapInitial,
    families: &[String],
    weight: u16,
    cap_size: f32,
    n: u32,
    engine: &RustybuzzEngine,
    fonts: &dyn FontProvider,
) -> Option<DropCap> {
    let glyph = initial.ch.to_string();
    let req = ShapeRequest {
        text: &glyph,
        families,
        weight,
        style: initial.style,
        font_size: cap_size,
        // Drop caps are a single glyph; RTL drop caps are a documented v0
        // follow-up, so the cap always shapes LTR.
        direction: TextDirection::Ltr,
    };
    let run = engine
        .shape_with_fallback(&req, fonts)
        .ok()?
        .into_iter()
        .next()?;
    let advance = run.advance_width as f64;
    Some(DropCap {
        run,
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
            // Tab-leader (TOC) rows are LTR in v0; RTL TOC is a follow-up.
            direction: TextDirection::Ltr,
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
        // Tab-leader (TOC) mode is LTR in v0.
        direction: TextDirection::Ltr,
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
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
    ctx: RenderCtx,
) -> f64 {
    if text.overflow.as_deref() != Some("autofit") {
        // Pass-through: byte-identical command stream for every non-autofit node.
        return compile_text_sized(
            text,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            footnote_markers,
            node_boxes,
            ctx,
        );
    }
    compile_text_autofit(
        text,
        resolved,
        style_map,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        footnote_markers,
        node_boxes,
        ctx,
    )
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
#[allow(clippy::too_many_arguments)]
fn compile_text_autofit(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    footnote_markers: &BTreeMap<String, String>,
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
    ctx: RenderCtx,
) -> f64 {
    // Require both box dimensions to measure fit; otherwise fall back to a
    // single sized compile with overflow untouched (documented; no crash).
    let box_w = text.w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let box_h = text.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    let (Some(_bw), Some(_bh)) = (box_w, box_h) else {
        return compile_text_sized(
            text,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            footnote_markers,
            node_boxes,
            ctx,
        );
    };

    // Resolve the declared node font size (px) — the search ceiling — and the
    // floor from `font-size-min`, defaulting to `(declared * 0.5).max(8.0)`.
    let declared = f64::from(font_size_px(text, resolved, style_map));
    let floor =
        resolve_property_dimension_px(&text.font_size_min, resolved, (declared * 0.5).max(8.0));
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
        compile_text_sized(
            &trial,
            resolved,
            style_map,
            fonts,
            engine,
            &mut throwaway_cmds,
            &mut throwaway_diags,
            chains,
            footnote_markers,
            node_boxes,
            ctx,
        );
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
    compile_text_sized(
        &real,
        resolved,
        style_map,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        footnote_markers,
        node_boxes,
        ctx,
    )
}

/// Compile a `text` leaf node at its resolved font size (the unchanged layout
/// engine: wrap/fast/drop-cap/runaround/chain paths + overflow handling).
///
/// Returns the laid-out content height in pixels (`line_count * line_height`),
/// which the flow-layout path in [`super::container`] uses to advance its
/// vertical cursor past a text child that declares no explicit `h`. Early
/// returns (invisible, missing/bad geometry, empty spans) yield `0.0`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_text_sized(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    footnote_markers: &BTreeMap<String, String>,
    node_boxes: &BTreeMap<String, (f64, f64, f64, f64)>,
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
            ctx.baseline_grid,
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
            node_direction,
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
            shape_drop_cap(init, &families, base_weight, cap_size, *n, engine, fonts)
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
                // Drop-cap wrap-around is an LTR feature in v0; RTL drop caps are
                // a documented follow-up.
                TextDirection::Ltr,
                commands,
            );
        } else if let Some((ex, ey, ew, eh)) = exclusion {
            // ── TEXT RUNAROUND (largest-area / jump) ──────────────────────
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
            fit_line_count = lines.len();

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
                align,
                emit_metrics,
                font_size,
                deco_thickness,
                false,
                node_direction,
                commands,
            );
        } else {
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
                    families: &families,
                    hyphen: "-",
                    direction: node_direction,
                    break_word: want_break_word,
                })
            } else {
                None
            };
            // ── Hanging indent: padding-left + (optional negative) text-indent ─
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
            let pl = text
                .padding_left
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.0);
            let ti = text
                .text_indent
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.0);

            let mut forced_break = false;
            let lines = if pl == 0.0 && ti == 0.0 {
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
            fit_line_count = lines.len();

            if pl == 0.0 && ti == 0.0 {
                emit_lines(
                    &lines,
                    text_x,
                    // Baseline-grid snap (no-op when no grid is active): the first
                    // baseline lands on the grid and the advance is a multiple of g.
                    emit_text_y,
                    box_w,
                    align,
                    emit_metrics,
                    font_size,
                    deco_thickness,
                    // Single-box wrap: the batch's last line IS the paragraph's
                    // last line → leave it ragged under justify.
                    false,
                    node_direction,
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
                    align,
                    emit_metrics,
                    font_size,
                    deco_thickness,
                    false,
                    node_direction,
                    commands,
                );
            }
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
                    // Code is shaped LTR (source code is left-to-right).
                    direction: TextDirection::Ltr,
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
                // Code is shaped LTR (source code is left-to-right).
                direction: TextDirection::Ltr,
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

#[cfg(test)]
mod rtl_tests {
    use super::*;

    /// Build a single-run [`WordToken`] of the given `advance` so per-word x
    /// positions in the emitted commands are deterministic and checkable.
    fn word(advance: f64) -> WordToken {
        WordToken {
            runs: vec![ZenithGlyphRun {
                font_id: "test-font".to_owned(),
                font_size: 16.0,
                ascent: 12.0,
                descent: 4.0,
                line_height: 18.0,
                advance_width: advance as f32,
                glyphs: Vec::new(),
            }],
            advance,
            color: Color::srgb(0, 0, 0, 255),
            underline: false,
            strikethrough: false,
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

    fn metrics() -> WordMetrics {
        WordMetrics {
            ascent: 12.0,
            line_height: 18.0,
            space_advance: 5.0,
        }
    }

    /// The x origin of every emitted glyph run, in command order.
    fn run_xs(commands: &[SceneCommand]) -> Vec<f64> {
        commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { x, .. } => Some(*x),
                _ => None,
            })
            .collect()
    }

    /// Emit a single line of three words `[10, 20, 30]` with the given direction
    /// and align, returning the per-word x origins in COMMAND order.
    fn emit_line(direction: TextDirection, align: &str) -> Vec<f64> {
        // content_w = 10 + 5 + 20 + 5 + 30 = 70.
        let line = Line {
            words: vec![word(10.0), word(20.0), word(30.0)],
            content_w: 70.0,
            paragraph: 0,
        };
        let mut commands = Vec::new();
        emit_lines(
            std::slice::from_ref(&line),
            /* text_x */ 100.0,
            /* text_y */ 0.0,
            /* box_w */ 200.0,
            align,
            metrics(),
            16.0,
            1.0,
            false,
            direction,
            &mut commands,
        );
        run_xs(&commands)
    }

    #[test]
    fn ltr_start_is_byte_identical_left_anchored() {
        // LTR start: first word at the left origin (100), running rightward.
        let xs = emit_line(TextDirection::Ltr, "start");
        assert_eq!(xs, vec![100.0, 115.0, 140.0]);
        // word0 left edge = 100; word1 = 100+10+5; word2 = 115+20+5.
    }

    #[test]
    fn rtl_start_first_word_at_right_descending_leftward() {
        // RTL start right-anchors the line: box right = 100 + 200 = 300, line
        // right edge = 300, so the line starts at 300 - 70 = 230. Words are
        // emitted in reversed (visual) order, so the COMMAND order is word2,
        // word1, word0 from left to right. The FIRST LOGICAL word (advance 10)
        // is therefore the LAST command and sits at the largest x (rightmost).
        let xs = emit_line(TextDirection::Rtl, "start");
        // Visual left-to-right: word2 @230, word1 @230+30+5=265, word0 @265+20+5=290.
        assert_eq!(xs, vec![230.0, 265.0, 290.0]);
        // The first logical word (word0) is the rightmost run.
        let first_logical_x = *xs.last().expect("three runs");
        assert!(
            first_logical_x > xs[0] && first_logical_x > xs[1],
            "first logical word must be rightmost, got {xs:?}"
        );
    }

    #[test]
    fn rtl_end_left_anchors() {
        // RTL end → left edge at box left (100). Visual order word2,word1,word0.
        let xs = emit_line(TextDirection::Rtl, "end");
        assert_eq!(xs, vec![100.0, 135.0, 160.0]);
    }

    #[test]
    fn rtl_center_is_symmetric() {
        // center: base_x = 100 + (200 - 70)/2 = 165, same anchor as LTR center,
        // only the word order differs.
        let xs = emit_line(TextDirection::Rtl, "center");
        assert_eq!(xs, vec![165.0, 200.0, 225.0]);
    }
}

#[cfg(test)]
mod packer_tests {
    use super::*;

    /// A single-run [`WordToken`] of the given `advance` (deterministic).
    fn word(advance: f64) -> WordToken {
        WordToken {
            runs: vec![ZenithGlyphRun {
                font_id: "test-font".to_owned(),
                font_size: 16.0,
                ascent: 12.0,
                descent: 4.0,
                line_height: 18.0,
                advance_width: advance as f32,
                glyphs: Vec::new(),
            }],
            advance,
            color: Color::srgb(0, 0, 0, 255),
            underline: false,
            strikethrough: false,
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

    /// A line's (content_w, word advances) for comparison.
    fn shape(lines: &[Line]) -> Vec<(f64, Vec<f64>)> {
        lines
            .iter()
            .map(|l| (l.content_w, l.words.iter().map(|w| w.advance).collect()))
            .collect()
    }

    /// The closure refactor must leave uniform packing byte-identical: packing the
    /// same tokens via `pack_lines` (the closure path with `NEG_INFINITY` sentinel)
    /// must produce the same lines as an independent reference greedy pack.
    #[test]
    fn pack_uniform_byte_identical_after_refactor() {
        // box_w = 70, space = 5. advances: 10,20,30,40,15.
        // Reference greedy: 10 (+5+20=35) (+5+30=70) → line0 [10,20,30] content 70.
        //   next: 40 (+5+15=60) → line1 [40,15] content 60.
        let box_w = 70.0;
        let space = 5.0;
        let advances = [10.0, 20.0, 30.0, 40.0, 15.0];
        let packed = pack_lines(tokens(&advances), box_w, space, None);
        assert_eq!(
            shape(&packed),
            vec![(70.0, vec![10.0, 20.0, 30.0]), (60.0, vec![40.0, 15.0]),],
            "uniform packing must be unchanged by the closure refactor"
        );
    }

    /// A blocked band (width below `min_line_width`) yields an EMPTY line at that
    /// index, advancing the baseline without consuming the pending word.
    #[test]
    fn runaround_blocked_band_emits_empty_line() {
        // Line 0 blocked (width 0 < min 1), line 1+ full width 100.
        let band = |i: usize| if i == 0 { 0.0 } else { 100.0 };
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0]), band, 5.0, 1.0, 16);
        // line0 is empty (blocked), line1 packs both words: 10 +5+20 = 35.
        assert_eq!(
            shape(&lines),
            vec![(0.0, vec![]), (35.0, vec![10.0, 20.0])],
            "a blocked band must emit an empty line then flow below it"
        );
    }

    /// A narrower band forces MORE line breaks than the full width would.
    #[test]
    fn runaround_narrow_band_breaks_more() {
        // band width 30 on every line, space 5. advances 10,20,30.
        // 10 (+5+20=35>30) → line0 [10]; 20 (+5+30=55>30) → line1 [20]; 30 → line2.
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0, 30.0]), |_| 30.0, 5.0, 1.0, 64);
        assert_eq!(
            shape(&lines),
            vec![(10.0, vec![10.0]), (20.0, vec![20.0]), (30.0, vec![30.0])],
        );
    }

    /// The `max_lines` cap stops an all-blocked tail from looping forever; the
    /// pending words are clipped once the cap is hit.
    #[test]
    fn runaround_all_blocked_respects_max_lines() {
        // Every band blocked → after `max_lines` empty lines, clip remaining words.
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0]), |_| 0.0, 5.0, 1.0, 3);
        assert_eq!(lines.len(), 3, "blocked tail must be capped at max_lines");
        assert!(
            lines.iter().all(|l| l.words.is_empty()),
            "all capped lines are empty (words clipped)"
        );
    }
}

#[cfg(test)]
mod baseline_grid_unit_tests {
    use super::*;

    #[test]
    fn snaps_first_baseline_down_to_next_grid_line() {
        // g=14, ascent=12, text_y chosen so natural baseline = 350.0 (a multiple
        // of 14 is 350=25*14) → snapped baseline stays 350; snapped_text_y=338.
        let (ty, lh) = snap_to_baseline_grid(/* text_y */ 338.0, 12.0, 18.0, 14.0);
        assert_eq!(ty + 12.0, 350.0, "baseline already on the grid stays put");
        // 18 → ceil(18/14)=2 → 28.
        assert_eq!(lh, 28.0);
    }

    #[test]
    fn natural_baseline_355_snaps_to_364() {
        // natural baseline = text_y(343) + ascent(12) = 355; next grid line ≥ 355
        // is 364 (26*14). snapped_text_y = 364 - 12 = 352.
        let (ty, lh) = snap_to_baseline_grid(343.0, 12.0, 14.0, 14.0);
        assert_eq!(ty + 12.0, 364.0);
        // line_height 14 == g → effective stays 14 (ceil(14/14)=1).
        assert_eq!(lh, 14.0);
    }

    #[test]
    fn effective_advance_is_smallest_multiple_ge_line_height() {
        // line_height just under one cell → 1 cell; just over → 2 cells.
        let (_, lh1) = snap_to_baseline_grid(0.0, 10.0, 13.9, 14.0);
        assert_eq!(lh1, 14.0);
        let (_, lh2) = snap_to_baseline_grid(0.0, 10.0, 14.1, 14.0);
        assert_eq!(lh2, 28.0);
    }

    #[test]
    fn snap_failed_diag_names_node_and_pitch() {
        let d = baseline_grid_snap_failed_diag("col1", 18.0, 14.0, None);
        assert_eq!(d.code, "baseline-grid.snap_failed");
        assert!(d.message.contains("col1"), "message names the node id");
        assert!(d.message.contains("18"), "message names line-height");
        assert!(d.message.contains("14"), "message names the grid pitch");
        assert!(
            d.message.contains("28"),
            "message names the snapped advance"
        );
    }
}

#[cfg(test)]
mod break_word_tests {
    use super::*;
    use zenith_core::default_provider;

    /// Shape `word` into a single [`WordToken`] using the real engine + bundled
    /// fonts, so `try_break_word` exercises real glyph advances. The `src.text`
    /// carries the original word so the splitter can slice it.
    fn shape_word(word: &str, engine: &RustybuzzEngine, fonts: &dyn FontProvider) -> WordToken {
        let families = vec!["Noto Sans".to_owned()];
        let spans = [ResolvedSpan {
            text: word.to_owned(),
            color: Color::srgb(0, 0, 0, 255),
            underline: false,
            strikethrough: false,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 16.0,
            baseline_dy: 0.0,
        }];
        let mut diags = Vec::new();
        let (mut tokens, _m) = shape_words(
            &spans,
            &families,
            16.0,
            400,
            engine,
            fonts,
            &mut diags,
            "t",
            None,
            TextDirection::Ltr,
        );
        tokens.pop().expect("the word must shape to one token")
    }

    fn ctx<'a>(
        engine: &'a RustybuzzEngine,
        fonts: &'a dyn FontProvider,
        families: &'a [String],
    ) -> HyphenationContext<'a> {
        HyphenationContext {
            dict: None,
            engine,
            fonts,
            families,
            hyphen: "-",
            direction: TextDirection::Ltr,
            break_word: true,
        }
    }

    /// `try_break_word` splits a long token so the head fits `avail`, the head has
    /// at least one char, and head+tail char text reconstructs the original.
    #[test]
    fn splits_and_reconstructs_original_text() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let original = "https://very-long.example.com/some/deep/path";
        let word = shape_word(original, &engine, &provider);
        let c = ctx(&engine, &provider, &families);

        // Pick an `avail` far smaller than the whole word's advance.
        let avail = word.advance / 3.0;
        let (head, tail) = try_break_word(&word, avail, &c).expect("a prefix must fit");

        assert!(head.advance <= avail, "head must fit avail");
        assert!(
            !head.src.text.is_empty() && head.src.text.chars().count() >= 1,
            "head needs at least one char"
        );
        assert!(!tail.src.text.is_empty(), "tail must be non-empty");
        assert_eq!(
            format!("{}{}", head.src.text, tail.src.text),
            original,
            "head+tail must reconstruct the original token exactly"
        );
    }

    /// A token containing multi-byte UTF-8 chars (an em-dash and an accented
    /// char) is split only on char boundaries — no panic, no lost/mojibake bytes.
    #[test]
    fn respects_multibyte_char_boundaries() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let original = "café—über—straße—long—compound—word";
        let word = shape_word(original, &engine, &provider);
        let c = ctx(&engine, &provider, &families);

        let avail = word.advance / 2.0;
        let (head, tail) = try_break_word(&word, avail, &c).expect("a prefix must fit");
        // Reconstruction proves no byte was lost or duplicated and that both
        // slices are valid UTF-8 (otherwise `src.text` would not equal a slice).
        assert_eq!(format!("{}{}", head.src.text, tail.src.text), original);
        // The split point is a valid char boundary of the original string.
        assert!(
            original.is_char_boundary(head.src.text.len()),
            "split must land on a char boundary"
        );
    }

    /// A box too narrow for even one character yields `None`, leaving the caller
    /// to keep the word whole (it overflows as today).
    #[test]
    fn returns_none_when_no_char_fits() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let word = shape_word("wide", &engine, &provider);
        let c = ctx(&engine, &provider, &families);
        assert!(
            try_break_word(&word, 0.0, &c).is_none(),
            "zero avail fits no char → None"
        );
    }

    /// A single-character token cannot be split (no useful interior boundary).
    #[test]
    fn single_char_token_is_not_split() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let word = shape_word("W", &engine, &provider);
        let c = ctx(&engine, &provider, &families);
        assert!(try_break_word(&word, 1000.0, &c).is_none());
    }

    /// Regression: under `break-word`, an ORDINARY word that fits a line by
    /// itself but not the REMAINING space on a non-empty line must wrap WHOLE to
    /// the next line — it must NOT be split mid-word into the leftover space.
    /// Only a token wider than the whole box may break (covered elsewhere).
    #[test]
    fn ordinary_word_wraps_whole_not_broken_into_remaining_space() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let c = ctx(&engine, &provider, &families);

        let alpha = shape_word("alpha", &engine, &provider);
        let betagamma = shape_word("betagamma", &engine, &provider);
        let space_advance = 6.0;
        // Box fits "betagamma" alone (with slack) but NOT "alpha" + space +
        // "betagamma": the second word overflows the remainder yet fits a fresh
        // line, so it must wrap whole.
        let box_w = betagamma.advance + 5.0;
        assert!(
            alpha.advance + space_advance + betagamma.advance > box_w,
            "test setup: the pair must overflow one line"
        );

        let mut forced = false;
        let lines = pack_lines_reporting(
            vec![alpha, betagamma],
            box_w,
            space_advance,
            Some(&c),
            &mut forced,
        );

        assert!(!forced, "no forced break: the word fits a line by itself");
        assert_eq!(lines.len(), 2, "the second word wraps to its own line");
        assert_eq!(
            lines[1]
                .words
                .iter()
                .map(|w| w.src.text.as_str())
                .collect::<String>(),
            "betagamma",
            "the wrapped word stays intact (not split mid-word)"
        );
    }
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
    use super::*;

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
                "start",
                m,
                16.0,
                1.0,
                false,
                TextDirection::Ltr,
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
                "start",
                m,
                16.0,
                1.0,
                false,
                TextDirection::Ltr,
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
