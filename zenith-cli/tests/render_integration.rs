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

    let png = to_png_with_dir(&src, Some(&examples_dir), 1)
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
    let png2 = to_png_with_dir(&src, Some(&examples_dir), 1)
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
