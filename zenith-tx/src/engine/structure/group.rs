//! `Group` / `Ungroup` / `Reparent` application, plus the common-parent finder
//! and ungroup splice helper they use.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, Document, GroupNode, Node};

use crate::op::Position;

use super::super::{find_node_shared, node_id_of, record_affected, subtree_contains};
use super::finders::{find_container_children_mut, remove_node_by_id, resolve_position};

/// Find which page directly contains (at the top level of `page.children`) ALL
/// of the ids in `node_ids`. Returns `(page_index, sorted_indices)` where
/// `sorted_indices` is the list of positions within `page.children` in
/// ascending order, or `None` if the ids are not all siblings on one page.
///
/// We walk each page's *direct* children only — a flat O(pages × ids) scan.
/// Nesting is handled by a second pass that descends into containers.
fn find_common_parent_children_mut<'doc>(
    doc: &'doc mut Document,
    node_ids: &[String],
) -> Option<&'doc mut Vec<Node>> {
    // Phase 1 (shared scan): find which page + container has ALL ids as direct
    // children.  We search each page's full subtree of containers.
    struct Hit {
        page_index: usize,
        /// If `None` the parent is the page itself; otherwise it's the container id.
        container_id: Option<String>,
    }

    let hit: Option<Hit> = 'outer: {
        for (pi, page) in doc.body.pages.iter().enumerate() {
            // Check if all are direct children of this page.
            if node_ids.iter().all(|id| {
                page.children
                    .iter()
                    .any(|n| node_id_of(n) == Some(id.as_str()))
            }) {
                break 'outer Some(Hit {
                    page_index: pi,
                    container_id: None,
                });
            }
            // Walk containers within this page.
            if let Some(cid) = find_container_with_all_children(&page.children, node_ids) {
                break 'outer Some(Hit {
                    page_index: pi,
                    container_id: Some(cid),
                });
            }
        }
        None
    };

    let Hit {
        page_index,
        container_id,
    } = hit?;

    // Phase 2 (exclusive borrow): return a mutable ref to the right vec.
    match container_id {
        None => doc.body.pages.get_mut(page_index).map(|p| &mut p.children),
        Some(cid) => find_container_children_mut(doc, &cid),
    }
}

/// Walk `children` recursively and return the id of the first container whose
/// *direct* children include all ids in `node_ids`. Returns `None` if no such
/// container exists in this subtree.
fn find_container_with_all_children(children: &[Node], node_ids: &[String]) -> Option<String> {
    for node in children {
        let (container_id, grandchildren) = match node {
            Node::Frame(f) => (f.id.as_str(), f.children.as_slice()),
            Node::Group(g) => (g.id.as_str(), g.children.as_slice()),
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
            | Node::Unknown(_) => continue,
        };
        if node_ids.iter().all(|id| {
            grandchildren
                .iter()
                .any(|n| node_id_of(n) == Some(id.as_str()))
        }) {
            return Some(container_id.to_owned());
        }
        if let Some(found) = find_container_with_all_children(grandchildren, node_ids) {
            return Some(found);
        }
    }
    None
}

