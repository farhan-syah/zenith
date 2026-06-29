//! Shared value-extraction and span helpers for the KDL → AST transform.
//!
//! All fallible helpers return `Result<_, ParseError>` so no `.unwrap()` or
//! `.expect()` appears anywhere in the transform.

use std::collections::BTreeMap;

use kdl::{KdlEntry, KdlNode, KdlValue};

use crate::ast::{
    Span,
    node::{ObjectPosition, UnknownProperty, UnknownValue},
    value::{Dimension, PropertyValue, Unit},
};
use crate::error::{ParseError, ParseErrorCode};

// ---------------------------------------------------------------------------
// Span helpers
// ---------------------------------------------------------------------------

pub(super) fn node_span(node: &KdlNode) -> Option<Span> {
    // `KdlNode::span()` returns `miette::SourceSpan` (a transitive type from the
    // `kdl` crate). We read its offset/len via inherent methods and convert at
    // this boundary so the external span type never leaks past the parser.
    let span = node.span();
    let start = span.offset();
    Some(Span {
        start,
        end: start + span.len(),
    })
}

// ---------------------------------------------------------------------------
// Value extraction helpers
// ---------------------------------------------------------------------------

/// Extract the type annotation string from a `KdlEntry`, if present.
pub(super) fn entry_annotation(entry: &KdlEntry) -> Option<&str> {
    entry.ty().map(|id| id.value())
}

/// Convert a `KdlEntry` that carries an annotated or plain value into a
/// `PropertyValue`, handling `(token)"..."` annotations.
pub(super) fn entry_to_property_value(entry: &KdlEntry) -> Result<PropertyValue, ParseError> {
    let annotation = entry_annotation(entry);
    match annotation {
        Some("token") => match entry.value() {
            KdlValue::String(s) => Ok(PropertyValue::TokenRef(s.clone())),
            other => Err(ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("(token) annotation requires a string value, got: {other:?}"),
            )),
        },
        Some("data") => match entry.value() {
            KdlValue::String(s) => Ok(PropertyValue::DataRef(s.clone())),
            other => Err(ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("(data) annotation requires a string value, got: {other:?}"),
            )),
        },
        // A known/unknown unit annotation on a numeric value → dimension literal.
        // This brings literal visual dimensions (e.g. `font-size=(px)24`) to
        // parity with token-backed dimensions. Non-numeric annotated values fall
        // through to the literal branch unchanged.
        Some(ann) => match kdl_value_to_f64(entry.value()) {
            Some(value) => Ok(PropertyValue::Dimension(Dimension {
                value,
                unit: Unit::from_annotation(ann),
            })),
            None => Ok(PropertyValue::Literal(kdl_value_to_literal_string(
                entry.value(),
            ))),
        },
        None => {
            // Treat as a literal, serialised to a string.
            let literal = kdl_value_to_literal_string(entry.value());
            Ok(PropertyValue::Literal(literal))
        }
    }
}

/// Extract an `f64` magnitude from a numeric `KdlValue` (`Integer`/`Float`).
///
/// Returns `None` for non-numeric values. Shared by the dimension extraction in
/// both the geometry and visual-property parse paths so the `KdlValue → f64`
/// conversion lives in exactly one place.
pub(super) fn kdl_value_to_f64(v: &KdlValue) -> Option<f64> {
    match v {
        KdlValue::Integer(n) => Some(*n as f64),
        KdlValue::Float(f) => Some(*f),
        _ => None,
    }
}

pub(super) fn kdl_value_to_literal_string(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(n) => n.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => "null".to_owned(),
    }
}

/// Convert a `KdlEntry` that carries a dimensioned number (e.g. `(px)640`)
/// into a `Dimension`.
pub(super) fn entry_to_dimension(entry: &KdlEntry, prop: &str) -> Result<Dimension, ParseError> {
    let unit_str = entry_annotation(entry).ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::InvalidPropertyValue,
            format!("property `{prop}` requires a unit annotation such as (px) or (pt)"),
        )
    })?;
    let unit = Unit::from_annotation(unit_str);
    let value = kdl_value_to_f64(entry.value()).ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::InvalidPropertyValue,
            format!(
                "property `{prop}` must be numeric, got: {:?}",
                entry.value()
            ),
        )
    })?;
    Ok(Dimension { value, unit })
}

/// Get a required string property value from a node.
pub(super) fn required_string_prop<'a>(
    node: &'a KdlNode,
    key: &str,
) -> Result<&'a str, ParseError> {
    node.get(key)
        .and_then(|v| {
            if let KdlValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!(
                    "node `{}` is missing required string property `{key}`",
                    node.name().value()
                ),
            )
        })
}

