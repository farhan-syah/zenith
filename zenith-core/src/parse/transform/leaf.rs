//! Transforms for leaf renderable nodes: rect, ellipse, line, text, code,
//! image, polygon, polyline — plus the shared `point` and `span` children.

use kdl::{KdlNode, KdlValue};

use crate::ast::node::{
    CodeNode, EllipseNode, ImageNode, LineNode, Point, PolygonNode, PolylineNode, RectNode,
    TextNode, TextSpan,
};
use crate::data::DataFormat;
use crate::error::{ParseError, ParseErrorCode};
use crate::tokens::SyntaxTheme;

use super::helpers::{
    collect_unknown_props, entry_to_property_value, node_span, optional_bool_prop,
    optional_dimension_prop, optional_f64_prop, optional_object_position_prop,
    optional_property_value, optional_property_value_aliased, optional_string_prop,
    optional_string_prop_aliased, optional_u32_prop, required_string_prop,
};

pub(crate) const RECT_KNOWN_PROPS: &[&str] = &[
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
];

pub(super) fn transform_rect(node: &KdlNode) -> Result<RectNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
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

    // Per-corner radius overrides: accept both hyphenated and underscored spellings.
    let radius_tl = optional_property_value_aliased(node, "radius-tl", "radius_tl");
    let radius_tr = optional_property_value_aliased(node, "radius-tr", "radius_tr");
    let radius_br = optional_property_value_aliased(node, "radius-br", "radius_br");
    let radius_bl = optional_property_value_aliased(node, "radius-bl", "radius_bl");

    // Per-side border colors.
    let border_top = optional_property_value_aliased(node, "border-top", "border_top");
    let border_bottom = optional_property_value_aliased(node, "border-bottom", "border_bottom");
    let border_left = optional_property_value_aliased(node, "border-left", "border_left");
    let border_right = optional_property_value_aliased(node, "border-right", "border_right");
    // Shared border width.
    let border_width = optional_property_value_aliased(node, "border-width", "border_width");
    // Double-border (outer stroke).
    let stroke_outer = optional_property_value_aliased(node, "stroke-outer", "stroke_outer");
    let stroke_outer_width =
        optional_property_value_aliased(node, "stroke-outer-width", "stroke_outer_width");

    let unknown_props = collect_unknown_props(node, RECT_KNOWN_PROPS);

    Ok(RectNode {
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
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const IMAGE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "asset",
    "x",
    "y",
    "w",
    "h",
    "src-x",
    "src_x",
    "src-y",
    "src_y",
    "src-w",
    "src_w",
    "src-h",
    "src_h",
    "fit",
    "clip",
    "clip-radius",
    "clip_radius",
    "object-position-x",
    "object_position_x",
    "object-position-y",
    "object_position_y",
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
    "style",
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
];

pub(super) fn transform_image(node: &KdlNode) -> Result<ImageNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let asset = required_string_prop(node, "asset")?.to_owned();

    // object-position accepts hyphenated or underscored spellings.
    let object_position_x = optional_object_position_prop(node, "object-position-x")
        .or_else(|| optional_object_position_prop(node, "object_position_x"));
    let object_position_y = optional_object_position_prop(node, "object-position-y")
        .or_else(|| optional_object_position_prop(node, "object_position_y"));

    // src-* accept hyphenated or underscored spellings.
    let src_x =
        optional_dimension_prop(node, "src-x").or_else(|| optional_dimension_prop(node, "src_x"));
    let src_y =
        optional_dimension_prop(node, "src-y").or_else(|| optional_dimension_prop(node, "src_y"));
    let src_w =
        optional_dimension_prop(node, "src-w").or_else(|| optional_dimension_prop(node, "src_w"));
    let src_h =
        optional_dimension_prop(node, "src-h").or_else(|| optional_dimension_prop(node, "src_h"));

    let unknown_props = collect_unknown_props(node, IMAGE_KNOWN_PROPS);

    Ok(ImageNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        asset,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        src_x,
        src_y,
        src_w,
        src_h,
        fit: optional_string_prop(node, "fit").map(str::to_owned),
        clip: optional_string_prop(node, "clip").map(str::to_owned),
        clip_radius: optional_property_value_aliased(node, "clip-radius", "clip_radius"),
        object_position_x,
        object_position_y,
        shadow: optional_property_value(node, "shadow"),
        filter: optional_property_value(node, "filter"),
        mask: optional_property_value(node, "mask"),
        blend_mode: optional_string_prop_aliased(node, "blend-mode", "blend_mode")
            .map(str::to_owned),
        blur: optional_dimension_prop(node, "blur"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
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
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const ELLIPSE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "rx",
    "ry",
    "style",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
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
];

pub(super) fn transform_ellipse(node: &KdlNode) -> Result<EllipseNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_dash = optional_property_value_aliased(node, "stroke-dash", "stroke_dash");
    let stroke_gap = optional_property_value_aliased(node, "stroke-gap", "stroke_gap");
    let stroke_linecap =
        optional_string_prop_aliased(node, "stroke-linecap", "stroke_linecap").map(str::to_owned);
    let blend_mode =
        optional_string_prop_aliased(node, "blend-mode", "blend_mode").map(str::to_owned);

    // Independent axis radii (override inscribed-ellipse default).
    let rx = optional_property_value(node, "rx");
    let ry = optional_property_value(node, "ry");

    let unknown_props = collect_unknown_props(node, ELLIPSE_KNOWN_PROPS);

    Ok(EllipseNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        rx,
        ry,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_dash,
        stroke_gap,
        stroke_linecap,
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
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const LINE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x1",
    "y1",
    "x2",
    "y2",
    "style",
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
    // NOTE: "stroke-alignment" is intentionally absent — it does not apply to
    // line nodes. An author who writes it will receive a node.unknown_property
    // warning, which is the correct diagnostic for inapplicable properties.
];

pub(super) fn transform_line(node: &KdlNode) -> Result<LineNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_dash = optional_property_value_aliased(node, "stroke-dash", "stroke_dash");
    let stroke_gap = optional_property_value_aliased(node, "stroke-gap", "stroke_gap");
    let stroke_linecap =
        optional_string_prop_aliased(node, "stroke-linecap", "stroke_linecap").map(str::to_owned);

    let unknown_props = collect_unknown_props(node, LINE_KNOWN_PROPS);

    Ok(LineNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x1: optional_dimension_prop(node, "x1"),
        y1: optional_dimension_prop(node, "y1"),
        x2: optional_dimension_prop(node, "x2"),
        y2: optional_dimension_prop(node, "y2"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_dash,
        stroke_gap,
        stroke_linecap,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const TEXT_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "align",
    "v-align",
    "v_align",
    "direction",
    "overflow",
    "overflow-wrap",
    "overflow_wrap",
    "style",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "contrast-bg",
    "contrast_bg",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "font-size-min",
    "font_size_min",
    "font-weight",
    "font_weight",
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
    "chain",
    "drop-cap-lines",
    "drop_cap_lines",
    "hyphenate",
    "widow-orphan",
    "widow_orphan",
    "tab-leader",
    "tab_leader",
    "text-exclusion",
    "text_exclusion",
    "padding-left",
    "padding_left",
    "text-indent",
    "text_indent",
    "bullet",
    "bullet-gap",
    "bullet_gap",
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
    // span data-binding attrs (carried on `span` children, listed here so the
    // family of recognized text/span props is documented in one place).
    "data-ref",
    "data_ref",
    "format",
    "precision",
    "locale",
];

pub(super) fn transform_text(node: &KdlNode) -> Result<TextNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let font_size_min = optional_property_value_aliased(node, "font-size-min", "font_size_min");
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let drop_cap_lines = optional_u32_prop(node, "drop-cap-lines")
        .or_else(|| optional_u32_prop(node, "drop_cap_lines"));
    let hyphenate = optional_bool_prop(node, "hyphenate");
    let widow_orphan =
        optional_u32_prop(node, "widow-orphan").or_else(|| optional_u32_prop(node, "widow_orphan"));
    let tab_leader = optional_string_prop(node, "tab-leader")
        .or_else(|| optional_string_prop(node, "tab_leader"))
        .map(str::to_owned);
    let text_exclusion = optional_string_prop(node, "text-exclusion")
        .or_else(|| optional_string_prop(node, "text_exclusion"))
        .map(str::to_owned);
    // Optional signed geometry dimensions (text-indent may be NEGATIVE for a
    // hanging indent). Accept both hyphenated and underscored spellings.
    let padding_left = optional_dimension_prop(node, "padding-left")
        .or_else(|| optional_dimension_prop(node, "padding_left"));
    let text_indent = optional_dimension_prop(node, "text-indent")
        .or_else(|| optional_dimension_prop(node, "text_indent"));
    let stroke = optional_property_value(node, "stroke");
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let bullet = optional_string_prop(node, "bullet").map(str::to_owned);
    let bullet_gap = optional_dimension_prop(node, "bullet-gap")
        .or_else(|| optional_dimension_prop(node, "bullet_gap"));

    let mut spans: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                spans.push(transform_span(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, TEXT_KNOWN_PROPS);

    Ok(TextNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        align: optional_string_prop(node, "align").map(str::to_owned),
        v_align: optional_string_prop_aliased(node, "v-align", "v_align").map(str::to_owned),
        direction: optional_string_prop(node, "direction").map(str::to_owned),
        overflow: optional_string_prop(node, "overflow").map(str::to_owned),
        overflow_wrap: optional_string_prop(node, "overflow-wrap")
            .or_else(|| optional_string_prop(node, "overflow_wrap"))
            .map(str::to_owned),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke,
        stroke_width,
        contrast_bg: optional_property_value_aliased(node, "contrast-bg", "contrast_bg"),
        font_family,
        font_size,
        font_size_min,
        font_weight,
        shadow: optional_property_value(node, "shadow"),
        filter: optional_property_value(node, "filter"),
        mask: optional_property_value(node, "mask"),
        blend_mode: optional_string_prop_aliased(node, "blend-mode", "blend_mode")
            .map(str::to_owned),
        blur: optional_dimension_prop(node, "blur"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        chain: optional_string_prop(node, "chain").map(str::to_owned),
        drop_cap_lines,
        hyphenate,
        widow_orphan,
        tab_leader,
        text_exclusion,
        padding_left,
        text_indent,
        bullet,
        bullet_gap,
        spans,
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
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const CODE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "overflow",
    "language",
    "line-numbers",
    "line_numbers",
    "tab-width",
    "tab_width",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "font-weight",
    "font_weight",
    "syntax-theme",
    "syntax_theme",
    "opacity",
    "visible",
    "locked",
    "rotate",
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
];

pub(super) fn transform_code(node: &KdlNode) -> Result<CodeNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let line_numbers = optional_bool_prop(node, "line-numbers")
        .or_else(|| optional_bool_prop(node, "line_numbers"));
    let tab_width =
        optional_u32_prop(node, "tab-width").or_else(|| optional_u32_prop(node, "tab_width"));
    let syntax_theme = optional_string_prop(node, "syntax-theme")
        .or_else(|| optional_string_prop(node, "syntax_theme"))
        .and_then(SyntaxTheme::from_name);

    // The verbatim source is carried by a `content` child node whose first
    // positional argument is the DECODED string. KDL v2 multi-line string
    // dedent rules make a bare `r#"..."#` form lossy, so the carrier uses a
    // single-line escaped string which round-trips `\n \t \" \\` exactly.
    // Stored decoded here; `write_code` re-encodes the escapes.
    let mut content = String::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "content" {
                if let Some(KdlValue::String(s)) = child.get(0) {
                    content = s.clone();
                }
                break;
            }
        }
    }

    let unknown_props = collect_unknown_props(node, CODE_KNOWN_PROPS);

    Ok(CodeNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        overflow: optional_string_prop(node, "overflow").map(str::to_owned),
        language: optional_string_prop(node, "language").map(str::to_owned),
        line_numbers,
        tab_width,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        font_weight,
        syntax_theme,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        content,
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
        source_span: node_span(node),
        unknown_props,
    })
}

pub(crate) const POLYGON_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "stroke-alignment",
    "stroke_alignment",
    "fill-rule",
    "fill_rule",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

// NOTE: polyline intentionally omits stroke-alignment — an author
// writing it gets a node.unknown_property warning, which is correct.
pub(crate) const POLYLINE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "fill-rule",
    "fill_rule",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

/// Transform a `point` child node into a [`Point`].
///
/// `x` and `y` are optional at parse time; validate checks their presence.
fn transform_point(node: &KdlNode) -> Point {
    Point {
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
    }
}

pub(super) fn transform_polygon(node: &KdlNode) -> Result<PolygonNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_alignment =
        optional_string_prop_aliased(node, "stroke-alignment", "stroke_alignment")
            .map(str::to_owned);
    let fill_rule = optional_string_prop_aliased(node, "fill-rule", "fill_rule").map(str::to_owned);

    // Collect `point` child nodes — this is where the vertex list lives.
    let mut points: Vec<Point> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "point" {
                points.push(transform_point(child));
            }
        }
    }

    let unknown_props = collect_unknown_props(node, POLYGON_KNOWN_PROPS);

    Ok(PolygonNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_alignment,
        fill_rule,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        points,
        source_span: node_span(node),
        unknown_props,
    })
}

