//! Line emission: decoration FillRects + DrawGlyphRun commands for a sequence of
//! packed lines, stacked by line height with per-line horizontal alignment. A
//! single profiled body ([`emit_lines_profiled`]) drives both the uniform
//! single-width emit ([`emit_lines`]) and the per-line-geometry callers (drop
//! cap, runaround, hanging indent).

use zenith_layout::TextDirection;

use crate::ir::{Color, Paint, SceneCommand};

use super::ctx::{EmitStyle, UniformGeom};
use super::pack::Line;
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
    let font_size = style.font_size;
    let deco_thickness = style.deco_thickness;
    let justify_final_line = style.justify_final_line;
    let direction = style.direction;
    let glyph_stroke = style.glyph_stroke;

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

        // Background rects FIRST (painted before decorations and glyphs so
        // everything sits on top). One FillRect per word for each active
        // background: `highlight` (author color) and/or `code` (CODE_BG).
        // Words without either emit nothing (byte-identical).
        for (wi, word) in visual.iter().enumerate() {
            if let Some(hl_color) = word.highlight {
                let wx = word_x.get(wi).copied().unwrap_or(base_x);
                // Use the first run's metrics for the highlight band height.
                // When a word has no runs (empty token) there is nothing to
                // highlight; the `if let Some` below guards this.
                if let Some(first_run) = word.runs.first() {
                    let hl_y = baseline_y - first_run.ascent as f64;
                    let hl_h = (first_run.ascent + first_run.descent) as f64;
                    commands.push(SceneCommand::FillRect {
                        x: wx,
                        y: hl_y,
                        w: word.advance,
                        h: hl_h,
                        paint: Paint::solid(hl_color),
                    });
                }
            }
            if word.code {
                let wx = word_x.get(wi).copied().unwrap_or(base_x);
                if let Some(first_run) = word.runs.first() {
                    let bg_y = baseline_y - first_run.ascent as f64;
                    let bg_h = (first_run.ascent + first_run.descent) as f64;
                    commands.push(SceneCommand::FillRect {
                        x: wx,
                        y: bg_y,
                        w: word.advance,
                        h: bg_h,
                        paint: Paint::solid(CODE_BG),
                    });
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
    }
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