/// Get a required integer property from a node and convert to u32.
pub(super) fn required_u32_prop(node: &KdlNode, key: &str) -> Result<u32, ParseError> {
    node.get(key)
        .and_then(|v| {
            if let KdlValue::Integer(n) = v {
                u32::try_from(*n).ok()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!(
                    "node `{}` is missing required integer property `{key}`",
                    node.name().value()
                ),
            )
        })
}

/// Extract an optional non-negative integer property and convert to u32.
///
/// Absent properties, non-integer values, and out-of-range/negative integers
/// (which fail `u32::try_from`) all yield `None`.
pub(super) fn optional_u32_prop(node: &KdlNode, key: &str) -> Option<u32> {
    node.get(key).and_then(|v| {
        if let KdlValue::Integer(n) = v {
            u32::try_from(*n).ok()
        } else {
            None
        }
    })
}

/// Extract an optional boolean property value from a node.
///
/// KDL v2 booleans are the `KdlValue::Bool` variant (`#true`/`#false`).
pub(super) fn optional_bool_prop(node: &KdlNode, key: &str) -> Option<bool> {
    node.get(key).and_then(|v| {
        if let KdlValue::Bool(b) = v {
            Some(*b)
        } else {
            None
        }
    })
}

/// Extract an optional integer property as `i64` (negative values are valid,
/// e.g. a `seed`). Non-integer or absent values yield `None`.
pub(super) fn optional_i64_prop(node: &KdlNode, key: &str) -> Option<i64> {
    node.get(key).and_then(|v| {
        if let KdlValue::Integer(n) = v {
            i64::try_from(*n).ok()
        } else {
            None
        }
    })
}

/// Extract an optional f64 property.
pub(super) fn optional_f64_prop(node: &KdlNode, key: &str) -> Option<f64> {
    node.get(key).and_then(|v| match v {
        KdlValue::Float(f) => Some(*f),
        KdlValue::Integer(n) => Some(*n as f64),
        _ => None,
    })
}

/// Extract an optional string property.
pub(super) fn optional_string_prop<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.get(key).and_then(|v| {
        if let KdlValue::String(s) = v {
            Some(s.as_str())
        } else {
            None
        }
    })
}

/// Extract an optional dimension property from a node's entries.
pub(super) fn optional_dimension_prop(node: &KdlNode, key: &str) -> Option<Dimension> {
    let entry = node.entry(key)?;
    entry_to_dimension(entry, key).ok()
}

/// Extract an optional object-position property from a node.
///
/// Accepts EITHER a plain string anchor (`"start"`/`"center"`/`"end"`) OR a
/// KDL `(pct)N` annotated number → `ObjectPosition::Pct(N)`. Any other string
/// or shape yields `None` (the property is simply absent / unrecognized).
pub(super) fn optional_object_position_prop(node: &KdlNode, key: &str) -> Option<ObjectPosition> {
    let entry = node.entry(key)?;
    // A `(pct)N` annotated number → Pct(N).
    if entry_annotation(entry) == Some("pct") {
        let value = match entry.value() {
            KdlValue::Integer(n) => *n as f64,
            KdlValue::Float(f) => *f,
            _ => return None,
        };
        return Some(ObjectPosition::Pct(value));
    }
    // Otherwise a plain string anchor.
    match entry.value() {
        KdlValue::String(s) => match s.as_str() {
            "start" => Some(ObjectPosition::Start),
            "center" => Some(ObjectPosition::Center),
            "end" => Some(ObjectPosition::End),
            _ => None,
        },
        _ => None,
    }
}

/// Extract an optional property value (token ref or literal) from a node.
pub(super) fn optional_property_value(node: &KdlNode, key: &str) -> Option<PropertyValue> {
    let entry = node.entry(key)?;
    entry_to_property_value(entry).ok()
}

/// Try `primary_key` first, then `alias_key` (supports both hyphenated and
/// underscored spellings of the same property).
pub(super) fn optional_property_value_aliased(
    node: &KdlNode,
    primary_key: &str,
    alias_key: &str,
) -> Option<PropertyValue> {
    optional_property_value(node, primary_key).or_else(|| optional_property_value(node, alias_key))
}

/// Try `primary_key` first, then `alias_key` for string props.
pub(super) fn optional_string_prop_aliased<'a>(
    node: &'a KdlNode,
    primary_key: &str,
    alias_key: &str,
) -> Option<&'a str> {
    optional_string_prop(node, primary_key).or_else(|| optional_string_prop(node, alias_key))
}

/// Like [`required_string_prop`] but tries `primary_key` first, then
/// `alias_key`. Used for hyphenated/underscored prop aliases.
pub(super) fn required_string_prop_aliased<'a>(
    node: &'a KdlNode,
    primary_key: &str,
    alias_key: &str,
) -> Result<&'a str, ParseError> {
    if let Some(v) = optional_string_prop(node, primary_key) {
        return Ok(v);
    }
    required_string_prop(node, alias_key)
}

