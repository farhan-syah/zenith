//! Line emission: decoration FillRects + DrawGlyphRun commands for a sequence of
//! packed lines, stacked by line height with per-line horizontal alignment. A
//! single profiled body ([`emit_lines_profiled`]) drives both the uniform
//! single-width emit ([`emit_lines`]) and the per-line-geometry callers (drop
//! cap, runaround, hanging indent).

use zenith_layout::TextDirection;

use crate::ir::{Color, Paint, SceneCommand};

use super::ctx::{EmitStyle, UniformGeom};
use super::pack::{Line, LineStyle};
use super::shape::{CODE_BG, WordToken, run_to_scene_glyphs};

/// Emit decoration FillRects + DrawGlyphRun commands for a sequence of packed
/// lines, stacked by `line_height`, with per-line horizontal `align`.
///
/// This is the EXACT emit body lifted out of `compile_text`'s wrap path so the
/// single-box and chain renderers produce byte-identical command streams.
///
/// `style.justify_final_line` controls the LAST line of THIS batch under
/// `align="justify"`: `false` (the single-box wrap path and the FINAL chain
/// box) leaves it ragged per paragraph semantics; `true` (a non-final chain box
/// whose flow continues into the next box) keeps it justified, since the text
/// does not actually end at this box's last line.
pub(in crate::compile) fn emit_lines(
    lines: &[Line],
    text_x: f64,
    text_y: f64,
    box_w: f64,
    style: EmitStyle,
    commands: &mut Vec<SceneCommand>,
) {
    // Uniform geometry: every line shares `text_x`/`box_w`. Delegates to the
    // profiled emit with a constant per-line geometry so the two paths are one
    // body (byte-identical to the historical single-width emit).
    let geom = UniformGeom { text_x, box_w };
    emit_lines_profiled(lines, |i| geom.at(i), text_y, style, commands);
}