pub(in crate::engine) fn apply_group(
    node_ids: &[String],
    group_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Require at least one id.
    if node_ids.is_empty() {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_parent",
            "group requires at least one node id".to_owned(),
            None,
            None,
        ));
        return;
    }

    // Phase 1: locate the common parent children vec (shared-then-exclusive
    // two-phase, handled inside find_common_parent_children_mut).
    let children = match find_common_parent_children_mut(doc, node_ids) {
        Some(c) => c,
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_parent",
                "group requires all nodes to share a parent".to_owned(),
                None,
                None,
            ));
            return;
        }
    };

    // Phase 2: collect the indices of the named nodes within this children vec,
    // in ascending order so we can determine insert position and remove cleanly.
    let mut indices: Vec<usize> = node_ids
        .iter()
        .filter_map(|id| {
            children
                .iter()
                .position(|n| node_id_of(n) == Some(id.as_str()))
        })
        .collect();

    // All ids must resolve (filter_map would silently drop missing ones).
    if indices.len() != node_ids.len() {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_parent",
            "group requires all nodes to share a parent".to_owned(),
            None,
            None,
        ));
        return;
    }

    indices.sort_unstable();

    // Insert position = index of the first (lowest) member.
    // indices is non-empty: node_ids is non-empty and all ids resolved (guarded
    // by the length check above), so .first() will always be Some.
    let Some(&insert_at) = indices.first() else {
        return; // unreachable: guarded by node_ids.is_empty() check above
    };

    // Extract the nodes in their original relative order (lowest index first).
    // indices is already sorted ascending, so this produces source-order children.
    // All indices came from `.position()` on the same `children` slice and the
    // slice has not been mutated since — `.get()` returns Some for all of them.
    let group_children: Vec<Node> = indices
        .iter()
        .filter_map(|&i| children.get(i).cloned())
        .collect();

    // Remove from back to front to keep earlier indices stable.
    for &i in indices.iter().rev() {
        children.remove(i);
    }

    // Adjust insert_at: each removal of an index < insert_at shifts insert_at
    // down by one.  Since we sorted indices ascending and insert_at == indices[0],
    // all removed indices that were < insert_at have already been removed by the
    // rev-order loop above.  Actually insert_at is indices[0] (the minimum), so
    // no indices precede it — insert_at is stable after we remove >= insert_at
    // indices. We need to count how many indices were strictly less than insert_at
    // before the removals: since insert_at = indices[0] (minimum), zero indices
    // are smaller. So insert_at doesn't change.
    let insert_at = insert_at.min(children.len());

    // Build the group node with all fields at defaults (None / empty).
    // v0: x/y are None — no translation offset; authors must adjust child
    // geometry themselves if a specific group origin is needed.
    let group_node = Node::Group(GroupNode {
        id: group_id.to_owned(),
        name: None,
        role: None,
        x: None,
        y: None,
        w: None,
        h: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        anchor: None,
        anchor_zone: None,
        children: group_children,
        source_span: None,
        unknown_props: BTreeMap::new(),
    });

    children.insert(insert_at, group_node);
    record_affected(group_id, affected);
    // Post-validation catches group_id collision (id.duplicate).
}

pub(in crate::engine) fn apply_ungroup(
    group_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Phase 1 (shared scan): verify node exists and is a group; also capture
    // whether it has a non-zero x/y (for the advisory) and its page index.
    struct GroupInfo {
        page_index: usize,
        has_nonzero_offset: bool,
    }

    let info: Option<Result<GroupInfo, &'static str>> = {
        let mut result = None;
        'outer: for (pi, page) in doc.body.pages.iter().enumerate() {
            if let Some(node) = find_node_shared(&page.children, group_id) {
                let info = match node {
                    Node::Group(g) => {
                        let has_offset = g.x.as_ref().map(|d| d.value != 0.0).unwrap_or(false)
                            || g.y.as_ref().map(|d| d.value != 0.0).unwrap_or(false);
                        Ok(GroupInfo {
                            page_index: pi,
                            has_nonzero_offset: has_offset,
                        })
                    }
                    Node::Rect(_)
                    | Node::Ellipse(_)
                    | Node::Line(_)
                    | Node::Text(_)
                    | Node::Code(_)
                    | Node::Frame(_)
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
                    | Node::Unknown(_) => Err("not a group"),
                };
                result = Some(info);
                break 'outer;
            }
        }
        result
    };

    let info = match info {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", group_id),
                None,
                Some(group_id.to_owned()),
            ));
            return;
        }
        Some(Err(reason)) => {
            diagnostics.push(Diagnostic::error(
                "tx.unsupported_property",
                format!("ungroup: {:?} is {}", group_id, reason),
                None,
                Some(group_id.to_owned()),
            ));
            return;
        }
        Some(Ok(info)) => info,
    };

    // Advisory: v0 limitation — group x/y offset is not propagated to children.
    if info.has_nonzero_offset {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            format!(
                "ungroup: group {:?} has a non-zero x/y offset; v0 does not \
                 apply the offset to children on ungroup — child positions may \
                 shift visually",
                group_id
            ),
            None,
            Some(group_id.to_owned()),
        ));
    }

    // Phase 2 (exclusive borrow): splice the group's children in-place.
    let Some(page) = doc.body.pages.get_mut(info.page_index) else {
        return; // unreachable: page_index came from the shared scan above.
    };

    // Find and remove the group node from the page's subtree, then splice.
    splice_ungroup(&mut page.children, group_id);

    record_affected(group_id, affected);
}

