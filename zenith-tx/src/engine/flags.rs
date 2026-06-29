//! Flag and point op application: `set_visible`, `set_locked`, `set_points`,
//! and the field accessors they use.

use zenith_core::{Diagnostic, Document, Node, Point};

use crate::op::OpPoint;

use super::{find_node_any_mut, node_kind_str, px, record_affected};

// ── Field accessor helpers ────────────────────────────────────────────────────

/// Return a mutable reference to the `visible` field of a node, or `None`
/// for `Node::Unknown` which carries no `visible` field.
fn node_visible_mut(node: &mut Node) -> Option<&mut Option<bool>> {
    match node {
        Node::Rect(n) => Some(&mut n.visible),
        Node::Ellipse(n) => Some(&mut n.visible),
        Node::Line(n) => Some(&mut n.visible),
        Node::Text(n) => Some(&mut n.visible),
        Node::Code(n) => Some(&mut n.visible),
        Node::Frame(n) => Some(&mut n.visible),
        Node::Group(n) => Some(&mut n.visible),
        Node::Image(n) => Some(&mut n.visible),
        Node::Polygon(n) => Some(&mut n.visible),
        Node::Polyline(n) => Some(&mut n.visible),
        Node::Instance(n) => Some(&mut n.visible),
        Node::Field(n) => Some(&mut n.visible),
        Node::Toc(n) => Some(&mut n.visible),
        Node::Table(n) => Some(&mut n.visible),
        Node::Shape(n) => Some(&mut n.visible),
        Node::Connector(n) => Some(&mut n.visible),
        Node::Pattern(n) => Some(&mut n.visible),
        Node::Chart(n) => Some(&mut n.visible),
        Node::Light(n) => Some(&mut n.visible),
        // A footnote has no `visible` flag (it is auto-numbered page furniture);
        // set_visible honestly surfaces tx.unsupported_property.
        Node::Footnote(_) => None,
        Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `locked` field of a node, or `None`
/// for `Node::Unknown` which carries no `locked` field.
fn node_locked_mut(node: &mut Node) -> Option<&mut Option<bool>> {
    match node {
        Node::Rect(n) => Some(&mut n.locked),
        Node::Ellipse(n) => Some(&mut n.locked),
        Node::Line(n) => Some(&mut n.locked),
        Node::Text(n) => Some(&mut n.locked),
        Node::Code(n) => Some(&mut n.locked),
        Node::Frame(n) => Some(&mut n.locked),
        Node::Group(n) => Some(&mut n.locked),
        Node::Image(n) => Some(&mut n.locked),
        Node::Polygon(n) => Some(&mut n.locked),
        Node::Polyline(n) => Some(&mut n.locked),
        Node::Instance(n) => Some(&mut n.locked),
        Node::Field(n) => Some(&mut n.locked),
        Node::Toc(n) => Some(&mut n.locked),
        Node::Table(n) => Some(&mut n.locked),
        Node::Shape(n) => Some(&mut n.locked),
        Node::Connector(n) => Some(&mut n.locked),
        Node::Pattern(n) => Some(&mut n.locked),
        Node::Chart(n) => Some(&mut n.locked),
        Node::Light(n) => Some(&mut n.locked),
        // A footnote has no `locked` flag.
        Node::Footnote(_) => None,
        Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `points` field of a `polygon` or
/// `polyline` node, or `None` for all other variants.
fn node_points_mut(node: &mut Node) -> Option<&mut Vec<Point>> {
    match node {
        Node::Polygon(p) => Some(&mut p.points),
        Node::Polyline(p) => Some(&mut p.points),
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Text(_)
        | Node::Code(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Footnote(_)
        | Node::Toc(_)
        | Node::Table(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Unknown(_) => None,
    }
}

// ── SetVisible / SetLocked ────────────────────────────────────────────────────

/// Shared driver for `set_visible` and `set_locked`: finds the node by id,
/// calls `accessor` to get the `Option<bool>` slot, and sets it to `value`.
/// Emits `tx.unknown_node` or `tx.unsupported_property` on failure.
fn apply_set_bool_field(
    node_id: &str,
    value: bool,
    op_label: &str,
    accessor: fn(&mut Node) -> Option<&mut Option<bool>>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            // node_kind_str returns &'static str — no live borrow of `node` after this.
            let kind = node_kind_str(node);
            match accessor(node) {
                Some(slot) => {
                    *slot = Some(value);
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("{} is not supported on a {} node", op_label, kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
    }
}

pub(super) fn apply_set_visible(
    node_id: &str,
    visible: bool,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_bool_field(
        node_id,
        visible,
        "set_visible",
        node_visible_mut,
        doc,
        diagnostics,
        affected,
    );
}

pub(super) fn apply_set_locked(
    node_id: &str,
    locked: bool,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_bool_field(
        node_id,
        locked,
        "set_locked",
        node_locked_mut,
        doc,
        diagnostics,
        affected,
    );
}

// ── SetPoints ─────────────────────────────────────────────────────────────────

pub(super) fn apply_set_points(
    node_id: &str,
    points: &[OpPoint],
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            let kind = node_kind_str(node);
            match node_points_mut(node) {
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("set_points is not supported on a {} node", kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
                Some(pts) => {
                    *pts = points
                        .iter()
                        .map(|p| Point {
                            x: Some(px(p.x)),
                            y: Some(px(p.y)),
                        })
                        .collect();
                    record_affected(node_id, affected);
                }
            }
        }
    }
}