/// Per-line geometry resolver: maps a 0-based line index to its
/// `(line_origin_x, line_box_width)`. The drop-cap path returns an indented
/// origin + narrow width for the first `n` lines and `(text_x, full_w)` after;
/// the uniform path returns the same `(text_x, box_w)` for every line.
///
/// This is the SINGLE emit body; [`emit_lines`] is the uniform special case.
/// Alignment, decoration, and glyph emission are identical — only the per-line
/// horizontal origin/measure are read from `geom`.
pub(in crate::compile) fn emit_lines_profiled<F>(
    lines: &[Line],
    geom: F,
    text_y: f64,
    style: EmitStyle,
    commands: &mut Vec<SceneCommand>,
) where
    F: Fn(usize) -> (f64, f64),
{
    let align = style.align;
    let metrics = style.metrics;
    let justify_final_line = style.justify_final_line;
    let direction = style.direction;
    let glyph_stroke = style.glyph_stroke;

    // Node-global fallback scalars (used when `line.line_style` is `None`,
    // which is every existing path → byte-identical output).
    let global_ascent = metrics.ascent;
    let global_space_advance = metrics.space_advance;
    let global_font_size = style.font_size;
    let global_deco_thickness = style.deco_thickness;

    let last_idx = lines.len().saturating_sub(1);
    let is_rtl = direction == TextDirection::Rtl;
    // Cumulative vertical advance: sum of preceding lines' per-line heights.
    // When all heights equal the uniform `metrics.line_height` this produces the
    // same value as `i * line_height` (both are exact for common integer/simple
    // line-height values). See the `height_px` field on [`Line`].
    let mut y_offset: f64 = 0.0;
    for (i, line) in lines.iter().enumerate() {
        let (text_x, box_w) = geom(i);
        // Per-line scalars: use the line's own override when present, else fall
        // back to the node-global values. When `line_style` is `None` for every
        // line (all existing paths) the values are identical to the old hoisted
        // reads → byte-identical output guaranteed.
        let (ascent, space_advance, font_size, deco_thickness) = match line.line_style {
            Some(LineStyle {
                ascent,
                space_advance,
                font_size,
                deco_thickness,
            }) => (ascent, space_advance, font_size, deco_thickness),
            None => (
                global_ascent,
                global_space_advance,
                global_font_size,
                global_deco_thickness,
            ),
        };
        let baseline_y = text_y + ascent + y_offset;
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

        // Whether the inter-word gap BEFORE visual word `vi` (1-based boundary;
        // `vi` in `1..word_count`) is SUPPRESSED because the two words are glued
        // (source-adjacent, no whitespace). The glue flag lives on the
        // logically-LATER word. In LTR, visual order is logical order, so the
        // later word of the pair is `visual[vi]`. In RTL, the words are reversed,
        // so the later word is `visual[vi - 1]`. A line with no glued words has
        // every gap present, byte-identical to before.
        let gap_suppressed = |vi: usize| -> bool {
            let later = if is_rtl { vi.checked_sub(1) } else { Some(vi) };
            later.and_then(|j| visual.get(j)).is_some_and(|w| w.glued)
        };
        // Number of REAL (non-suppressed) gaps on the line — the count of
        // boundaries justify can stretch. Equals `word_count - 1` when no word is
        // glued, so justify is byte-identical for whitespace-only lines.
        let real_gap_count = (1..word_count).filter(|&vi| !gap_suppressed(vi)).count();

        // `(base_x, extra)`: the line's left origin and the PER-REAL-GAP stretch
        // added on top of `space_advance` under justify (0 otherwise). LTR keeps
        // the historical mapping exactly. RTL flips the anchor: `start`
        // right-anchors, `end` left-anchors, `center` is symmetric. Because
        // `content_w` is order-independent, the right-anchor offset
        // `box_w - content_w` is identical whichever order the words sit in.
        let (base_x, extra) = if is_rtl {
            match align {
                "center" => (text_x + (box_w - line.content_w) / 2.0, 0.0),
                // RTL `end` → left-anchor (left edge at box left).
                "end" => (text_x, 0.0),
                "justify" => {
                    // RTL justify: stretch inter-word gaps to fill, right-
                    // anchored; the last line stays right-aligned (ragged left).
                    let is_final_line = i == last_idx && !justify_final_line;
                    if !is_final_line && real_gap_count > 0 {
                        let extra = (box_w - line.content_w).max(0.0) / (real_gap_count as f64);
                        (text_x, extra)
                    } else {
                        (text_x + (box_w - line.content_w), 0.0)
                    }
                }
                // RTL `start`/unknown → right-anchor.
                _ => (text_x + (box_w - line.content_w), 0.0),
            }
        } else {
            match align {
                "center" => (text_x + (box_w - line.content_w) / 2.0, 0.0),
                "end" => (text_x + (box_w - line.content_w), 0.0),
                "justify" => {
                    // Justify stretches inter-word gaps so a non-final, multi-word
                    // line fills the box. The final line and lines with no real gap
                    // stay at the start offset (paragraph semantics). `extra` is
                    // clamped ≥ 0 so an overlong line never SHRINKS gaps below the
                    // normal space; `real_gap_count > 0` guards the divisor. A
                    // continuation chain box (`justify_final_line`) justifies its
                    // own last line too, since the paragraph flows on past it.
                    let is_final_line = i == last_idx && !justify_final_line;
                    if !is_final_line && real_gap_count > 0 {
                        let extra = (box_w - line.content_w).max(0.0) / (real_gap_count as f64);
                        (text_x, extra)
                    } else {
                        (text_x, 0.0)
                    }
                }
                _ => (text_x, 0.0),
            }
        };

        // Precompute each VISUAL word's left x along the line, left-to-right. A
        // suppressed (glued) boundary adds NO gap, so the glued word sits flush
        // against its neighbour; every other boundary adds `space_advance + extra`
        // (`extra` is non-zero only under justify).
        let mut word_x: Vec<f64> = Vec::with_capacity(word_count);
        {
            let mut x = base_x;
            for (wi, word) in visual.iter().enumerate() {
                word_x.push(x);
                x += word.advance;
                let next = wi + 1;
                if next < word_count && !gap_suppressed(next) {
                    x += space_advance + extra;
                }
            }
        }

        // Background rects FIRST (painted before decorations and glyphs so
        // everything sits on top). A multi-word highlighted run (or `code` run)
        // is coalesced into ONE FillRect spanning from the first word's start to
        // the last word's end, INCLUDING the inter-word spaces between them, so
        // the background is continuous with no gaps. Consecutive words are grouped
        // while they share the same background key (the same highlight color, or
        // both `code`); a colour change, a `None`, or a line break starts a fresh
        // rect. A single highlighted/code word yields one rect exactly as before,
        // so a document without multi-word runs is byte-identical.
        //
        // `highlight` and `code` are independent passes (a word may carry both),
        // mirroring the underline/strikethrough decoration grouping below. The
        // band geometry (y/h) is taken from the FIRST run-bearing word of the run.
        emit_background_run(&visual, &word_x, base_x, baseline_y, commands, |w| {
            w.highlight
        });
        emit_background_run(&visual, &word_x, base_x, baseline_y, commands, |w| {
            if w.code { Some(CODE_BG) } else { None }
        });

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
                        paint: Paint::solid(color),
                    });
                }
            }
            if let Some((sx, color)) = run_start.take() {
                commands.push(SceneCommand::FillRect {
                    x: sx,
                    y: deco_y,
                    w: run_right - sx,
                    h: deco_thickness,
                    paint: Paint::solid(color),
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
                    stroke_color: glyph_stroke.0,
                    stroke_width: glyph_stroke.1,
                    glyphs: run_to_scene_glyphs(run),
                });
                run_x += run.advance_width as f64;
            }
        }

        // Advance the vertical cursor by THIS line's own height for the next line.
        y_offset += line.height_px;
    }
}

