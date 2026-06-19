//! Pure logic for `zenith render`.
//!
//! Two public entry points:
//! - [`to_scene_json`] — parse → validate → compile → scene JSON string.
//! - [`to_png`]        — parse → validate → compile → PNG bytes.
//!
//! Both operate entirely on in-memory source text; the caller is responsible
//! for all filesystem I/O.

use std::path::Path;

use zenith_core::{
    AssetKind, BytesAssetProvider, Diagnostic, Document, KdlAdapter, KdlSource, default_provider,
    validate,
};
use zenith_render::render_png;
use zenith_scene::compile_page;

// ── Error type ────────────────────────────────────────────────────────────────

/// Error produced by the render command.
#[derive(Debug)]
pub struct RenderCmdErr {
    /// Human-readable message.
    pub message: String,
    /// Recommended exit code.
    pub exit_code: u8,
}

impl RenderCmdErr {
    fn new(msg: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: msg.into(),
            exit_code,
        }
    }
}

// ── Artifacts ─────────────────────────────────────────────────────────────────

/// Scene JSON plus the compile-stage diagnostics that produced it.
#[derive(Debug)]
pub struct SceneArtifact {
    /// The serialised scene JSON.
    pub json: String,
    /// Compile-stage diagnostics (advisories/warnings surfaced by `compile`).
    pub diagnostics: Vec<Diagnostic>,
}

/// Rendered PNG bytes plus the compile-stage diagnostics that produced them.
#[derive(Debug)]
pub struct PngArtifact {
    /// The encoded PNG bytes.
    pub png: Vec<u8>,
    /// Compile-stage diagnostics (advisories/warnings surfaced by `compile`).
    pub diagnostics: Vec<Diagnostic>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

/// Parse `src`, validate it, compile the requested `page` (1-based), and return
/// the scene JSON plus the compile-stage diagnostics.
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - The `page` is out of range (exit code 2).
/// - Scene JSON serialisation fails (exit code 2).
pub fn to_scene_json(src: &str, page: usize) -> Result<SceneArtifact, RenderCmdErr> {
    let provider = default_provider();
    let doc = parse_validate(src)?;
    let page_index = resolve_page_index(&doc, page)?;
    let compile_result = compile_page(&doc, &provider, page_index);
    let json = compile_result
        .scene
        .to_json()
        .map_err(|e| RenderCmdErr::new(format!("scene serialisation error: {e}"), 2))?;
    Ok(SceneArtifact {
        json,
        diagnostics: compile_result.diagnostics,
    })
}

/// Parse `src`, validate it, compile the scene, and return PNG bytes.
///
/// No image assets are loaded (an empty asset provider is used); any `image`
/// nodes are rendered without their raster (the bytes are unavailable). Use
/// [`to_png_with_dir`] to source image bytes relative to the document's
/// directory.
///
/// `page` is the 1-based page number to render.
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - The `page` is out of range (exit code 2).
/// - Rendering fails (exit code 2).
pub fn to_png(src: &str, page: usize) -> Result<PngArtifact, RenderCmdErr> {
    to_png_with_dir(src, None, page)
}

/// Like [`to_png`], but sources image asset bytes from `project_dir` (the
/// `.zen` file's parent directory) when provided.
///
/// For each `image`-kind `AssetDecl`, the `src` is resolved relative to
/// `project_dir` and read into a [`BytesAssetProvider`]. A read failure prints
/// a warning and skips that asset (the matching image is then skipped at
/// render time — never a panic). When `project_dir` is `None` no assets are
/// loaded.
///
/// `page` is the 1-based page number to render.
pub fn to_png_with_dir(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
) -> Result<PngArtifact, RenderCmdErr> {
    let fonts = default_provider();
    let doc = parse_validate(src)?;
    let page_index = resolve_page_index(&doc, page)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir),
        None => BytesAssetProvider::new(),
    };
    let compile_result = compile_page(&doc, &fonts, page_index);
    let png = render_png(&compile_result.scene, &fonts, &assets)
        .map_err(|e| RenderCmdErr::new(format!("render error: {e}"), 2))?;
    Ok(PngArtifact {
        png,
        diagnostics: compile_result.diagnostics,
    })
}

/// Build a [`BytesAssetProvider`] from a parsed document and the project
/// directory (the `.zen` file's parent).
///
/// Only `image`-kind assets are loaded (SVG/font are deferred). On a read
/// failure the asset is skipped with a warning. No `sha256` verification is
/// done in this unit.
//
// TODO(locked): verify sha256 in --locked mode
fn build_asset_provider(doc: &Document, project_dir: &Path) -> BytesAssetProvider {
    let mut provider = BytesAssetProvider::new();
    for decl in &doc.assets.assets {
        if decl.kind != AssetKind::Image {
            continue;
        }
        let path = project_dir.join(&decl.src);
        match std::fs::read(&path) {
            Ok(bytes) => provider.register(&decl.id, AssetKind::Image, bytes.into()),
            Err(e) => {
                eprintln!(
                    "warning: could not read asset '{}' from '{}': {}; image will be skipped",
                    decl.id,
                    path.display(),
                    e
                );
            }
        }
    }
    provider
}

// ── Shared pipeline helper ────────────────────────────────────────────────────

/// Parse → validate, returning the parsed [`Document`].
///
/// Returns early with an error if parse fails (exit code 2) or if validation
/// has errors (exit code 1).
fn parse_validate(src: &str) -> Result<Document, RenderCmdErr> {
    // Parse ─────────────────────────────────────────────────────────────────
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| RenderCmdErr::new(format!("error[parse.error]: {}", e.message), 2))?;

    // Validate ───────────────────────────────────────────────────────────────
    let report = validate(&doc);
    if report.has_errors() {
        let msgs: Vec<String> = report
            .diagnostics
            .iter()
            .filter(|d| d.severity == zenith_core::Severity::Error)
            .map(|d| format!("error[{}]: {}", d.code, d.message))
            .collect();
        return Err(RenderCmdErr::new(msgs.join("\n"), 1));
    }

    Ok(doc)
}

/// Resolve a 1-based `page` number to a 0-based page index within `doc`.
///
/// Returns `Err` (exit code 2) when the document has no pages or when `page`
/// is outside `1..=pages.len()`.
fn resolve_page_index(doc: &Document, page: usize) -> Result<usize, RenderCmdErr> {
    let n = doc.body.pages.len();
    if doc.body.pages.is_empty() || page < 1 || page > n {
        return Err(RenderCmdErr::new(
            format!("page {page} out of range; document has {n} page(s)"),
            2,
        ));
    }
    Ok(page - 1)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let artifact = to_scene_json(UNKNOWN_NODE_DOC, 1).expect("scene must succeed");
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
        let json = to_scene_json(VALID_DOC, 1)
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
        let result = to_scene_json(INVALID_DOC, 1);
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
}
