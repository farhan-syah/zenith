//! The render command's error type, output artifacts, and public entry points.

use std::path::Path;

use zenith_core::{BytesAssetProvider, DataContext, Diagnostic, dim_to_px};
use zenith_render::{render_pdf, render_png, render_spread_png};
use zenith_scene::compile_page;

use crate::config::CliPolicyFlags;

use super::assets::{build_asset_provider, build_font_provider, disk_diagnostics};
use super::pipeline::{govern_compile_diagnostics, parse_validate, resolve_page_index};

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
    pub(super) fn new(msg: impl Into<String>, exit_code: u8) -> Self {
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

/// Rendered vector PDF bytes plus the compile-stage diagnostics that produced
/// them.
#[derive(Debug)]
pub struct PdfArtifact {
    /// The encoded PDF bytes.
    pub pdf: Vec<u8>,
    /// Compile-stage diagnostics (advisories/warnings surfaced by `compile`).
    pub diagnostics: Vec<Diagnostic>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

/// Parse `src`, validate it with the merged diagnostic policy, compile the
/// requested `page` (1-based), and return the scene JSON plus the
/// compile-stage diagnostics.
///
/// `project_dir` is the `.zen` file's parent directory. When `Some`, font
/// assets declared in the document are loaded and registered in the font
/// provider so that `font.family` tokens referencing them resolve to the
/// actual face rather than falling back to the bundled Noto fonts. When
/// `None`, only the bundled fonts are available.
///
/// `data` is an optional data context for resolving `(data)"field"` property
/// references at compile time. When `None`, data refs produce
/// `data.missing_field` / `data.no_context` advisories (non-fatal).
///
/// `flags` carries the `--allow`/`--warn`/`--deny` CLI overrides; pass
/// `&CliPolicyFlags::default()` when no flags are available (e.g. MCP).
///
/// Returns `Err` when:
/// - A config file cannot be read (exit code 2).
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - The `page` is out of range (exit code 2).
/// - Scene JSON serialisation fails (exit code 2).
pub fn to_scene_json(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
    flags: &CliPolicyFlags,
    data: Option<&DataContext>,
) -> Result<SceneArtifact, RenderCmdErr> {
    let (doc, policy) = parse_validate(src, project_dir, flags)?;
    let fonts = build_font_provider(&doc, project_dir, false)?;
    let page_index = resolve_page_index(&doc, page)?;
    let compile_result = compile_page(&doc, &fonts, page_index, data);
    let json = compile_result
        .scene
        .to_json()
        .map_err(|e| RenderCmdErr::new(format!("scene serialisation error: {e}"), 2))?;
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(govern_compile_diagnostics(
        compile_result.diagnostics,
        &policy,
    ));
    Ok(SceneArtifact { json, diagnostics })
}

/// Parse `src`, validate it, compile the scene, and return PNG bytes.
///
/// No image or SVG assets are loaded (an empty asset provider is used); any
/// `image`/`svg` nodes are rendered without their content. Use
/// [`to_png_with_dir`] to source asset bytes relative to the document's
/// directory.
///
/// `page` is the 1-based page number to render. No CLI policy flags are
/// applied; config files are still resolved (global only, no `start_dir`).
/// No data context is supplied; data refs produce non-fatal advisories.
///
/// Returns `Err` when:
/// - A config file cannot be read (exit code 2).
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - The `page` is out of range (exit code 2).
/// - Rendering fails (exit code 2).
pub fn to_png(src: &str, page: usize) -> Result<PngArtifact, RenderCmdErr> {
    to_png_with_dir(src, None, page, false, &CliPolicyFlags::default(), None)
}

/// Like [`to_png`], but sources image and SVG asset bytes from `project_dir`
/// (the `.zen` file's parent directory) when provided, and honours the merged
/// diagnostic policy.
///
/// For each `image`- or `svg`-kind `AssetDecl`, the `src` is resolved relative
/// to `project_dir` and read into a [`BytesAssetProvider`]. A read failure
/// silently skips that asset; the missing file is instead surfaced as a hard
/// `asset.missing` Error diagnostic on the returned artifact (which trips the
/// render gate). When `project_dir` is `None` no assets are loaded.
///
/// When `locked` is set, every image and SVG asset's bytes are verified against
/// their declared `sha256` and any mismatch, missing hash, or read failure is a
/// hard error (exit code 2). When `project_dir` is `None` there are no assets,
/// so `locked` is a no-op.
///
/// `page` is the 1-based page number to render.
///
/// `data` is an optional data context for resolving `(data)"field"` property
/// references at compile time. When `None`, data refs produce non-fatal
/// advisories.
///
/// `flags` carries the `--allow`/`--warn`/`--deny` CLI overrides; pass
/// `&CliPolicyFlags::default()` when no flags are available (e.g. MCP).
pub fn to_png_with_dir(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
    locked: bool,
    flags: &CliPolicyFlags,
    data: Option<&DataContext>,
) -> Result<PngArtifact, RenderCmdErr> {
    let (doc, policy) = parse_validate(src, project_dir, flags)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_index = resolve_page_index(&doc, page)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let compile_result = compile_page(&doc, &fonts, page_index, data);
    let png = render_png(&compile_result.scene, &fonts, &assets)
        .map_err(|e| RenderCmdErr::new(format!("render error: {e}"), 2))?;
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(govern_compile_diagnostics(
        compile_result.diagnostics,
        &policy,
    ));
    Ok(PngArtifact { png, diagnostics })
}

/// Parse `src`, validate it with the merged diagnostic policy, compile the
/// requested `page`, and render a vector PDF, sourcing image/SVG and font asset
/// bytes from `project_dir` when provided (exactly like [`to_png_with_dir`]).
///
/// The PDF carries print box metadata (MediaBox / TrimBox / BleedBox /
/// CropBox) and native DeviceCMYK for CMYK-origin colors. Output is
/// deterministic. `page` is the 1-based page number.
///
/// `data` is an optional data context for resolving `(data)"field"` property
/// references at compile time. When `None`, data refs produce non-fatal
/// advisories.
///
/// `flags` carries the `--allow`/`--warn`/`--deny` CLI overrides; pass
/// `&CliPolicyFlags::default()` when no flags are available (e.g. MCP).
pub fn to_pdf_with_dir(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
    locked: bool,
    flags: &CliPolicyFlags,
    data: Option<&DataContext>,
) -> Result<PdfArtifact, RenderCmdErr> {
    let (doc, policy) = parse_validate(src, project_dir, flags)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_index = resolve_page_index(&doc, page)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let compile_result = compile_page(&doc, &fonts, page_index, data);
    let pdf = render_pdf(&compile_result.scene, &fonts, &assets);
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(govern_compile_diagnostics(
        compile_result.diagnostics,
        &policy,
    ));
    Ok(PdfArtifact { pdf, diagnostics })
}

/// Parse `src`, validate it with the merged diagnostic policy, and render
/// EVERY page to PNG, returning one [`PngArtifact`] per page in document
/// order (page 1 first).
///
/// Image and SVG asset bytes are sourced once from `project_dir` (shared
/// across all pages). Returns `Err` on parse failure (exit 2), validation
/// errors (exit 1), an empty document (exit 2), or a render failure (exit 2).
/// When `locked` is set, image and SVG asset bytes are verified against their
/// declared `sha256` (exit 2 on any mismatch/missing hash/read failure).
///
/// `data` is an optional data context for resolving `(data)"field"` property
/// references at compile time (applied to every page). When `None`, data refs
/// produce non-fatal advisories.
///
/// `flags` carries the `--allow`/`--warn`/`--deny` CLI overrides; pass
/// `&CliPolicyFlags::default()` when no flags are available (e.g. MCP).
pub fn to_png_all_pages(
    src: &str,
    project_dir: Option<&Path>,
    locked: bool,
    flags: &CliPolicyFlags,
    data: Option<&DataContext>,
) -> Result<Vec<PngArtifact>, RenderCmdErr> {
    let (doc, policy) = parse_validate(src, project_dir, flags)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_count = doc.body.pages.len();
    if page_count == 0 {
        return Err(RenderCmdErr::new("document has no pages to render", 2));
    }
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let disk_diagnostics = disk_diagnostics(&doc, project_dir);
    let mut artifacts = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        let compile_result = compile_page(&doc, &fonts, page_index, data);
        let png = render_png(&compile_result.scene, &fonts, &assets)
            .map_err(|e| RenderCmdErr::new(format!("render error on page {page_index}: {e}"), 2))?;
        let mut diagnostics = disk_diagnostics.clone();
        diagnostics.extend(govern_compile_diagnostics(
            compile_result.diagnostics,
            &policy,
        ));
        artifacts.push(PngArtifact { png, diagnostics });
    }
    Ok(artifacts)
}

/// Bundled render options for [`to_png_spread`], keeping its argument count
/// within the lint limit (the spread path also takes two page indices and a
/// gutter override). `Copy` so it cascades cheaply.
#[derive(Clone, Copy)]
pub struct SpreadRenderOpts<'a> {
    /// Verify asset sha256 and fail on mismatch.
    pub locked: bool,
    /// Diagnostic-policy CLI flags.
    pub flags: &'a CliPolicyFlags,
    /// Optional data context for `(data)` references.
    pub data: Option<&'a DataContext>,
}

