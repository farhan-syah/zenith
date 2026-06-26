//! SVG asset → native PDF vector operators.
//!
//! Unlike the raster backend (which rasterizes an SVG to a pixmap), the PDF
//! backend translates an SVG into the document's own vector graphics: each
//! `usvg` path becomes PDF path operators, solid fills become `rg`/`k` fills,
//! and linear gradients become Type 2 axial shadings — so the logo stays crisp
//! at any zoom and the file stays small.
//!
//! The SVG is parsed with the same `usvg` pipeline the raster backend uses
//! (text shaped to outlines against the registered faces, no system fonts), then
//! its node tree is walked with an accumulated affine transform mapping SVG user
//! space → the placement box in scene coordinates. The page's outer y-flip CTM
//! then maps scene space → PDF user space, exactly as for every other primitive.
//!
//! # Coverage and degradations (consistent with the rest of the v0 PDF backend)
//!
//! - **Paths** (lines + quadratic/cubic béziers), **solid fills**, **linear
//!   gradients** (`userSpaceOnUse` exactly; `objectBoundingBox` mapped via the
//!   path's local bbox), **solid strokes**, and **fill-rule** are translated.
//! - **Radial gradients** degrade to a solid fill of the first stop (matching
//!   `fill_region`); **patterns**, **clip-paths**, **masks**, and **nested image
//!   nodes** inside the SVG are skipped. Per-stop gradient alpha is not
//!   representable in an axial shading and is treated as opaque.
//! - Group/fill/stroke opacity and the placement opacity multiply into a single
//!   `ca`/`CA` alpha per paint.

use pdf_writer::Content;
use resvg::usvg::tiny_skia_path::{PathSegment, Point};
use resvg::usvg::{self, NodeKind, Paint, TreeParsing, TreeTextToPath, Units, Visibility};
use zenith_core::FontProvider;
use zenith_scene::{Color, FitMode, ImageClip};

use super::color;
use super::content::{ALPHA_PREFIX, PageResources, SHADING_PREFIX, name, push_gradient};
use super::geometry::{ellipse_path, rounded_rect_path};
use super::gradient::AxialGradient;

/// Where and how an SVG asset is placed on the page. Mirrors the fields the
/// raster image path uses; bundled into a `Copy` struct to stay within the
/// argument-count budget.
#[derive(Clone, Copy)]
pub(super) struct SvgPlacement<'a> {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
    pub(super) fit: FitMode,
    pub(super) pos_x: f64,
    pub(super) pos_y: f64,
    pub(super) opacity: f64,
    pub(super) clip_shape: &'a Option<ImageClip>,
}

