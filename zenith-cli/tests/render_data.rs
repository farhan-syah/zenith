//! Integration tests for the `--data` flag / `DataContext` threading on the
//! render path.
//!
//! These tests exercise [`load_data_context`] together with the render entry
//! functions to confirm that `(data)"field"` property references resolve end-
//! to-end, and that the render is byte-identical when `--data` is absent.

use std::io::Write;

use zenith_cli::commands::render::{load_data_context, to_png_with_dir, to_scene_json};
use zenith_cli::config::CliPolicyFlags;

// ── Helper: write a temp file ─────────────────────────────────────────────────

fn temp_file(suffix: &str, content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join(format!("data{suffix}"));
    std::fs::File::create(&path)
        .expect("create temp file")
        .write_all(content)
        .expect("write temp file");
    (dir, path)
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// A minimal document whose `background` property uses a `(data)` reference.
/// When the data context provides `c` as a valid hex color the page renders
/// with that color; without a data context the reference is unresolved but the
/// render is non-fatal (advisories only).
const DATA_REF_DOC: &str = r##"zenith version=1 {
  project id="proj.dr" name="Data Ref"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.dr" title="Data Ref" {
    page id="page.dr" w=(px)100 h=(px)100 background=(data)"c" {
    }
  }
}
"##;

/// A standard document with no data refs — used to confirm byte-identity when
/// `--data` is absent (no-op path).
const PLAIN_DOC: &str = r##"zenith version=1 {
  project id="proj.plain" name="Plain"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.plain" title="Plain" {
    page id="page.plain" w=(px)100 h=(px)100 {
      rect id="rect.plain" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.bg"
    }
  }
}
"##;

// ── load_data_context tests (JSON & CSV) ──────────────────────────────────────

#[test]
fn load_json_flat_fields_resolve() {
    let (_dir, path) = temp_file(".json", br##"{"c": "#ff0000", "name": "red"}"##);
    let ctx = load_data_context(&path).expect("load must succeed");
    assert_eq!(ctx.get("c"), Some("#ff0000"));
    assert_eq!(ctx.get("name"), Some("red"));
}

#[test]
fn load_json_nested_flattens_to_dot_paths() {
    let (_dir, path) = temp_file(
        ".json",
        br#"{"revenue": {"total": 42, "tax": 3.5}, "label": "Q1"}"#,
    );
    let ctx = load_data_context(&path).expect("load must succeed");
    assert_eq!(ctx.get("revenue.total"), Some("42"));
    assert_eq!(ctx.get("revenue.tax"), Some("3.5"));
    assert_eq!(ctx.get("label"), Some("Q1"));
}

#[test]
fn load_json_array_uses_first_element() {
    let (_dir, path) = temp_file(".json", br##"[{"c": "#00ff00"}, {"c": "#0000ff"}]"##);
    let ctx = load_data_context(&path).expect("load must succeed");
    assert_eq!(ctx.get("c"), Some("#00ff00"));
}

#[test]
fn load_json_empty_array_is_error() {
    let (_dir, path) = temp_file(".json", b"[]");
    assert!(load_data_context(&path).is_err());
}

#[test]
fn load_csv_header_and_first_row() {
    let (_dir, path) = temp_file(".csv", b"c,name\n#ff0000,red\n#00ff00,green");
    let ctx = load_data_context(&path).expect("load must succeed");
    assert_eq!(ctx.get("c"), Some("#ff0000"));
    assert_eq!(ctx.get("name"), Some("red"));
}

#[test]
fn load_csv_no_data_rows_is_error() {
    let (_dir, path) = temp_file(".csv", b"c,name\n");
    assert!(load_data_context(&path).is_err());
}

#[test]
fn load_unknown_extension_is_error() {
    let (_dir, path) = temp_file(".xml", b"<data/>");
    let err = load_data_context(&path).unwrap_err();
    assert!(
        err.message.contains("unsupported file extension"),
        "got: {}",
        err.message
    );
}

// ── End-to-end: render with data context ─────────────────────────────────────

/// Rendering a doc with `(data)"c"` when `--data` provides `c` must succeed
/// and produce a valid PNG.
#[test]
fn render_with_data_ctx_succeeds() {
    let (_dir, data_path) = temp_file(".json", br##"{"c": "#ff0000"}"##);
    let ctx = load_data_context(&data_path).expect("load");
    let artifact = to_png_with_dir(
        DATA_REF_DOC,
        None,
        1,
        false,
        &CliPolicyFlags::default(),
        Some(&ctx),
    )
    .expect("render must succeed");
    assert!(
        artifact.png.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
        "output must be a PNG"
    );
}

/// When `--data` is absent, a `(data)"c"` reference is non-fatal (advisories
/// only — the render still succeeds).
#[test]
fn render_without_data_ctx_is_non_fatal() {
    let result = to_png_with_dir(
        DATA_REF_DOC,
        None,
        1,
        false,
        &CliPolicyFlags::default(),
        None,
    );
    // The render must not return Err — data refs without a context are advisories.
    assert!(
        result.is_ok(),
        "render without data ctx must succeed (non-fatal advisories); got: {:?}",
        result.err().map(|e| e.message)
    );
}

/// A plain document (no data refs) must render byte-identically whether a data
/// context is provided or not.
#[test]
fn plain_doc_byte_identical_with_and_without_data() {
    let (_dir, data_path) = temp_file(".json", br#"{"irrelevant": "value"}"#);
    let ctx = load_data_context(&data_path).expect("load");

    let png_without = to_png_with_dir(PLAIN_DOC, None, 1, false, &CliPolicyFlags::default(), None)
        .expect("render without data must succeed")
        .png;

    let png_with = to_png_with_dir(
        PLAIN_DOC,
        None,
        1,
        false,
        &CliPolicyFlags::default(),
        Some(&ctx),
    )
    .expect("render with irrelevant data must succeed")
    .png;

    assert_eq!(
        png_without, png_with,
        "a doc with no data refs must render byte-identically with or without a data context"
    );
}

/// Scene JSON path with data context resolves refs; without data context no
/// hard-error diagnostic fires.
#[test]
fn scene_json_with_data_ctx_succeeds() {
    let (_dir, data_path) = temp_file(".json", br##"{"c": "#0000ff"}"##);
    let ctx = load_data_context(&data_path).expect("load");
    let artifact = to_scene_json(
        DATA_REF_DOC,
        None,
        1,
        &CliPolicyFlags::default(),
        Some(&ctx),
    )
    .expect("scene JSON must succeed with data ctx");
    // The scene JSON must be non-empty and schema-bearing.
    assert!(
        artifact.json.contains("zenith-scene-v1"),
        "scene JSON must contain schema field"
    );
    // No hard-error diagnostics.
    let hard: Vec<_> = artifact
        .diagnostics
        .iter()
        .filter(|d| d.severity == zenith_core::Severity::Error)
        .collect();
    assert!(
        hard.is_empty(),
        "no hard errors expected when data ctx is provided; got: {:?}",
        hard
    );
}