/// Parse `src`, validate it with the merged diagnostic policy, compile pages
/// `page_a` and `page_b` (both 1-based), composite them side by side (A on
/// the left, B on the right), and return the spread PNG bytes plus the merged
/// compile-stage diagnostics.
///
/// The output canvas width is `page_a_width + gutter_override_px + page_b_width`
/// (or `page_a_width + doc.spread_gutter + page_b_width` when the override is
/// `None`, defaulting to 0 when neither is set). A `gutter_px > 0` inserts that
/// many fully-transparent columns between the two pages. Image/SVG/font asset
/// bytes are sourced from `project_dir` (shared across both pages) exactly like
/// [`to_png_with_dir`].
///
/// `data` is an optional data context for resolving `(data)"field"` property
/// references at compile time (applied to both pages). When `None`, data refs
/// produce non-fatal advisories.
///
/// `flags` carries the `--allow`/`--warn`/`--deny` CLI overrides; pass
/// `&CliPolicyFlags::default()` when no flags are available (e.g. MCP).
///
/// Returns `Err` when:
/// - A config file cannot be read (exit code 2).
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - Either page is out of range (exit code 2).
/// - Rendering or compositing fails (exit code 2).
pub fn to_png_spread(
    src: &str,
    project_dir: Option<&Path>,
    page_a: usize,
    page_b: usize,
    gutter_override: Option<u32>,
    opts: SpreadRenderOpts<'_>,
) -> Result<PngArtifact, RenderCmdErr> {
    let SpreadRenderOpts {
        locked,
        flags,
        data,
    } = opts;
    let (doc, policy) = parse_validate(src, project_dir, flags)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let index_a = resolve_page_index(&doc, page_a)?;
    let index_b = resolve_page_index(&doc, page_b)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    // Resolve gutter: CLI override wins, then doc.spread_gutter, then 0.
    let gutter_px = gutter_override.unwrap_or_else(|| {
        doc.spread_gutter
            .as_ref()
            .and_then(|d| dim_to_px(d.value, &d.unit))
            .map(|px| px.max(0.0) as u32)
            .unwrap_or(0)
    });
    let compile_a = compile_page(&doc, &fonts, index_a, data);
    let compile_b = compile_page(&doc, &fonts, index_b, data);
    let png = render_spread_png(
        &compile_a.scene,
        &compile_b.scene,
        gutter_px,
        &fonts,
        &assets,
    )
    .map_err(|e| RenderCmdErr::new(format!("spread render error: {e}"), 2))?;
    let mut compile_diagnostics = compile_a.diagnostics;
    compile_diagnostics.extend(compile_b.diagnostics);
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(govern_compile_diagnostics(compile_diagnostics, &policy));
    Ok(PngArtifact { png, diagnostics })
}
