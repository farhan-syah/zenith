//! Integration-style pixel/determinism tests for the tiny-skia backend.
//!
//! Moved verbatim from the old inline `#[cfg(test)] mod tests` block in
//! `tiny_skia.rs`; exercises the public `render_image` / `render_png` entry
//! points and the `TinySkiaBackend` directly.

use std::sync::Arc;

use zenith_core::{AssetKind, BytesAssetProvider, FontStyle, default_provider};
use zenith_layout::{RustybuzzEngine, ShapeRequest, TextDirection, TextLayoutEngine};
use zenith_scene::{
    BlendMode, Color, FitMode, GradientPaint, GradientStop, ImageClip, Scene, SceneCommand,
    SceneGlyph, ShadowSpec, SrcRect, StrokeAlign,
};

use crate::backend::RasterBackend;
use crate::render::{render_image, render_png};

use super::TinySkiaBackend;

/// A shared empty asset provider for tests that draw no images.
fn no_assets() -> BytesAssetProvider {
    BytesAssetProvider::new()
}

fn red() -> Color {
    Color::srgb(255, 0, 0, 255)
}

fn make_solid_red_scene(page: f64) -> Scene {
    let mut s = Scene::new(page, page);
    s.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: page,
        h: page,
    });
    s.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: page,
        h: page,
        color: red(),
    });
    s.commands.push(SceneCommand::PopClip);
    s
}

/// Index into a straight-alpha RGBA8 buffer for pixel (px, py) in an image
/// of the given `width`.
fn pixel(rgba: &[u8], width: u32, px: u32, py: u32) -> (u8, u8, u8, u8) {
    let base = ((py * width + px) * 4) as usize;
    (rgba[base], rgba[base + 1], rgba[base + 2], rgba[base + 3])
}

// ── pixel correctness ─────────────────────────────────────────────────

#[test]
fn pixel_correctness_solid_red() {
    let scene = make_solid_red_scene(4.0);
    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
    // center pixel
    assert_eq!(pixel(&img.rgba, img.width, 2, 2), (255, 0, 0, 255));
    // corner pixel
    assert_eq!(pixel(&img.rgba, img.width, 0, 0), (255, 0, 0, 255));
}

// ── determinism ───────────────────────────────────────────────────────

#[test]
fn determinism_identical_png_bytes() {
    let scene = make_solid_red_scene(4.0);
    let backend = TinySkiaBackend;
    let provider = default_provider();
    let png1 = backend
        .rasterize(&scene, &provider, &no_assets())
        .and_then(|img| backend.encode_png(&img))
        .expect("first render");
    let png2 = backend
        .rasterize(&scene, &provider, &no_assets())
        .and_then(|img| backend.encode_png(&img))
        .expect("second render");
    assert_eq!(
        png1, png2,
        "PNG output must be byte-identical for the same scene"
    );
}

// ── PNG validity ──────────────────────────────────────────────────────

#[test]
fn png_magic_bytes() {
    let scene = make_solid_red_scene(4.0);
    let backend = TinySkiaBackend;
    let provider = default_provider();
    let png = backend
        .rasterize(&scene, &provider, &no_assets())
        .and_then(|img| backend.encode_png(&img))
        .expect("render");
    assert_eq!(
        &png[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "output must start with PNG magic bytes"
    );
}

// ── clip enforced ─────────────────────────────────────────────────────

#[test]
fn clip_clamps_fill_to_page() {
    // 4×4 page; FillRect extends well beyond the page edge.
    let mut scene = Scene::new(4.0, 4.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
    });
    scene.commands.push(SceneCommand::FillRect {
        x: 2.0,
        y: 2.0,
        w: 10.0,
        h: 10.0,
        color: red(),
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("must not panic or error");
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
    // Pixel inside the overlap region (3,3) should be red.
    assert_eq!(pixel(&img.rgba, img.width, 3, 3), (255, 0, 0, 255));
    // Pixel outside the fill (0,0) should be transparent.
    assert_eq!(pixel(&img.rgba, img.width, 0, 0), (0, 0, 0, 0));
}

// ── transparent default ───────────────────────────────────────────────

#[test]
fn transparent_default_no_fill() {
    let mut scene = Scene::new(4.0, 4.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("must succeed");
    // All pixels must be fully transparent.
    for i in 0..(img.width * img.height) {
        let base = (i * 4) as usize;
        assert_eq!(
            &img.rgba[base..base + 4],
            &[0, 0, 0, 0],
            "pixel {i} must be transparent"
        );
    }
}

// ── invalid size ──────────────────────────────────────────────────────

#[test]
fn invalid_zero_size_returns_error() {
    let scene = Scene::new(0.0, 0.0);
    let backend = TinySkiaBackend;
    let provider = default_provider();
    assert!(
        backend.rasterize(&scene, &provider, &no_assets()).is_err(),
        "zero-size scene must return RenderError"
    );
}

// ── glyph: draws pixels ───────────────────────────────────────────────

/// Build a DrawGlyphRun scene for the letter "A" using the bundled Noto Sans
/// font, then verify that at least one pixel in the output matches the run
/// color (i.e. text was actually rasterized).
#[test]
fn glyph_run_draws_pixels() {
    let provider = default_provider();
    let families = vec!["Noto Sans".to_string()];
    let font_size = 32.0_f32;

    // Shape "A" to get a real glyph id from the bundled font.
    let req = ShapeRequest {
        text: "A",
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        font_size,
        direction: TextDirection::Ltr,
    };
    let run = RustybuzzEngine::new()
        .shape(&req, &provider)
        .expect("shaping must succeed");

    // Page: 80×40.  Baseline at y=32 (leaves room for the glyph above).
    let page_w = 80.0_f64;
    let page_h = 40.0_f64;
    let baseline_y = 34.0_f64;
    let origin_x = 4.0_f64;

    let ink_color = Color::srgb(0, 0, 200, 255);

    // Map the shaped glyphs into SceneGlyph instances.
    let glyphs: Vec<SceneGlyph> = run
        .glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect();

    let mut scene = Scene::new(page_w, page_h);
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: origin_x,
        y: baseline_y,
        font_id: run.font_id.clone(),
        font_size,
        color: ink_color,
        stroke_color: None,
        stroke_width: None,
        glyphs,
    });

    let img = render_image(&scene, &provider, &no_assets()).expect("render must succeed");

    // At least one pixel must have non-zero blue (the ink color).
    let any_ink = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (r, g, b, a) = pixel(&img.rgba, img.width, px, py);
            // Anti-aliased: the pixel need not be exactly (0,0,200,255);
            // just check that the blue channel is dominant and alpha > 0.
            a > 0 && b > r && b > g
        })
    });

    assert!(
        any_ink,
        "DrawGlyphRun must rasterize at least one ink pixel for 'A' at 32px"
    );
}

// ── glyph: determinism ────────────────────────────────────────────────