/// Walk `children` to find the group with `group_id`, remove it, and insert
/// its children at the same index. Returns `true` if the group was found and
/// spliced, `false` otherwise (to continue recursion).
fn splice_ungroup(children: &mut Vec<Node>, group_id: &str) -> bool {
    // Check direct children first.
    if let Some(i) = children
        .iter()
        .position(|n| node_id_of(n) == Some(group_id))
    {
        // We confirmed it's a group in the shared-scan phase; use .get() for
        // checked access — the match arm handles the unreachable-but-safe case.
        let group_children = match children.get(i) {
            Some(Node::Group(g)) => g.children.clone(),
            _ => return false, // unreachable under normal flow
        };
        children.remove(i);
        // Insert the group's children at the same position, in order.
        for (offset, child) in group_children.into_iter().enumerate() {
            children.insert(i + offset, child);
        }
        return true;
    }
    // Descend into nested containers.
    for child in children.iter_mut() {
        let grandchildren = match child {
            Node::Frame(f) => &mut f.children,
            Node::Group(g) => &mut g.children,
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
            | Node::Unknown(_) => continue,
        };
        if splice_ungroup(grandchildren, group_id) {
            return true;
        }
    }
    false
}

pub(in crate::engine) fn apply_reparent(
    node_id: &str,
    new_parent: &str,
    position: &Position,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Phase 1 (shared scan): verify the node exists and capture the subtree so
    // we can run the cycle check without a mutable borrow.
    let node_page_index = doc.body.pages.iter().enumerate().find_map(|(pi, page)| {
        if page.children.iter().any(|n| subtree_contains(n, node_id)) {
            Some(pi)
        } else {
            None
        }
    });

    let pi = match node_page_index {
        Some(pi) => pi,
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
            return;
        }
    };

    // Cycle check (shared borrow): new_parent must not be node itself or a
    // descendant of node.  We locate the node in the shared slice and run
    // subtree_contains on it.
    {
        let page = match doc.body.pages.get(pi) {
            Some(p) => p,
            None => return, // unreachable
        };
        if let Some(node_ref) = find_node_shared(&page.children, node_id)
            && subtree_contains(node_ref, new_parent)
        {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_parent",
                format!(
                    "cannot reparent {:?} into {:?}: new_parent is within \
                     the node's own subtree",
                    node_id, new_parent
                ),
                None,
                Some(new_parent.to_owned()),
            ));
            return;
        }
        // Shared borrow of `page` ends here.
    }

    // Phase 2 (exclusive borrows): remove then re-insert.
    // Step 2a — remove the node from its current parent.
    let node = {
        // We need a mutable borrow of the page to remove; we know the page index.
        let page = match doc.body.pages.get_mut(pi) {
            Some(p) => p,
            None => return, // unreachable
        };
        match remove_node_by_id(&mut page.children, node_id) {
            Some(n) => n,
            None => {
                // Unexpected: the shared scan found it but remove didn't.
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("node {:?} disappeared during reparent", node_id),
                    None,
                    Some(node_id.to_owned()),
                ));
                return;
            }
        }
    };
    // The mutable borrow of `doc.body.pages[pi]` ends here.

    // Step 2b — locate the new parent's children vec.
    // `find_container_children_mut` handles page ids AND nested container ids.
    let new_children = match find_container_children_mut(doc, new_parent) {
        Some(c) => c,
        None => {
            // new_parent is not a container — roll back by re-inserting the node
            // at the end of its original page (best-effort; the transaction will
            // be rejected by the error diagnostic anyway).
            if let Some(page) = doc.body.pages.get_mut(pi) {
                page.children.push(node);
            }
            diagnostics.push(Diagnostic::error(
                "tx.invalid_parent",
                format!(
                    "no container with id {:?} (new_parent must be a page, group, or frame)",
                    new_parent
                ),
                None,
                Some(new_parent.to_owned()),
            ));
            return;
        }
    };

    // Step 2c — resolve the insertion index and insert.
    let idx = match resolve_position(position, new_children, new_parent, diagnostics) {
        Some(i) => i,
        None => {
            // resolve_position already pushed a diagnostic; roll back.
            if let Some(page) = doc.body.pages.get_mut(pi) {
                page.children.push(node);
            }
            return;
        }
    };

    new_children.insert(idx, node);
    record_affected(node_id, affected);
}
