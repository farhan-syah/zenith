//! The `RasterBackend` trait — the swappable seam between scene and pixels.
//!
//! No backend-specific types (e.g. tiny-skia) appear anywhere in this module.

use zenith_core::FontProvider;
use zenith_scene::Scene;

use crate::error::RenderError;

/// A rasterized image in straight-alpha RGBA8 format (row-major).
///
/// Pixels are stored as `[r, g, b, a, r, g, b, a, …]` with row stride
/// `width * 4`.  Alpha is **straight** (un-premultiplied), matching the
/// `Color` type in `zenith-scene`.
pub struct RasterImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Raw RGBA8 bytes (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

/// Trait that abstracts over different CPU rasterization backends.
///
/// The associated methods take and return only types from this crate or the
/// standard library — no backend-specific types cross the boundary.
pub trait RasterBackend {
    /// Rasterize a scene to straight-alpha RGBA8 pixels plus dimensions.
    ///
    /// The `fonts` parameter is used to resolve font bytes for glyph runs.
    /// Runs whose font id cannot be resolved are silently skipped — they do
    /// not cause an error.
    fn rasterize(
        &self,
        scene: &Scene,
        fonts: &dyn FontProvider,
    ) -> Result<RasterImage, RenderError>;

    /// Encode a [`RasterImage`] as deterministic PNG bytes.
    fn encode_png(&self, image: &RasterImage) -> Result<Vec<u8>, RenderError>;
}