#[test]
fn glyph_run_deterministic_png() {
    let provider = default_provider();
    let families = vec!["Noto Sans".to_string()];
    let font_size = 24.0_f32;

    let req = ShapeRequest {
        text: "Zenith",
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        font_size,
        direction: TextDirection::Ltr,
    };
    let run = RustybuzzEngine::new()
        .shape(&req, &provider)
        .expect("shaping must succeed");

    let glyphs: Vec<SceneGlyph> = run
        .glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect();

    let mut scene = Scene::new(200.0, 40.0);
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: 4.0,
        y: 30.0,
        font_id: run.font_id.clone(),
        font_size,
        color: Color::srgb(10, 10, 10, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs,
    });

    let png1 = render_png(&scene, &provider, &no_assets()).expect("first render");
    let png2 = render_png(&scene, &provider, &no_assets()).expect("second render");
    assert_eq!(
        png1, png2,
        "glyph run PNG must be byte-identical across two renders"
    );
}

// ── glyph: missing font id ────────────────────────────────────────────

#[test]
fn glyph_run_missing_font_id_succeeds_silently() {
    let provider = default_provider();

    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: 0.0,
        y: 20.0,
        font_id: "nonexistent-font-000-normal".to_string(),
        font_size: 16.0,
        color: Color::srgb(255, 0, 0, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs: vec![SceneGlyph {
            glyph_id: 36,
            dx: 0.0,
            dy: 0.0,
        }],
    });

    // Must succeed (Ok) — the run is skipped, no panic, no error.
    let img = render_image(&scene, &provider, &no_assets())
        .expect("render must succeed even with unknown font");

    // All pixels should be transparent (nothing was drawn).
    let any_opaque = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
            a > 0
        })
    });
    assert!(
        !any_opaque,
        "no pixels should be drawn when the font id is unknown"
    );
}

// ── image: stretch renders + determinism ──────────────────────────────

/// The committed 2×2 RGBA test PNG.
const SWATCH_PNG: &[u8] = include_bytes!("../../../examples/assets/swatch.png");

fn swatch_provider() -> BytesAssetProvider {
    let mut p = BytesAssetProvider::new();
    p.register("asset.swatch", AssetKind::Image, Arc::from(SWATCH_PNG));
    p
}

/// Build a scene that draws the swatch stretched into a box, clipped to it.
fn swatch_scene() -> Scene {
    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    scene.commands.push(SceneCommand::PushClip {
        x: 8.0,
        y: 8.0,
        w: 24.0,
        h: 24.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 8.0,
        y: 8.0,
        w: 24.0,
        h: 24.0,
        asset_id: "asset.swatch".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        src_rect: None,
    });
    scene.commands.push(SceneCommand::PopClip);
    scene.commands.push(SceneCommand::PopClip);
    scene
}

#[test]
fn draw_image_stretch_renders() {
    let backend = TinySkiaBackend;
    let fonts = default_provider();
    let assets = swatch_provider();
    let scene = swatch_scene();

    let img1 = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize 1");
    let img2 = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize 2");

    // (i) determinism: byte-identical pixels across two rasterizes.
    assert_eq!(
        img1.rgba, img2.rgba,
        "two rasterizes of the same image scene must be byte-identical"
    );

    // (ii) at least one pixel inside the box is non-transparent.
    let any_ink = (0..img1.height).any(|py| {
        (0..img1.width).any(|px| {
            let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
            a > 0
        })
    });
    assert!(
        any_ink,
        "DrawImage stretch must rasterize at least one non-transparent pixel"
    );
}

// ── image clip="ellipse": clip takes effect + determinism ─────────────

/// Build a scene that draws the swatch stretched to fill the whole page,
/// clipped to the inscribed ellipse (circle, since the box is square).
fn swatch_ellipse_scene() -> Scene {
    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    // Box-clip the compiler always emits before DrawImage.
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
        asset_id: "asset.swatch".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: Some(ImageClip::Ellipse),
        src_rect: None,
    });
    scene.commands.push(SceneCommand::PopClip);
    scene.commands.push(SceneCommand::PopClip);
    scene
}

#[test]
fn draw_image_ellipse_clip_takes_effect() {
    let backend = TinySkiaBackend;
    let fonts = default_provider();
    let assets = swatch_provider();
    let scene = swatch_ellipse_scene();

    let img1 = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize 1");
    let img2 = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize 2");

    // (i) determinism: byte-identical across two rasterizes.
    assert_eq!(
        img1.rgba, img2.rgba,
        "two rasterizes of the ellipse-clipped scene must be byte-identical"
    );

    // (ii) the center pixel (inside the inscribed ellipse) is painted, while a
    // corner pixel (outside the ellipse) is fully transparent — proving the
    // ellipse clip mask took effect (a plain box-clip would paint the corner).
    let (_, _, _, center_a) = pixel(&img1.rgba, img1.width, 20, 20);
    let (_, _, _, corner_a) = pixel(&img1.rgba, img1.width, 0, 0);
    assert!(
        center_a > 0,
        "center pixel must be painted inside the ellipse clip"
    );
    assert_eq!(
        corner_a, 0,
        "corner pixel must be clipped out by the ellipse mask"
    );
}

// ── FillPolygon: triangle renders + determinism ───────────────────────

