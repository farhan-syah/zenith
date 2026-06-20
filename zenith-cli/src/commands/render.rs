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
    AssetKind, BytesAssetProvider, BytesFontProvider, Diagnostic, Document, ImageNode, KdlAdapter,
    KdlSource, Node, default_provider, dim_to_px, validate,
};
use zenith_render::{render_pdf, render_png, render_spread_png};
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
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(compile_result.diagnostics);
    Ok(SceneArtifact { json, diagnostics })
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
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(compile_result.diagnostics);
    Ok(PngArtifact { png, diagnostics })
}

/// Parse `src`, validate it, compile the requested `page`, and render a vector
/// PDF, sourcing image/SVG and font asset bytes from `project_dir` when
/// provided (exactly like [`to_png_with_dir`]).
///
/// The PDF carries print box metadata (MediaBox / TrimBox / BleedBox /
/// CropBox) and native DeviceCMYK for CMYK-origin colors. Output is
/// deterministic. `page` is the 1-based page number.
pub fn to_pdf_with_dir(
    src: &str,
    project_dir: Option<&Path>,
    page: usize,
    locked: bool,
) -> Result<PdfArtifact, RenderCmdErr> {
    let doc = parse_validate(src)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let page_index = resolve_page_index(&doc, page)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let compile_result = compile_page(&doc, &fonts, page_index);
    let pdf = render_pdf(&compile_result.scene, &fonts, &assets);
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(compile_result.diagnostics);
    Ok(PdfArtifact { pdf, diagnostics })
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
    let disk_diagnostics = disk_diagnostics(&doc, project_dir);
    let mut artifacts = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        let compile_result = compile_page(&doc, &fonts, page_index);
        let png = render_png(&compile_result.scene, &fonts, &assets)
            .map_err(|e| RenderCmdErr::new(format!("render error on page {page_index}: {e}"), 2))?;
        let mut diagnostics = disk_diagnostics.clone();
        diagnostics.extend(compile_result.diagnostics);
        artifacts.push(PngArtifact { png, diagnostics });
    }
    Ok(artifacts)
}

