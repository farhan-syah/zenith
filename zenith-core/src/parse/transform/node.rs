//! The per-node-kind dispatch edge: maps a KDL node name to the matching
//! `transform_*` constructor.

use kdl::KdlNode;

use crate::ast::node::{Node, UnknownNode};
use crate::error::ParseError;

use super::chart::transform_chart;
use super::container::{transform_frame, transform_group, transform_instance, transform_table};
use super::document::transform_children;
use super::effect::{transform_light, transform_mesh};
use super::helpers::{collect_unknown_props, node_span, optional_string_prop};
use super::leaf::{
    transform_code, transform_ellipse, transform_image, transform_line, transform_polygon,
    transform_polyline, transform_rect, transform_text,
};
use super::pattern::transform_pattern;
use super::special::{
    transform_connector, transform_field, transform_footnote, transform_shape, transform_toc,
};

pub(super) fn transform_node(node: &KdlNode) -> Result<Node, ParseError> {
    match node.name().value() {
        "rect" => transform_rect(node).map(|r| Node::Rect(Box::new(r))),
        "ellipse" => transform_ellipse(node).map(Node::Ellipse),
        "line" => transform_line(node).map(Node::Line),
        "text" => transform_text(node).map(|t| Node::Text(Box::new(t))),
        "code" => transform_code(node).map(Node::Code),
        "frame" => transform_frame(node).map(Node::Frame),
        "group" => transform_group(node).map(Node::Group),
        "image" => transform_image(node).map(Node::Image),
        "polygon" => transform_polygon(node).map(Node::Polygon),
        "polyline" => transform_polyline(node).map(Node::Polyline),
        "instance" => transform_instance(node).map(Node::Instance),
        "field" => transform_field(node).map(Node::Field),
        "toc" => transform_toc(node).map(Node::Toc),
        "footnote" => transform_footnote(node).map(Node::Footnote),
        "table" => transform_table(node).map(|t| Node::Table(Box::new(t))),
        "shape" => transform_shape(node).map(|s| Node::Shape(Box::new(s))),
        "connector" => transform_connector(node).map(|c| Node::Connector(Box::new(c))),
        "pattern" => transform_pattern(node).map(|p| Node::Pattern(Box::new(p))),
        "chart" => transform_chart(node).map(|c| Node::Chart(Box::new(c))),
        "light" => transform_light(node).map(|l| Node::Light(Box::new(l))),
        "mesh" => transform_mesh(node).map(|m| Node::Mesh(Box::new(m))),
        _ => Ok(Node::Unknown(Box::new(UnknownNode {
            kind: node.name().value().to_owned(),
            id: optional_string_prop(node, "id").map(str::to_owned),
            unknown_props: collect_unknown_props(node, &["id"]),
            children: transform_children(node)?,
            source_span: node_span(node),
        }))),
    }
}
