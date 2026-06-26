//! Unit tests for the vector PDF backend.

use zenith_core::{AssetKind, AssetProvider, BytesAssetProvider, FontProvider, default_provider};
use zenith_scene::{
    Color, FilterSpec, FitMode, GradientPaint, GradientStop, Paint, Rect, Scene, SceneCommand,
    SceneGlyph,
};

use super::render_pdf;

/// A font + asset provider pair for tests. The default provider carries the
/// bundled Noto faces; the asset provider is empty unless a test registers one.
fn providers() -> (impl FontProvider, impl AssetProvider) {
    (default_provider(), BytesAssetProvider::new())
}

/// Resolve a bundled font id so glyph-run tests reference a real face.
fn a_font_id(fonts: &dyn FontProvider) -> String {
    fonts
        .all_faces()
        .first()
        .map(|f| f.id.clone())
        .expect("bundled fonts must be present")
}

fn render(scene: &Scene) -> Vec<u8> {
    let (fonts, assets) = providers();
    render_pdf(scene, &fonts, &assets)
}

#[test]
fn pdf_starts_and_ends_with_markers() {
    let mut scene = Scene::new(100.0, 80.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 80.0,
        paint: Paint::solid(Color::srgb(10, 20, 30, 255)),
    });
    let bytes = render(&scene);
    assert!(!bytes.is_empty(), "PDF must be non-empty");
    assert!(
        bytes.starts_with(b"%PDF-"),
        "PDF must start with %PDF- marker"
    );
    let tail_start = bytes.len().saturating_sub(8);
    let tail = &bytes[tail_start..];
    assert!(
        tail.windows(5).any(|w| w == b"%%EOF"),
        "PDF must end with %%EOF; tail was {:?}",
        String::from_utf8_lossy(tail)
    );
}

#[test]
fn render_is_byte_identical_across_runs() {
    let fonts = default_provider();
    let font_id = a_font_id(&fonts);
    let mut scene = Scene::new(120.0, 60.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 120.0,
        h: 60.0,
        paint: Paint::solid(Color::srgb(240, 240, 240, 255)),
    });
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: 10.0,
        y: 40.0,
        font_id,
        font_size: 24.0,
        color: Color::srgb(0, 0, 0, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs: vec![
            SceneGlyph {
                glyph_id: 5,
                dx: 0.0,
                dy: 0.0,
            },
            SceneGlyph {
                glyph_id: 8,
                dx: 14.0,
                dy: 0.0,
            },
        ],
    });
    let a = render(&scene);
    let b = render(&scene);
    assert_eq!(a, b, "two renders of the same scene must be byte-identical");
}

#[test]
fn cmyk_color_emits_device_cmyk_operator() {
    let mut scene = Scene::new(50.0, 50.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
        paint: Paint::solid(Color::cmyk(59.0, 85.0, 0.0, 7.0, 97, 36, 237)),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(" k\n") || text.contains(" k "),
        "CMYK fill must emit a DeviceCMYK `k` operator"
    );
}

#[test]
fn srgb_color_emits_device_rgb_operator() {
    let mut scene = Scene::new(50.0, 50.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
        paint: Paint::solid(Color::srgb(200, 100, 50, 255)),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(" rg\n") || text.contains(" rg "),
        "sRGB fill must emit a DeviceRGB `rg` operator"
    );
    assert!(
        !text.contains(" k\n"),
        "sRGB-only scene must not emit a DeviceCMYK `k` operator"
    );
}

#[test]
fn bleed_scene_has_distinct_trim_box() {
    // Media box larger than the trim box (bleed of 5 on each side).
    let mut scene = Scene::new(110.0, 90.0);
    scene.trim = Some(Rect {
        x: 5.0,
        y: 5.0,
        w: 100.0,
        h: 80.0,
    });
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 110.0,
        h: 90.0,
        paint: Paint::solid(Color::srgb(0, 0, 0, 255)),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/TrimBox"), "page must carry a /TrimBox");
    assert!(text.contains("/BleedBox"), "page must carry a /BleedBox");
    assert!(text.contains("/CropBox"), "page must carry a /CropBox");
    // The trim box, converted to PDF coords, is [5 5 105 85] — not the media
    // box [0 0 110 90].
    assert!(
        text.contains("/TrimBox [5 5 105 85]"),
        "TrimBox must differ from MediaBox; got document:\n{}",
        &text[..text.len().min(1200)]
    );
}

