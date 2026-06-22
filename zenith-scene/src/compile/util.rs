//! Small pure helpers shared across the scene-compilation submodules:
//! rotation-angle extraction, unsupported-unit diagnostics, and dimension
//! property resolution.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, PropertyValue, ResolvedToken, ResolvedValue, Span, Unit, dim_to_px,
};

use crate::ir::BlendMode;

// ── Rotation helper ───────────────────────────────────────────────────────────

/// If `rotate` is a non-zero angle, returns the degrees to rotate the node's
/// commands around its center; else None. (deg unit; value read directly.)
pub(super) fn rotation_degrees(rotate: Option<&Dimension>) -> Option<f64> {
    rotate.map(|d| d.value).filter(|a| *a != 0.0)
}

// ── Blend-mode helper ───────────────────────────────────────────────────────

/// Map a `blend-mode` attribute string to a non-`Normal` [`BlendMode`], or
/// `None` when no compositing layer is needed.
///
/// `None`, `"normal"`, and any unrecognized value all return `None` — those
/// nodes compile to a plain (layer-free) command stream, byte-identical to
/// before blend-mode existed. Only the 11 separable blends open a layer.
pub(super) fn blend_mode_ir(s: Option<&str>) -> Option<BlendMode> {
    match s {
        Some("multiply") => Some(BlendMode::Multiply),
        Some("screen") => Some(BlendMode::Screen),
        Some("overlay") => Some(BlendMode::Overlay),
        Some("darken") => Some(BlendMode::Darken),
        Some("lighten") => Some(BlendMode::Lighten),
        Some("color-dodge") => Some(BlendMode::ColorDodge),
        Some("color-burn") => Some(BlendMode::ColorBurn),
        Some("hard-light") => Some(BlendMode::HardLight),
        Some("soft-light") => Some(BlendMode::SoftLight),
        Some("difference") => Some(BlendMode::Difference),
        Some("exclusion") => Some(BlendMode::Exclusion),
        // "normal", None, and unrecognized values: no layer.
        _ => None,
    }
}

/// Build a `scene.unsupported_unit` advisory for a named geometry field.
///
/// `kind` is the human-readable node kind (e.g. `"rect"`, `"ellipse"`,
/// `"line"`, `"text node"`) used in the diagnostic message.
pub(super) fn unsupported_unit_diag(
    kind: &str,
    node_id: &str,
    field: &str,
    span: Option<Span>,
) -> Diagnostic {
    Diagnostic::advisory(
        "scene.unsupported_unit",
        format!(
            "{} '{}' field '{}' uses an unsupported unit; the {} is skipped",
            kind, node_id, field, kind
        ),
        span,
        Some(node_id.to_owned()),
    )
}

/// Build a `scene.missing_geometry` advisory for a node missing one or more of
/// its `x`/`y`/`w`/`h` geometry properties.
pub(super) fn missing_geometry_diag(kind: &str, node_id: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::advisory(
        "scene.missing_geometry",
        format!(
            "{kind} '{node_id}' is missing one or more geometry properties (x, y, w, h); \
             skipped"
        ),
        span,
        Some(node_id.to_owned()),
    )
}

/// Resolve a single position axis (`x` or `y`) to pixels, honoring a
/// page-relative anchor fallback.
///
/// - `dim = Some` (an explicitly-authored value): converts to px; on an
///   unsupported unit, pushes `scene.unsupported_unit` and returns `None`.
/// - `dim = None`: uses `anchor_val` when present (anchor-derived); otherwise
///   pushes `scene.missing_geometry` and returns `None`.
///
/// A `None` return always means a diagnostic was pushed and the caller must
/// skip the node. An explicit value always wins over the anchor.
pub(super) fn resolve_anchored_axis(
    kind: &str,
    node_id: &str,
    axis: &str,
    dim: Option<&Dimension>,
    anchor_val: Option<f64>,
    span: Option<Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    match dim {
        Some(d) => match dim_to_px(d.value, &d.unit) {
            Some(v) => Some(v),
            None => {
                diagnostics.push(unsupported_unit_diag(kind, node_id, axis, span));
                None
            }
        },
        None => match anchor_val {
            Some(v) => Some(v),
            None => {
                diagnostics.push(missing_geometry_diag(kind, node_id, span));
                None
            }
        },
    }
}

/// Build a `(px)`-unit [`Dimension`] from a raw pixel value.
///
/// Shared by `field` and `footnote` to synthesize geometry for their
/// constructed [`zenith_core::TextNode`]s.
pub(super) fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

/// Resolve an optional dimension-valued property to pixels.
///
/// Returns `default` when the property is absent, is a raw literal, references
/// a non-dimension (or unresolved) token, or carries an unsupported unit. The
/// idiomatic path is a token ref resolving to a `Dimension`. Shared by
/// font-size and stroke-width resolution.
pub(super) fn resolve_property_dimension_px(
    prop: &Option<PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: f64,
) -> f64 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::Dimension(dim) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
                _ => default,
            },
            None => default,
        },
        // A literal dimension (e.g. `font-size=(px)24`) resolves directly,
        // bringing literal visual dimensions to parity with token-backed ones.
        Some(PropertyValue::Dimension(dim)) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
        _ => default,
    }
}
