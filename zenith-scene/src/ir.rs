//! Scene IR — the backend-neutral display-list primitives.
//!
//! Every type in this module derives `Debug`, `Clone`, `PartialEq`, and
//! `serde::Serialize`.  No `HashMap` or `HashSet` is used anywhere in this
//! module, so JSON serialization is deterministic (struct field order is
//! stable; `BTreeMap` would be used if maps were ever needed).
//!
//! The `scene` field name is always the first field in `Scene` so the
//! `schema` key appears first in the serialized JSON.

use serde::Serialize;

// ── Color ─────────────────────────────────────────────────────────────────────

/// An sRGB 8-bit color with pre-multiplied-independent alpha.
///
/// `r`, `g`, `b`, `a` are all in `0..=255` (linear 8-bit sRGB per channel,
/// straight / un-pre-multiplied alpha).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

// ── Fit mode ────────────────────────────────────────────────────────────────

/// How a raster image asset scales to fill its declared box.
///
/// - `Contain` — scale to fit entirely inside the box (letterboxed).
/// - `Cover` — scale to cover the whole box (cropped, clipped to the box).
/// - `Stretch` — scale each axis independently to exactly fill the box.
/// - `None` — draw at native pixel size, anchored by object-position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    Contain,
    Cover,
    Stretch,
    None,
}

// ── Scene commands ────────────────────────────────────────────────────────────

/// A single display-list command in the scene.
///
/// All variants are tagged in JSON via `#[serde(tag = "op")]` so that each
/// serialized command carries an `"op"` field naming the primitive, e.g.
/// `{ "op": "FillRect", "x": 0.0, … }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op")]
pub enum SceneCommand {
    // ── Filled shapes ─────────────────────────────────────────────────────
    /// Fill an axis-aligned rectangle.
    FillRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Color,
    },
    /// Stroke an axis-aligned rectangle (inside the declared edge by default).
    StrokeRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Color,
        stroke_width: f64,
    },
    /// Fill a rectangle with uniform corner radius.
    FillRoundedRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        radius: f64,
        color: Color,
    },
    /// Stroke a rectangle with uniform corner radius.
    StrokeRoundedRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        radius: f64,
        color: Color,
        stroke_width: f64,
    },
    /// Fill an axis-aligned ellipse.
    FillEllipse {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Color,
    },
    /// Stroke an axis-aligned ellipse (centered on the ellipse path; no
    /// stroke-alignment in v0).
    StrokeEllipse {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Color,
        stroke_width: f64,
    },
    /// Stroke a line segment.
    StrokeLine {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: Color,
        stroke_width: f64,
    },
    /// Fill a closed polygon.
    FillPolygon {
        /// Flat list of `[x0, y0, x1, y1, …]` vertex coordinates.
        points: Vec<f64>,
        color: Color,
        /// When `true`, use the even-odd fill rule; otherwise nonzero (winding).
        #[serde(default)]
        even_odd: bool,
    },
    /// Stroke a polyline (open or closed depending on `closed`).
    StrokePolyline {
        /// Flat list of `[x0, y0, x1, y1, …]` vertex coordinates.
        points: Vec<f64>,
        color: Color,
        stroke_width: f64,
        /// When `true`, the path is closed before stroking (polygon outline).
        #[serde(default)]
        closed: bool,
    },
    // ── Asset commands ────────────────────────────────────────────────────
    /// Draw a raster image asset clipped to its declared box.
    ///
    /// The renderer re-resolves bytes via `AssetProvider::by_id` using only the
    /// `asset_id` string — no raw image bytes appear in the IR. `pos_x`/`pos_y`
    /// are the object-position anchors resolved to `0.0..=100.0`.
    DrawImage {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        /// Stable asset id; renderer resolves bytes via `AssetProvider::by_id`.
        asset_id: String,
        /// How the image scales to fill the box.
        fit: FitMode,
        /// Horizontal object-position anchor in `0.0..=100.0`.
        pos_x: f64,
        /// Vertical object-position anchor in `0.0..=100.0`.
        pos_y: f64,
        /// Effective opacity (node opacity × cascaded ctx opacity), `0.0..=1.0`.
        opacity: f64,
    },
    /// Draw a pre-resolved SVG asset.
    DrawSvgAsset {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        /// Asset path (project-relative).
        asset: String,
    },
    // ── Text ──────────────────────────────────────────────────────────────
    /// Draw a shaped, positioned glyph run.
    ///
    /// `x` is the text-box origin x in pixels; `y` is the baseline y in
    /// pixels (`text_box_top + ascent`).  The renderer re-resolves font bytes
    /// via `FontProvider::by_id` using only the `font_id` string — no raw
    /// font bytes appear in the IR.
    DrawGlyphRun {
        /// Text-box origin x in pixels.
        x: f64,
        /// Baseline y in pixels (`text_box_top + ascent`).
        y: f64,
        /// Stable font-face identifier; renderer resolves bytes via
        /// `FontProvider::by_id`.
        font_id: String,
        /// Font size at which glyphs were shaped, in pixels.
        font_size: f32,
        /// Fill color of the glyph run.
        color: Color,
        /// Positioned glyphs, baseline-relative.
        glyphs: Vec<SceneGlyph>,
    },
    // ── Clip / layer stack ────────────────────────────────────────────────
    /// Push an axis-aligned clip rectangle onto the clip stack.
    PushClip { x: f64, y: f64, w: f64, h: f64 },
    /// Pop the most-recently pushed clip rectangle.
    PopClip,
    /// Push a compositing layer (for opacity, blend, mask).
    PushLayer { opacity: f64 },
    /// Pop the most-recently pushed compositing layer.
    PopLayer,
}