#[test]
fn fill_polygon_renders() {
    // A simple triangle on a 100×100 page.
    let color = Color::srgb(0, 200, 0, 255);
    let mut scene = Scene::new(100.0, 100.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    scene.commands.push(SceneCommand::FillPolygon {
        // Triangle: top-center, bottom-right, bottom-left
        points: vec![50.0, 10.0, 90.0, 90.0, 10.0, 90.0],
        color,
        even_odd: false,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img1 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 1");

    // At least one pixel inside the triangle must be green.
    let any_ink = (0..img1.height).any(|py| {
        (0..img1.width).any(|px| {
            let (_, g, _, a) = pixel(&img1.rgba, img1.width, px, py);
            a > 0 && g > 0
        })
    });
    assert!(
        any_ink,
        "FillPolygon must rasterize at least one green pixel"
    );

    // Determinism: two renders must be byte-identical.
    let img2 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 2");
    assert_eq!(
        img1.rgba, img2.rgba,
        "two rasterizes of FillPolygon must be byte-identical"
    );
}

// ── StrokePolyline: open stroke renders + determinism ─────────────────

#[test]
fn stroke_polyline_renders() {
    let color = Color::srgb(255, 0, 0, 255);
    let mut scene = Scene::new(100.0, 100.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    scene.commands.push(SceneCommand::StrokePolyline {
        points: vec![10.0, 50.0, 50.0, 10.0, 90.0, 50.0],
        color,
        stroke_width: 4.0,
        closed: false,
        align: StrokeAlign::Center,
        fill_even_odd: false,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img1 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 1");

    // At least one pixel must be inked.
    let any_ink = (0..img1.height).any(|py| {
        (0..img1.width).any(|px| {
            let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
            a > 0
        })
    });
    assert!(
        any_ink,
        "StrokePolyline must rasterize at least one ink pixel"
    );

    // Determinism.
    let img2 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 2");
    assert_eq!(
        img1.rgba, img2.rgba,
        "two rasterizes of StrokePolyline must be byte-identical"
    );
}

#[test]
fn stroke_ellipse_renders() {
    let color = Color::srgb(255, 0, 0, 255);
    let mut scene = Scene::new(100.0, 100.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    scene.commands.push(SceneCommand::StrokeEllipse {
        x: 20.0,
        y: 30.0,
        w: 60.0,
        h: 40.0,
        rx: None,
        ry: None,
        color,
        stroke_width: 4.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img1 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 1");

    // At least one pixel must be inked.
    let any_ink = (0..img1.height).any(|py| {
        (0..img1.width).any(|px| {
            let (_, _, _, a) = pixel(&img1.rgba, img1.width, px, py);
            a > 0
        })
    });
    assert!(
        any_ink,
        "StrokeEllipse must rasterize at least one ink pixel"
    );

    // Determinism.
    let img2 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize 2");
    assert_eq!(
        img1.rgba, img2.rgba,
        "two rasterizes of StrokeEllipse must be byte-identical"
    );
}

#[test]
fn draw_image_missing_asset_is_skipped() {
    let backend = TinySkiaBackend;
    let fonts = default_provider();
    // Empty provider: the asset id is not registered.
    let assets = BytesAssetProvider::new();

    let mut scene = Scene::new(20.0, 20.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 20.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 20.0,
        asset_id: "asset.missing".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        src_rect: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    // Must not panic; renders without any image pixels.
    let img = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize must succeed even with a missing asset");
    let any_opaque = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
            a > 0
        })
    });
    assert!(
        !any_opaque,
        "no pixels should be drawn when the asset is missing"
    );
}

// ── ellipse: partial clip truncates, does not reshape ─────────────────

/// A 20×20 circle (FillEllipse x=0,y=0,w=20,h=20) is drawn inside a
/// bottom-right quadrant clip [10,10,20,20].
///
/// Correct behaviour (TRUNCATE): the ellipse is drawn at its TRUE bounds
/// and the mask chops off the parts outside [10,10,20,20].
///
/// Old wrong behaviour (RESHAPE): the ellipse bbox was intersected with the
/// clip, yielding a tiny oval fitted to [10,10,10,10].  A corner pixel such
/// as (18,18) — inside the clip box but outside the true circle — would
/// have been filled because the reshaping made the oval touch it.
///
/// We assert:
/// - (18,18) alpha == 0  (outside the true circle; must stay transparent)
/// - (12,12) alpha > 0   (inside both clip and true circle; must be filled)
#[test]
fn ellipse_partial_clip_truncates_not_reshapes() {
    let mut scene = Scene::new(20.0, 20.0);
    // Full-page outer clip.
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 20.0,
    });
    // Bottom-right quadrant sub-page clip.
    scene.commands.push(SceneCommand::PushClip {
        x: 10.0,
        y: 10.0,
        w: 10.0,
        h: 10.0,
    });
    // A circle that exactly fits the full page (center (10,10), r=10).
    scene.commands.push(SceneCommand::FillEllipse {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 20.0,
        rx: None,
        ry: None,
        color: Color::srgb(255, 255, 255, 255),
    });
    scene.commands.push(SceneCommand::PopClip);
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // (18,18): inside clip [10,10,20,20] but outside the true circle
    // (dist from center (10,10) ≈ √(8²+8²) ≈ 11.3 > 10).
    // Must be transparent — the ellipse should be TRUNCATED here, not
    // reshaping the oval to fill the entire clip box.
    let (_, _, _, a_outside) = pixel(&img.rgba, img.width, 18, 18);
    assert_eq!(
        a_outside, 0,
        "pixel (18,18) is outside the true circle; must be transparent (truncate, not reshape)"
    );

    // (12,12): inside both the clip box and the true circle
    // (dist from center (10,10) ≈ √(2²+2²) ≈ 2.8 < 10).
    // Must have been drawn (alpha > 0).
    let (_, _, _, a_inside) = pixel(&img.rgba, img.width, 12, 12);
    assert!(
        a_inside > 0,
        "pixel (12,12) is inside both the clip and the circle; must be filled"
    );
}

// ── stroke line: sub-page clip mask is honored ────────────────────────

/// A diagonal stroked line spanning the page is wrapped in a small top-left
/// clip [0,0,5,5]. After wiring StrokeLine to `mask.as_ref()`, ink beyond the
/// clip (e.g. (15,15), on the line but outside the box) must be suppressed,
/// while ink inside the clip (near (2,2)) remains. Before the fix the line
/// drew its full length (sub-page clip ignored) and (15,15) would be inked.
#[test]
fn stroke_line_clipped_to_subpage_clip() {
    let mut scene = Scene::new(20.0, 20.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 20.0,
        h: 20.0,
    });
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    });
    scene.commands.push(SceneCommand::StrokeLine {
        x1: 0.0,
        y1: 0.0,
        x2: 20.0,
        y2: 20.0,
        color: Color::srgb(0, 0, 0, 255),
        stroke_width: 4.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    scene.commands.push(SceneCommand::PopClip);
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // (15,15): on the line but outside the [0,0,5,5] clip → must be clipped away.
    let (_, _, _, a_outside) = pixel(&img.rgba, img.width, 15, 15);
    assert_eq!(
        a_outside, 0,
        "pixel (15,15) is outside the sub-page clip; the stroked line must be truncated there"
    );

    // (2,2): on the line and inside the clip → must be inked.
    let (_, _, _, a_inside) = pixel(&img.rgba, img.width, 2, 2);
    assert!(
        a_inside > 0,
        "pixel (2,2) is on the line inside the clip; must be inked"
    );
}

// ── glyph run: sub-page clip mask is honored ──────────────────────────

/// A glyph run for "A" at 32px is placed at x≈20, baseline≈34 on an
/// 80×40 page, then wrapped in a tiny clip [0,0,4,4] that lies far from
/// the glyph ink.  After fixing DrawGlyphRun to pass `mask.as_ref()`, the
/// effective clip mask must suppress all ink → NO opaque pixel anywhere.
///
/// Before the fix (mask=None) tiny-skia only clips to the pixmap edge, so
/// the glyph would render normally and the test would fail.
#[test]
fn glyph_run_clipped_to_subpage_clip() {
    let provider = default_provider();
    let families = vec!["Noto Sans".to_string()];
    let font_size = 32.0_f32;

    let req = ShapeRequest {
        text: "A",
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        font_size,
        direction: TextDirection::Ltr,
    };
    let run = RustybuzzEngine::new()
        .shape(&req, &provider)
        .expect("shaping must succeed");

    let glyphs: Vec<SceneGlyph> = run
        .glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect();

    let mut scene = Scene::new(80.0, 40.0);
    // Tiny clip box [0,0,4,4] — entirely disjoint from the glyph ink.
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
    });
    // Glyph ink lands around x≥20, y up to baseline 34 — well outside the clip.
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: 20.0,
        y: 34.0,
        font_id: run.font_id.clone(),
        font_size,
        color: Color::srgb(0, 0, 200, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // The clip mask must suppress all glyph ink — no opaque pixel anywhere.
    let any_opaque = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
            a > 0
        })
    });
    assert!(
        !any_opaque,
        "glyph ink must be fully clipped by the sub-page mask; found opaque pixels"
    );
}

