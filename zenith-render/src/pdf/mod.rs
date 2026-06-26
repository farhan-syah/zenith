//! Vector PDF export backend for Zenith.
//!
//! Translates a backend-neutral [`Scene`] display list into a deterministic,
//! print-ready vector PDF: real path/text/shading operators (not a rasterized
//! image), print box metadata (MediaBox / TrimBox / BleedBox / CropBox), and
//! native DeviceCMYK colors for CMYK-origin tokens.
//!
//! # Coordinate system
//!
//! The scene is y-DOWN with the origin at the top-left; PDF user space is y-UP
//! with the origin at the bottom-left. Every page content stream begins with
//! the flip CTM `1 0 0 -1 0 H` (H = canvas height), so a scene point `(x, y)`
//! maps directly to PDF user space with no per-primitive flipping.
//!
//! # Units
//!
//! 1 scene pixel == 1 PDF unit (1/72"). Trim / bleed sizes are already in
//! pixels, so an A5 trim of 1748 px wide becomes a 1748 pt page. The absolute
//! point size is large but the *box ratios* (Trim vs Media vs Bleed) are exact,
//! which is what print pre-flight checks.
//!
//! # Determinism
//!
//! `pdf-writer` writes no `/CreationDate`, `/ModDate`, or document `/ID` unless
//! asked, and uses no time or randomness. Object ids are assigned in a fixed
//! ordered walk of the scene, so identical input yields byte-identical output.
//!
//! # Module layout
//!
//! - `document` — top-level orchestration ([`render_pdf`]); page boxes,
//!   resource collection, object-id allocation, trailer.
//! - `content`  — the scene-command → content-operator translator.
//! - `color`    — `Color` → DeviceRGB / DeviceCMYK fill+stroke ops, alpha
//!   ExtGState allocation.
//! - `geometry` — rounded-rect / ellipse bezier path builders and the glyph
//!   outline pen.
//! - `gradient` — linear gradient → PDF axial (Type 2) shading dictionaries.
//! - `image`    — raster image → FlateDecode RGB image XObject (+ alpha SMask).
//! - `svg`      — SVG asset → native PDF vector operators (paths + shadings).

mod color;
mod content;
mod document;
mod geometry;
mod gradient;
mod image;
mod raster_embed;
mod svg;

#[cfg(test)]
mod tests;

pub use document::{render_pdf, render_pdf_multi};
