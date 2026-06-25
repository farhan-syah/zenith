//! Transform for the `chart` node: the data-visualization primitive that
//! carries one or more `series` data children and optional `categories` and
//! `label-colors` children. The common visual/geometry props are read exactly
//! like `pattern`; the chart-specific props (`kind`, `title`, `caption`,
//! `legend`, `axis-min`, `axis-max`, `axis-style`, `bar-mode`,
//! `point-placement`, `value-labels`, `value-color`) describe the chart
//! presentation. Series, categories, and label-colors children are pure data
//! (not renderable nodes).

use kdl::{KdlNode, KdlValue};

use crate::ast::node::{ChartNode, ChartSeries};
use crate::ast::value::PropertyValue;
use crate::error::{ParseError, ParseErrorCode};

use super::helpers::{
    collect_unknown_props, entry_to_property_value, node_span, optional_bool_prop,
    optional_dimension_prop, optional_f64_prop, optional_property_value,
    optional_property_value_aliased, optional_string_prop, optional_string_prop_aliased,
    required_string_prop,
};

pub(crate) const CHART_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "radius",
    "radius-tl",
    "radius_tl",
    "radius-tr",
    "radius_tr",
    "radius-br",
    "radius_br",
    "radius-bl",
    "radius_bl",
    "style",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "stroke-alignment",
    "stroke_alignment",
    "stroke-dash",
    "stroke_dash",
    "stroke-gap",
    "stroke_gap",
    "stroke-linecap",
    "stroke_linecap",
    "shadow",
    "filter",
    "mask",
    "blend-mode",
    "blend_mode",
    "blur",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "border-top",
    "border_top",
    "border-bottom",
    "border_bottom",
    "border-left",
    "border_left",
    "border-right",
    "border_right",
    "border-width",
    "border_width",
    "stroke-outer",
    "stroke_outer",
    "stroke-outer-width",
    "stroke_outer_width",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-sibling",
    "anchor_sibling",
    "anchor-edge",
    "anchor_edge",
    "anchor-gap",
    "anchor_gap",
    "anchor-parent",
    "anchor_parent",
    "kind",
    "title",
    "caption",
    "legend",
    "axis-min",
    "axis_min",
    "axis-max",
    "axis_max",
    "axis-style",
    "axis_style",
    "bar-mode",
    "bar_mode",
    "point-placement",
    "point_placement",
    "value-labels",
    "value_labels",
    "value-color",
    "value_color",
];

