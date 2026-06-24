//! Integration tests: read fixture files from the workspace root `examples/`
//! directory and render them to PNG, asserting valid, non-empty output and
//! byte-identical determinism across two back-to-back renders.

use zenith_cli::commands::render::to_png_with_dir;

/// Render `examples/<name>.zen` twice and assert the output is a valid,
/// byte-identical PNG.  All per-fixture tests delegate here.
///
/// The asset provider is built from the example's own directory (`examples/`)
/// so that `image` nodes can source their raster bytes.
fn assert_example_renders(name: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let examples_dir = std::path::Path::new(manifest_dir)
        .join("..")
        .join("examples");
    let fixture = examples_dir.join(format!("{name}.zen"));

    let src = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("could not read {}: {}", fixture.display(), e));

    let png = to_png_with_dir(&src, Some(&examples_dir), 1, false)
        .unwrap_or_else(|e| panic!("render failed (exit {}): {}", e.exit_code, e.message))
        .png;

    // Must be non-empty.
    assert!(!png.is_empty(), "PNG output must not be empty");

    // Must start with the PNG magic bytes.
    assert!(
        png.len() >= 8,
        "PNG must have at least 8 bytes; got {}",
        png.len()
    );
    assert_eq!(
        &png[0..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "first 8 bytes must be the PNG signature"
    );

    // Determinism: two renders must be byte-identical.
    let png2 = to_png_with_dir(&src, Some(&examples_dir), 1, false)
        .unwrap_or_else(|e| panic!("second render failed (exit {}): {}", e.exit_code, e.message))
        .png;
    assert_eq!(
        png, png2,
        "two renders of {name}.zen must produce identical bytes"
    );
}

#[test]
fn rect_zen_renders_to_valid_png() {
    assert_example_renders("rect");
}

#[test]
fn line_zen_renders_to_valid_png() {
    assert_example_renders("line");
}

#[test]
fn group_zen_renders_to_valid_png() {
    assert_example_renders("group");
}

#[test]
fn ellipse_zen_renders_to_valid_png() {
    assert_example_renders("ellipse");
}

#[test]
fn frame_zen_renders_to_valid_png() {
    assert_example_renders("frame");
}

#[test]
fn image_zen_renders_to_valid_png() {
    assert_example_renders("image");
}

#[test]
fn polygon_zen_renders_to_valid_png() {
    assert_example_renders("polygon");
}

#[test]
fn polyline_zen_renders_to_valid_png() {
    assert_example_renders("polyline");
}

#[test]
fn star_zen_renders_to_valid_png() {
    assert_example_renders("star");
}

#[test]
fn pattern_zen_renders_to_valid_png() {
    assert_example_renders("pattern");
}

#[test]
fn anchors_zen_renders_to_valid_png() {
    assert_example_renders("anchors");
}

#[test]
fn blur_zen_renders_to_valid_png() {
    assert_example_renders("blur");
}

#[test]
fn bold_zen_renders_to_valid_png() {
    assert_example_renders("bold");
}

#[test]
fn decorations_zen_renders_to_valid_png() {
    assert_example_renders("decorations");
}

#[test]
fn filter_zen_renders_to_valid_png() {
    assert_example_renders("filter");
}

#[test]
fn flowchart_zen_renders_to_valid_png() {
    assert_example_renders("flowchart");
}

#[test]
fn gradient_zen_renders_to_valid_png() {
    assert_example_renders("gradient");
}

#[test]
fn hello_zen_renders_to_valid_png() {
    assert_example_renders("hello");
}

#[test]
fn italic_zen_renders_to_valid_png() {
    assert_example_renders("italic");
}

#[test]
fn mask_zen_renders_to_valid_png() {
    assert_example_renders("mask");
}

#[test]
fn multipage_zen_renders_to_valid_png() {
    assert_example_renders("multipage");
}

#[test]
fn richtext_zen_renders_to_valid_png() {
    assert_example_renders("richtext");
}

#[test]
fn shadow_zen_renders_to_valid_png() {
    assert_example_renders("shadow");
}

#[test]
fn stroke_align_zen_renders_to_valid_png() {
    assert_example_renders("stroke-align");
}

#[test]
fn table_zen_renders_to_valid_png() {
    assert_example_renders("table");
}

#[test]
fn noise_zen_renders_to_valid_png() {
    assert_example_renders("noise");
}

#[test]
fn styled_zen_renders_to_valid_png() {
    assert_example_renders("styled");
}

#[test]
fn rounded_zen_renders_to_valid_png() {
    assert_example_renders("rounded");
}

#[test]
fn code_zen_renders_to_valid_png() {
    assert_example_renders("code");
}

// ── --locked sha256 verification ────────────────────────────────────────────────

/// Path to the workspace-root `examples/` directory (where `assets/swatch.png`
/// lives) so `--locked` asset verification can read real bytes.
fn examples_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples")
}

