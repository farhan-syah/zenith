//! Paint resolvers: turn `PropertyValue`s into concrete `Color`,
//! `GradientPaint`, and `ShadowSpec` values, plus the gradient opacity cascade.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, PropertyValue, ResolvedToken, ResolvedValue};

use crate::color::{parse_color, parse_srgb_hex};
use crate::ir::{Color, GradientPaint, GradientStop, ShadowSpec};

/// Build an [`ir::Color`](Color) from a resolved color token, preserving its
/// CMYK origin when present. Returns `None` only when the resolved value is not
/// a color or its stored hex is somehow unparseable (which token resolution
/// already guarantees not to happen).
fn color_from_resolved(rv: &ResolvedValue) -> Option<Color> {
    let hex = rv.as_color_hex()?;
    let mut color = parse_srgb_hex(hex)?;
    if let Some((c, m, y, k)) = rv.cmyk() {
        color.cmyk = Some([c, m, y, k]);
    }
    Some(color)
}

/// Resolve a `PropertyValue` to a `Color`, or push a diagnostic and return
/// `None`.
///
/// Accepts:
/// - `TokenRef(id)` → looks up in `resolved`, must be a `ResolvedValue::Color`.
/// - `Literal(hex)` → parses as sRGB hex string directly.
pub(super) fn resolve_property_color(
    prop: &PropertyValue,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    subject_id: &str,
) -> Option<Color> {
    match prop {
        PropertyValue::TokenRef(token_id) => {
            match resolved.get(token_id.as_str()) {
                Some(rt) if rt.value.as_color_hex().is_some() => {
                    match color_from_resolved(&rt.value) {
                        Some(c) => Some(c),
                        None => {
                            // Should not happen — token resolution validates the
                            // hex / cmyk literal — but be robust.
                            diagnostics.push(Diagnostic::advisory(
                                "scene.invalid_color",
                                format!(
                                    "token '{}' resolved to an invalid color; skipped",
                                    token_id
                                ),
                                None,
                                Some(subject_id.to_owned()),
                            ));
                            None
                        }
                    }
                }
                Some(rt) => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.wrong_token_type",
                        format!(
                            "node '{}' references token '{}' which resolved to a \
                             non-color value ({:?}); skipped",
                            subject_id, token_id, &rt.value
                        ),
                        None,
                        Some(subject_id.to_owned()),
                    ));
                    None
                }
                None => {
                    diagnostics.push(Diagnostic::advisory(
                        "scene.unresolved_token",
                        format!(
                            "node '{}' references token '{}' which did not resolve \
                             (check token diagnostics); skipped",
                            subject_id, token_id
                        ),
                        None,
                        Some(subject_id.to_owned()),
                    ));
                    None
                }
            }
        }
        PropertyValue::Literal(literal) => match parse_color(literal) {
            Some(c) => Some(c),
            None => {
                diagnostics.push(Diagnostic::advisory(
                    "scene.invalid_color",
                    format!(
                        "node '{}' has a fill literal '{}' that is not a valid \
                         sRGB hex or cmyk(...) color; skipped",
                        subject_id, literal
                    ),
                    None,
                    Some(subject_id.to_owned()),
                ));
                None
            }
        },
        // A dimension is not a color; advise and skip (mirrors wrong-type tokens).
        PropertyValue::Dimension(_) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.wrong_token_type",
                format!(
                    "node '{}' has a dimension value where a color is expected; skipped",
                    subject_id
                ),
                None,
                Some(subject_id.to_owned()),
            ));
            None
        }
    }
}

/// Resolve a fill `PropertyValue` into a [`GradientPaint`], or `None`.
///
/// Returns `Some` only when `prop` is a `TokenRef` whose token resolved to a
/// `ResolvedValue::Gradient`. Each stop's color is resolved from its
/// `color_token_id` via the resolved token map (must be `ResolvedValue::Color`);
/// stops whose color cannot resolve are skipped. Returns `None` (so the caller
/// falls back to the solid path) for non-gradient props, or when fewer than two
/// valid stops survive.
pub(super) fn resolve_property_gradient(
    prop: &PropertyValue,
    resolved: &BTreeMap<String, ResolvedToken>,
    _subject_id: &str,
) -> Option<GradientPaint> {
    let PropertyValue::TokenRef(token_id) = prop else {
        return None;
    };
    let ResolvedValue::Gradient(g) = &resolved.get(token_id.as_str())?.value else {
        return None;
    };

    let mut stops: Vec<GradientStop> = Vec::with_capacity(g.stops.len());
    for (offset, color_token_id) in &g.stops {
        let Some(rt) = resolved.get(color_token_id.as_str()) else {
            continue;
        };
        let Some(color) = color_from_resolved(&rt.value) else {
            continue;
        };
        stops.push(GradientStop {
            offset: *offset,
            color,
        });
    }

    if stops.len() < 2 {
        return None;
    }
    Some(GradientPaint {
        angle_deg: g.angle_deg,
        stops,
    })
}

/// Resolve a `shadow` `PropertyValue` into a list of [`ShadowSpec`] layers, or
/// `None`.
///
/// Mirrors [`resolve_property_gradient`]: returns `Some` only when `prop` is a
/// `TokenRef` whose token resolved to a `ResolvedValue::Shadow`. Each layer's
/// color is resolved from its `color_token` via the resolved token map (must be
/// `ResolvedValue::Color`); layers whose color cannot resolve are skipped.
/// Returns `None` for non-shadow props, or when zero valid layers survive.
pub(super) fn resolve_property_shadow(
    prop: &PropertyValue,
    resolved: &BTreeMap<String, ResolvedToken>,
    _subject_id: &str,
) -> Option<Vec<ShadowSpec>> {
    let PropertyValue::TokenRef(token_id) = prop else {
        return None;
    };
    let ResolvedValue::Shadow(s) = &resolved.get(token_id.as_str())?.value else {
        return None;
    };

    let mut layers: Vec<ShadowSpec> = Vec::with_capacity(s.layers.len());
    for layer in &s.layers {
        let Some(rt) = resolved.get(layer.color_token.as_str()) else {
            continue;
        };
        let Some(color) = color_from_resolved(&rt.value) else {
            continue;
        };
        layers.push(ShadowSpec {
            dx: layer.dx,
            dy: layer.dy,
            blur: layer.blur,
            color,
        });
    }

    if layers.is_empty() {
        return None;
    }
    Some(layers)
}

/// Apply the cascaded opacity multiplier to every stop's alpha, matching the
/// solid path's `color.a = (color.a * node_opacity * ctx.opacity).round()`.
pub(super) fn apply_gradient_opacity(
    gradient: &mut GradientPaint,
    node_opacity: f64,
    ctx_opacity: f64,
) {
    for stop in &mut gradient.stops {
        stop.color.a = (stop.color.a as f64 * node_opacity * ctx_opacity).round() as u8;
    }
}
