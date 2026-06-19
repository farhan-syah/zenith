//! Raster image decoding (PNG, JPEG) into premultiplied `Pixmap`s.

use tiny_skia::Pixmap;

/// Decode a raster image asset into a premultiplied `Pixmap`.
///
/// Supports PNG (via tiny-skia's built-in decoder) and baseline/progressive
/// JPEG (via `jpeg-decoder`). Returns `None` for unsupported formats or
/// malformed data, in which case the caller skips drawing the asset.
/// Deterministic: pixel output depends only on the input bytes.
pub(super) fn decode_raster_image(bytes: &[u8]) -> Option<Pixmap> {
    // PNG signature.
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Pixmap::decode_png(bytes).ok();
    }
    // JPEG SOI marker.
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return decode_jpeg(bytes);
    }
    None
}

/// Decode a JPEG into an opaque premultiplied `Pixmap`. Handles RGB24 and L8
/// (grayscale) pixel formats; other formats (L16, CMYK32) return `None`.
fn decode_jpeg(bytes: &[u8]) -> Option<Pixmap> {
    use jpeg_decoder::{Decoder, PixelFormat};
    use tiny_skia::PremultipliedColorU8;

    let mut decoder = Decoder::new(std::io::Cursor::new(bytes));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let mut pixmap = Pixmap::new(u32::from(info.width), u32::from(info.height))?;
    let dst = pixmap.pixels_mut();

    match info.pixel_format {
        PixelFormat::RGB24 => {
            for (chunk, px) in pixels.chunks_exact(3).zip(dst.iter_mut()) {
                let [r, g, b] = chunk else { continue };
                // Opaque source: premultiplied == straight at alpha 255.
                *px = PremultipliedColorU8::from_rgba(*r, *g, *b, 255)?;
            }
        }
        PixelFormat::L8 => {
            for (v, px) in pixels.iter().zip(dst.iter_mut()) {
                *px = PremultipliedColorU8::from_rgba(*v, *v, *v, 255)?;
            }
        }
        _ => return None,
    }
    Some(pixmap)
}
