use std::path::Path;

use crate::config::CliPolicyFlags;

use super::{to_png, to_png_all_pages, to_png_with_dir, to_scene_json};

const VALID_DOC: &str = r##"zenith version=1 {
  project id="proj.r" name="Render Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.accent" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.r" title="Render Test" {
    page id="page.r" w=(px)320 h=(px)200 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)320 h=(px)200 fill=(token)"color.bg"
      rect id="rect.accent" x=(px)40 y=(px)40 w=(px)240 h=(px)120 fill=(token)"color.accent"
    }
  }
}
"##;

const INVALID_DOC: &str = r##"zenith version=1 {
  project id="proj.inv" name="Invalid"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.bg" type="color" value="#000000"
  }
  styles {}
  document id="doc.inv" title="Invalid" {
    page id="page.inv" w=(px)100 h=(px)100 {
      rect id="rect.inv" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.bg"
    }
  }
}
"##;

/// A document whose only content node is an UNKNOWN kind. It parses (the
/// kind is preserved for forward-compat), validates without errors (unknown
/// kinds are a warning, not an error), and compiles with a
/// `scene.unsupported_node` ADVISORY — a reliable compile-stage diagnostic.
const UNKNOWN_NODE_DOC: &str = r##"zenith version=1 {
  project id="proj.u" name="Unknown Node"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.u" title="Unknown Node" {
    page id="page.u" w=(px)100 h=(px)100 {
      sparkle id="sparkle.1"
    }
  }
}
"##;

#[test]
fn to_png_returns_png_magic_bytes() {
    let artifact = to_png(VALID_DOC, 1).expect("render must succeed");
    let png = &artifact.png;
    assert!(
        png.len() >= 4,
        "PNG must have at least 4 bytes; got {}",
        png.len()
    );
    assert_eq!(
        &png[0..4],
        &[0x89, 0x50, 0x4E, 0x47],
        "PNG must start with magic bytes 89 50 4E 47"
    );
}

#[test]
fn to_png_is_non_empty() {
    let artifact = to_png(VALID_DOC, 1).expect("render must succeed");
    assert!(!artifact.png.is_empty(), "PNG output must not be empty");
}

#[test]
fn to_png_surfaces_compile_diagnostics() {
    let artifact = to_png(UNKNOWN_NODE_DOC, 1).expect("render must succeed");
    assert!(
        artifact
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.unsupported_node"),
        "render must surface the compile-stage advisory; got {:?}",
        artifact
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn to_scene_json_surfaces_compile_diagnostics() {
    let artifact = to_scene_json(UNKNOWN_NODE_DOC, None, 1, &CliPolicyFlags::default(), None)
        .expect("scene must succeed");
    assert!(
        artifact
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.unsupported_node"),
        "scene JSON path must surface the compile-stage advisory"
    );
}

#[test]
fn to_png_with_validation_error_returns_err() {
    let result = to_png(INVALID_DOC, 1);
    assert!(
        result.is_err(),
        "document with validation errors must not render"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.exit_code, 1,
        "validation errors must produce exit code 1"
    );
}

#[test]
fn to_scene_json_contains_schema_field() {
    let json = to_scene_json(VALID_DOC, None, 1, &CliPolicyFlags::default(), None)
        .expect("scene JSON must succeed")
        .json;
    assert!(
        json.contains("zenith-scene-v1"),
        "scene JSON must contain schema field; got snippet: {}",
        &json[..json.len().min(200)]
    );
}

#[test]
fn to_scene_json_with_validation_error_returns_err() {
    let result = to_scene_json(INVALID_DOC, None, 1, &CliPolicyFlags::default(), None);
    assert!(result.is_err(), "invalid doc must not produce scene JSON");
}

#[test]
fn to_png_deterministic_two_runs_equal() {
    let png1 = to_png(VALID_DOC, 1).expect("run 1").png;
    let png2 = to_png(VALID_DOC, 1).expect("run 2").png;
    assert_eq!(png1, png2, "two renders of the same doc must be identical");
}

/// A two-page document used to exercise the 1-based page selector.
const TWO_PAGE_DOC: &str = r##"zenith version=1 {
  project id="proj.mp" name="MP"
  tokens format="zenith-token-v1" {
    token id="color.p1" type="color" value="#252525"
    token id="color.p2" type="color" value="#dcdcdc"
  }
  styles {}
  document id="doc.mp" title="MP" {
    page id="page.p1" w=(px)100 h=(px)100 {
      rect id="rect.p1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.p1"
    }
    page id="page.p2" w=(px)100 h=(px)100 {
      rect id="rect.p2" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.p2"
    }
  }
}
"##;

#[test]
fn to_png_page_two_is_ok() {
    let result = to_png(TWO_PAGE_DOC, 2);
    assert!(result.is_ok(), "rendering page 2 must succeed");
}

#[test]
fn to_png_page_out_of_range_is_err_exit_2() {
    let err = to_png(TWO_PAGE_DOC, 3).expect_err("page 3 must be out of range");
    assert_eq!(err.exit_code, 2, "out-of-range page must exit with code 2");
}

#[test]
fn to_png_page_zero_is_err_exit_2() {
    let err = to_png(TWO_PAGE_DOC, 0).expect_err("page 0 is invalid (1-based)");
    assert_eq!(err.exit_code, 2, "page 0 must exit with code 2");
}

#[test]
fn to_png_all_pages_returns_one_artifact_per_page() {
    let artifacts = to_png_all_pages(TWO_PAGE_DOC, None, false, &CliPolicyFlags::default(), None)
        .expect("all-pages render must succeed");
    assert_eq!(
        artifacts.len(),
        2,
        "a two-page doc must yield two artifacts"
    );
    for (i, a) in artifacts.iter().enumerate() {
        assert!(
            a.png.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
            "page {} must be a valid PNG",
            i + 1
        );
    }
    // The two pages have different backgrounds → different bytes.
    assert_ne!(
        artifacts[0].png, artifacts[1].png,
        "distinct pages must render to distinct PNGs"
    );
}

#[test]
fn to_png_all_pages_empty_doc_is_err() {
    let empty = r##"zenith version=1 {
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.e" title="E" {}
}
"##;
    let err = to_png_all_pages(empty, None, false, &CliPolicyFlags::default(), None)
        .expect_err("a doc with no pages must error");
    // A zero-page document is now rejected at validation (document.no_pages,
    // exit 1) rather than later at the render stage (exit 2).
    assert_eq!(err.exit_code, 1);
    assert!(
        err.message.contains("document.no_pages"),
        "expected a document.no_pages diagnostic; got: {}",
        err.message
    );
}

