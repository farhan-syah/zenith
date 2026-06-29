//! Container / child finders and tree-mutation helpers shared across the
//! structural op submodules: locate a container's children vec, test whether a
//! subtree contains a container, remove a node by id, and resolve a `Position`.

use zenith_core::{Diagnostic, Document, Node};

use crate::op::Position;

use super::super::node_id_of;

/// Return a mutable reference to the children vec of the container identified by
/// `parent_id` — either a page (matched by page `id`) or a nested `group`/`frame`
/// (matched by node `id`). Returns `None` if no such container exists (including
/// when the id names a leaf node).
///
/// Two-phase borrow, mirroring [`find_node_any_mut`]: a shared scan locates the
/// target page index first, then a single exclusive borrow descends.
pub(super) fn find_container_children_mut<'doc>(
    doc: &'doc mut Document,
    parent_id: &str,
) -> Option<&'doc mut Vec<Node>> {
    // Phase 1: find which page is, or contains, the target container.
    let page_index = doc.body.pages.iter().enumerate().find_map(|(pi, page)| {
        if page.id == parent_id {
            return Some(pi);
        }
        let found = page
            .children
            .iter()
            .any(|n| subtree_contains_container(n, parent_id));
        if found { Some(pi) } else { None }
    });

    // Phase 2: take the exclusive borrow we deferred.
    match page_index {
        None => None,
        Some(pi) => match doc.body.pages.get_mut(pi) {
            None => None,
            Some(page) => {
                if page.id == parent_id {
                    Some(&mut page.children)
                } else {
                    find_container_in_children_mut(&mut page.children, parent_id)
                }
            }
        },
    }
}

/// Returns true if `node` is, or transitively contains, a `group`/`frame`
/// container whose `id == parent_id`. Leaves are never containers.
pub(super) fn subtree_contains_container(node: &Node, parent_id: &str) -> bool {
    match node {
        Node::Frame(f) => {
            f.id == parent_id
                || f.children
                    .iter()
                    .any(|c| subtree_contains_container(c, parent_id))
        }
        Node::Group(g) => {
            g.id == parent_id
                || g.children
                    .iter()
                    .any(|c| subtree_contains_container(c, parent_id))
        }
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
        | Node::Table(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Unknown(_) => false,
    }
}

/// Descend into a children slice and return a mutable reference to the children
/// vec of the `group`/`frame` whose `id == parent_id`. Two-phase borrow.
fn find_container_in_children_mut<'a>(
    children: &'a mut [Node],
    parent_id: &str,
) -> Option<&'a mut Vec<Node>> {
    // `Direct(i)` — children[i] is the container itself.
    // `Descend(i)` — the container lives somewhere inside children[i].
    enum Hit {
        Direct(usize),
        Descend(usize),
    }

    let hit = children
        .iter()
        .enumerate()
        .find_map(|(i, node)| match node {
            Node::Frame(f) if f.id == parent_id => Some(Hit::Direct(i)),
            Node::Group(g) if g.id == parent_id => Some(Hit::Direct(i)),
            Node::Frame(f)
                if f.children
                    .iter()
                    .any(|c| subtree_contains_container(c, parent_id)) =>
            {
                Some(Hit::Descend(i))
            }
            Node::Group(g)
                if g.children
                    .iter()
                    .any(|c| subtree_contains_container(c, parent_id)) =>
            {
                Some(Hit::Descend(i))
            }
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Frame(_)
            | Node::Group(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
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
        });

    match hit {
        None => None,
        Some(Hit::Direct(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => Some(&mut f.children),
            Some(Node::Group(g)) => Some(&mut g.children),
            // unreachable: phase-1 confirmed a container at i
            Some(Node::Rect(_))
            | Some(Node::Ellipse(_))
            | Some(Node::Line(_))
            | Some(Node::Text(_))
            | Some(Node::Code(_))
            | Some(Node::Image(_))
            | Some(Node::Polygon(_))
            | Some(Node::Polyline(_))
            | Some(Node::Instance(_))
            | Some(Node::Field(_))
            | Some(Node::Footnote(_))
            | Some(Node::Toc(_))
            | Some(Node::Table(_))
            | Some(Node::Shape(_))
            | Some(Node::Connector(_))
            | Some(Node::Pattern(_))
            | Some(Node::Chart(_))
            | Some(Node::Light(_))
            | Some(Node::Unknown(_))
            | None => None,
        },
        Some(Hit::Descend(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => find_container_in_children_mut(&mut f.children, parent_id),
            Some(Node::Group(g)) => find_container_in_children_mut(&mut g.children, parent_id),
            // unreachable
            Some(Node::Rect(_))
            | Some(Node::Ellipse(_))
            | Some(Node::Line(_))
            | Some(Node::Text(_))
            | Some(Node::Code(_))
            | Some(Node::Image(_))
            | Some(Node::Polygon(_))
            | Some(Node::Polyline(_))
            | Some(Node::Instance(_))
            | Some(Node::Field(_))
            | Some(Node::Footnote(_))
            | Some(Node::Toc(_))
            | Some(Node::Table(_))
            | Some(Node::Shape(_))
            | Some(Node::Connector(_))
            | Some(Node::Pattern(_))
            | Some(Node::Chart(_))
            | Some(Node::Light(_))
            | Some(Node::Unknown(_))
            | None => None,
        },
    }
}

/// Remove the node with `id` from `children` or any nested container
/// (`group`, `frame`, `table` cell, or `unknown`) within it, returning the
/// removed node, or `None` if absent.
pub(super) fn remove_node_by_id(children: &mut Vec<Node>, id: &str) -> Option<Node> {
    if let Some(i) = children.iter().position(|n| node_id_of(n) == Some(id)) {
        return Some(children.remove(i));
    }
    for child in children.iter_mut() {
        let nested = match child {
            Node::Frame(f) => remove_node_by_id(&mut f.children, id),
            Node::Group(g) => remove_node_by_id(&mut g.children, id),
            Node::Table(t) => {
                let mut found = None;
                'table: for row in &mut t.rows {
                    for cell in &mut row.cells {
                        if let Some(n) = remove_node_by_id(&mut cell.children, id) {
                            found = Some(n);
                            break 'table;
                        }
                    }
                }
                found
            }
            Node::Unknown(u) => remove_node_by_id(&mut u.children, id),
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
            | Node::Light(_) => None,
        };
        if nested.is_some() {
            return nested;
        }
    }
    None
}

/// Resolve the index of a `Position` within `children`, emitting a diagnostic
/// and returning `None` if a `Before`/`After` sibling id cannot be found.
///
/// Extracted so both `apply_add_node` and `apply_reparent` share identical
/// resolution logic without duplication.
pub(super) fn resolve_position(
    position: &Position,
    children: &[Node],
    parent_id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    match position {
        Position::Last => Some(children.len()),
        Position::First => Some(0),
        Position::Index { index } => Some((*index).min(children.len())),
        Position::Before { id } => {
            match children
                .iter()
                .position(|n| node_id_of(n) == Some(id.as_str()))
            {
                Some(i) => Some(i),
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unknown_node",
                        format!("sibling {:?} not found in parent {:?}", id, parent_id),
                        None,
                        Some(id.to_owned()),
                    ));
                    None
                }
            }
        }
        Position::After { id } => {
            match children
                .iter()
                .position(|n| node_id_of(n) == Some(id.as_str()))
            {
                Some(i) => Some(i + 1),
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unknown_node",
                        format!("sibling {:?} not found in parent {:?}", id, parent_id),
                        None,
                        Some(id.to_owned()),
                    ));
                    None
                }
            }
        }
    }
}
