//! Small pure helpers shared across the scene-compilation submodules:
//! rotation-angle extraction, unsupported-unit diagnostics, and dimension
//! property resolution.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, PropertyValue, ResolvedToken, ResolvedValue, Span, Unit, dim_to_px,
};

// ── Rotation helper ───────────────────────────────────────────────────────────

/// If `rotate` is a non-zero angle, returns the degrees to rotate the node's
/// commands around its center; else None. (deg unit; value read directly.)
pub(super) fn rotation_degrees(rotate: Option<&Dimension>) -> Option<f64> {
    rotate.map(|d| d.value).filter(|a| *a != 0.0)
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