/// Emit coalesced background FillRects for one background channel of a line's
/// VISUAL words. `key(word)` returns `Some(color)` when the word carries this
/// background (the highlight color, or [`CODE_BG`] for a `code` word) and `None`
/// otherwise. Maximal runs of consecutive words sharing the SAME `Some(color)`
/// are merged into a single rect spanning the first word's left edge to the last
/// word's right edge — which INCLUDES the inter-word spaces between them, since
/// `word_x` already encodes each word's placed origin. A `None`, a colour change,
/// or the end of the line closes the current run. The band's vertical geometry
/// (`y`/`h`) is taken from the FIRST run-bearing word of the run; a run made
/// entirely of empty (run-less) words emits nothing, matching the prior per-word
/// guard. A line with at most one background word per run is byte-identical to
/// the previous per-word emission.
fn emit_background_run<F>(
    visual: &[&WordToken],
    word_x: &[f64],
    base_x: f64,
    baseline_y: f64,
    commands: &mut Vec<SceneCommand>,
    key: F,
) where
    F: Fn(&WordToken) -> Option<Color>,
{
    // Open run state: the active color, the run's left/right x, and the band
    // geometry (set from the first run-bearing word; `None` until then).
    let mut run: Option<BgRun> = None;

    let flush = |run: Option<BgRun>, commands: &mut Vec<SceneCommand>| {
        if let Some(BgRun {
            color,
            left,
            right,
            band: Some((y, h)),
        }) = run
        {
            commands.push(SceneCommand::FillRect {
                x: left,
                y,
                w: right - left,
                h,
                paint: Paint::solid(color),
            });
        }
    };

    for (wi, word) in visual.iter().enumerate() {
        let wx = word_x.get(wi).copied().unwrap_or(base_x);
        let band = word.runs.first().map(|r| {
            let y = baseline_y - r.ascent as f64;
            let h = (r.ascent + r.descent) as f64;
            (y, h)
        });
        match key(word) {
            Some(color) => match run.take() {
                // Extend the current run when the color matches.
                Some(cur) if cur.color == color => {
                    run = Some(BgRun {
                        color,
                        left: cur.left,
                        right: wx + word.advance,
                        // Adopt the band from the first run-bearing word if not yet set.
                        band: cur.band.or(band),
                    });
                }
                // A different color (or no open run): flush, then open a new run.
                other => {
                    flush(other, commands);
                    run = Some(BgRun {
                        color,
                        left: wx,
                        right: wx + word.advance,
                        band,
                    });
                }
            },
            None => flush(run.take(), commands),
        }
    }
    flush(run.take(), commands);
}

/// One open background run accumulated by [`emit_background_run`]: the active
/// fill color, the run's left/right x extent, and its vertical band `(y, h)`
/// (taken from the first run-bearing word; `None` until one is seen).
struct BgRun {
    color: Color,
    left: f64,
    right: f64,
    band: Option<(f64, f64)>,
}

#[cfg(test)]
mod rtl_tests {
    use super::{EmitStyle, Line, WordToken, emit_lines};
    use zenith_core::FontStyle;
    use zenith_layout::{TextDirection, ZenithGlyphRun};

    use crate::ir::{Color, SceneCommand};

