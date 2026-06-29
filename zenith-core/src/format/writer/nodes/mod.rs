//! Node-tree writing: the `document` body, `page`, the per-node writers
//! (rect/ellipse/line/text/code/image/group/frame/polygon/polyline), and the
//! `span` / `point` / `content` leaf emitters.
//!
//! This module root is wiring only: the submodule declarations, the cross-tree
//! re-exports consumed by `format::writer`, the `write_node` dispatcher, and the
//! shared child-block edge that feeds it.

use crate::ast::Node;

mod container;
mod document;
mod effect;
mod helpers;
mod leaf;
mod special;

// Re-exported for `format::writer::mod` (the component-block + document writers).
pub(in crate::format::writer) use document::write_document_body;

use container::{write_frame, write_group, write_table};
use effect::write_light;
use leaf::{
    write_chart, write_code, write_ellipse, write_image, write_line, write_pattern, write_polygon,
    write_polyline, write_rect, write_text,
};
use special::{
    write_connector, write_field, write_footnote, write_instance, write_shape, write_toc,
    write_unknown_node,
};

/// Dispatch a single node to its per-kind writer.
pub(super) fn write_node(node: &Node, out: &mut String, depth: usize) {
    match node {
        Node::Rect(r) => write_rect(r, out, depth),
        Node::Ellipse(e) => write_ellipse(e, out, depth),
        Node::Line(l) => write_line(l, out, depth),
        Node::Text(t) => write_text(t, out, depth),
        Node::Code(c) => write_code(c, out, depth),
        Node::Frame(f) => write_frame(f, out, depth),
        Node::Group(g) => write_group(g, out, depth),
        Node::Image(i) => write_image(i, out, depth),
        Node::Polygon(p) => write_polygon(p, out, depth),
        Node::Polyline(p) => write_polyline(p, out, depth),
        Node::Instance(i) => write_instance(i, out, depth),
        Node::Field(f) => write_field(f, out, depth),
        Node::Toc(t) => write_toc(t, out, depth),
        Node::Footnote(f) => write_footnote(f, out, depth),
        Node::Table(t) => write_table(t, out, depth),
        Node::Shape(s) => write_shape(s, out, depth),
        Node::Connector(c) => write_connector(c, out, depth),
        Node::Unknown(u) => write_unknown_node(u, out, depth),
        Node::Pattern(p) => write_pattern(p, out, depth),
        Node::Chart(c) => write_chart(c, out, depth),
        Node::Light(l) => write_light(l, out, depth),
    }
}

/// Emit each child node in source order at `depth + 1` indentation.
///
/// Used by `write_page`, `write_group`, and `write_frame` so the child-block
/// logic lives in exactly one place.
///
/// # Known limitation
/// Frames and groups nest recursively via `write_node` → `write_frame` /
/// `write_group` → `write_children_block` with no depth guard.  This is an
/// accepted v0 limitation; stack overflow is only possible with pathologically
/// deep trees.
pub(super) fn write_children_block(children: &[Node], out: &mut String, depth: usize) {
    for child in children {
        write_node(child, out, depth + 1);
    }
}

/// Emit a component definition's child nodes at `depth + 1` indentation.
///
/// Public to the writer module so the `components` block writer in the module
/// root can reuse the exact same per-node serialization the page/group/frame
/// child blocks use. (`write_children_block` indents relative to a container
/// node's own depth; here `depth` is the `component` node's depth.)
pub(in crate::format::writer) fn write_component_children(
    children: &[Node],
    out: &mut String,
    depth: usize,
) {
    write_children_block(children, out, depth);
}