/// Read a `<key>=(token)"id"` color-token reference off `node`, if present.
///
/// Captures the `(token)` annotation idiom (a `token`-annotated string entry)
/// for an arbitrary prop name (e.g. `color` on a gradient stop / shadow layer,
/// or `shadow`/`highlight` on a duotone filter op). Any other shape (missing,
/// unannotated, non-string) yields `None`.
pub(super) fn optional_token_ref_prop(node: &KdlNode, key: &str) -> Option<String> {
    let entry = node.entry(key)?;
    match (entry_annotation(entry), entry.value()) {
        (Some("token"), KdlValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Map a `KdlValue` to its `UnknownValue` counterpart, preserving type.
pub(super) fn kdl_value_to_unknown_value(v: &KdlValue) -> UnknownValue {
    match v {
        KdlValue::String(s) => UnknownValue::String(s.clone()),
        KdlValue::Integer(n) => UnknownValue::Integer(*n),
        KdlValue::Float(f) => UnknownValue::Float(*f),
        KdlValue::Bool(b) => UnknownValue::Bool(*b),
        KdlValue::Null => UnknownValue::Null,
    }
}

/// Return the compile-time known-props slice for a given node kind string.
///
/// Used by the validator's "did you mean?" suggestion helper to compute
/// edit-distance candidates without duplicating the per-kind arrays. Returns
/// `&[]` for kinds that either have no fixed prop list or are not recognised.
/// The kind strings match the KDL node-name strings (lowercase, no namespace).
pub(crate) fn known_props_for_kind(kind: &str) -> &'static [&'static str] {
    // Import the per-submodule consts here at the point of use.
    use super::chart::CHART_KNOWN_PROPS;
    use super::container::{
        CELL_KNOWN_PROPS, COLUMN_KNOWN_PROPS, FRAME_KNOWN_PROPS, GROUP_KNOWN_PROPS,
        INSTANCE_KNOWN_PROPS, ROW_KNOWN_PROPS, TABLE_KNOWN_PROPS,
    };
    use super::effect::LIGHT_KNOWN_PROPS;
    use super::leaf::{
        CODE_KNOWN_PROPS, ELLIPSE_KNOWN_PROPS, IMAGE_KNOWN_PROPS, LINE_KNOWN_PROPS,
        POLYGON_KNOWN_PROPS, POLYLINE_KNOWN_PROPS, RECT_KNOWN_PROPS, TEXT_KNOWN_PROPS,
    };
    use super::pattern::PATTERN_KNOWN_PROPS;
    use super::special::{
        CONNECTOR_KNOWN_PROPS, FIELD_KNOWN_PROPS, FOOTNOTE_KNOWN_PROPS, SHAPE_KNOWN_PROPS,
        TOC_KNOWN_PROPS,
    };

    match kind {
        "rect" => RECT_KNOWN_PROPS,
        "ellipse" => ELLIPSE_KNOWN_PROPS,
        "image" => IMAGE_KNOWN_PROPS,
        "text" => TEXT_KNOWN_PROPS,
        "code" => CODE_KNOWN_PROPS,
        "line" => LINE_KNOWN_PROPS,
        "polygon" => POLYGON_KNOWN_PROPS,
        "polyline" => POLYLINE_KNOWN_PROPS,
        "frame" => FRAME_KNOWN_PROPS,
        "group" => GROUP_KNOWN_PROPS,
        "table" => TABLE_KNOWN_PROPS,
        "cell" => CELL_KNOWN_PROPS,
        "row" => ROW_KNOWN_PROPS,
        "column" => COLUMN_KNOWN_PROPS,
        "shape" => SHAPE_KNOWN_PROPS,
        "connector" => CONNECTOR_KNOWN_PROPS,
        "field" => FIELD_KNOWN_PROPS,
        "toc" => TOC_KNOWN_PROPS,
        "footnote" => FOOTNOTE_KNOWN_PROPS,
        "pattern" => PATTERN_KNOWN_PROPS,
        "chart" => CHART_KNOWN_PROPS,
        "light" => LIGHT_KNOWN_PROPS,
        "instance" => INSTANCE_KNOWN_PROPS,
        _ => &[],
    }
}

/// Collect all entries that are NOT in `known_keys` into `unknown_props`.
pub(super) fn collect_unknown_props(
    node: &KdlNode,
    known_keys: &[&str],
) -> BTreeMap<String, UnknownProperty> {
    let mut map = BTreeMap::new();
    for entry in node.entries() {
        if let Some(name_id) = entry.name() {
            let key = name_id.value();
            if !known_keys.contains(&key) {
                map.insert(
                    key.to_owned(),
                    UnknownProperty {
                        value: kdl_value_to_unknown_value(entry.value()),
                        ty: entry.ty().map(|id| id.value().to_owned()),
                    },
                );
            }
        }
    }
    map
}
