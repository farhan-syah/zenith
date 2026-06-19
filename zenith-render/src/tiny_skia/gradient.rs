//! Linear-gradient shader construction for the tiny-skia backend.

use tiny_skia::{
    Color as TsColor, GradientStop as TsGradientStop, LinearGradient, Point, Shader, SpreadMode,
    Transform,
};
use zenith_scene::GradientPaint;

/// Build a tiny-skia linear-gradient [`Shader`] for a fill box.
///
/// The gradient line runs through the box center at `gradient.angle_deg`
/// (clockwise from +x in screen coordinates, so `90°` = top-to-bottom). The
/// line length is the CSS gradient-line length `|w·cosθ| + |h·sinθ|`. Returns
/// `None` when tiny-skia rejects the stops (e.g. fewer than two).
pub(super) fn gradient_shader(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    gradient: &GradientPaint,
) -> Option<Shader<'static>> {
    let theta = gradient.angle_deg.to_radians();
    let (dir_x, dir_y) = (theta.cos(), theta.sin());
    let center = (x + w / 2.0, y + h / 2.0);
    // CSS gradient-line length, then half-extent on each side of center.
    let line_len = (w * dir_x).abs() + (h * dir_y).abs();
    let half = line_len / 2.0;
    let start = Point::from_xy(
        (center.0 - dir_x * half) as f32,
        (center.1 - dir_y * half) as f32,
    );
    let end = Point::from_xy(
        (center.0 + dir_x * half) as f32,
        (center.1 + dir_y * half) as f32,
    );
    let stops: Vec<TsGradientStop> = gradient
        .stops
        .iter()
        .map(|s| {
            TsGradientStop::new(
                s.offset as f32,
                TsColor::from_rgba8(s.color.r, s.color.g, s.color.b, s.color.a),
            )
        })
        .collect();
    LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
}