#[test]
fn non_bleed_scene_has_equal_boxes() {
    let mut scene = Scene::new(64.0, 48.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 64.0,
        h: 48.0,
        paint: Paint::solid(Color::srgb(1, 2, 3, 255)),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/MediaBox [0 0 64 48]"));
    assert!(text.contains("/TrimBox [0 0 64 48]"));
    assert!(text.contains("/BleedBox [0 0 64 48]"));
    assert!(text.contains("/CropBox [0 0 64 48]"));
}

#[test]
fn glyph_run_emits_path_fill_ops() {
    let fonts = default_provider();
    let font_id = a_font_id(&fonts);
    let mut scene = Scene::new(200.0, 60.0);
    scene.commands.push(SceneCommand::DrawGlyphRun {
        x: 10.0,
        y: 40.0,
        font_id,
        font_size: 32.0,
        color: Color::srgb(0, 0, 0, 255),
        stroke_color: None,
        stroke_width: None,
        glyphs: vec![SceneGlyph {
            glyph_id: 36, // 'A' in many fonts; any outlined glyph works
            dx: 0.0,
            dy: 0.0,
        }],
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    // Glyphs are drawn as filled outlines: the content stream must contain a
    // curve op (`c`) and a nonzero fill (`f`).
    assert!(
        text.contains(" c\n"),
        "glyph outline must emit cubic-curve `c` ops"
    );
    assert!(
        text.contains("\nf\n") || text.contains(" f\n"),
        "glyph run must emit a fill `f` op"
    );
}

#[test]
fn svg_asset_emits_vector_paths_and_shading_not_raster() {
    // A logo-shaped SVG: one gradient-filled polygon + one solid polygon. The
    // PDF backend must translate it to VECTOR ops — a nonzero fill `f` for the
    // solid polygon and an axial shading `/sh0 sh` for the gradient one — and
    // must NOT embed the SVG as a rasterized image XObject.
    const LOGO_SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'>\
        <defs><linearGradient id='g' gradientUnits='userSpaceOnUse' x1='0' y1='0' x2='100' y2='100'>\
        <stop offset='0' stop-color='#24b6dd'/><stop offset='1' stop-color='#0860a0'/>\
        </linearGradient></defs>\
        <polygon points='0,0 100,0 0,100' fill='url(#g)'/>\
        <polygon points='100,0 100,100 0,100' fill='#051b3e'/></svg>";

    let fonts = default_provider();
    let mut assets = BytesAssetProvider::new();
    assets.register("asset.logo", AssetKind::Svg, std::sync::Arc::from(LOGO_SVG));

    let mut scene = Scene::new(200.0, 200.0);
    scene.commands.push(SceneCommand::DrawImage {
        x: 20.0,
        y: 20.0,
        w: 160.0,
        h: 160.0,
        asset_id: "asset.logo".to_string(),
        fit: FitMode::Contain,
        pos_x: 50.0,
        pos_y: 50.0,
        opacity: 1.0,
        clip_shape: None,
        src_rect: None,
    });

    let bytes = render_pdf(&scene, &fonts, &assets);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("\nf\n") || text.contains(" f\n"),
        "SVG solid polygon must emit a nonzero fill `f` op"
    );
    assert!(
        text.contains("/sh0 sh"),
        "SVG linear gradient must emit an axial shading `/sh0 sh`"
    );
    assert!(
        !text.contains("/Subtype /Image"),
        "SVG must be vector, not a rasterized image XObject"
    );
}

#[test]
fn gradient_scene_renders_shading() {
    let mut scene = Scene::new(100.0, 100.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
        paint: Paint::Gradient(GradientPaint {
            angle_deg: 90.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Color::srgb(255, 0, 0, 255),
                },
                GradientStop {
                    offset: 0.5,
                    color: Color::srgb(0, 255, 0, 255),
                },
                GradientStop {
                    offset: 1.0,
                    color: Color::srgb(0, 0, 255, 255),
                },
            ],
            radial: false,
            center_x: None,
            center_y: None,
            radius_frac: None,
        }),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/ShadingType 2"),
        "linear gradient must emit an axial (Type 2) shading"
    );
    assert!(
        text.contains("/FunctionType 3"),
        "a 3-stop gradient must use a Type 3 stitching function"
    );
    assert!(text.contains(" sh\n"), "shading must be painted with `sh`");
}

