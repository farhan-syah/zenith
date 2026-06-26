//! Greedy line packing. A single core packer ([`pack_lines_core`]) is shared by
//! the uniform wrap path, the drop-cap variable-width profile, and the runaround
//! band path; the public wrappers thread the right per-line width source and the
//! opt-in hyphenation/break-word behaviours into it.

use super::hyphen::{HyphenationContext, try_break_word, try_hyphenate};
use super::shape::WordToken;

/// Line-level scalar metrics threaded into the core packer.
///
/// Bundled into a single `Copy` struct so [`pack_lines_core`] stays under the
/// argument-count limit enforced by `clippy::too_many_arguments`.
#[derive(Clone, Copy)]
pub(in crate::compile) struct LineMetrics {
    /// Width of a single inter-word space in pixels.
    pub(in crate::compile) space_advance: f64,
    /// Minimum usable band width for runaround (set to `f64::NEG_INFINITY` on
    /// the uniform/drop-cap paths to make the blocked-line skip unreachable).
    pub(in crate::compile) min_line_width: f64,
    /// Vertical advance stored in every emitted [`Line`].
    pub(in crate::compile) line_height: f64,
}

/// Per-line style scalars that override the node-global [`EmitStyle`] values
/// when `Some`. When `None` (the default for every existing path), the emit
/// loop falls back to the node-global values and produces byte-identical output.
///
/// Used by the next unit to vary ascent/spacing per heading level inside one
/// flow without touching any existing path.
#[derive(Clone, Copy)]
pub(in crate::compile) struct LineStyle {
    pub(in crate::compile) ascent: f64,
    pub(in crate::compile) space_advance: f64,
    pub(in crate::compile) font_size: f32,
    pub(in crate::compile) deco_thickness: f64,
}

/// One packed line: its words plus the summed content width (no trailing space).
pub(in crate::compile) struct Line {
    pub(in crate::compile) words: Vec<WordToken>,
    pub(in crate::compile) content_w: f64,
    /// The 0-based paragraph index this line belongs to, taken from its first
    /// word. Used by widow/orphan control in the chain distributor; the
    /// single-box path ignores it. A line is always within ONE paragraph because
    /// the packer never mixes words from different paragraphs onto one line.
    pub(in crate::compile) paragraph: usize,
    /// The vertical advance (height) for THIS line in pixels. Currently always
    /// set to the uniform `WordMetrics.line_height` at construction, making
    /// cumulative-height stacking identical to `index * line_height`. A later
    /// unit will vary this per-line (e.g. markdown headings vs body in one flow)
    /// without changing any existing output.
    pub(in crate::compile) height_px: f64,
    /// Per-line style override. `None` (the default) means use the node-global
    /// [`EmitStyle`] values; `Some(ls)` substitutes `ls` for this line's
    /// ascent, space_advance, font_size, and deco_thickness. Byte-identical
    /// when `None` for every line (all existing paths).
    pub(in crate::compile) line_style: Option<LineStyle>,
}

/// Greedy-pack word tokens into lines for a given box width, left-to-right and
/// deterministic. Identical algorithm to the original inline wrap packer when
/// `hyph` is `None`; passing a [`HyphenationContext`] enables word splitting at
/// embedded break points (see [`pack_lines_core`]).
pub(in crate::compile) fn pack_lines(
    tokens: Vec<WordToken>,
    box_w: f64,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
    line_height: f64,
) -> Vec<Line> {
    // `min_line_width = NEG_INFINITY` disables the blocked-line skip (a width is
    // never `< -inf`), so uniform packing is byte-identical to before. `max_lines`
    // is unused when no line is ever blocked; a large cap keeps it inert. The
    // forced-break sentinel is inert for callers that do not read it back.
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        |_| box_w,
        LineMetrics {
            space_advance,
            min_line_width: f64::NEG_INFINITY,
            line_height,
        },
        hyph,
        usize::MAX,
        &mut forced_break,
    )
}