/// Parse `src`, validate it, compile pages `page_a` and `page_b` (both 1-based),
/// composite them side by side (A on the left, B on the right), and return the
/// spread PNG bytes plus the merged compile-stage diagnostics.
///
/// The output canvas width is `page_a_width + page_b_width`; its height is the
/// max of the two page heights. Image/SVG/font asset bytes are sourced from
/// `project_dir` (shared across both pages) exactly like [`to_png_with_dir`].
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - Either page is out of range (exit code 2).
/// - Rendering or compositing fails (exit code 2).
pub fn to_png_spread(
    src: &str,
    project_dir: Option<&Path>,
    page_a: usize,
    page_b: usize,
    locked: bool,
) -> Result<PngArtifact, RenderCmdErr> {
    let doc = parse_validate(src)?;
    let fonts = build_font_provider(&doc, project_dir, locked)?;
    let index_a = resolve_page_index(&doc, page_a)?;
    let index_b = resolve_page_index(&doc, page_b)?;
    let assets = match project_dir {
        Some(dir) => build_asset_provider(&doc, dir, locked)?,
        None => BytesAssetProvider::new(),
    };
    let compile_a = compile_page(&doc, &fonts, index_a);
    let compile_b = compile_page(&doc, &fonts, index_b);
    let png = render_spread_png(&compile_a.scene, &compile_b.scene, &fonts, &assets)
        .map_err(|e| RenderCmdErr::new(format!("spread render error: {e}"), 2))?;
    let mut diagnostics = disk_diagnostics(&doc, project_dir);
    diagnostics.extend(compile_a.diagnostics);
    diagnostics.extend(compile_b.diagnostics);
    Ok(PngArtifact { png, diagnostics })
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
/// Non-locked failures (unreadable file, unparseable font) silently skip the
/// asset; a missing file is instead reported as a hard `asset.missing` Error
/// diagnostic by [`collect_missing_asset_diagnostics`]. When `locked` is `true`,
/// the same conditions
/// are hard errors (exit code 2), and every font asset's bytes are verified
/// against its declared `sha256` exactly like image and SVG assets.
pub(crate) fn build_font_provider(
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
                // Missing/unreadable file is surfaced as a hard `asset.missing`
                // diagnostic by `collect_missing_asset_diagnostics`; skip here.
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
/// When `locked` is `false` (the default), a read failure silently skips the
/// asset and no hash is checked (a missing file is surfaced separately as a hard
/// `asset.missing` diagnostic). When `locked` is `true`, every image or
/// SVG asset must read successfully and its bytes must match its declared
/// `sha256` (compared case-insensitively, trimmed); a read failure, a missing
/// hash, or a mismatch is a hard error (exit code 2).
pub(crate) fn build_asset_provider(
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
                // Missing/unreadable file is surfaced as a hard `asset.missing`
                // diagnostic by `collect_missing_asset_diagnostics`; skip here.
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

/// Collect a hard `asset.missing` diagnostic for every declared asset whose
/// file does not exist on disk under `project_dir`.
///
/// All asset kinds are checked (image, svg, font). Declarations are iterated in
/// declaration order, so the resulting diagnostics are deterministic. The
/// returned diagnostics are `Severity::Error`, so once prepended to a render
/// artifact's diagnostics they trip the render gate and block output.
pub(crate) fn collect_missing_asset_diagnostics(
    doc: &Document,
    project_dir: &Path,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for decl in &doc.assets.assets {
        let path = project_dir.join(&decl.src);
        if !path.exists() {
            diagnostics.push(Diagnostic::error(
                "asset.missing",
                format!("asset '{}' file not found: '{}'", decl.id, path.display()),
                decl.source_span,
                Some(decl.id.clone()),
            ));
        }
    }
    diagnostics
}

/// Collect `image.overflow` and `image.upscale` advisories for all image nodes
/// in the document.
///
/// - **`image.overflow`** (fit="none" only): the image's intrinsic pixel
///   dimensions exceed the declared box, so the image clips unexpectedly.
/// - **`image.upscale`**: the image will be rendered LARGER than its intrinsic
///   pixels (raster will appear pixelated), computed per the active fit mode.
///
/// SVG assets are exempt (vector, scales cleanly). Image nodes whose box uses
/// `(pct)` or other non-absolute units are skipped (not false positives).
/// Nodes referencing unknown or missing assets are skipped (covered elsewhere).
/// Both diagnostics are `Severity::Advisory` and do NOT block rendering.
pub fn collect_image_dimension_diagnostics(doc: &Document, project_dir: &Path) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for page in &doc.body.pages {
        walk_images(&page.children, doc, project_dir, &mut out);
    }
    out
}

/// Collect all disk-based diagnostics (`asset.missing` + `image.overflow` /
/// `image.upscale`) for a document and its project directory.
///
/// When `project_dir` is `None`, no filesystem access is attempted and an
/// empty `Vec` is returned. When `Some`, both
/// [`collect_missing_asset_diagnostics`] and
/// [`collect_image_dimension_diagnostics`] are run and their results merged.
/// This is the single call-site replacement for the repeated inline block:
/// ```text
/// match project_dir {
///     Some(dir) => { let mut d = collect_missing...; d.extend(collect_image...); d }
///     None => Vec::new(),
/// }
/// ```
fn disk_diagnostics(doc: &Document, project_dir: Option<&Path>) -> Vec<Diagnostic> {
    match project_dir {
        Some(dir) => {
            let mut d = collect_missing_asset_diagnostics(doc, dir);
            d.extend(collect_image_dimension_diagnostics(doc, dir));
            d
        }
        None => Vec::new(),
    }
}

/// Recursively walk `nodes`, collecting image dimension diagnostics.
///
/// Containers (`Frame`, `Group`) are recursed into. All other node variants
/// are listed explicitly and treated as no-ops (exhaustive match guards against
/// silently missing a future container type).
fn walk_images(nodes: &[Node], doc: &Document, project_dir: &Path, out: &mut Vec<Diagnostic>) {
    for node in nodes {
        match node {
            Node::Image(img) => {
                check_image(img, doc, project_dir, out);
            }
            Node::Frame(f) => {
                walk_images(&f.children, doc, project_dir, out);
            }
            Node::Group(g) => {
                walk_images(&g.children, doc, project_dir, out);
            }
            // Leaf nodes that cannot contain children — explicit for exhaustiveness:
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Check a single image node and push any `image.overflow` / `image.upscale`
/// advisories into `out`.
fn check_image(img: &ImageNode, doc: &Document, project_dir: &Path, out: &mut Vec<Diagnostic>) {
    // Resolve box dimensions to pixels — skip if either axis uses a non-pixel
    // unit (pct, deg, unknown) to avoid false positives.
    let w_dim = match img.w.as_ref() {
        Some(d) => d,
        None => return,
    };
    let h_dim = match img.h.as_ref() {
        Some(d) => d,
        None => return,
    };
    let w = match dim_to_px(w_dim.value, &w_dim.unit) {
        Some(px) => px,
        None => return,
    };
    let h = match dim_to_px(h_dim.value, &h_dim.unit) {
        Some(px) => px,
        None => return,
    };

    // Look up the asset declaration — skip if unknown (unknown_reference handles it).
    let decl = match doc.assets.assets.iter().find(|d| d.id == img.asset) {
        Some(d) => d,
        None => return,
    };

    // SVG assets are vector — they scale without quality loss; skip.
    if decl.kind != AssetKind::Image {
        return;
    }

    // Read only the image header (cheap — no full decode).
    let path = project_dir.join(&decl.src);
    let isz = match imagesize::size(&path) {
        Ok(s) => s,
        Err(_) => return, // missing/unreadable — asset.missing covers it
    };
    let iw = isz.width as f64;
    let ih = isz.height as f64;

    let fit = img.fit.as_deref();

    // ── image.overflow ───────────────────────────────────────────────────────
    // Only emitted for fit="none": the image is placed at intrinsic size with
    // no scaling, so if intrinsic > box the image clips.
    if fit == Some("none") && (iw > w || ih > h) {
        out.push(Diagnostic::advisory(
            "image.overflow",
            format!(
                "image '{}': intrinsic size {}x{} exceeds its box {}x{} (fit=\"none\")",
                img.id, iw as u32, ih as u32, w as u32, h as u32,
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // ── image.upscale ────────────────────────────────────────────────────────
    // Emitted when the rendered size is larger than the intrinsic pixel count,
    // per fit mode. fit="none" never upscales (image is placed at intrinsic
    // size). Unknown fit strings are skipped (validate already warns).
    let upscales = match fit {
        Some("none") => false,
        Some("stretch") | None => w > iw || h > ih,
        Some("contain") => {
            // Scale factor = min of both axes; upscale when that factor > 1.
            let s = (w / iw).min(h / ih);
            s > 1.0
        }
        Some("cover") => {
            // Scale factor = max of both axes; upscale when that factor > 1.
            let s = (w / iw).max(h / ih);
            s > 1.0
        }
        Some(_) => false, // unknown fit string — skip
    };

    if upscales {
        out.push(Diagnostic::advisory(
            "image.upscale",
            format!(
                "image '{}': rendered larger than its intrinsic {}x{} px; raster will appear pixelated",
                img.id,
                iw as u32,
                ih as u32,
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }
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
        let artifact = to_png_with_dir(MISSING_ASSET_DOC, Some(dir), 1, false)
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
        let artifact =
            to_scene_json(MISSING_ASSET_DOC, Some(dir), 1).expect("scene JSON must succeed");
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
}
