//! Concrete rasterization backend powered by `tiny-skia`.
//!
//! This is the **only** module in the crate that names `tiny_skia` types or
//! `ttf_parser` types.  All other modules see only the backend-neutral types
//! from `backend.rs`.
//!
//! Self-contained helpers live in focused submodules: image decoding
//! ([`raster`]), gradient shaders ([`gradient`]), drop-shadow blur/compositing
//! ([`shadow`]), geometry/path helpers ([`paths`]), and dimension/pixel-format
//! conversions ([`pixels`]). The command-dispatch render loop — which depends on
//! the loop-local clip/transform stacks and capture state — stays here.

use resvg::usvg;
use resvg::usvg::TreeParsing;
use resvg::usvg::TreeTextToPath;
use tiny_skia::{
    FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};
use zenith_core::{AssetKind, AssetProvider, FontProvider};
use zenith_scene::{FitMode, Scene, SceneCommand, ShadowSpec};

use crate::backend::{RasterBackend, RasterImage};
use crate::error::RenderError;

mod gradient;
mod paths;
mod pixels;
mod raster;
mod shadow;

#[cfg(test)]
mod tests;

use gradient::gradient_shader;
use paths::{
    GlyphOutlinePen, build_poly_path, build_rounded_rect_path, clip_mask, intersect_rects,
};
use pixels::{f64_to_px, premultiplied_to_straight};
use raster::decode_raster_image;
use shadow::composite_shadows;

// ── TinySkiaBackend ───────────────────────────────────────────────────────────

/// CPU rasterization backend backed by the `tiny-skia` library.
///
/// Determinism guarantees:
/// - Anti-aliasing is disabled for rect fills → integer-aligned rects produce
///   exact, reproducible pixels with no sub-pixel variance.
/// - Anti-aliasing is enabled for glyph fills — glyph edges are curved and
///   require AA for legible output. tiny-skia AA is pure-software and
///   deterministic on the same machine (no GPU, no random numbers).
/// - No `HashMap`, no random numbers, no timestamps.
/// - PNG encoding via `tiny_skia::Pixmap::encode_png` writes no timestamps.
pub struct TinySkiaBackend;

