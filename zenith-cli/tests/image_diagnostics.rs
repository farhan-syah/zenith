//! Integration tests for `image.overflow` and `image.upscale` advisories.
//!
//! Tests call `collect_image_dimension_diagnostics` directly with real raster
//! fixtures written to a `tempfile::TempDir`.  PNG bytes with known intrinsic
//! dimensions are produced via `tiny_skia::Pixmap::new(W, H).encode_png()`.

use tempfile::TempDir;
use zenith_cli::commands::render::collect_image_dimension_diagnostics;
use zenith_core::{KdlAdapter, KdlSource};

// ── Fixture helpers ───────────────────────────────────────────────────────────

/// Write a PNG of exactly `w × h` pixels into `dir/<name>.png` and return the
/// relative filename.
fn write_png(dir: &TempDir, name: &str, w: u32, h: u32) -> String {
    let filename = format!("{name}.png");
    let path = dir.path().join(&filename);
    let pixmap = tiny_skia::Pixmap::new(w, h).expect("Pixmap::new must succeed for positive dims");
    let png_bytes = pixmap.encode_png().expect("encode_png must succeed");
    std::fs::write(&path, &png_bytes).expect("could not write PNG fixture");
    filename
}

/// Write a minimal valid SVG into `dir/<name>.svg` and return the relative filename.
fn write_svg(dir: &TempDir, name: &str) -> String {
    let filename = format!("{name}.svg");
    let path = dir.path().join(&filename);
    std::fs::write(
        &path,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"/>"#,
    )
    .expect("could not write SVG fixture");
    filename
}

/// Parse a `.zen` source string into a `Document`, panicking on failure.
fn parse(src: &str) -> zenith_core::Document {
    KdlAdapter
        .parse(src.as_bytes())
        .expect("fixture document must parse without error")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A 600×600 image in a 400×400 box with fit="none" must produce
/// `image.overflow` (intrinsic > box).
#[test]
fn image_overflow_emitted_when_intrinsic_exceeds_none_box() {
    let tmp = TempDir::new().unwrap();
    let png_name = write_png(&tmp, "big", 600, 600);

    let src = format!(
        r#"zenith version=1 {{
  project id="proj.t1" name="T1"
  assets {{
    asset id="asset.big" kind="image" src="{png_name}"
  }}
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.t1" title="T1" {{
    page id="page.t1" w=(px)800 h=(px)800 {{
      image id="img.big" asset="asset.big" x=(px)0 y=(px)0 w=(px)400 h=(px)400 fit="none"
    }}
  }}
}}"#
    );

    let doc = parse(&src);
    let diags = collect_image_dimension_diagnostics(&doc, tmp.path());

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"image.overflow"),
        "expected image.overflow; got: {codes:?}"
    );
    // fit="none" with intrinsic > box should NOT produce image.upscale
    assert!(
        !codes.contains(&"image.upscale"),
        "fit=none overflow must not produce image.upscale; got: {codes:?}"
    );
}

/// A 100×100 image in an 800×800 box with fit="contain" must produce
/// `image.upscale` (scale factor = 8 > 1).
#[test]
fn image_upscale_emitted_for_contain_scale_up() {
    let tmp = TempDir::new().unwrap();
    let png_name = write_png(&tmp, "small", 100, 100);

    let src = format!(
        r#"zenith version=1 {{
  project id="proj.t2" name="T2"
  assets {{
    asset id="asset.small" kind="image" src="{png_name}"
  }}
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.t2" title="T2" {{
    page id="page.t2" w=(px)1000 h=(px)1000 {{
      image id="img.small" asset="asset.small" x=(px)0 y=(px)0 w=(px)800 h=(px)800 fit="contain"
    }}
  }}
}}"#
    );

    let doc = parse(&src);
    let diags = collect_image_dimension_diagnostics(&doc, tmp.path());

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"image.upscale"),
        "expected image.upscale for contain scale-up; got: {codes:?}"
    );
}

/// A 2000×2000 image in a 400×400 box with fit="contain" is a downscale
/// (scale factor = 0.2 < 1) — NO advisory should be emitted.
#[test]
fn no_advisory_for_contain_scale_down() {
    let tmp = TempDir::new().unwrap();
    let png_name = write_png(&tmp, "large", 2000, 2000);

    let src = format!(
        r#"zenith version=1 {{
  project id="proj.t3" name="T3"
  assets {{
    asset id="asset.large" kind="image" src="{png_name}"
  }}
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.t3" title="T3" {{
    page id="page.t3" w=(px)800 h=(px)800 {{
      image id="img.large" asset="asset.large" x=(px)0 y=(px)0 w=(px)400 h=(px)400 fit="contain"
    }}
  }}
}}"#
    );

    let doc = parse(&src);
    let diags = collect_image_dimension_diagnostics(&doc, tmp.path());

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.is_empty(),
        "downscaled contain image must produce no advisories; got: {codes:?}"
    );
}

/// An SVG asset in a large box must produce NO advisory (SVG is vector, exempt).
#[test]
fn no_advisory_for_svg_asset() {
    let tmp = TempDir::new().unwrap();
    let svg_name = write_svg(&tmp, "logo");

    let src = format!(
        r#"zenith version=1 {{
  project id="proj.t4" name="T4"
  assets {{
    asset id="asset.logo" kind="svg" src="{svg_name}"
  }}
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.t4" title="T4" {{
    page id="page.t4" w=(px)1000 h=(px)1000 {{
      image id="img.logo" asset="asset.logo" x=(px)0 y=(px)0 w=(px)800 h=(px)800 fit="contain"
    }}
  }}
}}"#
    );

    let doc = parse(&src);
    let diags = collect_image_dimension_diagnostics(&doc, tmp.path());

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.is_empty(),
        "SVG asset must produce no advisories; got: {codes:?}"
    );
}

/// A 100×100 image in a 400×400 box with fit="none" — intrinsic LESS THAN box —
/// must produce neither `image.overflow` nor `image.upscale`.
#[test]
fn no_advisory_for_fit_none_intrinsic_within_box() {
    let tmp = TempDir::new().unwrap();
    let png_name = write_png(&tmp, "tiny", 100, 100);

    let src = format!(
        r#"zenith version=1 {{
  project id="proj.t5" name="T5"
  assets {{
    asset id="asset.tiny" kind="image" src="{png_name}"
  }}
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.t5" title="T5" {{
    page id="page.t5" w=(px)800 h=(px)800 {{
      image id="img.tiny" asset="asset.tiny" x=(px)0 y=(px)0 w=(px)400 h=(px)400 fit="none"
    }}
  }}
}}"#
    );

    let doc = parse(&src);
    let diags = collect_image_dimension_diagnostics(&doc, tmp.path());

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.is_empty(),
        "fit=none with intrinsic within box must produce no advisories; got: {codes:?}"
    );
}