// ── StrokeRect: border pixels are inked ───────────────────────────────

#[test]
fn stroke_rect_draws_border_pixels() {
    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    scene.commands.push(SceneCommand::StrokeRect {
        x: 10.0,
        y: 10.0,
        w: 20.0,
        h: 20.0,
        color: Color::srgb(0, 0, 0, 255),
        stroke_width: 4.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // A pixel on the top border (around y=10) must be inked.
    let (_, _, _, a_border) = pixel(&img.rgba, img.width, 20, 10);
    assert!(a_border > 0, "top border pixel (20,10) must be inked");

    // The interior center (20,20) must be EMPTY (stroke, not fill).
    let (_, _, _, a_center) = pixel(&img.rgba, img.width, 20, 20);
    assert_eq!(a_center, 0, "stroke-only interior (20,20) must be empty");
}

// ── FillRoundedRect: corner is cut, center is filled ──────────────────

#[test]
fn fill_rounded_rect_cuts_corner_fills_center() {
    // A rect [0,0,40,40] with a large radius (20 → fully circular) leaves the
    // extreme corner pixel (0,0) at background while the center is filled.
    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    scene.commands.push(SceneCommand::FillRoundedRect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
        radius: 20.0,
        radii: None,
        color: Color::srgb(255, 255, 255, 255),
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // Corner (0,0) is outside the rounded shape → transparent.
    let (_, _, _, a_corner) = pixel(&img.rgba, img.width, 0, 0);
    assert_eq!(
        a_corner, 0,
        "corner pixel (0,0) must be cut by the radius (transparent)"
    );

    // Center (20,20) is inside → filled.
    let (_, _, _, a_center) = pixel(&img.rgba, img.width, 20, 20);
    assert!(a_center > 0, "center pixel (20,20) must be filled");
}

// ── determinism: StrokeRect + FillRoundedRect + StrokeRoundedRect ─────

#[test]
fn rounded_and_stroke_rects_deterministic_png() {
    let mut scene = Scene::new(80.0, 80.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 80.0,
        h: 80.0,
    });
    scene.commands.push(SceneCommand::StrokeRect {
        x: 5.0,
        y: 5.0,
        w: 30.0,
        h: 30.0,
        color: Color::srgb(200, 0, 0, 255),
        stroke_width: 3.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    scene.commands.push(SceneCommand::FillRoundedRect {
        x: 40.0,
        y: 5.0,
        w: 30.0,
        h: 30.0,
        radius: 10.0,
        radii: None,
        color: Color::srgb(0, 200, 0, 255),
    });
    scene.commands.push(SceneCommand::StrokeRoundedRect {
        x: 20.0,
        y: 40.0,
        w: 40.0,
        h: 30.0,
        radius: 8.0,
        radii: None,
        color: Color::srgb(0, 0, 200, 255),
        stroke_width: 3.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    let provider = default_provider();
    let png1 = render_png(&scene, &provider, &no_assets()).expect("first render");
    let png2 = render_png(&scene, &provider, &no_assets()).expect("second render");
    assert_eq!(
        png1, png2,
        "StrokeRect + FillRoundedRect + StrokeRoundedRect scene must render byte-identically"
    );
}

// ── dashed StrokeRect: renders without panic ──────────────────────────

/// A `StrokeRect` with `stroke_dash=Some(8.0)` and `stroke_gap=Some(4.0)` must
/// rasterize without panicking and ink at least one pixel (proving the dashed
/// path is exercised). The dashed render must also differ from the solid render
/// (different pixel pattern).
#[test]
fn dashed_stroke_rect_renders_without_panic() {
    let color = Color::srgb(255, 0, 0, 255);

    let mut dashed_scene = Scene::new(60.0, 60.0);
    dashed_scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 60.0,
        h: 60.0,
    });
    dashed_scene.commands.push(SceneCommand::StrokeRect {
        x: 5.0,
        y: 5.0,
        w: 50.0,
        h: 50.0,
        color,
        stroke_width: 3.0,
        stroke_dash: Some(8.0),
        stroke_gap: Some(4.0),
        stroke_linecap: Some(zenith_scene::ir::LineCap::Round),
    });
    dashed_scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&dashed_scene, &provider, &no_assets())
        .expect("dashed StrokeRect must rasterize without panic");

    // At least one pixel must be inked (the dashed path does produce ink).
    let any_ink = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (_, _, _, a) = pixel(&img.rgba, img.width, px, py);
            a > 0
        })
    });
    assert!(any_ink, "dashed StrokeRect must ink at least one pixel");

    // The dashed version differs from a solid stroke (different pixel pattern).
    let mut solid_scene = Scene::new(60.0, 60.0);
    solid_scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 60.0,
        h: 60.0,
    });
    solid_scene.commands.push(SceneCommand::StrokeRect {
        x: 5.0,
        y: 5.0,
        w: 50.0,
        h: 50.0,
        color,
        stroke_width: 3.0,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
    });
    solid_scene.commands.push(SceneCommand::PopClip);
    let solid_img = backend
        .rasterize(&solid_scene, &provider, &no_assets())
        .expect("solid StrokeRect must rasterize");

    assert_ne!(
        img.rgba, solid_img.rgba,
        "dashed and solid strokes must produce different pixel output"
    );
}

// ── SVG asset: rasterizes and draws red pixels ────────────────────────