impl RasterBackend for TinySkiaBackend {
    fn rasterize(
        &self,
        scene: &Scene,
        fonts: &dyn FontProvider,
        assets: &dyn AssetProvider,
    ) -> Result<RasterImage, RenderError> {
        let width = f64_to_px(scene.width, "width")?;
        let height = f64_to_px(scene.height, "height")?;

        let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
            RenderError::new(format!("failed to allocate pixmap ({width}×{height})"))
        })?;
        // Background starts fully transparent (0,0,0,0) — the deterministic default.

        // Clip stack: each entry is (x, y, x2, y2) in scene coordinates.
        // The outermost clip is the page rectangle.
        let page_clip = (0.0_f64, 0.0_f64, scene.width, scene.height);
        let mut clip_stack: Vec<(f64, f64, f64, f64)> = vec![page_clip];

        // Transform stack: the top entry is the current affine transform applied
        // to every draw. The base entry is identity, so unrotated scenes pass
        // `Transform::identity()` to every draw call (byte-identical to before).
        let mut transform_stack: Vec<Transform> = vec![Transform::identity()];

        // Lazily-built fontdb for SVG text→path conversion. Initialised at most
        // once per render, only when an SVG asset is actually drawn. Never loads
        // system fonts — only the registered faces from `fonts`.
        let mut svg_fontdb: Option<resvg::usvg::fontdb::Database> = None;

        // Active offscreen shadow capture: the target pixmap that buffers the
        // ink of a shadowed leaf node, plus the pending shadow specs to paint at
        // the matching EndShadow. `None` means draws target the real canvas.
        // v0 shadows are leaf-only and never nest (at most one active capture).
        let mut capture: Option<(Pixmap, Vec<ShadowSpec>)> = None;

        for cmd in &scene.commands {
            // Hoist once per iteration. Push/pop arms mutate the stack and
            // never consume current_ts; draw arms read it and never mutate the
            // stack — so hoisting is behavior-identical to reading in each arm.
            let current_ts = *transform_stack.last().unwrap_or(&Transform::identity());

            // ── Structural / capture commands first ───────────────────────────
            // These never draw into a target pixmap; they mutate the clip /
            // transform stacks or open/close the shadow capture, then `continue`
            // so the drawing match below is reached only by drawing commands.
            match cmd {
                SceneCommand::PushClip { x, y, w, h } => {
                    let new_rect = (*x, *y, x + w, y + h);
                    let current = *clip_stack.last().unwrap_or(&page_clip);
                    // Push the intersection so the stack always represents the
                    // effective clip at the current nesting depth.
                    let intersected =
                        intersect_rects(current, new_rect).unwrap_or((0.0, 0.0, 0.0, 0.0)); // empty → degenerate
                    clip_stack.push(intersected);
                    continue;
                }

                // Never pop below the page clip (index 0).
                SceneCommand::PopClip => {
                    if clip_stack.len() > 1 {
                        clip_stack.pop();
                    }
                    continue;
                }

                SceneCommand::PushTransform { angle_deg, cx, cy } => {
                    let rot = Transform::from_rotate_at(*angle_deg as f32, *cx as f32, *cy as f32);
                    transform_stack.push(current_ts.pre_concat(rot));
                    continue;
                }

                SceneCommand::PopTransform => {
                    if transform_stack.len() > 1 {
                        transform_stack.pop();
                    }
                    continue;
                }

                // Open an offscreen capture for shadowed ink. v0 shadows are
                // leaf-only and DO NOT nest; if a capture is already active we
                // keep the current one (inner draws fold into it) rather than
                // crash. On allocation failure we fall back to a no-capture
                // state (nothing is captured; the ink draws crisp, no shadow).
                SceneCommand::BeginShadow { shadows } => {
                    if capture.is_none()
                        && let Some(offscreen) = Pixmap::new(width, height)
                    {
                        capture = Some((offscreen, shadows.clone()));
                    }
                    continue;
                }

                // Close the active capture: paint the blurred shadow layers onto
                // the real canvas, then composite the crisp ink on top.
                SceneCommand::EndShadow => {
                    if let Some((ink, shadows)) = capture.take() {
                        composite_shadows(&mut pixmap, &ink, &shadows, width, height);
                    }
                    continue;
                }

                _ => {}
            }

            // The active drawing target: the offscreen capture when one is open,
            // otherwise the real canvas. Computed once per drawing command, after
            // the structural match above has run (so no borrow overlaps).
            let target: &mut Pixmap = match capture.as_mut() {
                Some((pm, _)) => pm,
                None => &mut pixmap,
            };

            match cmd {
                SceneCommand::FillRect { x, y, w, h, color } => {
                    if current_ts.is_identity() {
                        // ── Unrotated (identity) path — byte-identical to before ──
                        let fill_rect = (*x, *y, x + w, y + h);
                        let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                        // Intersect the fill rect with the current effective clip.
                        let (ix, iy, ix2, iy2) = match intersect_rects(fill_rect, effective_clip) {
                            Some(r) => r,
                            None => continue, // nothing to draw
                        };

                        let iw = ix2 - ix;
                        let ih = iy2 - iy;

                        // tiny-skia requires positive, finite values for Rect::from_xywh.
                        if iw <= 0.0
                            || ih <= 0.0
                            || !ix.is_finite()
                            || !iy.is_finite()
                            || !iw.is_finite()
                            || !ih.is_finite()
                        {
                            continue;
                        }

                        let rect = match Rect::from_xywh(ix as f32, iy as f32, iw as f32, ih as f32)
                        {
                            Some(r) => r,
                            None => continue,
                        };

                        let mut paint = Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = false; // deterministic: no edge AA variance

                        // Drawing outside the pixmap simply touches no pixels; not an error.
                        target.fill_rect(rect, &paint, Transform::identity(), None);
                    } else {
                        // ── Rotated path: fill the rect as a path under the current
                        // transform, AA-on, masked by the (axis-aligned) clip. ──
                        let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                        let mask = match clip_mask(effective_clip, width, height) {
                            None => continue,
                            Some(m) => m,
                        };
                        let Some(rect) =
                            Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                        else {
                            continue;
                        };
                        let path = PathBuilder::from_rect(rect);
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = true;
                        target.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            current_ts,
                            mask.as_ref(),
                        );
                    }
                }

                SceneCommand::FillEllipse { x, y, w, h, color } => {
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Early-out: skip if the ellipse bbox is entirely outside the clip.
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    // Guard against non-finite or degenerate dimensions.
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    // Build the oval at its TRUE bounding box — NOT the intersected box.
                    // Intersecting the bbox before building the oval would reshape (squish)
                    // the ellipse under partial clip; instead we draw the full ellipse and
                    // let the clip mask truncate it.
                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let Some(path) = PathBuilder::from_oval(rect) else {
                        continue; // degenerate rect: skip
                    };

                    // Build clip mask from the effective clip (truncates, not reshapes).
                    // AA-on: curved fill, deterministic same-machine.
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.fill_path(&path, &paint, FillRule::Winding, current_ts, mask.as_ref());
                }

                SceneCommand::StrokeEllipse {
                    x,
                    y,
                    w,
                    h,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Ink-bbox early-out: the stroke extends half its width beyond
                    // the ellipse edge on all sides.
                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    // Build the oval path at its TRUE bounding box — NOT the
                    // intersected box. The clip mask truncates without reshaping.
                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let Some(path) = PathBuilder::from_oval(rect) else {
                        continue; // degenerate rect: skip
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA-on: curved stroke edge, deterministic same-machine.
                    paint.anti_alias = true;

                    target.stroke_path(&path, &paint, &stroke, current_ts, mask.as_ref());
                }

                SceneCommand::StrokeLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    stroke_width,
                } => {
                    // Guard against non-finite or out-of-f32-range values before
                    // building the path — tiny-skia requires finite f32 values,
                    // and a finite-but-huge f64 would overflow to f32::INFINITY.
                    if !x1.is_finite()
                        || !y1.is_finite()
                        || !x2.is_finite()
                        || !y2.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // A line is 1-D so we cannot reshape it to the clip; instead we
                    // compute the ink bounding box (endpoints expanded by half the
                    // stroke width) as a cheap early-out, then clip the stroke to the
                    // effective clip via a mask so sub-page (frame) clips truncate the
                    // line at the frame edge.
                    let half_sw = stroke_width / 2.0;
                    let ink_x = x1.min(*x2) - half_sw;
                    let ink_y = y1.min(*y2) - half_sw;
                    let ink_x2 = x1.max(*x2) + half_sw;
                    let ink_y2 = y1.max(*y2) + half_sw;
                    if intersect_rects((ink_x, ink_y, ink_x2, ink_y2), effective_clip).is_none() {
                        continue;
                    }

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    // Build path: a single open segment from (x1,y1) to (x2,y2).
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x1 as f32, *y1 as f32);
                    pb.line_to(*x2 as f32, *y2 as f32);
                    let path = match pb.finish() {
                        Some(p) => p,
                        None => continue, // degenerate (zero-length) line: skip
                    };

                    // Stroke defaults: Butt cap, Miter join, miter_limit 4.
                    // These are the normative v0 values (doc 09); we intentionally
                    // keep the defaults for cap/join and only set the width, so the
                    // defaults remain authoritative.
                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA on: diagonal lines need sub-pixel coverage; deterministic
                    // same-machine like ellipse/glyph fills.
                    paint.anti_alias = true;

                    target.stroke_path(&path, &paint, &stroke, current_ts, mask.as_ref());
                }

                SceneCommand::DrawGlyphRun {
                    x,
                    y,
                    font_id,
                    font_size,
                    color,
                    glyphs,
                } => {
                    // ── 1. Resolve font bytes ─────────────────────────────────
                    let font_data = match fonts.by_id(font_id) {
                        Some(fd) => fd,
                        None => {
                            // Unknown font id: skip the run silently. The page
                            // renders correctly for all other commands.
                            continue;
                        }
                    };

                    // ── 2. Parse the font face ────────────────────────────────
                    let face = match ttf_parser::Face::parse(&font_data.bytes, font_data.index) {
                        Ok(f) => f,
                        Err(_) => continue, // malformed font bytes: skip run
                    };

                    // ── 3. Compute scale from font units to pixels ────────────
                    let units_per_em = face.units_per_em();
                    if units_per_em == 0 {
                        continue; // degenerate font: skip
                    }
                    let scale = font_size / f32::from(units_per_em);

                    // ── 4. Build the paint for the glyph color ────────────────
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    // AA is on for glyphs: curved outlines need sub-pixel coverage.
                    // tiny-skia AA is pure-software; output is deterministic on
                    // the same machine (no GPU, no random state).
                    paint.anti_alias = true;

                    // ── 5. Build the clip mask (once per run) ─────────────────
                    // Glyph ink is clipped to the effective clip via the mask, so
                    // text inside a frame is truncated at the frame edge; deterministic
                    // same-machine (pure-software AA, no GPU).
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue, // entire run is off-canvas / clip is empty
                        Some(m) => m,
                    };

                    // ── 6. Rasterize each glyph ───────────────────────────────
                    for glyph in glyphs {
                        let origin_x = *x as f32 + glyph.dx;
                        let baseline_y = *y as f32 + glyph.dy;

                        // Build path via outline pen.
                        let mut pen = GlyphOutlinePen::new(origin_x, baseline_y, scale);

                        // outline_glyph returns None for glyphs with no outlines
                        // (e.g. space, .notdef in some fonts). Skip those.
                        if face
                            .outline_glyph(ttf_parser::GlyphId(glyph.glyph_id), &mut pen)
                            .is_none()
                        {
                            continue;
                        }

                        // Finalise the path; None means an empty or degenerate path.
                        let path = match pen.builder.finish() {
                            Some(p) => p,
                            None => continue,
                        };

                        target.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            current_ts,
                            mask.as_ref(),
                        );
                    }
                }

                SceneCommand::DrawImage {
                    x,
                    y,
                    w,
                    h,
                    asset_id,
                    fit,
                    pos_x,
                    pos_y,
                    opacity,
                } => {
                    // ── a. Resolve bytes; only raster images are drawn ────────
                    let Some(asset) = assets.by_id(asset_id) else {
                        continue; // unknown/missing asset: skip (no panic)
                    };
                    // ── b. Produce a raster Pixmap from Image (PNG) or Svg ────
                    let src: Pixmap = match asset.kind {
                        AssetKind::Image => {
                            let Some(p) = decode_raster_image(&asset.bytes) else {
                                continue; // unsupported/malformed raster image: skip
                            };
                            p
                        }
                        AssetKind::Svg => {
                            // Build the fontdb at most once per render, only when
                            // an SVG is drawn. Loaded from the registered faces in
                            // deterministic BTreeMap (by_id) order — no system fonts.
                            let fontdb: &resvg::usvg::fontdb::Database = svg_fontdb
                                .get_or_insert_with(|| {
                                    let mut db = resvg::usvg::fontdb::Database::new();
                                    db.set_sans_serif_family("Noto Sans");
                                    db.set_serif_family("Noto Sans");
                                    db.set_monospace_family("Noto Sans Mono");
                                    for face in fonts.all_faces() {
                                        db.load_font_data(face.bytes.to_vec());
                                    }
                                    db
                                });
                            // Set default font-family so unstyled SVG <text> resolves
                            // to "Noto Sans" instead of the usvg default "Times New Roman".
                            let opts = usvg::Options {
                                font_family: "Noto Sans".to_owned(),
                                ..Default::default()
                            };
                            let Ok(mut usvg_tree) = usvg::Tree::from_data(&asset.bytes, &opts)
                            else {
                                continue; // malformed SVG: skip
                            };
                            usvg_tree.convert_text(fontdb);
                            let sz = usvg_tree.size;
                            let (svw, svh) = (f64::from(sz.width()), f64::from(sz.height()));
                            if !(svw > 0.0 && svh > 0.0) {
                                continue;
                            }
                            // Rasterize at destination resolution so the
                            // downstream bilinear scale is near 1:1 (crisp),
                            // preserving the SVG's own aspect ratio.
                            let raster_scale = ((*w / svw).max(*h / svh)).clamp(0.01, 16.0);
                            let pw = ((svw * raster_scale).ceil() as u32).max(1);
                            let ph = ((svh * raster_scale).ceil() as u32).max(1);
                            let Some(mut pm) = Pixmap::new(pw, ph) else {
                                continue;
                            };
                            let resvg_tree = resvg::Tree::from_usvg(&usvg_tree);
                            resvg_tree.render(
                                Transform::from_scale(raster_scale as f32, raster_scale as f32),
                                &mut pm.as_mut(),
                            );
                            pm
                        }
                        // Font or Unknown: not a drawable image; skip.
                        _ => continue,
                    };
                    let (sw, sh) = (f64::from(src.width()), f64::from(src.height()));
                    if !(sw > 0.0 && sh > 0.0) {
                        continue;
                    }

                    // ── c. Compute the fit transform (sx, sy, tx, ty) ─────────
                    // pos_x / pos_y are 0..=100 object-position anchors.
                    let (sx, sy, tx, ty) = match fit {
                        FitMode::Stretch => (w / sw, h / sh, *x, *y),
                        FitMode::Contain => {
                            let s = (w / sw).min(h / sh);
                            let (rw, rh) = (sw * s, sh * s);
                            let tx = x + (w - rw) * pos_x / 100.0;
                            let ty = y + (h - rh) * pos_y / 100.0;
                            (s, s, tx, ty)
                        }
                        FitMode::Cover => {
                            let s = (w / sw).max(h / sh);
                            let (rw, rh) = (sw * s, sh * s);
                            let tx = x - (rw - w) * pos_x / 100.0;
                            let ty = y - (rh - h) * pos_y / 100.0;
                            (s, s, tx, ty)
                        }
                        FitMode::None => {
                            let tx = x - (sw - w) * pos_x / 100.0;
                            let ty = y - (sh - h) * pos_y / 100.0;
                            (1.0, 1.0, tx, ty)
                        }
                    };
                    if !sx.is_finite()
                        || !sy.is_finite()
                        || !tx.is_finite()
                        || !ty.is_finite()
                        || sx <= 0.0
                        || sy <= 0.0
                    {
                        continue;
                    }

                    // ── d. Build the clip Mask from the effective clip ────────
                    // The compiler emits PushClip(box) before DrawImage, so
                    // clip_stack.last() already equals the image box ∩ enclosing
                    // clips (G-22 box-clip).  clip_mask() handles the full-pixmap
                    // fast path (returns Some(None) → no mask allocation) and the
                    // sub-page case (returns Some(Some(mask))).
                    let mask =
                        match clip_mask(*clip_stack.last().unwrap_or(&page_clip), width, height) {
                            None => continue, // clip fully off-canvas
                            Some(m) => m,
                        };

                    // ── e. Paint: opacity + bilinear filtering ────────────────
                    let paint = PixmapPaint {
                        opacity: (*opacity as f32).clamp(0.0, 1.0),
                        quality: FilterQuality::Bilinear,
                        ..Default::default()
                    };

                    // ── f. Scale + translate transform ────────────────────────
                    // Compose the rotation transform stack on top of the fit
                    // transform. For the identity case `current_ts.pre_concat(fit)`
                    // == `fit`, so the unrotated output is byte-identical.
                    let fit =
                        Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, tx as f32, ty as f32);
                    let transform = current_ts.pre_concat(fit);

                    // ── g. Composite. Box-clip (G-22) is enforced by the Mask;
                    // deterministic same-machine (pure-software bilinear). ─────
                    target.draw_pixmap(0, 0, src.as_ref(), &paint, transform, mask.as_ref());
                }

                SceneCommand::FillPolygon {
                    points,
                    color,
                    even_odd,
                } => {
                    // Guard: need at least 3 points (6 coordinates).
                    if points.len() < 6 {
                        continue;
                    }
                    // Guard: any non-finite coordinate.
                    if points.iter().any(|v| !v.is_finite()) {
                        continue;
                    }

                    let path = match build_poly_path(points, true) {
                        Some(p) => p,
                        None => continue,
                    };

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let fill_rule = if *even_odd {
                        FillRule::EvenOdd
                    } else {
                        FillRule::Winding
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.fill_path(&path, &paint, fill_rule, current_ts, mask.as_ref());
                }

                SceneCommand::StrokePolyline {
                    points,
                    color,
                    stroke_width,
                    closed,
                } => {
                    // Guard: need at least 2 points (4 coordinates).
                    if points.len() < 4 {
                        continue;
                    }
                    // Guard: any non-finite coordinate or invalid stroke_width.
                    if points.iter().any(|v| !v.is_finite())
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                    {
                        continue;
                    }

                    let path = match build_poly_path(points, *closed) {
                        Some(p) => p,
                        None => continue,
                    };

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    // Stroke defaults: Butt cap, Miter join, miter_limit 4 — normative v0.
                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.stroke_path(&path, &paint, &stroke, current_ts, mask.as_ref());
                }

                SceneCommand::StrokeRect {
                    x,
                    y,
                    w,
                    h,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Ink-bbox early-out: the stroke extends half its width beyond
                    // the rect edge on all sides.
                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let path = PathBuilder::from_rect(rect);

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.stroke_path(&path, &paint, &stroke, current_ts, mask.as_ref());
                }

                SceneCommand::FillRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    color,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !radius.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    let Some(path) = build_rounded_rect_path(
                        *x as f32,
                        *y as f32,
                        *w as f32,
                        *h as f32,
                        *radius as f32,
                    ) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.fill_path(&path, &paint, FillRule::Winding, current_ts, mask.as_ref());
                }

                SceneCommand::StrokeRoundedRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    color,
                    stroke_width,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !radius.is_finite()
                        || !stroke_width.is_finite()
                        || *stroke_width > f64::from(f32::MAX)
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    let half_sw = stroke_width / 2.0;
                    if intersect_rects(
                        (x - half_sw, y - half_sw, x + w + half_sw, y + h + half_sw),
                        effective_clip,
                    )
                    .is_none()
                    {
                        continue;
                    }

                    let Some(path) = build_rounded_rect_path(
                        *x as f32,
                        *y as f32,
                        *w as f32,
                        *h as f32,
                        *radius as f32,
                    ) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let stroke = Stroke {
                        width: *stroke_width as f32,
                        ..Default::default()
                    };

                    let mut paint = Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    paint.anti_alias = true;

                    target.stroke_path(&path, &paint, &stroke, current_ts, mask.as_ref());
                }

                SceneCommand::FillRectGradient {
                    x,
                    y,
                    w,
                    h,
                    gradient,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let path = PathBuilder::from_rect(rect);

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let Some(shader) = gradient_shader(*x, *y, *w, *h, gradient) else {
                        continue;
                    };
                    let paint = Paint {
                        shader,
                        anti_alias: true,
                        ..Default::default()
                    };

                    target.fill_path(&path, &paint, FillRule::Winding, current_ts, mask.as_ref());
                }

                SceneCommand::FillRoundedRectGradient {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    gradient,
                } => {
                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || !radius.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    let Some(path) = build_rounded_rect_path(
                        *x as f32,
                        *y as f32,
                        *w as f32,
                        *h as f32,
                        *radius as f32,
                    ) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let Some(shader) = gradient_shader(*x, *y, *w, *h, gradient) else {
                        continue;
                    };
                    let paint = Paint {
                        shader,
                        anti_alias: true,
                        ..Default::default()
                    };

                    target.fill_path(&path, &paint, FillRule::Winding, current_ts, mask.as_ref());
                }

                SceneCommand::FillEllipseGradient {
                    x,
                    y,
                    w,
                    h,
                    gradient,
                } => {
                    let effective_clip = *clip_stack.last().unwrap_or(&page_clip);

                    // Early-out: skip if the ellipse bbox is entirely outside the clip.
                    if intersect_rects((*x, *y, x + w, y + h), effective_clip).is_none() {
                        continue;
                    }

                    if !x.is_finite()
                        || !y.is_finite()
                        || !w.is_finite()
                        || !h.is_finite()
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        continue;
                    }

                    let Some(rect) = Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32)
                    else {
                        continue;
                    };
                    let Some(path) = PathBuilder::from_oval(rect) else {
                        continue;
                    };

                    let mask = match clip_mask(effective_clip, width, height) {
                        None => continue,
                        Some(m) => m,
                    };

                    let Some(shader) = gradient_shader(*x, *y, *w, *h, gradient) else {
                        continue;
                    };
                    let paint = Paint {
                        shader,
                        anti_alias: true,
                        ..Default::default()
                    };

                    target.fill_path(&path, &paint, FillRule::Winding, current_ts, mask.as_ref());
                }

                // PopClip when the stack is already at the page clip (depth 0),
                // and any future variants not yet handled: skip deterministically.
                _ => {}
            }
        }

        // Convert tiny-skia's premultiplied RGBA8 to straight-alpha RGBA8.
        let raw = pixmap.data(); // &[u8], len = width*height*4, premul RGBA
        let mut rgba = Vec::with_capacity(raw.len());
        for chunk in raw.chunks_exact(4) {
            let (sr, sg, sb, sa) =
                premultiplied_to_straight(chunk[0], chunk[1], chunk[2], chunk[3]);
            rgba.push(sr);
            rgba.push(sg);
            rgba.push(sb);
            rgba.push(sa);
        }

        Ok(RasterImage {
            width,
            height,
            rgba,
        })
    }

    fn encode_png(&self, image: &RasterImage) -> Result<Vec<u8>, RenderError> {
        // Re-premultiply straight-alpha back to premultiplied for tiny-skia.
        let mut premul = Vec::with_capacity(image.rgba.len());
        for chunk in image.rgba.chunks_exact(4) {
            let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
            if a == 0 {
                premul.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let a_u16 = u16::from(a);
                let mul = |v: u8| -> u8 {
                    let result = (u16::from(v) * a_u16 + 127) / 255;
                    result.min(255) as u8
                };
                premul.push(mul(r));
                premul.push(mul(g));
                premul.push(mul(b));
                premul.push(a);
            }
        }

        let mut pixmap = Pixmap::new(image.width, image.height).ok_or_else(|| {
            RenderError::new(format!(
                "failed to allocate pixmap for encoding ({}×{})",
                image.width, image.height
            ))
        })?;

        let dst = pixmap.data_mut();
        if dst.len() != premul.len() {
            return Err(RenderError::new(
                "pixel buffer length mismatch during PNG encoding",
            ));
        }
        dst.copy_from_slice(&premul);

        pixmap
            .encode_png()
            .map_err(|e| RenderError::new(format!("PNG encoding failed: {e}")))
    }
}