pub(super) fn transform_chart(node: &KdlNode) -> Result<ChartNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let kind = required_string_prop(node, "kind")?.to_owned();

    // Common visual props (mirror transform_pattern): accept hyphen + underscore.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_alignment =
        optional_string_prop_aliased(node, "stroke-alignment", "stroke_alignment")
            .map(str::to_owned);
    let stroke_dash = optional_property_value_aliased(node, "stroke-dash", "stroke_dash");
    let stroke_gap = optional_property_value_aliased(node, "stroke-gap", "stroke_gap");
    let stroke_linecap =
        optional_string_prop_aliased(node, "stroke-linecap", "stroke_linecap").map(str::to_owned);
    let blend_mode =
        optional_string_prop_aliased(node, "blend-mode", "blend_mode").map(str::to_owned);

    let radius_tl = optional_property_value_aliased(node, "radius-tl", "radius_tl");
    let radius_tr = optional_property_value_aliased(node, "radius-tr", "radius_tr");
    let radius_br = optional_property_value_aliased(node, "radius-br", "radius_br");
    let radius_bl = optional_property_value_aliased(node, "radius-bl", "radius_bl");

    let border_top = optional_property_value_aliased(node, "border-top", "border_top");
    let border_bottom = optional_property_value_aliased(node, "border-bottom", "border_bottom");
    let border_left = optional_property_value_aliased(node, "border-left", "border_left");
    let border_right = optional_property_value_aliased(node, "border-right", "border_right");
    let border_width = optional_property_value_aliased(node, "border-width", "border_width");
    let stroke_outer = optional_property_value_aliased(node, "stroke-outer", "stroke_outer");
    let stroke_outer_width =
        optional_property_value_aliased(node, "stroke-outer-width", "stroke_outer_width");

    let axis_min =
        optional_f64_prop(node, "axis-min").or_else(|| optional_f64_prop(node, "axis_min"));
    let axis_max =
        optional_f64_prop(node, "axis-max").or_else(|| optional_f64_prop(node, "axis_max"));
    let axis_style =
        optional_string_prop_aliased(node, "axis-style", "axis_style").map(str::to_owned);
    let bar_mode = optional_string_prop_aliased(node, "bar-mode", "bar_mode").map(str::to_owned);
    let point_placement =
        optional_string_prop_aliased(node, "point-placement", "point_placement").map(str::to_owned);
    let value_labels =
        optional_string_prop_aliased(node, "value-labels", "value_labels").map(str::to_owned);
    let value_color = optional_property_value_aliased(node, "value-color", "value_color");

    // Series, categories, and label-colors: each `series` child node becomes a
    // ChartSeries; the single `categories` child carries string positional args as
    // category labels; the single `label-colors` child carries positional
    // PropertyValue entries (e.g. `(token)"color.x"`) as per-slice label colors.
    // All are pure data children — not renderable nodes.
    let mut series: Vec<ChartSeries> = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut label_colors: Vec<PropertyValue> = Vec::new();
    if let Some(children_block) = node.children() {
        for child in children_block.nodes() {
            match child.name().value() {
                "series" => {
                    // Collect positional f64 arguments as data values.
                    let mut values: Vec<f64> = Vec::new();
                    for entry in child.entries() {
                        if entry.name().is_some() {
                            // Named prop — handled separately below.
                            continue;
                        }
                        match entry.value() {
                            KdlValue::Float(v) => values.push(*v),
                            KdlValue::Integer(n) => values.push(*n as f64),
                            other => {
                                return Err(ParseError::spanless(
                                    ParseErrorCode::InvalidPropertyValue,
                                    format!(
                                        "chart '{id}': series value must be a number, got: {other:?}"
                                    ),
                                ));
                            }
                        }
                    }

                    let label = optional_string_prop(child, "label").map(str::to_owned);
                    let color = optional_property_value(child, "color");
                    let label_color =
                        optional_property_value_aliased(child, "label-color", "label_color");
                    let data_ref = optional_string_prop(child, "data-ref")
                        .or_else(|| optional_string_prop(child, "data_ref"))
                        .map(str::to_owned);

                    series.push(ChartSeries {
                        label,
                        color,
                        label_color,
                        data_ref,
                        values,
                    });
                }
                "label-colors" => {
                    // Collect positional PropertyValue entries as per-slice label colors.
                    // Values may be token refs `(token)"id"`, data refs `(data)"path"`,
                    // or bare literals. Named props on the node are skipped.
                    for entry in child.entries() {
                        if entry.name().is_some() {
                            // Named props are not expected on a label-colors node; skip.
                            continue;
                        }
                        label_colors.push(entry_to_property_value(entry)?);
                    }
                }
                "categories" => {
                    // Collect positional string arguments as category labels.
                    for entry in child.entries() {
                        if entry.name().is_some() {
                            // Named props are not expected on a categories node; skip.
                            continue;
                        }
                        match entry.value() {
                            KdlValue::String(s) => categories.push(s.clone()),
                            other => {
                                return Err(ParseError::spanless(
                                    ParseErrorCode::InvalidPropertyValue,
                                    format!(
                                        "chart '{id}': category label must be a string, got: {other:?}"
                                    ),
                                ));
                            }
                        }
                    }
                }
                _ => {
                    // Other children are silently ignored at parse time.
                    // The validator will flag unrecognised child node names if needed.
                }
            }
        }
    }

    let unknown_props = collect_unknown_props(node, CHART_KNOWN_PROPS);

    Ok(ChartNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        radius: optional_property_value(node, "radius"),
        radius_tl,
        radius_tr,
        radius_br,
        radius_bl,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_alignment,
        stroke_dash,
        stroke_gap,
        stroke_linecap,
        border_top,
        border_bottom,
        border_left,
        border_right,
        border_width,
        stroke_outer,
        stroke_outer_width,
        shadow: optional_property_value(node, "shadow"),
        filter: optional_property_value(node, "filter"),
        mask: optional_property_value(node, "mask"),
        blend_mode,
        blur: optional_dimension_prop(node, "blur"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_sibling: optional_string_prop(node, "anchor-sibling")
            .or_else(|| optional_string_prop(node, "anchor_sibling"))
            .map(str::to_owned),
        anchor_edge: optional_string_prop(node, "anchor-edge")
            .or_else(|| optional_string_prop(node, "anchor_edge"))
            .map(str::to_owned),
        anchor_gap: optional_dimension_prop(node, "anchor-gap")
            .or_else(|| optional_dimension_prop(node, "anchor_gap")),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        kind,
        title: optional_string_prop(node, "title").map(str::to_owned),
        caption: optional_string_prop(node, "caption").map(str::to_owned),
        legend: optional_bool_prop(node, "legend"),
        axis_min,
        axis_max,
        axis_style,
        bar_mode,
        point_placement,
        value_labels,
        value_color,
        label_colors,
        categories,
        series,
        source_span: node_span(node),
        unknown_props,
    })
}
