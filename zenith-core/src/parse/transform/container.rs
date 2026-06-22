//! Transforms for container renderable nodes: frame, group, table (with its
//! cell/row/column children), and instance (with its override children).

use kdl::KdlNode;

use crate::ast::node::{
    FrameNode, GroupNode, InstanceNode, Override, TableCell, TableColumn, TableNode, TableRow,
    TextSpan,
};
use crate::error::ParseError;

use super::document::transform_children;
use super::helpers::{
    collect_unknown_props, node_span, optional_bool_prop, optional_dimension_prop,
    optional_f64_prop, optional_property_value, optional_property_value_aliased,
    optional_string_prop, optional_string_prop_aliased, optional_u32_prop, required_string_prop,
};
use super::leaf::transform_span;

const FRAME_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "layout",
    "columns",
    "rows",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "blend-mode",
    "blend_mode",
    "blur",
    "style",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-parent",
    "anchor_parent",
];

pub(super) fn transform_frame(node: &KdlNode) -> Result<FrameNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let unknown_props = collect_unknown_props(node, FRAME_KNOWN_PROPS);

    Ok(FrameNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        layout: optional_string_prop(node, "layout").map(str::to_owned),
        columns: optional_u32_prop(node, "columns"),
        rows: optional_u32_prop(node, "rows"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        blend_mode: optional_string_prop_aliased(node, "blend-mode", "blend_mode")
            .map(str::to_owned),
        blur: optional_dimension_prop(node, "blur"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        children: transform_children(node)?,
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const GROUP_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "blend-mode",
    "blend_mode",
    "blur",
    "style",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-parent",
    "anchor_parent",
];

pub(super) fn transform_group(node: &KdlNode) -> Result<GroupNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let unknown_props = collect_unknown_props(node, GROUP_KNOWN_PROPS);

    Ok(GroupNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        blend_mode: optional_string_prop_aliased(node, "blend-mode", "blend_mode")
            .map(str::to_owned),
        blur: optional_dimension_prop(node, "blur"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        children: transform_children(node)?,
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const CELL_KNOWN_PROPS: &[&str] = &[
    "colspan",
    "rowspan",
    "fill",
    "border",
    "border-width",
    "border_width",
    "h-align",
    "h_align",
    "v-align",
    "v_align",
];

const ROW_KNOWN_PROPS: &[&str] = &[];

const COLUMN_KNOWN_PROPS: &[&str] = &["width"];

const TABLE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "header-rows",
    "header_rows",
    "flows",
    "gap",
    "cell-padding",
    "cell_padding",
    "border-collapse",
    "border_collapse",
    "fill",
    "border",
    "border-width",
    "border_width",
    "header-fill",
    "header_fill",
    "header-style",
    "header_style",
    "h-align",
    "h_align",
    "v-align",
    "v_align",
    "style",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-parent",
    "anchor_parent",
];

/// Transform a `cell` child node into a [`TableCell`].
///
/// `colspan`/`rowspan` default to 1; arbitrary child nodes parse via the same
/// [`transform_children`] used for frame/group children.
fn transform_cell(node: &KdlNode) -> Result<TableCell, ParseError> {
    let unknown_props = collect_unknown_props(node, CELL_KNOWN_PROPS);
    Ok(TableCell {
        colspan: optional_u32_prop(node, "colspan").unwrap_or(1),
        rowspan: optional_u32_prop(node, "rowspan").unwrap_or(1),
        children: transform_children(node)?,
        fill: optional_property_value(node, "fill"),
        border: optional_property_value(node, "border"),
        border_width: optional_property_value_aliased(node, "border-width", "border_width"),
        h_align: optional_string_prop_aliased(node, "h-align", "h_align").map(str::to_owned),
        v_align: optional_string_prop_aliased(node, "v-align", "v_align").map(str::to_owned),
        source_span: node_span(node),
        unknown_props,
    })
}

/// Transform a `row` child node into a [`TableRow`] by collecting its `cell`
/// children in source order.
fn transform_row(node: &KdlNode) -> Result<TableRow, ParseError> {
    let mut cells: Vec<TableCell> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "cell" {
                cells.push(transform_cell(child)?);
            }
        }
    }
    let unknown_props = collect_unknown_props(node, ROW_KNOWN_PROPS);
    Ok(TableRow {
        cells,
        source_span: node_span(node),
        unknown_props,
    })
}

pub(super) fn transform_table(node: &KdlNode) -> Result<TableNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Collect `column` and `row` children in source order.
    let mut columns: Vec<TableColumn> = Vec::new();
    let mut rows: Vec<TableRow> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "column" => columns.push(TableColumn {
                    width: optional_dimension_prop(child, "width"),
                    source_span: node_span(child),
                    unknown_props: collect_unknown_props(child, COLUMN_KNOWN_PROPS),
                }),
                "row" => rows.push(transform_row(child)?),
                _ => {}
            }
        }
    }

    let unknown_props = collect_unknown_props(node, TABLE_KNOWN_PROPS);

    Ok(TableNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        columns,
        rows,
        header_rows: optional_u32_prop(node, "header-rows")
            .or_else(|| optional_u32_prop(node, "header_rows")),
        flows: optional_string_prop(node, "flows").map(str::to_owned),
        gap: optional_property_value(node, "gap"),
        cell_padding: optional_property_value_aliased(node, "cell-padding", "cell_padding"),
        border_collapse: optional_string_prop_aliased(node, "border-collapse", "border_collapse")
            .map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        border: optional_property_value(node, "border"),
        border_width: optional_property_value_aliased(node, "border-width", "border_width"),
        header_fill: optional_property_value_aliased(node, "header-fill", "header_fill"),
        header_style: optional_string_prop_aliased(node, "header-style", "header_style")
            .map(str::to_owned),
        h_align: optional_string_prop_aliased(node, "h-align", "h_align").map(str::to_owned),
        v_align: optional_string_prop_aliased(node, "v-align", "v_align").map(str::to_owned),
        style: optional_string_prop(node, "style").map(str::to_owned),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const INSTANCE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "component",
    "x",
    "y",
    "opacity",
    "visible",
    "locked",
];

