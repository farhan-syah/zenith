//! Print crop/trim mark generation for pages declaring a `bleed` margin.
//!
//! When a page declares a positive bleed `b`, the media box expands to
//! `(w + 2b) × (h + 2b)` and the trim box is the inner rectangle
//! `[b, b, w, h]`. Standard crop marks are an L-shaped bracket of two short
//! line segments at each of the four trim corners, drawn entirely OUTSIDE the
//! trim box in the surrounding bleed margin (they never cross into the trim
//! area). They are painted in a registration color (black, ~1px) on top of all
//! page content so they remain visible.
//!
//! Marks are emitted as [`SceneCommand::StrokeLine`] primitives — the IR's
//! dedicated axis-aligned line command — so each corner bracket is exactly two
//! segments and a full page yields eight segments. All geometry is computed
//! deterministically from `b`, `w`, and `h` (no floating-point nondeterminism:
//! every coordinate is an exact sum/difference of the inputs).

use crate::ir::{Color, SceneCommand};

/// Registration color for crop marks: opaque black.
const MARK_COLOR: Color = Color::srgb(0, 0, 0, 255);

/// Crop-mark stroke width in pixels (thin hairline).
const MARK_STROKE_WIDTH: f64 = 1.0;

/// Maximum mark length in pixels. The actual length is `min(b, MARK_MAX_LEN)`
/// so the bracket always fits within the bleed margin (a mark can never be
/// longer than the bleed it lives in) while staying a recognizable size for
/// generous bleeds.
const MARK_MAX_LEN: f64 = 18.0;

/// Append crop-mark stroke commands for the four trim corners.
///
/// `b` is the (positive) bleed in pixels; `trim_w`/`trim_h` are the trim-box
/// width/height in pixels. The trim box occupies `[b, b]` … `[b + trim_w,
/// b + trim_h]` within the media box.
///
/// Each corner gets an L-bracket of two segments that run from the trim corner
/// OUTWARD into the bleed (away from the trim box), so no segment overlaps the
/// trim area. Exactly eight segments are appended.
pub(super) fn emit_crop_marks(commands: &mut Vec<SceneCommand>, b: f64, trim_w: f64, trim_h: f64) {
    let len = b.min(MARK_MAX_LEN);

    // Trim-box edges in media-box coordinates.
    let left = b;
    let right = b + trim_w;
    let top = b;
    let bottom = b + trim_h;

    // For each corner we draw one horizontal and one vertical segment, each
    // starting at the trim edge and extending `len` outward into the bleed.
    //
    // Top-left corner: horizontal goes left, vertical goes up.
    push_line(commands, left - len, top, left, top); // horizontal, into left bleed
    push_line(commands, left, top - len, left, top); // vertical, into top bleed

    // Top-right corner: horizontal goes right, vertical goes up.
    push_line(commands, right, top, right + len, top);
    push_line(commands, right, top - len, right, top);

    // Bottom-left corner: horizontal goes left, vertical goes down.
    push_line(commands, left - len, bottom, left, bottom);
    push_line(commands, left, bottom, left, bottom + len);

    // Bottom-right corner: horizontal goes right, vertical goes down.
    push_line(commands, right, bottom, right + len, bottom);
    push_line(commands, right, bottom, right, bottom + len);
}

/// Push a single hairline crop-mark segment.
fn push_line(commands: &mut Vec<SceneCommand>, x1: f64, y1: f64, x2: f64, y2: f64) {
    commands.push(SceneCommand::StrokeLine {
        x1,
        y1,
        x2,
        y2,
        color: MARK_COLOR,
        stroke_width: MARK_STROKE_WIDTH,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
}
