//! Transforms for the specialized renderable nodes: field, toc, footnote,
//! shape, and connector.

use kdl::KdlNode;

use crate::ast::node::{ConnectorNode, FieldNode, FootnoteNode, ShapeNode, TextSpan, TocNode};
use crate::error::ParseError;

use super::helpers::{
    collect_unknown_props, node_span, optional_bool_prop, optional_dimension_prop,
    optional_f64_prop, optional_property_value, optional_property_value_aliased,
    optional_string_prop, optional_string_prop_aliased, required_string_prop,
};
use super::leaf::transform_span;

const SHAPE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "kind",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "radius",
    "stroke-alignment",
    "stroke_alignment",
    "padding",
    "h-align",
    "h_align",
    "v-align",
    "v_align",
    "text-style",
    "text_style",
    "style",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-sibling",
    "anchor_sibling",
    "anchor-parent",
    "anchor_parent",
];

pub(super) fn transform_shape(node: &KdlNode) -> Result<ShapeNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let mut spans: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                spans.push(transform_span(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, SHAPE_KNOWN_PROPS);

    Ok(ShapeNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        kind: optional_string_prop(node, "kind").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width: optional_property_value_aliased(node, "stroke-width", "stroke_width"),
        radius: optional_property_value(node, "radius"),
        stroke_alignment: optional_string_prop_aliased(
            node,
            "stroke-alignment",
            "stroke_alignment",
        )
        .map(str::to_owned),
        padding: optional_property_value(node, "padding"),
        h_align: optional_string_prop_aliased(node, "h-align", "h_align").map(str::to_owned),
        v_align: optional_string_prop_aliased(node, "v-align", "v_align").map(str::to_owned),
        text_style: optional_string_prop_aliased(node, "text-style", "text_style")
            .map(str::to_owned),
        spans,
        style: optional_string_prop(node, "style").map(str::to_owned),
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
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const CONNECTOR_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "from",
    "to",
    "from-anchor",
    "from_anchor",
    "to-anchor",
    "to_anchor",
    "route",
    "marker-start",
    "marker_start",
    "marker-end",
    "marker_end",
    "stroke",
    "stroke-width",
    "stroke_width",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

pub(super) fn transform_connector(node: &KdlNode) -> Result<ConnectorNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let unknown_props = collect_unknown_props(node, CONNECTOR_KNOWN_PROPS);

    Ok(ConnectorNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        from: optional_string_prop(node, "from").map(str::to_owned),
        to: optional_string_prop(node, "to").map(str::to_owned),
        from_anchor: optional_string_prop_aliased(node, "from-anchor", "from_anchor")
            .map(str::to_owned),
        to_anchor: optional_string_prop_aliased(node, "to-anchor", "to_anchor").map(str::to_owned),
        route: optional_string_prop(node, "route").map(str::to_owned),
        marker_start: optional_string_prop_aliased(node, "marker-start", "marker_start")
            .map(str::to_owned),
        marker_end: optional_string_prop_aliased(node, "marker-end", "marker_end")
            .map(str::to_owned),
        stroke: optional_property_value(node, "stroke"),
        stroke_width: optional_property_value_aliased(node, "stroke-width", "stroke_width"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        source_span: node_span(node),
        unknown_props,
    })
}

const FIELD_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "type",
    "recto",
    "verso",
    "target",
    "folio-style",
    "folio_style",
    "suppress-first",
    "suppress_first",
    "x",
    "y",
    "w",
    "h",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "opacity",
    "visible",
    "locked",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-sibling",
    "anchor_sibling",
    "anchor-parent",
    "anchor_parent",
];

