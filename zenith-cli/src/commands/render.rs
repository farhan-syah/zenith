//! Pure logic for `zenith render`.
//!
//! Two public entry points:
//! - [`to_scene_json`] — parse → validate → compile → scene JSON string.
//! - [`to_png`]        — parse → validate → compile → PNG bytes.
//!
//! Both operate entirely on in-memory source text; the caller is responsible
//! for all filesystem I/O.

use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use zenith_core::{
    AssetKind, BytesAssetProvider, BytesFontProvider, Diagnostic, Document, KdlAdapter, KdlSource,
    default_provider, validate,
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
/// `project_dir` is the `.zen` file's parent directory. When `Some`, font
/// assets declared in the document are loaded and registered in the font
/// provider so that `font.family` tokens referencing them resolve to the
/// actual face rather than falling back to the bundled Noto fonts. When
/// `None`, only the bundled fonts are available.
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - The `page` is out of range (exit code 2).
/// - Scene JSON serialisation fails (exit code 2).
pub fn to_scene_json(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
) -> Result<SceneArtifact, RenderCmdErr> {
    let doc = parse_validate(src)?;
    let fonts = build_font_provider(&doc, project_dir, false)?;
    let page_index = resolve_page_index(&doc, page)?;
    let compile_result = compile_page(&doc, &fonts, page_index);
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
/// No image or SVG assets are loaded (an empty asset provider is used); any
/// `image`/`svg` nodes are rendered without their content. Use
/// [`to_png_with_dir`] to source asset bytes relative to the document's
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
    to_png_with_dir(src, None, page, false)
}

/// Like [`to_png`], but sources image and SVG asset bytes from `project_dir`
/// (the `.zen` file's parent directory) when provided.
///
/// For each `image`- or `svg`-kind `AssetDecl`, the `src` is resolved relative
/// to `project_dir` and read into a [`BytesAssetProvider`]. A read failure
/// prints a warning and skips that asset (the node is then skipped at render
/// time — never a panic). When `project_dir` is `None` no assets are loaded.
///
/// When `locked` is set, every image and SVG asset's bytes are verified against
/// their declared `sha256` and any mismatch, missing hash, or read failure is a
/// hard error (exit code 2). When `project_dir` is `None` there are no assets,
/// so `locked` is a no-op.
///
/// `page` is the 1-based page number to render.
pub fn to_png_with_dir(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
    locked: bool,
) -> Result<PngArtifact, RenderCmdErr> {
    let doc = parse_validate(src)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_index = resolve_page_index(&doc, page)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
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

/// Parse `src`, validate it, and render EVERY page to PNG, returning one
/// [`PngArtifact`] per page in document order (page 1 first).
///
/// Image and SVG asset bytes are sourced once from `project_dir` (shared
/// across all pages). Returns `Err` on parse failure (exit 2), validation
/// errors (exit 1), an empty document (exit 2), or a render failure (exit 2).
/// When `locked` is set, image and SVG asset bytes are verified against their
/// declared `sha256` (exit 2 on any mismatch/missing hash/read failure).
pub fn to_png_all_pages(
    src: &str,
    project_dir: Option<&Path>,
    locked: bool,
) -> Result<Vec<PngArtifact>, RenderCmdErr> {
    let doc = parse_validate(src)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_count = doc.body.pages.len();
    if page_count == 0 {
        return Err(RenderCmdErr::new("document has no pages to render", 2));
    }
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let mut artifacts = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        let compile_result = compile_page(&doc, &fonts, page_index);
        let png = render_png(&compile_result.scene, &fonts, &assets)
            .map_err(|e| RenderCmdErr::new(format!("render error on page {page_index}: {e}"), 2))?;
        artifacts.push(PngArtifact {
            png,
            diagnostics: compile_result.diagnostics,
        });
    }
    Ok(artifacts)
}

/// Build a [`BytesFontProvider`] preloaded with bundled fonts and any
/// `font`-kind assets declared in the document.
///
/// When `project_dir` is `None`, returns the default bundled-only provider
/// immediately (no filesystem access is attempted). When `Some`, each
/// `font`-kind [`AssetDecl`] in the document is read from disk and its
/// family/weight/style metadata is extracted via
/// [`zenith_layout::face_metadata`]. Successfully read faces are registered
/// under their real family name so that a `font.family` token whose value
/// matches that family resolves to the actual face instead of falling back
/// to Noto.
///
/// Non-locked failures (unreadable file, unparseable font) emit a warning
/// to stderr and skip the asset. When `locked` is `true`, the same conditions
/// are hard errors (exit code 2), and every font asset's bytes are verified
/// against its declared `sha256` exactly like image and SVG assets.
fn build_font_provider(
    doc: &Document,
    project_dir: Option<&Path>,
    locked: bool,
) -> Result<BytesFontProvider, RenderCmdErr> {
    let mut provider = default_provider();
    let dir = match project_dir {
        Some(d) => d,
        None => return Ok(provider),
    };
    for decl in &doc.assets.assets {
        if decl.kind != AssetKind::Font {
            continue;
        }
        let path = dir.join(&decl.src);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                if locked {
                    return Err(RenderCmdErr::new(
                        format!(
                            "--locked: could not read font asset '{}' from '{}': {}",
                            decl.id,
                            path.display(),
                            e
                        ),
                        2,
                    ));
                }
                eprintln!(
                    "warning: could not read font asset '{}' from '{}': {} — skipping",
                    decl.id,
                    path.display(),
                    e
                );
                continue;
            }
        };

        if locked {
            verify_locked_sha256(&decl.id, "font asset", decl.sha256.as_deref(), &bytes)?;
        }

        let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
        match zenith_layout::face_metadata(&arc, 0) {
            Ok(m) => {
                provider.register(&m.family, m.weight, m.style, arc, 0);
            }
            Err(e) => {
                if locked {
                    return Err(RenderCmdErr::new(
                        format!(
                            "--locked: font asset '{}' could not be parsed: {}",
                            decl.id, e
                        ),
                        2,
                    ));
                }
                eprintln!(
                    "warning: font asset '{}' could not be parsed: {} — skipping",
                    decl.id, e
                );
            }
        }
    }
    Ok(provider)
}