/// An inline 10×10 SVG filled solid red is registered as `AssetKind::Svg`,
/// drawn stretched into a 10×10 box on a 10×10 page, and the center pixel
/// must be red (proving the SVG was rasterized and composited).
#[test]
fn draw_image_svg_asset_renders_red_pixels() {
    const RED_SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' \
        width='10' height='10'>\
        <rect width='10' height='10' fill='#ff0000'/>\
        </svg>";

    let mut assets = BytesAssetProvider::new();
    assets.register("asset.red", AssetKind::Svg, Arc::from(RED_SVG));

    let mut scene = Scene::new(10.0, 10.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
        asset_id: "asset.red".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        src_rect: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let fonts = default_provider();
    let img = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("SVG rasterize must succeed");

    // Center pixel must be red (r dominant, a > 0).
    let (r, g, b, a) = pixel(&img.rgba, img.width, 5, 5);
    assert!(a > 0, "center pixel must be opaque after SVG rasterize");
    assert!(
        r > g && r > b,
        "center pixel must be red-dominant; got r={r} g={g} b={b}"
    );
}

// ── SVG <text>: text element converts to paths and rasterizes ─────────

/// An inline SVG containing a red `<text>` element is registered as
/// `AssetKind::Svg`, drawn via DrawImage, and the output pixmap must contain
/// at least one RED pixel — proving the text was converted to paths and
/// rasterized (not silently dropped due to an empty fontdb).
#[test]
fn draw_image_svg_text_renders_red_pixels() {
    const TEXT_SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' \
        width='200' height='60'>\
        <text x='0' y='40' font-size='40' fill='#ff0000'>Hi</text>\
        </svg>";

    let mut assets = BytesAssetProvider::new();
    assets.register("asset.text_svg", AssetKind::Svg, Arc::from(TEXT_SVG));

    let mut scene = Scene::new(200.0, 60.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 60.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 60.0,
        asset_id: "asset.text_svg".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        src_rect: None,
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let fonts = default_provider();
    let img = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("SVG text rasterize must succeed");

    // At least one pixel must be red-dominant — text paths were rasterized.
    let any_red = (0..img.height).any(|py| {
        (0..img.width).any(|px| {
            let (r, g, b, a) = pixel(&img.rgba, img.width, px, py);
            a > 0 && r > g && r > b
        })
    });
    assert!(
        any_red,
        "SVG <text> must produce at least one red pixel after convert_text + rasterize"
    );
}

// ── PushTransform: rotation moves ink outside the axis-aligned bbox ────

/// A red FillRect [25,25,50,50] on a 100×100 page is wrapped in a 45°
/// rotation about the rect center (50,50). We assert:
/// - at least one red pixel exists (the rect still renders), and
/// - the inked-pixel set differs from the SAME FillRect rendered WITHOUT
///   the transform (proving the rotation actually rotated the geometry), and
/// - two renders of the rotated scene are byte-identical (deterministic).
#[test]
fn push_transform_rotates_fill_rect() {
    let red_color = Color::srgb(255, 0, 0, 255);

    // Rotated scene.
    let mut rotated = Scene::new(100.0, 100.0);
    rotated.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    rotated.commands.push(SceneCommand::PushTransform {
        angle_deg: 45.0,
        cx: 50.0,
        cy: 50.0,
    });
    rotated.commands.push(SceneCommand::FillRect {
        x: 25.0,
        y: 25.0,
        w: 50.0,
        h: 50.0,
        color: red_color,
    });
    rotated.commands.push(SceneCommand::PopTransform);
    rotated.commands.push(SceneCommand::PopClip);

    // Unrotated baseline (same FillRect, no transform).
    let mut plain = Scene::new(100.0, 100.0);
    plain.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    plain.commands.push(SceneCommand::FillRect {
        x: 25.0,
        y: 25.0,
        w: 50.0,
        h: 50.0,
        color: red_color,
    });
    plain.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let rot1 = backend
        .rasterize(&rotated, &provider, &no_assets())
        .expect("rotated rasterize 1");
    let rot2 = backend
        .rasterize(&rotated, &provider, &no_assets())
        .expect("rotated rasterize 2");
    let base = backend
        .rasterize(&plain, &provider, &no_assets())
        .expect("plain rasterize");

    // (i) Determinism: two renders of the rotated scene are byte-identical.
    assert_eq!(
        rot1.rgba, rot2.rgba,
        "two rasterizes of the rotated scene must be byte-identical"
    );

    // (ii) At least one red pixel was drawn.
    let any_red = (0..rot1.height).any(|py| {
        (0..rot1.width).any(|px| {
            let (r, g, b, a) = pixel(&rot1.rgba, rot1.width, px, py);
            a > 0 && r > g && r > b
        })
    });
    assert!(
        any_red,
        "rotated FillRect must produce at least one red pixel"
    );

    // (iii) The inked-pixel set must differ from the unrotated baseline —
    // a 45° rotation pushes ink past the original [25,75] axis-aligned bbox
    // (the rotated diamond reaches the page edges at x≈14.6 and x≈85.4).
    assert_ne!(
        rot1.rgba, base.rgba,
        "rotation must change the inked pixels versus the unrotated FillRect"
    );

    // (iv) A specific pixel OUTSIDE the original axis-aligned bbox but inside
    // the rotated diamond must be inked. The rotated diamond has corners at
    // (50,~14.6),(85.4,50),(50,85.4),(14.6,50); the point (20,50) lies inside
    // the diamond but outside the original [25..75]×[25..75] rect.
    let (_, _, _, a_outside_bbox) = pixel(&rot1.rgba, rot1.width, 20, 50);
    assert!(
        a_outside_bbox > 0,
        "pixel (20,50) is outside the unrotated bbox but inside the rotated diamond; must be inked"
    );
    // Sanity: the same pixel is transparent in the unrotated baseline.
    let (_, _, _, a_base) = pixel(&base.rgba, base.width, 20, 50);
    assert_eq!(
        a_base, 0,
        "pixel (20,50) must be transparent in the unrotated baseline"
    );
}

// ── Gradient fill (GRAD-2) ────────────────────────────────────────────

/// A `FillRectGradient` (vertical, 90°) renders without error, produces a
/// non-uniform fill (top row differs from bottom row), and is byte-identical
/// across two renders.
#[test]
fn fill_rect_gradient_renders_non_uniform_and_deterministic() {
    let mut scene = Scene::new(40.0, 40.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    });
    scene.commands.push(SceneCommand::FillRectGradient {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
        gradient: GradientPaint {
            angle_deg: 90.0, // top-to-bottom
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Color::srgb(0, 0, 0, 255),
                },
                GradientStop {
                    offset: 1.0,
                    color: Color::srgb(255, 255, 255, 255),
                },
            ],
            radial: false,
            center_x: None,
            center_y: None,
            radius_frac: None,
        },
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // Vertical gradient: the top row must be darker than the bottom row.
    let (top_r, _, _, _) = pixel(&img.rgba, img.width, 20, 1);
    let (bot_r, _, _, _) = pixel(&img.rgba, img.width, 20, 38);
    assert!(
        bot_r > top_r,
        "vertical gradient must brighten downward: top={top_r}, bottom={bot_r}"
    );

    // Byte-identical across two renders.
    let img2 = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");
    assert_eq!(img.rgba, img2.rgba, "gradient render must be deterministic");
}

// ── Radial gradient (GRAD-2) ──────────────────────────────────────────

/// A radial gradient with center at (0.5, 0.5) must render symmetrically:
/// - The luminance at the horizontal center on both sides must be equal
///   (left-center ≈ right-center, demonstrating radial symmetry).
/// - The center pixel must differ from a corner pixel (demonstrating the
///   gradient actually varies across the surface).
#[test]
fn radial_gradient_renders_symmetric_and_varies() {
    let size = 40.0_f64;
    let mut scene = Scene::new(size, size);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: size,
        h: size,
    });
    // White center → black edge, centered radial gradient.
    scene.commands.push(SceneCommand::FillRectGradient {
        x: 0.0,
        y: 0.0,
        w: size,
        h: size,
        gradient: GradientPaint {
            angle_deg: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Color::srgb(255, 255, 255, 255),
                },
                GradientStop {
                    offset: 1.0,
                    color: Color::srgb(0, 0, 0, 255),
                },
            ],
            radial: true,
            center_x: Some(0.5),
            center_y: Some(0.5),
            radius_frac: None,
        },
    });
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("radial gradient rasterize must succeed");

    let mid = (size / 2.0) as u32;
    let edge = 2_u32;

    // Symmetry: left-center and right-center luminance should be equal.
    let (left_r, _, _, _) = pixel(&img.rgba, img.width, edge, mid);
    let (right_r, _, _, _) = pixel(&img.rgba, img.width, size as u32 - edge - 1, mid);
    assert_eq!(
        left_r, right_r,
        "radial gradient must be horizontally symmetric: left={left_r}, right={right_r}"
    );

    // Center must differ from corner (gradient actually varies).
    let (center_r, _, _, _) = pixel(&img.rgba, img.width, mid, mid);
    let (corner_r, _, _, _) = pixel(&img.rgba, img.width, 1, 1);
    assert!(
        center_r != corner_r,
        "radial center must differ from corner: center={center_r}, corner={corner_r}"
    );
}