pub(super) fn transform_polyline(node: &KdlNode) -> Result<PolylineNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let fill_rule = optional_string_prop_aliased(node, "fill-rule", "fill_rule").map(str::to_owned);

    // Collect `point` child nodes.
    let mut points: Vec<Point> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "point" {
                points.push(transform_point(child));
            }
        }
    }

    let unknown_props = collect_unknown_props(node, POLYLINE_KNOWN_PROPS);

    Ok(PolylineNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        fill_rule,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        points,
        source_span: node_span(node),
        unknown_props,
    })
}

pub(super) fn transform_span(node: &KdlNode) -> Result<TextSpan, ParseError> {
    // First positional argument is the text content.
    let text = node
        .get(0)
        .and_then(|v| {
            if let KdlValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                "`span` node must have a string argument as its first value",
            )
        })?;

    let fill = node
        .entry("fill")
        .and_then(|e| entry_to_property_value(e).ok());
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let italic = optional_bool_prop(node, "italic");
    let underline = optional_bool_prop(node, "underline");
    let strikethrough = optional_bool_prop(node, "strikethrough");
    let vertical_align =
        optional_string_prop_aliased(node, "vertical-align", "vertical_align").map(str::to_owned);
    let footnote_ref =
        optional_string_prop_aliased(node, "footnote-ref", "footnote_ref").map(str::to_owned);

    // Data-binding: a `data-ref="path"` ties the span's TEXT CONTENT to a runtime
    // field; an optional `format` (+ `precision` / `locale`) styles the resolved
    // value. Both spellings (`data-ref`/`data_ref`) are accepted.
    let data_ref = optional_string_prop_aliased(node, "data-ref", "data_ref").map(str::to_owned);
    let data_format = parse_span_data_format(node);

    // Per-span highlight background color (token ref or raw color string),
    // mirroring the `fill` read path. Absent → `None` (no highlight).
    let highlight = node
        .entry("highlight")
        .and_then(|e| entry_to_property_value(e).ok());

    // Inline code mark: `code=#true` shapes this span in monospace + emits a
    // subtle background rect. Absent → `None` (byte-identical).
    let code = optional_bool_prop(node, "code");

    // Hyperlink URL: `link="https://…"` renders the span underlined in the
    // internal link color (unless `fill` is set) and retains the URL for
    // future PDF annotation. Absent → `None` (byte-identical).
    let link = optional_string_prop(node, "link").map(str::to_owned);

    Ok(TextSpan {
        text,
        fill,
        font_weight,
        italic,
        underline,
        strikethrough,
        vertical_align,
        footnote_ref,
        data_ref,
        data_format,
        highlight,
        code,
        link,
    })
}

/// Build a [`DataFormat`] for a `span` from its `format` / `precision` / `locale`
/// attributes. Returns `None` when no `format` attribute is present (the span
/// substitutes the raw data value verbatim). `precision` is an optional integer
/// clamped into `u8`; `locale` is an optional string (only carried by currency).
/// An unrecognized `format` value yields `None`.
fn parse_span_data_format(node: &KdlNode) -> Option<DataFormat> {
    let format = optional_string_prop(node, "format")?;
    let precision: Option<u8> =
        optional_u32_prop(node, "precision").and_then(|n| u8::try_from(n).ok());
    let locale = optional_string_prop(node, "locale").map(str::to_owned);
    match format {
        "currency" => Some(DataFormat::Currency { locale, precision }),
        "percent" => Some(DataFormat::Percent { precision }),
        "number" => Some(DataFormat::Number { precision }),
        _ => None,
    }
}