/// A 2-D affine map `(x, y) → (a·x + c·y + e, b·x + d·y + f)`, in scene units.
#[derive(Clone, Copy)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Affine {
    /// A pure scale + translate (no rotation/skew).
    fn scale_translate(sx: f64, sy: f64, tx: f64, ty: f64) -> Self {
        Affine {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: tx,
            f: ty,
        }
    }

    /// Convert a `usvg`/`tiny_skia` transform to an [`Affine`].
    fn from_usvg(t: usvg::Transform) -> Self {
        Affine {
            a: f64::from(t.sx),
            b: f64::from(t.ky),
            c: f64::from(t.kx),
            d: f64::from(t.sy),
            e: f64::from(t.tx),
            f: f64::from(t.ty),
        }
    }

    /// `self ∘ inner`: apply `inner` first, then `self`.
    fn then(self, inner: Affine) -> Affine {
        Affine {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }

    /// Map a point.
    fn map(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Map a `tiny_skia` point.
    fn map_pt(self, p: Point) -> (f64, f64) {
        self.map(f64::from(p.x), f64::from(p.y))
    }

    /// Average linear scale factor `√|det|`, used to scale stroke widths.
    fn avg_scale(self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

/// Translate the SVG `bytes` into vector PDF operators placed per `place`.
pub(super) fn emit_svg(
    content: &mut Content,
    res: &mut PageResources,
    fonts: &dyn FontProvider,
    bytes: &[u8],
    place: SvgPlacement<'_>,
) {
    let SvgPlacement {
        x,
        y,
        w,
        h,
        fit,
        pos_x,
        pos_y,
        opacity,
        clip_shape,
    } = place;
    if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite()) {
        return;
    }

    // Parse with the same options + font handling as the raster backend so text
    // resolves identically. Build the fontdb from the registered faces only
    // (deterministic order, no system fonts).
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.set_sans_serif_family("Noto Sans");
    fontdb.set_serif_family("Noto Sans");
    fontdb.set_monospace_family("Noto Sans Mono");
    for face in fonts.all_faces() {
        fontdb.load_font_data(face.bytes.to_vec());
    }
    let opts = usvg::Options {
        font_family: "Noto Sans".to_owned(),
        ..Default::default()
    };
    let Ok(mut tree) = usvg::Tree::from_data(bytes, &opts) else {
        return; // malformed SVG: draw nothing (no fallback raster)
    };
    tree.convert_text(&fontdb);

    let (svw, svh) = (f64::from(tree.size.width()), f64::from(tree.size.height()));
    if !(svw > 0.0 && svh > 0.0) {
        return;
    }

    // Fit transform: SVG viewBox box [0,0,svw,svh] → placement box, preserving
    // aspect per `fit` and `object-position`. Identical math to `emit_image`.
    let (sx, sy, tx, ty) = match fit {
        FitMode::Stretch => (w / svw, h / svh, x, y),
        FitMode::Contain => {
            let s = (w / svw).min(h / svh);
            (
                s,
                s,
                x + (w - svw * s) * pos_x / 100.0,
                y + (h - svh * s) * pos_y / 100.0,
            )
        }
        FitMode::Cover => {
            let s = (w / svw).max(h / svh);
            (
                s,
                s,
                x - (svw * s - w) * pos_x / 100.0,
                y - (svh * s - h) * pos_y / 100.0,
            )
        }
        FitMode::None => (
            1.0,
            1.0,
            x - (svw - w) * pos_x / 100.0,
            y - (svh - h) * pos_y / 100.0,
        ),
    };
    if !(sx.is_finite()
        && sy.is_finite()
        && tx.is_finite()
        && ty.is_finite()
        && sx > 0.0
        && sy > 0.0)
    {
        return;
    }
    let fit = Affine::scale_translate(sx, sy, tx, ty);

    content.save_state();

    // Box clip (rect / inscribed shape), matching the raster image placement so
    // an SVG that overflows its box is clipped identically.
    match clip_shape {
        None => {
            content.rect(x as f32, y as f32, w as f32, h as f32);
            content.clip_nonzero();
            content.end_path();
        }
        Some(ImageClip::Ellipse) => {
            ellipse_path(content, x, y, w, h, None, None);
            content.clip_nonzero();
            content.end_path();
        }
        Some(ImageClip::RoundedRect { radius }) => {
            rounded_rect_path(content, x, y, w, h, [*radius; 4]);
            content.clip_nonzero();
            content.end_path();
        }
    }

    // Walk the tree. The root's children live in the root's local space, which
    // maps to scene via `fit ∘ root.transform`.
    paint_node(content, res, &tree.root, fit, opacity.clamp(0.0, 1.0));

    content.restore_state();
}

/// Recursively emit a node. `parent_to_scene` maps the node's PARENT coordinate
/// space to scene space; `opacity` is the accumulated group/placement alpha.
fn paint_node(
    content: &mut Content,
    res: &mut PageResources,
    node: &usvg::Node,
    parent_to_scene: Affine,
    opacity: f64,
) {
    let kind = node.borrow();
    let local_to_scene = parent_to_scene.then(Affine::from_usvg(kind.transform()));
    match &*kind {
        NodeKind::Group(g) => {
            let child_opacity = opacity * f64::from(g.opacity.get());
            for child in node.children() {
                paint_node(content, res, &child, local_to_scene, child_opacity);
            }
        }
        NodeKind::Path(p) => emit_path(content, res, p, local_to_scene, opacity),
        // Nested raster images and unconverted text are out of scope for v0.
        NodeKind::Image(_) | NodeKind::Text(_) => {}
    }
}

/// Emit one path's fill then stroke under transform `t` and accumulated `opacity`.
fn emit_path(
    content: &mut Content,
    res: &mut PageResources,
    path: &usvg::Path,
    t: Affine,
    opacity: f64,
) {
    if path.visibility != Visibility::Visible {
        return;
    }

    if let Some(fill) = &path.fill {
        let even_odd = matches!(fill.rule, usvg::FillRule::EvenOdd);
        let alpha = opacity * f64::from(fill.opacity.get());
        match &fill.paint {
            Paint::Color(c) => {
                content.save_state();
                set_alpha(content, res, alpha);
                color::set_fill(content, &svg_color(*c));
                let produced = build_path(content, path, t);
                fill_path(content, produced, even_odd);
                content.restore_state();
            }
            Paint::LinearGradient(lg) => {
                if let Some(g) = resolve_linear(lg, path, t) {
                    let id = push_gradient(res, g);
                    content.save_state();
                    set_alpha(content, res, alpha);
                    if build_path(content, path, t) {
                        if even_odd {
                            content.clip_even_odd();
                        } else {
                            content.clip_nonzero();
                        }
                        content.end_path();
                        content.shading(name(SHADING_PREFIX, id).as_name());
                    } else {
                        content.end_path();
                    }
                    content.restore_state();
                }
            }
            Paint::RadialGradient(rg) => {
                // Degrade to a solid fill of the first stop (as `fill_region`).
                if let Some(stop) = rg.stops.first() {
                    content.save_state();
                    set_alpha(content, res, alpha);
                    color::set_fill(content, &svg_color(stop.color));
                    let produced = build_path(content, path, t);
                    fill_path(content, produced, even_odd);
                    content.restore_state();
                }
            }
            Paint::Pattern(_) => {}
        }
    }

    if let Some(stroke) = &path.stroke {
        // Only a solid stroke color is vectorized; a gradient stroke degrades to
        // its first stop. Pattern strokes are skipped.
        let stroke_color = match &stroke.paint {
            Paint::Color(c) => Some(svg_color(*c)),
            Paint::LinearGradient(lg) => lg.stops.first().map(|s| svg_color(s.color)),
            Paint::RadialGradient(rg) => rg.stops.first().map(|s| svg_color(s.color)),
            Paint::Pattern(_) => None,
        };
        if let Some(sc) = stroke_color {
            let alpha = opacity * f64::from(stroke.opacity.get());
            let width = f64::from(stroke.width.get()) * t.avg_scale();
            content.save_state();
            set_alpha(content, res, alpha);
            color::set_stroke(content, &sc);
            content.set_line_width(width as f32);
            if build_path(content, path, t) {
                content.stroke();
            } else {
                content.end_path();
            }
            content.restore_state();
        }
    }
}

/// Emit the path operators for `path.data` under transform `t`. Returns whether
/// any segment was produced. Quadratics are elevated to cubics (PDF has no
/// quadratic operator).
fn build_path(content: &mut Content, path: &usvg::Path, t: Affine) -> bool {
    let mut produced = false;
    let mut cur = (0.0_f64, 0.0_f64);
    for seg in path.data.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                let (px, py) = t.map_pt(p);
                content.move_to(px as f32, py as f32);
                cur = (px, py);
                produced = true;
            }
            PathSegment::LineTo(p) => {
                let (px, py) = t.map_pt(p);
                content.line_to(px as f32, py as f32);
                cur = (px, py);
                produced = true;
            }
            PathSegment::QuadTo(p0, p1) => {
                // Quadratic → cubic: c1 = s + 2/3(q - s), c2 = e + 2/3(q - e).
                let (qx, qy) = t.map_pt(p0);
                let (ex, ey) = t.map_pt(p1);
                let (sx, sy) = cur;
                let c1 = (sx + 2.0 / 3.0 * (qx - sx), sy + 2.0 / 3.0 * (qy - sy));
                let c2 = (ex + 2.0 / 3.0 * (qx - ex), ey + 2.0 / 3.0 * (qy - ey));
                content.cubic_to(
                    c1.0 as f32,
                    c1.1 as f32,
                    c2.0 as f32,
                    c2.1 as f32,
                    ex as f32,
                    ey as f32,
                );
                cur = (ex, ey);
                produced = true;
            }
            PathSegment::CubicTo(p0, p1, p2) => {
                let (c1x, c1y) = t.map_pt(p0);
                let (c2x, c2y) = t.map_pt(p1);
                let (ex, ey) = t.map_pt(p2);
                content.cubic_to(
                    c1x as f32, c1y as f32, c2x as f32, c2y as f32, ex as f32, ey as f32,
                );
                cur = (ex, ey);
                produced = true;
            }
            PathSegment::Close => {
                content.close_path();
            }
        }
    }
    produced
}