// ── Shadow (SHAD-2) ───────────────────────────────────────────────────

/// A `BeginShadow` (one black, blurred, slightly-offset layer) wrapping a
/// `FillRect`, closed by `EndShadow`, must:
/// (i) render deterministically (two runs byte-identical), and
/// (ii) bleed shadow ink OUTSIDE the original rect but within blur range.
#[test]
fn shadow_blurs_and_bleeds_outward_deterministically() {
    // 40×40 canvas; an opaque red rect at [15,25]×[15,25].
    let build = || {
        let mut scene = Scene::new(40.0, 40.0);
        scene.commands.push(SceneCommand::BeginShadow {
            shadows: vec![ShadowSpec {
                dx: 1.0,
                dy: 1.0,
                blur: 3.0,
                color: Color::srgb(0, 0, 0, 255),
            }],
        });
        scene.commands.push(SceneCommand::FillRect {
            x: 15.0,
            y: 15.0,
            w: 10.0,
            h: 10.0,
            color: Color::srgb(255, 0, 0, 255),
        });
        scene.commands.push(SceneCommand::EndShadow);
        scene
    };

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img1 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize must succeed");
    let img2 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize must succeed");

    // (i) Determinism.
    assert_eq!(
        img1.rgba, img2.rgba,
        "shadow render must be byte-identical across runs"
    );

    // (ii) The crisp red ink is intact inside the rect.
    let (cr, _, _, ca) = pixel(&img1.rgba, img1.width, 20, 20);
    assert!(ca == 255 && cr > 200, "rect center must stay opaque red");

    // (iii) Shadow bleeds OUTSIDE the original rect: a pixel just left of the
    // rect's left edge (x=12, well outside [15,25)) but within blur range
    // must be non-transparent and dark (the black shadow).
    let (sr, sg, sb, sa) = pixel(&img1.rgba, img1.width, 12, 20);
    assert!(
        sa > 0,
        "shadow must bleed outside the rect (x=12): got alpha {sa}"
    );
    assert!(
        sr < 128 && sg < 128 && sb < 128,
        "the bled shadow pixel must be dark: ({sr},{sg},{sb})"
    );

    // (iv) Far outside the blur range stays transparent.
    let (_, _, _, far_a) = pixel(&img1.rgba, img1.width, 0, 0);
    assert_eq!(far_a, 0, "corner far from the shadow must stay transparent");
}

// ── src-rect raster crop ──────────────────────────────────────────────────

/// Build a 3×3 PNG whose columns are pure RED (x=0), GREEN (x=1), BLUE (x=2).
///
/// Uses tiny-skia to compose the image rather than pulling in the `image`
/// crate or embedding a hand-crafted PNG byte string.
fn three_column_rgb_png() -> Arc<[u8]> {
    use tiny_skia::{Pixmap, PremultipliedColorU8};

    let mut pm = Pixmap::new(3, 3).expect("3x3 pixmap");
    // Fill each pixel using PremultipliedColorU8 (alpha=255 → premult == straight).
    let pixels = pm.pixels_mut();
    for row in 0..3_usize {
        for col in 0..3_usize {
            let idx = row * 3 + col;
            pixels[idx] = match col {
                0 => PremultipliedColorU8::from_rgba(255, 0, 0, 255).expect("red"),
                1 => PremultipliedColorU8::from_rgba(0, 255, 0, 255).expect("green"),
                _ => PremultipliedColorU8::from_rgba(0, 0, 255, 255).expect("blue"),
            };
        }
    }
    let png = pm.encode_png().expect("PNG encode must succeed");
    Arc::from(png.as_slice())
}

/// A DrawImage with `src_rect=Some(SrcRect{x:2,y:0,w:1,h:3})` selecting the
/// BLUE column of a red/green/blue 3×3 image, stretched into a 4×4 box, must
/// render entirely blue pixels (the crop replaces the source).
#[test]
fn draw_image_src_rect_crops_to_blue_column() {
    let png = three_column_rgb_png();
    let mut assets = BytesAssetProvider::new();
    assets.register("asset.cols", AssetKind::Image, png);

    // 4×4 canvas; the image fills the entire page.
    let mut scene = Scene::new(4.0, 4.0);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
    });
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
    });
    scene.commands.push(SceneCommand::DrawImage {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
        asset_id: "asset.cols".to_string(),
        fit: FitMode::Stretch,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        // Crop to the blue column (x=2, w=1, full height).
        src_rect: Some(SrcRect {
            x: 2.0,
            y: 0.0,
            w: 1.0,
            h: 3.0,
        }),
    });
    scene.commands.push(SceneCommand::PopClip);
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let fonts = default_provider();
    let img = backend
        .rasterize(&scene, &fonts, &assets)
        .expect("rasterize must succeed");

    // Every pixel in the 4×4 output must be blue-dominant and opaque.
    for py in 0..img.height {
        for px_x in 0..img.width {
            let (r, g, b, a) = pixel(&img.rgba, img.width, px_x, py);
            assert!(a > 0, "pixel ({px_x},{py}) must be opaque (alpha={a})");
            assert!(
                b > r && b > g,
                "pixel ({px_x},{py}) must be blue-dominant after src-rect crop; got r={r} g={g} b={b}"
            );
        }
    }
}

// ── Blend-mode layer compositing ──────────────────────────────────────

/// A blue rect wrapped in a `PushLayer { Multiply } … PopLayer` over a red
/// background composites the overlap as multiply (red×blue → black), which is
/// darker than either source. The render must not panic.
#[test]
fn blend_multiply_layer_darkens_overlap() {
    let page = 8.0;
    let mut scene = Scene::new(page, page);
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: page,
        h: page,
    });
    // Opaque red background covering the page.
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: page,
        h: page,
        color: Color::srgb(255, 0, 0, 255),
    });
    // Blue rect over the red, composited with multiply via a layer.
    scene.commands.push(SceneCommand::PushLayer {
        opacity: 1.0,
        blend_mode: Some(BlendMode::Multiply),
    });
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: page,
        h: page,
        color: Color::srgb(0, 0, 255, 255),
    });
    scene.commands.push(SceneCommand::PopLayer);
    scene.commands.push(SceneCommand::PopClip);

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");

    // Multiply(red, blue) = (255*0, 0*0, 0*255)/255 = (0,0,0): black, darker
    // than both the red and the blue source.
    let (r, g, b, a) = pixel(&img.rgba, img.width, 4, 4);
    assert_eq!(a, 255, "overlap must be opaque");
    assert!(
        r < 16 && g < 16 && b < 16,
        "multiply overlap must be near-black (darker than red and blue); got r={r} g={g} b={b}"
    );
}