/// Transform an `instance` node into an [`InstanceNode`].
///
/// Reads required `id` and `component`; optional `x`/`y` origin dimensions,
/// `opacity`/`visible`/`locked`; and collects `override` child nodes into
/// [`Override`]s. Non-`override` children are ignored (forward-compat).
pub(super) fn transform_instance(node: &KdlNode) -> Result<InstanceNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let component = required_string_prop(node, "component")?.to_owned();

    let mut overrides: Vec<Override> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "override" {
                overrides.push(transform_override(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, INSTANCE_KNOWN_PROPS);

    Ok(InstanceNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        component,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        overrides,
        source_span: node_span(node),
        unknown_props,
    })
}

/// Transform an `override ref="..." { … }` instance child into an [`Override`].
///
/// `ref` (required) names a component-LOCAL descendant id. Supported v0 override
/// payload: `span` children (→ `spans`), a `fill` prop, and a `visible` prop.
fn transform_override(node: &KdlNode) -> Result<Override, ParseError> {
    let ref_id = required_string_prop(node, "ref")?.to_owned();

    // Collect `span` children; only set `spans` when at least one is present so
    // an override that does not touch text leaves the target's spans untouched.
    let mut span_list: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                span_list.push(transform_span(child)?);
            }
        }
    }
    let spans = if span_list.is_empty() {
        None
    } else {
        Some(span_list)
    };

    Ok(Override {
        ref_id,
        spans,
        fill: optional_property_value(node, "fill"),
        visible: optional_bool_prop(node, "visible"),
        source_span: node_span(node),
    })
}