// ── Scene glyph ───────────────────────────────────────────────────────────────

/// A single positioned glyph within a [`SceneCommand::DrawGlyphRun`].
///
/// Offsets `dx` and `dy` are pen offsets from the run origin, baseline-relative.
/// Positive `dx` is rightward; positive `dy` is downward (0 = on the baseline).
/// No font bytes appear here — only the glyph ID within the resolved font face.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneGlyph {
    /// Glyph identifier within the resolved font face.
    pub glyph_id: u16,
    /// Horizontal pen offset from the run origin, in pixels.
    pub dx: f32,
    /// Vertical offset from the baseline, in pixels (positive = below baseline).
    pub dy: f32,
}

// ── Scene ─────────────────────────────────────────────────────────────────────

/// A fully resolved, backend-neutral display list.
///
/// The `schema` field is always `"zenith-scene-v1"` and is declared first so
/// that it serializes as the first key in the JSON output, satisfying the
/// normative requirement from doc 09 / doc 16.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    /// Always `"zenith-scene-v1"`.  Declared first so it appears first in JSON.
    pub schema: &'static str,
    /// Page / canvas width in pixels.
    pub width: f64,
    /// Page / canvas height in pixels.
    pub height: f64,
    /// Ordered display list.  Paint order: index 0 is painted first (bottom).
    pub commands: Vec<SceneCommand>,
}

impl Scene {
    /// Construct an empty scene for the given page dimensions.
    ///
    /// `schema` is always set to `"zenith-scene-v1"`.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            schema: "zenith-scene-v1",
            width,
            height,
            commands: Vec::new(),
        }
    }

    /// Serialize this scene to a pretty-printed JSON string.
    ///
    /// Uses `serde_json::to_string_pretty` which produces deterministic output
    /// because `Scene` and its fields use only `Vec` (ordered) and `struct`
    /// (stable field order in Rust + serde), never `HashMap`.
    ///
    /// # Errors
    ///
    /// Returns an error only if serialization fails, which cannot happen for
    /// the types used in `Scene` (all fields are plain numerics, strings, and
    /// `u8`s).  The `Result` is kept for API robustness.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_new_sets_schema() {
        let s = Scene::new(800.0, 600.0);
        assert_eq!(s.schema, "zenith-scene-v1");
        assert_eq!(s.width, 800.0);
        assert_eq!(s.height, 600.0);
        assert!(s.commands.is_empty());
    }

    #[test]
    fn to_json_schema_is_first_key() {
        let s = Scene::new(100.0, 200.0);
        let json = s.to_json().expect("serialization must succeed");
        // The very first `"` after `{` must be `"schema"`.
        let trimmed = json.trim_start_matches('{').trim_start();
        assert!(
            trimmed.starts_with(r#""schema""#),
            "schema must be the first JSON key; got: {trimmed}"
        );
    }

    #[test]
    fn to_json_deterministic() {
        let mut s = Scene::new(640.0, 360.0);
        s.commands.push(SceneCommand::FillRect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 360.0,
            color: Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
        });
        let a = s.to_json().expect("first serialize");
        let b = s.to_json().expect("second serialize");
        assert_eq!(a, b, "serialization must be deterministic");
    }

    #[test]
    fn fill_rect_serializes_op_tag() {
        let cmd = SceneCommand::FillRect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            color: Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        assert!(
            json.contains(r#""op":"FillRect""#),
            "op tag must be FillRect; got: {json}"
        );
    }
}