// ── overflow="fit" artifact-level tests ──────────────────────────────────

/// A text node with `overflow="fit"` that overflows its box must produce
/// a `text.fit_failed` Error-severity diagnostic in the PNG artifact.
/// (Whether the file is written is verified manually via the CLI — the
/// render command logic in lib.rs blocks the write when diagnostics contain
/// Error, exercised here at the artifact level.)
#[test]
fn to_png_overflow_fit_exceeded_has_error_diagnostic() {
    const OVERFLOW_FIT_DOC: &str = r##"zenith version=1 {
  project id="proj.fitcli" name="Fit CLI"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fitcli" title="Fit CLI" {
    page id="page.fitcli" w=(px)400 h=(px)400 {
      text id="text.fitcli" x=(px)10 y=(px)10 w=(px)60 h=(px)20 overflow="fit" {
        span "The quick brown fox jumps over the lazy dog and keeps on going"
      }
    }
  }
}
"##;
    // to_png still returns Ok — the artifact carries the Error diagnostic.
    // The CLI dispatcher (lib.rs) is what blocks the file write.
    let artifact = to_png(OVERFLOW_FIT_DOC, 1).expect("compile+render must not hard-fail");
    let has_fit_error = artifact
        .diagnostics
        .iter()
        .any(|d| d.code == "text.fit_failed" && d.severity == zenith_core::Severity::Error);
    assert!(
        has_fit_error,
        "artifact must carry a text.fit_failed Error diagnostic; got: {:?}",
        artifact.diagnostics
    );
}

/// A document referencing an asset whose file does not exist under the
/// project directory. Used to exercise the `asset.missing` hard diagnostic.
const MISSING_ASSET_DOC: &str = r##"zenith version=1 {
  project id="proj.missing" name="Missing Asset"
  assets {
    asset id="asset.absent" kind="image" src="__zenith_does_not_exist__.png"
  }
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.missing" title="Missing Asset" {
    page id="page.missing" w=(px)100 h=(px)100 background=(token)"color.bg" {
      image id="img.absent" asset="asset.absent" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="stretch"
    }
  }
}
"##;

#[test]
fn to_png_missing_asset_has_asset_missing_error_diagnostic() {
    // A directory that is guaranteed not to contain the asset file. The
    // render still returns Ok (the gate in lib.rs is what blocks the write);
    // the artifact must carry the asset.missing Error diagnostic.
    let dir = Path::new("/nonexistent-zenith-project-dir");
    let artifact = to_png_with_dir(
        MISSING_ASSET_DOC,
        Some(dir),
        1,
        false,
        &CliPolicyFlags::default(),
        None,
    )
    .expect("render must not hard-fail; missing asset is carried as a diagnostic");
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
fn to_scene_json_missing_asset_has_asset_missing_error_diagnostic() {
    let dir = Path::new("/nonexistent-zenith-project-dir");
    let artifact = to_scene_json(
        MISSING_ASSET_DOC,
        Some(dir),
        1,
        &CliPolicyFlags::default(),
        None,
    )
    .expect("scene JSON must succeed");
    assert!(
        artifact
            .diagnostics
            .iter()
            .any(|d| d.code == "asset.missing" && d.severity == zenith_core::Severity::Error),
        "scene artifact must carry an asset.missing Error diagnostic"
    );
}

/// A text node with `overflow="fit"` that FITS must produce no
/// `text.fit_failed` diagnostic, and the render must succeed cleanly.
#[test]
fn to_png_overflow_fit_fits_no_error_diagnostic() {
    const FIT_OK_DOC: &str = r##"zenith version=1 {
  project id="proj.fitok" name="Fit OK"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fitok" title="Fit OK" {
    page id="page.fitok" w=(px)400 h=(px)400 {
      text id="text.fitok" x=(px)10 y=(px)10 w=(px)300 h=(px)100 overflow="fit" {
        span "Hi"
      }
    }
  }
}
"##;
    let artifact = to_png(FIT_OK_DOC, 1).expect("render must succeed");
    let fit_errors: Vec<_> = artifact
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert!(
        fit_errors.is_empty(),
        "fitting text must produce no text.fit_failed; got: {:?}",
        fit_errors
    );
}