/// A plain opaque FillRect with NO filter bracket renders via the vector path and
/// embeds zero image XObjects — the page carries no `/Subtype /Image`.
#[test]
fn unfiltered_fill_rect_embeds_no_image() {
    let mut scene = Scene::new(60.0, 40.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 10.0,
        y: 10.0,
        w: 30.0,
        h: 20.0,
        paint: Paint::solid(Color::srgb(200, 60, 40, 255)),
    });
    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains("/Subtype /Image"),
        "an unfiltered fill rect must not embed any image XObject"
    );
}

/// The SAME rect wrapped in a `BeginFilter{Grayscale 1.0}` / `EndFilter` bracket
/// is rasterized and embedded as an image XObject — proving the filtered region
/// is no longer a no-op (it produces ≥1 image where the vector path produces 0).
#[test]
fn filtered_fill_rect_embeds_image_xobject() {
    let mut scene = Scene::new(60.0, 40.0);
    scene.commands.push(SceneCommand::BeginFilter {
        filters: vec![FilterSpec::Grayscale(1.0)],
    });
    scene.commands.push(SceneCommand::FillRect {
        x: 10.0,
        y: 10.0,
        w: 30.0,
        h: 20.0,
        paint: Paint::solid(Color::srgb(200, 60, 40, 255)),
    });
    scene.commands.push(SceneCommand::EndFilter);

    let bytes = render(&scene);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/Subtype /Image"),
        "a filtered fill rect must rasterize+embed an image XObject"
    );
    // The XObject is referenced from the page Resources under the `im0` name.
    assert!(
        text.contains("/im0"),
        "the embedded filter image must be named im0 in page resources"
    );
}

/// Rasterize-and-embed must stay deterministic: two renders of a filtered scene
/// are byte-identical (no time, no randomness, fixed deflate level + rounding).
#[test]
fn filtered_region_render_is_byte_identical() {
    let mut scene = Scene::new(80.0, 50.0);
    scene.commands.push(SceneCommand::BeginFilter {
        filters: vec![FilterSpec::Sepia(1.0)],
    });
    scene.commands.push(SceneCommand::FillRect {
        x: 5.0,
        y: 5.0,
        w: 40.0,
        h: 30.0,
        paint: Paint::solid(Color::srgb(20, 140, 200, 255)),
    });
    scene.commands.push(SceneCommand::EndFilter);

    let a = render(&scene);
    let b = render(&scene);
    assert_eq!(
        a, b,
        "two renders of a filtered scene must be byte-identical"
    );
}

#[test]
fn multi_page_pdf_has_one_page_object_per_scene() {
    use super::render_pdf_multi;
    let (fonts, assets) = providers();

    let mut p1 = Scene::new(100.0, 80.0);
    p1.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 80.0,
        paint: Paint::solid(Color::srgb(10, 20, 30, 255)),
    });
    let mut p2 = Scene::new(120.0, 60.0);
    p2.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 120.0,
        h: 60.0,
        paint: Paint::solid(Color::srgb(200, 100, 50, 255)),
    });

    let bytes = render_pdf_multi(&[p1, p2], &fonts, &assets);
    let text = String::from_utf8_lossy(&bytes);

    // The page tree must report two kids, and each page its own MediaBox.
    assert!(
        text.contains("/Count 2"),
        "two-scene PDF must declare /Count 2"
    );
    let mediaboxes = text.matches("/MediaBox").count();
    assert_eq!(mediaboxes, 2, "each page must carry its own MediaBox");
    // Distinct page sizes confirm both scenes were written, not one twice.
    assert!(
        text.contains("/MediaBox [0 0 100 80]"),
        "page 1 box missing"
    );
    assert!(
        text.contains("/MediaBox [0 0 120 60]"),
        "page 2 box missing"
    );
}

#[test]
fn single_scene_multi_is_byte_identical_to_render_pdf() {
    use super::render_pdf_multi;
    let (fonts, assets) = providers();
    let mut scene = Scene::new(64.0, 48.0);
    scene.commands.push(SceneCommand::FillRect {
        x: 0.0,
        y: 0.0,
        w: 64.0,
        h: 48.0,
        paint: Paint::solid(Color::srgb(1, 2, 3, 255)),
    });
    let via_wrapper = render_pdf(&scene, &fonts, &assets);
    let via_multi = render_pdf_multi(std::slice::from_ref(&scene), &fonts, &assets);
    assert_eq!(
        via_wrapper, via_multi,
        "single-scene multi-page path must be byte-identical to render_pdf"
    );
}