/// Build a [`BytesAssetProvider`] from a parsed document and the project
/// directory (the `.zen` file's parent).
///
/// `image`- and `svg`-kind assets are loaded; `font`-kind assets are handled
/// separately by [`build_font_provider`].
///
/// When `locked` is `false` (the default), a read failure skips the asset with
/// a warning and no hash is checked. When `locked` is `true`, every image or
/// SVG asset must read successfully and its bytes must match its declared
/// `sha256` (compared case-insensitively, trimmed); a read failure, a missing
/// hash, or a mismatch is a hard error (exit code 2).
fn build_asset_provider(
    doc: &Document,
    project_dir: &Path,
    locked: bool,
) -> Result<BytesAssetProvider, RenderCmdErr> {
    let mut provider = BytesAssetProvider::new();
    for decl in &doc.assets.assets {
        if !matches!(decl.kind, AssetKind::Image | AssetKind::Svg) {
            continue;
        }
        let path = project_dir.join(&decl.src);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                if locked {
                    return Err(RenderCmdErr::new(
                        format!(
                            "--locked: could not read asset '{}' from '{}': {}",
                            decl.id,
                            path.display(),
                            e
                        ),
                        2,
                    ));
                }
                eprintln!(
                    "warning: could not read asset '{}' ({}) from '{}': {} — skipping",
                    decl.id,
                    decl.kind.kind_str(),
                    path.display(),
                    e
                );
                continue;
            }
        };

        if locked {
            verify_locked_sha256(&decl.id, "asset", decl.sha256.as_deref(), &bytes)?;
        }

        provider.register(&decl.id, decl.kind.clone(), bytes.into());
    }
    Ok(provider)
}

// ── Shared pipeline helpers ───────────────────────────────────────────────────

/// Verify that `bytes` match the `sha256` field declared on an asset.
///
/// `id` is the asset identifier (for error messages); `kind` is a short noun
/// used in error messages (`"asset"` or `"font asset"`).
///
/// Returns `Err` (exit code 2) when:
/// - `sha256` is `None` (no hash declared).
/// - The computed SHA-256 hex digest does not match `sha256` (case-insensitive,
///   trimmed).
fn verify_locked_sha256(
    id: &str,
    kind: &str,
    sha256: Option<&str>,
    bytes: &[u8],
) -> Result<(), RenderCmdErr> {
    let declared = sha256.ok_or_else(|| {
        RenderCmdErr::new(format!("--locked: {kind} '{id}' has no declared sha256"), 2)
    })?;
    let hex = format!("{:x}", Sha256::digest(bytes));
    if declared.trim().to_lowercase() != hex {
        return Err(RenderCmdErr::new(
            format!("--locked: {kind} '{id}' sha256 mismatch (declared {declared}, actual {hex})"),
            2,
        ));
    }
    Ok(())
}

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
        let artifact = to_scene_json(UNKNOWN_NODE_DOC, None, 1).expect("scene must succeed");
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
        let json = to_scene_json(VALID_DOC, None, 1)
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
        let result = to_scene_json(INVALID_DOC, None, 1);
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
        let artifacts =
            to_png_all_pages(TWO_PAGE_DOC, None, false).expect("all-pages render must succeed");
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
        let err = to_png_all_pages(empty, None, false).expect_err("a doc with no pages must error");
        assert_eq!(err.exit_code, 2);
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
}
