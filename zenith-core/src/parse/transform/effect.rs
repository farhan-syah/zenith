//! Transforms for effect-producing renderable nodes.

use kdl::KdlNode;

use crate::ast::node::{LightNode, MeshNode};
use crate::error::ParseError;

use super::helpers::{
    collect_unknown_props, node_span, optional_bool_prop, optional_dimension_prop,
    optional_f64_prop, optional_property_value, optional_string_prop, optional_u32_prop,
    required_string_prop,
};

pub(crate) const LIGHT_KNOWN_PROPS: &[&str] = &[
    "id", "name", "role", "kind", "x", "y", "radius", "color", "opacity", "visible", "locked",
    "angle",
];

pub(crate) const MESH_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "kind",
    "x",
    "y",
    "w",
    "h",
    "columns",
    "rows",
    "vanishing-x",
    "vanishing_x",
    "vanishing-y",
    "vanishing_y",
    "extend",
    "stroke",
    "stroke-width",
    "stroke_width",
    "stroke-dash",
    "stroke_dash",
    "stroke-gap",
    "stroke_gap",
    "stroke-linecap",
    "stroke_linecap",
    "opacity",
    "visible",
    "locked",
];

pub(super) fn transform_light(node: &KdlNode) -> Result<LightNode, ParseError> {
    Ok(LightNode {
        id: required_string_prop(node, "id")?.to_owned(),
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        kind: optional_string_prop(node, "kind").map(str::to_owned),
        x: optional_property_value(node, "x"),
        y: optional_property_value(node, "y"),
        radius: optional_property_value(node, "radius"),
        color: optional_property_value(node, "color"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        source_span: node_span(node),
        unknown_props: collect_unknown_props(node, LIGHT_KNOWN_PROPS),
        angle: optional_dimension_prop(node, "angle"),
    })
}

pub(super) fn transform_mesh(node: &KdlNode) -> Result<MeshNode, ParseError> {
    Ok(MeshNode {
        id: required_string_prop(node, "id")?.to_owned(),
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        kind: optional_string_prop(node, "kind").map(str::to_owned),
        x: optional_property_value(node, "x"),
        y: optional_property_value(node, "y"),
        w: optional_property_value(node, "w"),
        h: optional_property_value(node, "h"),
        columns: optional_u32_prop(node, "columns"),
        rows: optional_u32_prop(node, "rows"),
        vanishing_x: optional_property_value(node, "vanishing-x")
            .or_else(|| optional_property_value(node, "vanishing_x")),
        vanishing_y: optional_property_value(node, "vanishing-y")
            .or_else(|| optional_property_value(node, "vanishing_y")),
        extend: optional_property_value(node, "extend"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width: optional_property_value(node, "stroke-width")
            .or_else(|| optional_property_value(node, "stroke_width")),
        stroke_dash: optional_property_value(node, "stroke-dash")
            .or_else(|| optional_property_value(node, "stroke_dash")),
        stroke_gap: optional_property_value(node, "stroke-gap")
            .or_else(|| optional_property_value(node, "stroke_gap")),
        stroke_linecap: optional_string_prop(node, "stroke-linecap")
            .or_else(|| optional_string_prop(node, "stroke_linecap"))
            .map(str::to_owned),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        source_span: node_span(node),
        unknown_props: collect_unknown_props(node, MESH_KNOWN_PROPS),
    })
}