    use super::super::shape::{WordMetrics, WordSource};

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
            height_px: 18.0,
            line_style: None,
        };
        let mut commands = Vec::new();
        emit_lines(
            std::slice::from_ref(&line),
            /* text_x */ 100.0,
            /* text_y */ 0.0,
            /* box_w */ 200.0,
            EmitStyle {
                align,
                metrics: metrics(),
                font_size: 16.0,
                deco_thickness: 1.0,
                justify_final_line: false,
                direction,
                glyph_stroke: (None, None),
            },
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
mod line_style_tests {
    use super::{EmitStyle, Line, WordToken, emit_lines};
    use zenith_core::FontStyle;
    use zenith_layout::{TextDirection, ZenithGlyphRun};

    use crate::ir::{Color, SceneCommand};

    use super::super::pack::LineStyle;
    use super::super::shape::{WordMetrics, WordSource};

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

    fn global_metrics() -> WordMetrics {
        WordMetrics {
            ascent: 12.0,
            line_height: 18.0,
            space_advance: 5.0,
        }
    }

    /// Collect baseline_y for every DrawGlyphRun command (the `y` field).
    fn baseline_ys(commands: &[SceneCommand]) -> Vec<f64> {
        commands
            .iter()
            .filter_map(|c| match c {
                SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
                _ => None,
            })
            .collect()
    }

    /// (a) A line with `line_style: None` uses the node-global ascent to
    /// position its baseline. With `text_y = 0` and `global ascent = 12`,
    /// `baseline_y = 0 + 12 + 0 = 12`.
    #[test]
    fn none_line_uses_global_ascent() {
        let line = Line {
            words: vec![word(20.0)],
            content_w: 20.0,
            paragraph: 0,
            height_px: 18.0,
            line_style: None,
        };
        let mut commands = Vec::new();
        emit_lines(
            std::slice::from_ref(&line),
            0.0,
            /* text_y */ 0.0,
            100.0,
            EmitStyle {
                align: "start",
                metrics: global_metrics(),
                font_size: 16.0,
                deco_thickness: 1.0,
                justify_final_line: false,
                direction: TextDirection::Ltr,
                glyph_stroke: (None, None),
            },
            &mut commands,
        );
        let ys = baseline_ys(&commands);
        // baseline_y = text_y(0) + global_ascent(12) + y_offset(0) = 12.
        assert_eq!(ys, vec![12.0], "None line must use global ascent (12)");
    }

    /// (b) A line with `line_style: Some(LineStyle { ascent: 24, .. })` uses
    /// its own larger ascent, shifting the baseline down relative to a None line.
    /// With `text_y = 0` and `line ascent = 24`, `baseline_y = 0 + 24 + 0 = 24`.
    #[test]
    fn some_line_uses_override_ascent() {
        let line = Line {
            words: vec![word(20.0)],
            content_w: 20.0,
            paragraph: 0,
            height_px: 36.0,
            line_style: Some(LineStyle {
                ascent: 24.0,
                space_advance: 8.0,
                font_size: 32.0,
                deco_thickness: 2.0,
            }),
        };
        let mut commands = Vec::new();
        emit_lines(
            std::slice::from_ref(&line),
            0.0,
            /* text_y */ 0.0,
            100.0,
            EmitStyle {
                align: "start",
                metrics: global_metrics(), // global ascent = 12, NOT used for this line
                font_size: 16.0,
                deco_thickness: 1.0,
                justify_final_line: false,
                direction: TextDirection::Ltr,
                glyph_stroke: (None, None),
            },
            &mut commands,
        );
        let ys = baseline_ys(&commands);
        // baseline_y = text_y(0) + line_ascent(24) + y_offset(0) = 24.
        assert_eq!(
            ys,
            vec![24.0],
            "Some line must use its own ascent (24), not the global (12)"
        );
    }

    /// Two lines: line0 has `None` (global ascent 12), line1 has
    /// `Some(ascent=24)`. The baselines must be at 12 and 12+18+24=54
    /// (line0 height_px=18 advances y_offset to 18, then line1 baseline
    /// = text_y(0) + line1_ascent(24) + y_offset(18) = 42).
    #[test]
    fn two_lines_mixed_style_correct_baselines() {
        let line0 = Line {
            words: vec![word(20.0)],
            content_w: 20.0,
            paragraph: 0,
            height_px: 18.0,
            line_style: None,
        };
        let line1 = Line {
            words: vec![word(20.0)],
            content_w: 20.0,
            paragraph: 0,
            height_px: 36.0,
            line_style: Some(LineStyle {
                ascent: 24.0,
                space_advance: 8.0,
                font_size: 32.0,
                deco_thickness: 2.0,
            }),
        };
        let mut commands = Vec::new();
        emit_lines(
            &[line0, line1],
            0.0,
            0.0,
            100.0,
            EmitStyle {
                align: "start",
                metrics: global_metrics(),
                font_size: 16.0,
                deco_thickness: 1.0,
                justify_final_line: false,
                direction: TextDirection::Ltr,
                glyph_stroke: (None, None),
            },
            &mut commands,
        );
        let ys = baseline_ys(&commands);
        // line0: baseline = 0 + 12 + 0 = 12. y_offset advances by 18.
        // line1: baseline = 0 + 24 + 18 = 42.
        assert_eq!(
            ys,
            vec![12.0, 42.0],
            "mixed None/Some lines must produce independent baselines"
        );
    }
}