/// A normal scene with NO layer commands renders unchanged: a solid red page
/// stays solid red (the layer mechanism never touches the no-layer path).
#[test]
fn no_layer_scene_unchanged() {
    let scene = make_solid_red_scene(4.0);
    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img = backend
        .rasterize(&scene, &provider, &no_assets())
        .expect("rasterize must succeed");
    assert_eq!(pixel(&img.rgba, img.width, 2, 2), (255, 0, 0, 255));
}

// ── Element gaussian blur ─────────────────────────────────────────────────

/// A `BeginBlur { radius: 6.0 }` wrapping a `FillRect`, closed by `EndBlur`,
/// must render deterministically and produce a visually softer result:
/// (i) the render does not panic, (ii) runs are byte-identical, (iii) the
/// blurred image has non-zero alpha outside the original rect (spread), and
/// (iv) the center of the blurred rect has reduced alpha compared with
/// the unblurred version (the blur spreads the ink outward).
#[test]
fn begin_end_blur_renders_deterministically_and_softens_ink() {
    let build = || {
        let mut scene = Scene::new(60.0, 60.0);
        scene.commands.push(SceneCommand::BeginBlur { radius: 6.0 });
        scene.commands.push(SceneCommand::FillRect {
            x: 20.0,
            y: 20.0,
            w: 20.0,
            h: 20.0,
            color: Color::srgb(255, 0, 0, 255),
        });
        scene.commands.push(SceneCommand::EndBlur);
        scene
    };

    let build_crisp = || {
        let mut scene = Scene::new(60.0, 60.0);
        // Same rect but NO blur bracket.
        scene.commands.push(SceneCommand::FillRect {
            x: 20.0,
            y: 20.0,
            w: 20.0,
            h: 20.0,
            color: Color::srgb(255, 0, 0, 255),
        });
        scene
    };

    let backend = TinySkiaBackend;
    let provider = default_provider();

    let img1 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize must not panic");
    let img2 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize must not panic (second run)");
    let img_crisp = backend
        .rasterize(&build_crisp(), &provider, &no_assets())
        .expect("crisp rasterize must not panic");

    // (i) + (ii) Determinism: two blurred runs must be byte-identical.
    assert_eq!(
        img1.rgba, img2.rgba,
        "blur render must be byte-identical across runs"
    );

    // (iii) Spread: a pixel just outside the original rect (x=18, inside
    // the blur radius) must have non-zero alpha.
    let (_, _, _, spread_a) = pixel(&img1.rgba, img1.width, 18, 30);
    assert!(
        spread_a > 0,
        "blur must spread ink outside the original rect: alpha at (18,30) = {spread_a}"
    );

    // (iv) Softening: the center of the blurred rect must have lower alpha
    // than the crisp version (blur dilutes the peak density).
    let (_, _, _, blurred_center_a) = pixel(&img1.rgba, img1.width, 30, 30);
    let (_, _, _, crisp_center_a) = pixel(&img_crisp.rgba, img_crisp.width, 30, 30);
    assert!(
        blurred_center_a < crisp_center_a,
        "blur must reduce peak alpha at the rect center: blurred={blurred_center_a} crisp={crisp_center_a}"
    );
}

/// A scene with NO `BeginBlur`/`EndBlur` must produce output byte-identical
/// to what it produced before blur was introduced (no regression for
/// blur-free documents).
#[test]
fn no_blur_command_scene_is_byte_identical() {
    // Two independently built scenes with no blur.
    let build = || {
        let mut scene = Scene::new(20.0, 20.0);
        scene.commands.push(SceneCommand::FillRect {
            x: 5.0,
            y: 5.0,
            w: 10.0,
            h: 10.0,
            color: Color::srgb(0, 128, 255, 200),
        });
        scene
    };

    let backend = TinySkiaBackend;
    let provider = default_provider();
    let img1 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize");
    let img2 = backend
        .rasterize(&build(), &provider, &no_assets())
        .expect("rasterize");
    assert_eq!(
        img1.rgba, img2.rgba,
        "non-blur scene must be byte-identical"
    );
}

// ── glyph stroke: renders without panic ───────────────────────────────

