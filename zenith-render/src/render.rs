//! Public entry points: rasterize a scene to pixels or PNG bytes.

use zenith_core::FontProvider;
use zenith_scene::Scene;

use crate::backend::{RasterBackend, RasterImage};
use crate::error::RenderError;
use crate::tiny_skia::TinySkiaBackend;

/// Rasterize `scene` and encode the result as PNG bytes.
///
/// Uses the [`TinySkiaBackend`] internally.  The output is deterministic:
/// the same scene always produces identical bytes.
///
/// The `fonts` parameter is used to resolve font bytes for any
/// [`zenith_scene::SceneCommand::DrawGlyphRun`] commands in the scene.
/// Runs whose font id cannot be resolved are silently skipped.
///
/// # Errors
///
/// Returns [`RenderError`] when the scene dimensions are invalid or PNG
/// encoding fails.
pub fn render_png(scene: &Scene, fonts: &dyn FontProvider) -> Result<Vec<u8>, RenderError> {
    let backend = TinySkiaBackend;
    let image = backend.rasterize(scene, fonts)?;
    backend.encode_png(&image)
}

/// Rasterize `scene` to a [`RasterImage`] (straight-alpha RGBA8 pixels).
///
/// Useful for pixel-level assertions in tests without decoding a PNG.
///
/// The `fonts` parameter is used to resolve font bytes for any
/// [`zenith_scene::SceneCommand::DrawGlyphRun`] commands in the scene.
/// Runs whose font id cannot be resolved are silently skipped.
///
/// # Errors
///
/// Returns [`RenderError`] when the scene dimensions are invalid.
pub fn render_image(scene: &Scene, fonts: &dyn FontProvider) -> Result<RasterImage, RenderError> {
    let backend = TinySkiaBackend;
    backend.rasterize(scene, fonts)
}