/// Like [`pack_lines`] but also reports (via `forced_break`) whether the packer
/// performed a forced character-boundary break for `overflow-wrap="break-word"`.
/// The single-box wrap path uses this to emit ONE `text.forced_break` advisory.
pub(in crate::compile) fn pack_lines_reporting(
    tokens: Vec<WordToken>,
    box_w: f64,
    space_advance: f64,
    hyph: Option<&HyphenationContext>,
    forced_break: &mut bool,
    line_height: f64,
) -> Vec<Line> {
    pack_lines_core(
        tokens,
        |_| box_w,
        LineMetrics {
            space_advance,
            min_line_width: f64::NEG_INFINITY,
            line_height,
        },
        hyph,
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
pub(in crate::compile) fn pack_lines_runaround(
    tokens: Vec<WordToken>,
    band_width: impl Fn(usize) -> f64,
    space_advance: f64,
    min_line_width: f64,
    max_lines: usize,
    line_height: f64,
) -> Vec<Line> {
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        band_width,
        LineMetrics {
            space_advance,
            min_line_width,
            line_height,
        },
        None,
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
pub(in crate::compile) struct WidthProfile {
    pub(in crate::compile) narrow_w: f64,
    pub(in crate::compile) narrow_count: usize,
    pub(in crate::compile) full_w: f64,
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
pub(in crate::compile) fn pack_lines_variable(
    tokens: Vec<WordToken>,
    profile: WidthProfile,
    space_advance: f64,
    line_height: f64,
) -> Vec<Line> {
    // The drop-cap path does not hyphenate (a documented v0 follow-up like the
    // chain/flow drop-cap combination), so it threads `None`. Drop-cap widths are
    // always meaningful, so `min_line_width = NEG_INFINITY` disables the
    // blocked-line skip and packing stays byte-identical to before.
    let mut forced_break = false;
    pack_lines_core(
        tokens,
        |i| profile.width_for(i),
        LineMetrics {
            space_advance,
            min_line_width: f64::NEG_INFINITY,
            line_height,
        },
        None,
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
///    ([`super::shape::WordSource::paragraph`]); a word whose paragraph differs
///    from the line being filled forces a line break, so a line never mixes
///    paragraphs and every [`Line::paragraph`] is well-defined. A single-paragraph
///    document (every index 0) never triggers this, so output is unchanged.
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
pub(in crate::compile) fn pack_lines_core(
    tokens: Vec<WordToken>,
    width_for: impl Fn(usize) -> f64,
    metrics: LineMetrics,
    hyph: Option<&HyphenationContext>,
    max_lines: usize,
    forced_break: &mut bool,
) -> Vec<Line> {
    let LineMetrics {
        space_advance,
        min_line_width,
        line_height,
    } = metrics;
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
                    height_px: line_height,
                    line_style: None,
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

        // The inter-word gap BEFORE this word on a non-empty line: zero when the
        // word is GLUED to its predecessor (source-adjacent, no whitespace) so it
        // sits flush against it; otherwise the normal `space_advance`. A glued
        // word never opens a line with a gap (the `cur.is_empty()` cases below all
        // use a zero gap regardless). For non-glued words this is `space_advance`,
        // byte-identical to before.
        let lead_gap = if tok.glued { 0.0 } else { space_advance };

        let overflow = !cur.is_empty() && line_w + lead_gap + tok.advance > box_w;

        if overflow && !para_break {
            // Try to hyphenate the word into the remaining space before wrapping.
            // `avail` is the width left on the current line after a space gap.
            if let Some(ctx) = hyph {
                let avail = box_w - line_w - lead_gap;
                if avail > 0.0
                    && let Some(split) = try_hyphenate(&tok, avail, ctx)
                {
                    // Head + hyphen joins the current line; close the line. The
                    // head inherits the word's glue, so the gap before it matches
                    // `lead_gap` (zero for a glued word, `space_advance` otherwise).
                    line_w += lead_gap + split.head.advance;
                    cur.push(split.head);
                    lines.push(Line {
                        words: std::mem::take(&mut cur),
                        content_w: line_w,
                        paragraph: cur_para,
                        height_px: line_height,
                        line_style: None,
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
                    height_px: line_height,
                    line_style: None,
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
                    height_px: line_height,
                    line_style: None,
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
                height_px: line_height,
                line_style: None,
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
        // A line-opening word contributes no leading gap; otherwise use the
        // glue-aware `lead_gap` (zero when glued, `space_advance` otherwise).
        let gap = if cur.is_empty() { 0.0 } else { lead_gap };
        line_w += gap + tok.advance;
        cur.push(tok);
    }
    if !cur.is_empty() {
        lines.push(Line {
            words: cur,
            content_w: line_w,
            paragraph: cur_para,
            height_px: line_height,
            line_style: None,
        });
    }
    lines
}

#[cfg(test)]
mod packer_tests {
    use super::*;
    use zenith_core::FontStyle;
    use zenith_layout::ZenithGlyphRun;

    use super::super::shape::{WordSource, WordToken};
    use crate::ir::Color;

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
            highlight: None,
            code: false,
            link: None,
            baseline_dy: 0.0,
            glued: false,
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
        let packed = pack_lines(tokens(&advances), box_w, space, None, 18.0);
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
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0]), band, 5.0, 1.0, 16, 18.0);
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
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0, 30.0]), |_| 30.0, 5.0, 1.0, 64, 18.0);
        assert_eq!(
            shape(&lines),
            vec![(10.0, vec![10.0]), (20.0, vec![20.0]), (30.0, vec![30.0])],
        );
    }

    /// When every line carries the same `height_px` (the uniform case), accumulating
    /// `height_px` left-to-right produces the same offset as `index * line_height`.
    /// This is the byte-identity guarantee for the emit / chain height-cut
    /// refactor: as long as `height_px` is always set to the uniform `line_height`,
    /// the cumulative-sum path and the old multiplication path agree exactly.
    #[test]
    fn uniform_height_px_cumulative_equals_index_times_line_height() {
        // Use a representative line_height. All packed lines must carry it.
        let line_height = 18.0_f64;
        let space = 5.0;
        let advances = [10.0_f64, 20.0, 30.0, 15.0, 25.0];
        let lines = pack_lines(tokens(&advances), 70.0, space, None, line_height);
        assert!(
            !lines.is_empty(),
            "test requires at least one line to be meaningful"
        );
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(
                line.height_px, line_height,
                "line {i}: height_px must equal the uniform line_height"
            );
            // Cumulative offset = sum of height_px for lines 0..i.
            let cumulative: f64 = lines[..i].iter().map(|l| l.height_px).sum();
            let by_index = (i as f64) * line_height;
            assert_eq!(
                cumulative, by_index,
                "line {i}: cumulative sum ({cumulative}) must equal index*line_height ({by_index})"
            );
        }
    }

    /// The `max_lines` cap stops an all-blocked tail from looping forever; the
    /// pending words are clipped once the cap is hit.
    #[test]
    fn runaround_all_blocked_respects_max_lines() {
        // Every band blocked → after `max_lines` empty lines, clip remaining words.
        let lines = pack_lines_runaround(tokens(&[10.0, 20.0]), |_| 0.0, 5.0, 1.0, 3, 18.0);
        assert_eq!(lines.len(), 3, "blocked tail must be capped at max_lines");
        assert!(
            lines.iter().all(|l| l.words.is_empty()),
            "all capped lines are empty (words clipped)"
        );
    }
}