/// A DrawGlyphRun with stroke_color+stroke_width set must rasterize without
/// panic. We also verify that the rendered output DIFFERS from the same run
/// without stroke (the outline adds extra pixels).
#[test]
fn glyph_run_with_stroke_renders_without_panic() {
    let provider = default_provider();
    let families = vec!["Noto Sans".to_string()];
    let font_size = 32.0_f32;

    let req = ShapeRequest {
        text: "A",
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        font_size,
        direction: TextDirection::Ltr,
    };
    let run = RustybuzzEngine::new()
        .shape(&req, &provider)
        .expect("shaping must succeed");

    let glyphs: Vec<SceneGlyph> = run
        .glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect();

    // Scene WITH stroke (red outline, 3 px wide, on a blue fill).
    let mut scene_with = Scene::new(80.0, 50.0);
    scene_with.commands.push(SceneCommand::DrawGlyphRun {
        x: 4.0,
        y: 38.0,
        font_id: run.font_id.clone(),
        font_size,
        color: Color::srgb(0, 0, 200, 255),
        stroke_color: Some(Color::srgb(200, 0, 0, 255)),
        stroke_width: Some(3.0),
        glyphs: glyphs.clone(),
    });

    // Scene WITHOUT stroke (fill only, same geometry — byte-identical to prior).
    let mut scene_without = Scene::new(80.0, 50.0);
    scene_without.commands.push(SceneCommand::DrawGlyphRun {
        x: 4.0,
        y: 38.0,
        font_id: run.font_id.clone(),
        font_size,
        color: Color::srgb(0, 0, 200, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs,
    });

    let backend = TinySkiaBackend;
    let img_with = backend
        .rasterize(&scene_with, &provider, &no_assets())
        .expect("stroke render must succeed without panic");
    let img_without = backend
        .rasterize(&scene_without, &provider, &no_assets())
        .expect("no-stroke render must succeed");

    // The stroke adds a red outline: at least one pixel differs.
    assert_ne!(
        img_with.rgba, img_without.rgba,
        "a 3px red stroke must alter at least one pixel vs. fill-only"
    );
}

/// A DrawGlyphRun with stroke_color=None renders byte-identically to
/// the same run with stroke_width=None (default-off / byte-identical guarantee).
#[test]
fn glyph_run_without_stroke_is_byte_identical() {
    let provider = default_provider();
    let families = vec!["Noto Sans".to_string()];
    let font_size = 24.0_f32;

    let req = ShapeRequest {
        text: "Z",
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        font_size,
        direction: TextDirection::Ltr,
    };
    let run = RustybuzzEngine::new()
        .shape(&req, &provider)
        .expect("shaping must succeed");

    let glyphs: Vec<SceneGlyph> = run
        .glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect();

    let make = |sc: Option<Color>, sw: Option<f64>| {
        let mut scene = Scene::new(60.0, 40.0);
        scene.commands.push(SceneCommand::DrawGlyphRun {
            x: 4.0,
            y: 30.0,
            font_id: run.font_id.clone(),
            font_size,
            color: Color::srgb(10, 10, 10, 255),
            stroke_color: sc,
            stroke_width: sw,
            glyphs: glyphs.clone(),
        });
        scene
    };

    // Both None → must be byte-identical.
    let img_a = backend_render(&make(None, None), &provider);
    let img_b = backend_render(&make(None, None), &provider);
    assert_eq!(img_a, img_b, "two no-stroke renders must be byte-identical");

    // stroke_width=0 with a color → treated as no stroke (≤0 filtered out).
    let img_zero = backend_render(
        &make(Some(Color::srgb(255, 0, 0, 255)), Some(0.0)),
        &provider,
    );
    assert_eq!(
        img_a, img_zero,
        "stroke_width=0 must produce byte-identical output to no stroke"
    );
}

fn backend_render(scene: &Scene, provider: &zenith_core::BytesFontProvider) -> Vec<u8> {
    let backend = TinySkiaBackend;
    backend
        .rasterize(scene, provider, &no_assets())
        .expect("rasterize must succeed")
        .rgba
}

// ── Color-filter pixel math (apply_filters) ───────────────────────────────────

/// Build a 1×1 opaque Pixmap with straight-alpha RGB (alpha = 255, so
/// premultiplied == straight).
#[cfg(test)]
fn opaque_pixel(r: u8, g: u8, b: u8) -> tiny_skia::Pixmap {
    use tiny_skia::{Pixmap, PremultipliedColorU8};
    let mut pm = Pixmap::new(1, 1).expect("1x1 pixmap");
    pm.pixels_mut()[0] = PremultipliedColorU8::from_rgba(r, g, b, 255).expect("opaque pixel");
    pm
}

/// Read back the single pixel's premultiplied RGBA bytes (alpha 255 → straight).
#[cfg(test)]
fn read_pixel(pm: &tiny_skia::Pixmap) -> (u8, u8, u8, u8) {
    let d = pm.data();
    (d[0], d[1], d[2], d[3])
}

/// `Grayscale(1.0)` on an opaque color collapses R=G=B to the luma value
/// (within a rounding tolerance), leaving alpha unchanged.
#[test]
fn apply_filters_grayscale_collapses_to_luma() {
    use super::filter::apply_filters;
    use zenith_scene::FilterSpec;

    let mut pm = opaque_pixel(255, 0, 0);
    apply_filters(&mut pm, &[FilterSpec::Grayscale(1.0)]);
    let (r, g, b, a) = read_pixel(&pm);
    // luma of pure red = 0.2126 → 0.2126*255 ≈ 54.
    assert_eq!(r, g, "grayscale: R == G");
    assert_eq!(g, b, "grayscale: G == B");
    assert!((i32::from(r) - 54).abs() <= 1, "luma ≈ 54, got {r}");
    assert_eq!(a, 255, "alpha is unchanged");
}

/// `Invert(1.0)` flips every channel; alpha is unchanged.
#[test]
fn apply_filters_invert_flips_channels() {
    use super::filter::apply_filters;
    use zenith_scene::FilterSpec;

    let mut pm = opaque_pixel(10, 20, 30);
    apply_filters(&mut pm, &[FilterSpec::Invert(1.0)]);
    let (r, g, b, a) = read_pixel(&pm);
    assert_eq!((r, g, b), (245, 235, 225), "1 - channel");
    assert_eq!(a, 255, "alpha is unchanged");
}

/// `Brightness(0.0)` drives every channel to black; alpha is unchanged.
#[test]
fn apply_filters_brightness_zero_is_black() {
    use super::filter::apply_filters;
    use zenith_scene::FilterSpec;

    let mut pm = opaque_pixel(200, 100, 50);
    apply_filters(&mut pm, &[FilterSpec::Brightness(0.0)]);
    let (r, g, b, a) = read_pixel(&pm);
    assert_eq!((r, g, b), (0, 0, 0), "brightness 0 → black");
    assert_eq!(a, 255, "alpha is unchanged");
}

/// A fully-transparent pixel is left untouched by any color filter.
#[test]
fn apply_filters_skips_transparent_pixel() {
    use super::filter::apply_filters;
    use tiny_skia::Pixmap;
    use zenith_scene::FilterSpec;

    let mut pm = Pixmap::new(1, 1).expect("1x1 pixmap"); // all zero (transparent)
    apply_filters(&mut pm, &[FilterSpec::Invert(1.0)]);
    assert_eq!(read_pixel(&pm), (0, 0, 0, 0), "transparent pixel untouched");
}

/// Determinism: applying the same filter to two identical copies yields
/// byte-identical results.
#[test]
fn apply_filters_is_deterministic() {
    use super::filter::apply_filters;
    use zenith_scene::FilterSpec;

    let filters = [FilterSpec::Sepia(1.0), FilterSpec::HueRotate(90.0)];
    let mut a = opaque_pixel(123, 45, 200);
    let mut b = opaque_pixel(123, 45, 200);
    apply_filters(&mut a, &filters);
    apply_filters(&mut b, &filters);
    assert_eq!(a.data(), b.data(), "same input + filters → identical bytes");
}

/// `Duotone` with shadow=black, highlight=white, amount=1.0 maps luma onto the
/// black→white axis: pure black → ≈shadow (black), pure white → ≈highlight
/// (white), mid-gray → ≈gray (luma-mapped). Determinism is also checked.
#[test]
fn apply_filters_duotone_maps_luma_between_colors() {
    use super::filter::apply_filters;
    use zenith_scene::{Color, FilterSpec};

    let duo = FilterSpec::Duotone {
        amount: 1.0,
        shadow: Color::srgb(0, 0, 0, 255),
        highlight: Color::srgb(255, 255, 255, 255),
    };

    // Pure black input → luma 0 → shadow color (black).
    let mut black = opaque_pixel(0, 0, 0);
    apply_filters(&mut black, &[duo]);
    assert_eq!(read_pixel(&black), (0, 0, 0, 255), "black → shadow");

    // Pure white input → luma 1 → highlight color (white).
    let mut white = opaque_pixel(255, 255, 255);
    apply_filters(&mut white, &[duo]);
    assert_eq!(
        read_pixel(&white),
        (255, 255, 255, 255),
        "white → highlight"
    );

    // Mid-gray input → luma ≈ 0.5 → gray; R==G==B and ~mid.
    let mut gray = opaque_pixel(128, 128, 128);
    apply_filters(&mut gray, &[duo]);
    let (r, g, b, a) = read_pixel(&gray);
    assert_eq!(r, g, "duotone gray: R == G");
    assert_eq!(g, b, "duotone gray: G == B");
    assert!(
        (i32::from(r) - 128).abs() <= 2,
        "luma-mapped ≈ 128, got {r}"
    );
    assert_eq!(a, 255, "alpha is unchanged");

    // Determinism: two identical inputs → identical bytes.
    let mut c1 = opaque_pixel(70, 140, 210);
    let mut c2 = opaque_pixel(70, 140, 210);
    apply_filters(&mut c1, &[duo]);
    apply_filters(&mut c2, &[duo]);
    assert_eq!(c1.data(), c2.data(), "duotone is deterministic");
}