/// Apply the fill operator (`f`/`f*`), or `n` when no path was produced.
fn fill_path(content: &mut Content, produced: bool, even_odd: bool) {
    if produced {
        if even_odd {
            content.fill_even_odd();
        } else {
            content.fill_nonzero();
        }
    } else {
        content.end_path();
    }
}

/// Set a fill+stroke alpha ExtGState for `alpha` (0..=1) when below opaque.
fn set_alpha(content: &mut Content, res: &mut PageResources, alpha: f64) {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a < 255 {
        let idx = res.intern_alpha(a);
        content.set_parameters(name(ALPHA_PREFIX, idx).as_name());
    }
}

/// Convert a `usvg` color to an opaque sRGB scene color (alpha rides on `ca`).
fn svg_color(c: usvg::Color) -> Color {
    Color::srgb(c.red, c.green, c.blue, 255)
}

/// Resolve a `usvg` linear gradient over `path` (under transform `t`) into a PDF
/// axial gradient with endpoints in scene space. `userSpaceOnUse` maps the
/// declared endpoints directly; `objectBoundingBox` maps them through the path's
/// local bounding box first. Returns `None` with fewer than two stops.
fn resolve_linear(
    lg: &usvg::LinearGradient,
    path: &usvg::Path,
    t: Affine,
) -> Option<AxialGradient> {
    if lg.stops.len() < 2 {
        return None;
    }
    // Endpoints in the path's local user space.
    let (p1, p2) = match lg.units {
        Units::UserSpaceOnUse => (
            (f64::from(lg.x1), f64::from(lg.y1)),
            (f64::from(lg.x2), f64::from(lg.y2)),
        ),
        Units::ObjectBoundingBox => {
            let b = path.data.bounds();
            let (bx, by) = (f64::from(b.x()), f64::from(b.y()));
            let (bw, bh) = (f64::from(b.width()), f64::from(b.height()));
            (
                (bx + f64::from(lg.x1) * bw, by + f64::from(lg.y1) * bh),
                (bx + f64::from(lg.x2) * bw, by + f64::from(lg.y2) * bh),
            )
        }
    };
    // Apply the gradientTransform, then the path's local→scene transform.
    let g = t.then(Affine::from_usvg(lg.base.transform));
    let (x0, y0) = g.map(p1.0, p1.1);
    let (x1, y1) = g.map(p2.0, p2.1);
    let stops: Vec<(f32, [f32; 3])> = lg
        .stops
        .iter()
        .map(|s| {
            (
                s.offset.get().clamp(0.0, 1.0),
                [
                    f32::from(s.color.red) / 255.0,
                    f32::from(s.color.green) / 255.0,
                    f32::from(s.color.blue) / 255.0,
                ],
            )
        })
        .collect();
    Some(AxialGradient {
        coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
        stops,
    })
}
