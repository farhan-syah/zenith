//! Z-order reorder ops: move a node forward/backward/to-front/to-back within the
//! children slice that directly contains it.

use zenith_core::{Diagnostic, Document, Node};

use super::super::{node_id_of, record_affected};

/// Which z-order reorder to perform.
#[derive(Copy, Clone)]
pub(in crate::engine) enum ReorderKind {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

/// Outcome of a reorder attempt.
enum MoveOutcome {
    NotFound,
    /// The node is already at the target extreme for this operation; no change made.
    NoChange,
    Moved,
}

pub(in crate::engine) fn apply_reorder(
    node_id: &str,
    kind: ReorderKind,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    for page in doc.body.pages.iter_mut() {
        match reorder_in(&mut page.children, node_id, kind) {
            MoveOutcome::NotFound => {
                // Try the next page.
            }
            MoveOutcome::Moved => {
                record_affected(node_id, affected);
                return;
            }
            MoveOutcome::NoChange => {
                // Already at the target extreme — emit a kind-specific advisory.
                let msg = match kind {
                    ReorderKind::Forward | ReorderKind::ToFront => {
                        format!("node {:?} is already at the front of its parent", node_id)
                    }
                    ReorderKind::Backward | ReorderKind::ToBack => {
                        format!("node {:?} is already at the back of its parent", node_id)
                    }
                };
                diagnostics.push(Diagnostic::advisory(
                    "tx.noop",
                    msg,
                    None,
                    Some(node_id.to_owned()),
                ));
                return;
            }
        }
    }
    // No page contained the node.
    diagnostics.push(Diagnostic::error(
        "tx.unknown_node",
        format!("node {:?} not found in document", node_id),
        None,
        Some(node_id.to_owned()),
    ));
}

/// Reorder the node with `id` within whatever children slice directly contains
/// it, according to `kind`. Recurses into `Group`, `Frame`, `Table`, and
/// `Unknown` containers.
fn reorder_in(children: &mut [Node], id: &str, kind: ReorderKind) -> MoveOutcome {
    if let Some(i) = children.iter().position(|n| node_id_of(n) == Some(id)) {
        let len = children.len();
        match kind {
            ReorderKind::Forward => {
                if i + 1 >= len {
                    return MoveOutcome::NoChange;
                }
                children.swap(i, i + 1);
            }
            ReorderKind::Backward => {
                if i == 0 {
                    return MoveOutcome::NoChange;
                }
                children.swap(i, i - 1);
            }
            ReorderKind::ToFront => {
                if i + 1 >= len {
                    return MoveOutcome::NoChange;
                }
                // rotate_left(1) on children[i..] moves element at index i to
                // the last position, shifting i+1..len-1 one step left.
                children[i..].rotate_left(1);
            }
            ReorderKind::ToBack => {
                if i == 0 {
                    return MoveOutcome::NoChange;
                }
                // rotate_right(1) on children[..=i] moves element at index i
                // to index 0, shifting 0..i-1 one step right.
                children[..=i].rotate_right(1);
            }
        }
        return MoveOutcome::Moved;
    }
    for child in children.iter_mut() {
        match child {
            Node::Frame(f) => match reorder_in(&mut f.children, id, kind) {
                MoveOutcome::NotFound => {}
                other @ (MoveOutcome::NoChange | MoveOutcome::Moved) => return other,
            },
            Node::Group(g) => match reorder_in(&mut g.children, id, kind) {
                MoveOutcome::NotFound => {}
                other @ (MoveOutcome::NoChange | MoveOutcome::Moved) => return other,
            },
            Node::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        match reorder_in(&mut cell.children, id, kind) {
                            MoveOutcome::NotFound => {}
                            other @ (MoveOutcome::NoChange | MoveOutcome::Moved) => return other,
                        }
                    }
                }
            }
            Node::Unknown(u) => match reorder_in(&mut u.children, id, kind) {
                MoveOutcome::NotFound => {}
                other @ (MoveOutcome::NoChange | MoveOutcome::Moved) => return other,
            },
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Mesh(_) => {}
        }
    }
    MoveOutcome::NotFound
}