/// The known SHA-256 of `examples/assets/swatch.png`.
const SWATCH_SHA256: &str = "9c3fdf4f9c609c6ec749d4ccbd75fda384b32962d5d3893424e22e1fad44c042";

/// Build a `.zen` document referencing `assets/swatch.png` with the given
/// `sha256` property text. When `sha256` is `None` the property is omitted.
fn swatch_doc(sha256: Option<&str>) -> String {
    let sha_prop = match sha256 {
        Some(h) => format!(" sha256=\"{h}\""),
        None => String::new(),
    };
    format!(
        r##"zenith version=1 {{
  project id="proj.image" name="Image Example"
  assets {{
    asset id="asset.swatch" kind="image" src="assets/swatch.png"{sha_prop}
  }}
  tokens format="zenith-token-v1" {{
    token id="color.bg" type="color" value="#f8fafc"
  }}
  styles {{
  }}
  document id="doc.image" title="Image Example" {{
    page id="page.image" w=(px)320 h=(px)200 background=(token)"color.bg" {{
      image id="img.swatch" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 fit="stretch"
    }}
  }}
}}
"##
    )
}

#[test]
fn locked_correct_sha256_renders_ok() {
    let src = swatch_doc(Some(SWATCH_SHA256));
    let result = to_png_with_dir(&src, Some(&examples_dir()), 1, true);
    assert!(
        result.is_ok(),
        "correct sha256 in --locked mode must render: {:?}",
        result.err().map(|e| e.message)
    );
}

#[test]
fn locked_wrong_sha256_errors_exit_2() {
    // Flip the last hex digit.
    let wrong = "9c3fdf4f9c609c6ec749d4ccbd75fda384b32962d5d3893424e22e1fad44c043";
    let src = swatch_doc(Some(wrong));
    let err = to_png_with_dir(&src, Some(&examples_dir()), 1, true)
        .expect_err("wrong sha256 in --locked mode must error");
    assert_eq!(err.exit_code, 2, "sha256 mismatch must exit with code 2");
}

#[test]
fn locked_missing_sha256_errors_exit_2() {
    let src = swatch_doc(None);
    let err = to_png_with_dir(&src, Some(&examples_dir()), 1, true)
        .expect_err("missing sha256 in --locked mode must error");
    assert_eq!(err.exit_code, 2, "missing sha256 must exit with code 2");
}

#[test]
fn unlocked_wrong_sha256_renders_ok() {
    let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
    let src = swatch_doc(Some(wrong));
    let result = to_png_with_dir(&src, Some(&examples_dir()), 1, false);
    assert!(
        result.is_ok(),
        "wrong sha256 must be ignored when not locked: {:?}",
        result.err().map(|e| e.message)
    );
}

// ── asset.missing diagnostics ─────────────────────────────────────────────────

/// A `.zen` document referencing an asset whose file does not exist under the
/// project directory. The `src` points at a deterministic, never-present path
/// inside `examples/`.
fn missing_asset_doc() -> String {
    r##"zenith version=1 {
  project id="proj.missing" name="Missing Asset"
  assets {
    asset id="asset.absent" kind="image" src="assets/__zenith_does_not_exist__.png"
  }
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
  }
  styles {
  }
  document id="doc.missing" title="Missing Asset" {
    page id="page.missing" w=(px)320 h=(px)200 background=(token)"color.bg" {
      image id="img.absent" asset="asset.absent" x=(px)40 y=(px)40 w=(px)160 h=(px)120 fit="stretch"
    }
  }
}
"##
    .to_owned()
}

#[test]
fn render_missing_asset_yields_asset_missing_error_diagnostic() {
    // Render still returns Ok (the artifact carries the Error); the lib.rs gate
    // is what blocks the PNG write.
    let src = missing_asset_doc();
    let artifact = to_png_with_dir(&src, Some(&examples_dir()), 1, false)
        .expect("render must not hard-fail; the missing asset is carried as a diagnostic");
    let has_missing = artifact
        .diagnostics
        .iter()
        .any(|d| d.code == "asset.missing" && d.severity == zenith_core::Severity::Error);
    assert!(
        has_missing,
        "artifact must carry an asset.missing Error diagnostic; got: {:?}",
        artifact
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn validate_missing_asset_reports_error_exit_1() {
    let src = missing_asset_doc();
    let out = zenith_cli::commands::validate::run(&src, Some(&examples_dir()), false);
    assert_eq!(
        out.exit_code, 1,
        "a missing asset must make validate report a hard error; stdout: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("asset.missing"),
        "validate output must mention asset.missing; got: {}",
        out.stdout
    );
}

#[test]
fn validate_missing_asset_json_reports_error() {
    let src = missing_asset_doc();
    let out = zenith_cli::commands::validate::run(&src, Some(&examples_dir()), true);
    assert!(
        out.stdout.contains("asset.missing"),
        "validate --json output must include asset.missing; got: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""valid": false"#),
        "validate --json must mark the doc invalid; got: {}",
        out.stdout
    );
}