/// Transform a `field` node into a [`FieldNode`].
///
/// Reads required `id` and `type`; optional `recto`/`verso`/`target` strings;
/// optional `x`/`y`/`w`/`h` geometry; and visual props (`style`/`fill`/
/// `font-family`/`font-size`). The `type` value is preserved verbatim (the
/// validator warns on an unknown type), as is `target` (the compiler resolves
/// the page-ref and the validator warns on an unresolved one).
pub(super) fn transform_field(node: &KdlNode) -> Result<FieldNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let field_type = required_string_prop(node, "type")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let folio_style =
        optional_string_prop_aliased(node, "folio-style", "folio_style").map(str::to_owned);
    let suppress_first = optional_bool_prop(node, "suppress-first")
        .or_else(|| optional_bool_prop(node, "suppress_first"));

    let unknown_props = collect_unknown_props(node, FIELD_KNOWN_PROPS);

    Ok(FieldNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        field_type,
        recto: optional_string_prop(node, "recto").map(str::to_owned),
        verso: optional_string_prop(node, "verso").map(str::to_owned),
        target: optional_string_prop(node, "target").map(str::to_owned),
        folio_style,
        suppress_first,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_sibling: optional_string_prop(node, "anchor-sibling")
            .or_else(|| optional_string_prop(node, "anchor_sibling"))
            .map(str::to_owned),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const TOC_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "match-role",
    "match_role",
    "match-style",
    "match_style",
    "leader",
    "folio-style",
    "folio_style",
    "x",
    "y",
    "w",
    "h",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "opacity",
    "visible",
    "locked",
    "anchor",
    "anchor-zone",
    "anchor_zone",
    "anchor-sibling",
    "anchor_sibling",
    "anchor-parent",
    "anchor_parent",
];

/// Transform a `toc` node into a [`TocNode`].
///
/// Reads required `id`; optional selector props (`match-role`, `match-style`);
/// optional `leader` and `folio-style` strings; optional `x`/`y`/`w`/`h`
/// geometry; and visual props (`style`/`fill`/`font-family`/`font-size`).
/// The `match-role`/`match-style` values are preserved verbatim for the
/// compiler; the validator warns when both are absent. The folio style is
/// preserved verbatim (validated at compile time).
pub(super) fn transform_toc(node: &KdlNode) -> Result<TocNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let match_role =
        optional_string_prop_aliased(node, "match-role", "match_role").map(str::to_owned);
    let match_style =
        optional_string_prop_aliased(node, "match-style", "match_style").map(str::to_owned);
    let leader = optional_string_prop(node, "leader").map(str::to_owned);
    let folio_style =
        optional_string_prop_aliased(node, "folio-style", "folio_style").map(str::to_owned);
    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");

    let unknown_props = collect_unknown_props(node, TOC_KNOWN_PROPS);

    Ok(TocNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        match_role,
        match_style,
        leader,
        folio_style,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        anchor: optional_string_prop(node, "anchor").map(str::to_owned),
        anchor_zone: optional_string_prop(node, "anchor-zone")
            .or_else(|| optional_string_prop(node, "anchor_zone"))
            .map(str::to_owned),
        anchor_sibling: optional_string_prop(node, "anchor-sibling")
            .or_else(|| optional_string_prop(node, "anchor_sibling"))
            .map(str::to_owned),
        anchor_parent: optional_bool_prop(node, "anchor-parent")
            .or_else(|| optional_bool_prop(node, "anchor_parent")),
        source_span: node_span(node),
        unknown_props,
    })
}

const FOOTNOTE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "marker",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
];

/// Transform a `footnote` node into a [`FootnoteNode`].
///
/// Reads the required `id`; the optional `marker` override; visual props
/// (`style`/`fill`/`font-family`/`font-size`); and the content `span` children
/// (the same span model a `text` node uses). A footnote has NO geometry.
pub(super) fn transform_footnote(node: &KdlNode) -> Result<FootnoteNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");

    let mut spans: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                spans.push(transform_span(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, FOOTNOTE_KNOWN_PROPS);

    Ok(FootnoteNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        marker: optional_string_prop(node, "marker").map(str::to_owned),
        spans,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        source_span: node_span(node),
        unknown_props,
    })
}
