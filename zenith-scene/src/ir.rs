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
///
/// `cmyk` is `None` for sRGB-origin colors. When a color token was declared in
/// CMYK (`cmyk(c,m,y,k)`), this carries the original `[c, m, y, k]` percentages
/// (`0.0..=100.0`) so a future PDF backend can emit native DeviceCMYK; the
/// `r`/`g`/`b` channels then hold the naive device sRGB conversion. The PNG
/// renderer ignores `cmyk` entirely and paints with `r`/`g`/`b`/`a`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    /// Original CMYK channels `[c, m, y, k]` (percentages) when this color was
    /// declared in CMYK; `None` for sRGB-origin colors. Skipped in JSON when
    /// absent so existing sRGB scenes serialize byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmyk: Option<[f32; 4]>,
}

impl Color {
    /// Construct an sRGB-origin color (`cmyk` is `None`).
    pub const fn srgb(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            cmyk: None,
        }
    }

    /// Construct a CMYK-origin opaque color from the original channels plus the
    /// already-converted sRGB triple. `c`/`m`/`y`/`k` are percentages
    /// (`0.0..=100.0`); the converted `r`/`g`/`b` are supplied by the caller so
    /// the deterministic conversion lives in exactly one place
    /// (`zenith_core::cmyk_to_srgb`). Alpha is always `255`.
    pub const fn cmyk(c: f32, m: f32, y: f32, k: f32, r: u8, g: u8, b: u8) -> Self {
        Self {
            r,
            g,
            b,
            a: 255,
            cmyk: Some([c, m, y, k]),
        }
    }
}

// ── Gradient paint ────────────────────────────────────────────────────────────

/// A single color stop within a [`GradientPaint`].
///
/// `offset` is the normalized position along the gradient line in `0.0..=1.0`;
/// `color` is the (alpha-cascaded) stop color.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GradientStop {
    /// Normalized position along the gradient line, `0.0..=1.0`.
    pub offset: f64,
    /// Stop color (straight / un-pre-multiplied alpha).
    pub color: Color,
}

/// A linear gradient fill paint.
///
/// `angle_deg` is the gradient-line direction in degrees, clockwise from +x
/// (screen coordinates: +y is downward, so `90` is top-to-bottom). `stops`
/// holds at least two ordered color stops.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GradientPaint {
    /// Gradient-line angle in degrees, clockwise from +x.
    pub angle_deg: f64,
    /// Ordered color stops (at least two).
    pub stops: Vec<GradientStop>,
}

// ── Shadow ────────────────────────────────────────────────────────────────────

/// A single drop-shadow / outer-glow layer.
///
/// `dx`/`dy` are the offset (pixels) of the shadow relative to the ink; `blur`
/// is the Gaussian blur sigma (pixels, `>= 0`); `color` is the shadow color
/// (straight / un-pre-multiplied alpha). A node may carry several layers, all
/// painted behind the ink.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShadowSpec {
    /// Horizontal offset in pixels (positive = rightward).
    pub dx: f64,
    /// Vertical offset in pixels (positive = downward).
    pub dy: f64,
    /// Gaussian blur sigma in pixels (`>= 0`).
    pub blur: f64,
    /// Shadow color (straight / un-pre-multiplied alpha).
    pub color: Color,
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

// ── Image clip shape ──────────────────────────────────────────────────────────

/// A non-rectangular clip shape applied to a [`SceneCommand::DrawImage`].
///
/// `None` on the `DrawImage` (no `clip_shape`) means the default rectangular
/// box-clip (the raster is clipped to its declared `[x, y, w, h]` box). A
/// `Some` value constrains the blit to a shape INSCRIBED in that box:
///
/// - `Ellipse` — the ellipse inscribed in the box (a circle when the box is
///   square): the circular-avatar case.
/// - `RoundedRect { radius }` — a rounded rectangle with uniform corner radius.
///
/// Tagged in JSON via `#[serde(tag = "shape")]` for a self-describing form,
/// consistent with the `op`-tagged [`SceneCommand`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "shape")]
pub enum ImageClip {
    /// Clip to the ellipse inscribed in the image's `[x, y, w, h]` box.
    Ellipse,
    /// Clip to a rounded rectangle with uniform corner `radius` (pixels).
    RoundedRect { radius: f64 },
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
    /// Fill an axis-aligned rectangle with a linear gradient.
    FillRectGradient {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        gradient: GradientPaint,
    },
    /// Fill a rectangle with uniform corner radius, painted with a linear gradient.
    FillRoundedRectGradient {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        radius: f64,
        gradient: GradientPaint,
    },
    /// Fill an axis-aligned ellipse with a linear gradient.
    FillEllipseGradient {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        gradient: GradientPaint,
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
        /// Optional non-rectangular clip shape inscribed in the box. `None` =
        /// the default rectangular box-clip (existing behavior, unchanged).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip_shape: Option<ImageClip>,
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
    /// Push an affine rotation around a pivot; composes onto the renderer's transform stack.
    PushTransform { angle_deg: f64, cx: f64, cy: f64 },
    /// Pop the most recent pushed transform.
    PopTransform,
    // ── Shadow capture ────────────────────────────────────────────────────
    /// Open an isolated capture of the following draw commands. The captured
    /// ink is buffered offscreen until the matching [`SceneCommand::EndShadow`].
    ///
    /// `shadows` are painted in *reverse* order at `EndShadow` (so the
    /// first-declared layer ends up on top of later layers), all *behind* the
    /// crisp ink.
    BeginShadow { shadows: Vec<ShadowSpec> },
    /// Close the active shadow capture: paint the blurred shadow layers, then
    /// composite the captured ink on top.
    EndShadow,
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
            color: Color::srgb(10, 20, 30, 255),
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
            color: Color::srgb(255, 0, 0, 255),
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        assert!(
            json.contains(r#""op":"FillRect""#),
            "op tag must be FillRect; got: {json}"
        );
    }

    #[test]
    fn srgb_color_omits_cmyk_in_json() {
        let cmd = SceneCommand::FillRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            color: Color::srgb(1, 2, 3, 255),
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        assert!(
            !json.contains("cmyk"),
            "sRGB-origin color must not serialize a cmyk key; got: {json}"
        );
    }

    #[test]
    fn cmyk_color_carries_channels_and_serializes() {
        // cmyk(59,85,0,7) → #6124ed (97,36,237).
        let c = Color::cmyk(59.0, 85.0, 0.0, 7.0, 97, 36, 237);
        assert_eq!((c.r, c.g, c.b, c.a), (97, 36, 237, 255));
        assert_eq!(c.cmyk, Some([59.0, 85.0, 0.0, 7.0]));
        let json = serde_json::to_string(&c).expect("serialize");
        assert!(
            json.contains(r#""cmyk":[59.0,85.0,0.0,7.0]"#),
            "got: {json}"
        );
    }
}
