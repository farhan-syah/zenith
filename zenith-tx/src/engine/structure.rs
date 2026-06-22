//! Structural op application: reorder, add/remove, group/ungroup, reparent,
//! duplicate — plus the container/child finders and tree-mutation helpers they
//! share.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, Document, GroupNode, KdlAdapter, KdlSource, Node, Page, PropertyValue,
    Unit,
};

use crate::op::Position;

use super::{find_node_shared, node_id_of, node_kind_str, record_affected, subtree_contains};

// ── Reorder ops ───────────────────────────────────────────────────────────────

/// Which z-order reorder to perform.
#[derive(Copy, Clone)]
pub(super) enum ReorderKind {
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

pub(super) fn apply_reorder(
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
                other => return other,
            },
            Node::Group(g) => match reorder_in(&mut g.children, id, kind) {
                MoveOutcome::NotFound => {}
                other => return other,
            },
            Node::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        match reorder_in(&mut cell.children, id, kind) {
                            MoveOutcome::NotFound => {}
                            other => return other,
                        }
                    }
                }
            }
            Node::Unknown(u) => match reorder_in(&mut u.children, id, kind) {
                MoveOutcome::NotFound => {}
                other => return other,
            },
            _ => {}
        }
    }
    MoveOutcome::NotFound
}

// ── Container / child finders ─────────────────────────────────────────────────

/// Return a mutable reference to the children vec of the container identified by
/// `parent_id` — either a page (matched by page `id`) or a nested `group`/`frame`
/// (matched by node `id`). Returns `None` if no such container exists (including
/// when the id names a leaf node).
///
/// Two-phase borrow, mirroring [`find_node_any_mut`]: a shared scan locates the
/// target page index first, then a single exclusive borrow descends.
fn find_container_children_mut<'doc>(
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
fn subtree_contains_container(node: &Node, parent_id: &str) -> bool {
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
        _ => false,
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
            _ => None,
        });

    match hit {
        None => None,
        Some(Hit::Direct(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => Some(&mut f.children),
            Some(Node::Group(g)) => Some(&mut g.children),
            _ => None, // unreachable: phase-1 confirmed a container at i
        },
        Some(Hit::Descend(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => find_container_in_children_mut(&mut f.children, parent_id),
            Some(Node::Group(g)) => find_container_in_children_mut(&mut g.children, parent_id),
            _ => None, // unreachable
        },
    }
}

/// Remove the node with `id` from `children` or any nested container
/// (`group`, `frame`, `table` cell, or `unknown`) within it, returning the
/// removed node, or `None` if absent.
fn remove_node_by_id(children: &mut Vec<Node>, id: &str) -> Option<Node> {
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
            _ => None,
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
fn resolve_position(
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

// ── AddNode / RemoveNode ──────────────────────────────────────────────────────

/// Construct a single [`Node`] from a `.zen` node fragment by wrapping it in a
/// minimal synthetic document and parsing it through the canonical KDL parser.
///
/// Reusing the parser means every node kind, nested children (for group/frame),
/// tokens, and properties are supported with no per-field mapping. The wrapper's
/// `tokens`/`styles` blocks are left to their AST defaults (empty) — the real
/// candidate document, which carries the real tokens/assets, is what
/// post-validation actually checks.
///
/// Returns `Err` with a human-readable message if the fragment does not parse or
/// does not contain exactly one top-level node.
fn build_node_from_fragment(fragment: &str) -> Result<Node, String> {
    let synthetic = format!(
        "zenith version=1 {{\n  document id=\"__tx_doc\" {{\n    page id=\"__tx_page\" w=(px)1 h=(px)1 {{\n{fragment}\n    }}\n  }}\n}}\n"
    );
    let doc = KdlAdapter
        .parse(synthetic.as_bytes())
        .map_err(|e| format!("failed to parse node fragment: {e}"))?;
    let mut page = doc
        .body
        .pages
        .into_iter()
        .next()
        .ok_or_else(|| "synthetic document produced no page".to_owned())?;
    if page.children.len() != 1 {
        return Err(format!(
            "expected exactly one node in fragment, found {}",
            page.children.len()
        ));
    }
    Ok(page.children.remove(0))
}

pub(super) fn apply_add_node(
    parent: &str,
    position: &Position,
    source: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // 1. Build the node from the `.zen` fragment.
    let node = match build_node_from_fragment(source) {
        Ok(n) => n,
        Err(e) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_node_spec",
                format!("could not construct node from source fragment: {e}"),
                None,
                None,
            ));
            return;
        }
    };

    // 2. Locate the parent container.
    let children = match find_container_children_mut(doc, parent) {
        Some(c) => c,
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_parent",
                format!(
                    "no container node with id {:?} (parent must be a page, group, or frame)",
                    parent
                ),
                None,
                Some(parent.to_owned()),
            ));
            return;
        }
    };

    // 3. Resolve the insertion index against the current children.
    let idx = match resolve_position(position, children, parent, diagnostics) {
        Some(i) => i,
        None => return, // resolve_position already pushed a diagnostic
    };

    // 4. Capture the new node's id (if any) before moving it in, then insert.
    let new_id = node_id_of(&node).map(|s| s.to_owned());
    children.insert(idx, node);
    if let Some(id) = new_id {
        record_affected(&id, affected);
    }
    // 5. Post-validation handles duplicate-id / missing-geometry / unknown-token / etc.
}

pub(super) fn apply_remove_node(
    node_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    for page in doc.body.pages.iter_mut() {
        if remove_node_by_id(&mut page.children, node_id).is_some() {
            record_affected(node_id, affected);
            return;
        }
    }
    diagnostics.push(Diagnostic::error(
        "tx.unknown_node",
        format!("no node with id {:?}", node_id),
        None,
        Some(node_id.to_owned()),
    ));
}

// ── DuplicateNode ─────────────────────────────────────────────────────────────

/// Return `true` if `node` is a container variant (`Frame` or `Group`).
///
/// Used by [`apply_duplicate_node`] to enforce the v0 leaf-only restriction.
/// Duplicating a container would clone all descendant ids verbatim, producing
/// document-wide duplicate ids. Re-id'ing an entire subtree is deferred.
fn node_is_container(node: &Node) -> bool {
    // `Instance` is treated as container-ish here so the leaf-only duplicate
    // guard rejects it: its expanded subtree re-ids descendants by an
    // instance-id prefix, so duplicating it verbatim (with a single new id) is
    // not a v0-supported operation — the same deferral as Frame/Group.
    // `Table` is container-ish: its cells hold descendant ids, so a verbatim
    // duplicate would clone those ids. Re-id'ing the subtree is deferred, the
    // same deferral as Frame/Group.
    matches!(
        node,
        Node::Frame(_) | Node::Group(_) | Node::Instance(_) | Node::Table(_)
    )
}

/// Set the `id` field on a leaf [`Node`] variant to `new_id`.
///
/// Mirrors [`node_id_of`] (the shared-borrow id reader). Only leaf variants
/// are covered; `Frame` and `Group` are deliberately excluded because
/// [`apply_duplicate_node`] rejects containers before calling this helper.
/// Returns `false` for containers and for an `Unknown` node (whose id lives in
/// an `Option<String>`, set via [`node_set_id_any`], not this leaf setter). That
/// path is unreachable from `apply_duplicate_node`, which rejects containers up
/// front and never duplicates an unknown node verbatim.
fn node_set_id(node: &mut Node, new_id: String) -> bool {
    match node {
        Node::Rect(r) => {
            r.id = new_id;
            true
        }
        Node::Ellipse(e) => {
            e.id = new_id;
            true
        }
        Node::Line(l) => {
            l.id = new_id;
            true
        }
        Node::Text(t) => {
            t.id = new_id;
            true
        }
        Node::Code(c) => {
            c.id = new_id;
            true
        }
        Node::Image(i) => {
            i.id = new_id;
            true
        }
        Node::Polygon(p) => {
            p.id = new_id;
            true
        }
        Node::Polyline(p) => {
            p.id = new_id;
            true
        }
        Node::Field(f) => {
            f.id = new_id;
            true
        }
        Node::Toc(t) => {
            t.id = new_id;
            true
        }
        Node::Footnote(f) => {
            f.id = new_id;
            true
        }
        Node::Shape(s) => {
            s.id = new_id;
            true
        }
        Node::Connector(c) => {
            c.id = new_id;
            true
        }
        // Containers (and the container-ish instance) are handled by the v0
        // guard in apply_duplicate_node. An Unknown node's id lives in an
        // `Option<String>` (not the leaf `id: String` this setter writes); its
        // re-id path goes through `node_set_id_any`, so it returns false here.
        Node::Frame(_) | Node::Group(_) | Node::Instance(_) | Node::Table(_) | Node::Unknown(_) => {
            false
        }
    }
}

/// Walk `children` looking for a node with `id`. When found, clone it, set
/// its id to `new_id`, and insert the clone immediately after the original.
/// Returns `true` on success, `false` if the id is not in this slice (recurse
/// into container children to continue the search).
///
/// Callers that need the source-is-container check must do so before calling
/// this function (see [`apply_duplicate_node`]).
fn duplicate_in_children(children: &mut Vec<Node>, id: &str, new_id: &str) -> bool {
    // Phase 1 (shared scan): find the index of the source node in this slice.
    let direct = children.iter().position(|n| node_id_of(n) == Some(id));

    if let Some(i) = direct {
        // Source is a direct child — clone it here, assign the new id, insert.
        // Allocate the owned String only at the single insertion site.
        let mut clone = children[i].clone();
        node_set_id(&mut clone, new_id.to_owned());
        children.insert(i + 1, clone);
        return true;
    }

    // Phase 2: descend into container children. The recursive call performs its
    // own search-and-insert, so we just recurse into each container and stop at
    // the first that reports success. No String clone per iteration — new_id is
    // a borrowed &str that is passed down without allocation.
    for child in children.iter_mut() {
        // Each container kind contributes one or more child lists to recurse into
        // (a table contributes every cell's children). Collecting the mutable
        // borrows first keeps the recursion uniform across node kinds.
        let lists: Vec<&mut Vec<Node>> = match child {
            Node::Frame(f) => vec![&mut f.children],
            Node::Group(g) => vec![&mut g.children],
            Node::Table(t) => t
                .rows
                .iter_mut()
                .flat_map(|row| row.cells.iter_mut().map(|cell| &mut cell.children))
                .collect(),
            Node::Unknown(u) => vec![&mut u.children],
            _ => Vec::new(),
        };
        for list in lists {
            if duplicate_in_children(list, id, new_id) {
                return true;
            }
        }
    }

    false
}

pub(super) fn apply_duplicate_node(
    node_id: &str,
    new_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // 1. Verify the source node exists anywhere in the document (shared scan).
    //    We also need its variant to enforce the v0 leaf-only restriction.
    //    Use a two-phase approach mirroring find_node_any_mut.
    let page_index = doc.body.pages.iter().enumerate().find_map(|(pi, page)| {
        let found = page.children.iter().any(|n| subtree_contains(n, node_id));
        if found { Some(pi) } else { None }
    });

    let pi = match page_index {
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

    // 2. Check whether the source is a container (v0 restriction).
    //    We must obtain a shared reference to inspect the variant, then release
    //    it before taking the mutable borrow needed for the clone-insert step.
    {
        let Some(page) = doc.body.pages.get(pi) else {
            return; // unreachable: pi came from the enumerate scan above.
        };
        if let Some(src) = find_node_shared(&page.children, node_id)
            && node_is_container(src)
        {
            let kind = node_kind_str(src);
            diagnostics.push(Diagnostic::error(
                "tx.unsupported_property",
                format!(
                    "duplicating a {} is not supported in v0; re-id'ing a subtree \
                     is deferred — only leaf nodes may be duplicated",
                    kind
                ),
                None,
                Some(node_id.to_owned()),
            ));
            return;
        }
        // Shared borrow of `page` ends here.
    }

    // 3. Clone the source and insert it immediately after the original.
    //    `duplicate_in_children` does the clone+id-set+insert in one pass.
    let Some(page) = doc.body.pages.get_mut(pi) else {
        return; // unreachable: pi came from the enumerate scan above.
    };
    duplicate_in_children(&mut page.children, node_id, new_id);

    // 4. Record the clone as affected. Post-validation (step 4 of run_transaction)
    //    will catch a new_id collision with an existing node via id.duplicate.
    record_affected(new_id, affected);
}

// ── DuplicatePage ─────────────────────────────────────────────────────────────

/// Recursively append `id_suffix` to the id of every node in `children`,
/// descending into `group`/`frame`/`table`/`unknown` containers.
///
/// Mirrors the ordered recursion of [`duplicate_in_children`]: a plain in-order
/// walk over the slice with no HashMap, so the result is deterministic. Ids are
/// read/written through the shared [`node_id_of`] reader and the
/// [`node_set_id_any`] setter; leaf and container nodes alike get suffixed, and
/// containers also recurse into their own children.
fn suffix_ids_in_children(children: &mut [Node], id_suffix: &str) {
    for child in children.iter_mut() {
        // Suffix this node's own id (if it has one), then recurse.
        if let Some(old_id) = node_id_of(child) {
            let new_id = format!("{old_id}{id_suffix}");
            node_set_id_any(child, new_id);
        }
        match child {
            Node::Frame(f) => suffix_ids_in_children(&mut f.children, id_suffix),
            Node::Group(g) => suffix_ids_in_children(&mut g.children, id_suffix),
            Node::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        suffix_ids_in_children(&mut cell.children, id_suffix);
                    }
                }
            }
            Node::Unknown(u) => suffix_ids_in_children(&mut u.children, id_suffix),
            _ => {}
        }
    }
}

/// Set the `id` of any id-bearing [`Node`] variant, including containers.
///
/// [`node_set_id`] deliberately excludes `Frame`/`Group` because the leaf-only
/// `duplicate_node` path never re-ids a container. `duplicate_page` does need to
/// re-id containers, so this sibling covers every variant that [`node_id_of`]
/// can read an id from. `Unknown` nodes that carry an `id` attribute are re-id'd
/// here; those without one are a no-op (the caller skips them: `node_id_of`
/// returns `None`, so no suffix is computed and this function is not called).
fn node_set_id_any(node: &mut Node, new_id: String) {
    match node {
        Node::Frame(f) => f.id = new_id,
        Node::Group(g) => g.id = new_id,
        // The instance is an id-bearing container-ish node; set it directly
        // (node_set_id deliberately excludes it as a non-leaf).
        Node::Instance(i) => i.id = new_id,
        // The table is an id-bearing container; set it directly (node_set_id
        // excludes it as a non-leaf, like Frame/Group).
        Node::Table(t) => t.id = new_id,
        // Leaf variants share the existing setter.
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Text(_)
        | Node::Code(_)
        | Node::Image(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Shape(_)
        | Node::Connector(_) => {
            node_set_id(node, new_id);
        }
        // An unknown node is id-bearing when authored with an `id` attribute;
        // re-id it on page-duplicate so cloned subtrees stay unique. When it has
        // no id this is a no-op (the caller also skips it: `node_id_of` returns
        // `None`, so no suffix is computed).
        Node::Unknown(u) => {
            if u.id.is_some() {
                u.id = Some(new_id);
            }
        }
    }
}

pub(super) fn apply_duplicate_page(
    page_id: &str,
    new_id: &str,
    id_suffix: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // 1. Locate the source page by id.
    let Some(position) = doc.body.pages.iter().position(|p| p.id == page_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("duplicate_page: page {:?} not found", page_id),
            None,
            Some(page_id.to_owned()),
        ));
        return;
    };

    // Advisory: an empty suffix cannot keep descendant ids unique. Post-validation
    // will still reject via id.duplicate; this just makes the cause obvious.
    if id_suffix.is_empty() {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            format!(
                "duplicate_page: empty id_suffix will not keep cloned node ids \
                 unique for page {:?}; the transaction will be rejected",
                page_id
            ),
            None,
            Some(page_id.to_owned()),
        ));
    }

    // 2. Clone the source page. `.get()` is checked though `position` is valid.
    let Some(source) = doc.body.pages.get(position) else {
        return; // unreachable: position came from the scan above.
    };
    let mut clone = source.clone();

    // 3. The new page takes new_id exactly; clear its stale source span.
    clone.id = new_id.to_owned();
    clone.source_span = None;

    // 4. Suffix every descendant node id and every safe-zone id in the copy.
    suffix_ids_in_children(&mut clone.children, id_suffix);
    for zone in clone.safe_zones.iter_mut() {
        zone.id.push_str(id_suffix);
        zone.source_span = None;
    }
    for fold in clone.folds.iter_mut() {
        fold.id.push_str(id_suffix);
        fold.source_span = None;
    }

    // 5. Insert the clone immediately after the source page.
    doc.body.pages.insert(position + 1, clone);

    // 6. Record the new page id. Post-validation catches any residual duplicate id.
    record_affected(new_id, affected);
}

// ── Page structure ops (AddPage / DeletePage / ReorderPages) ──────────────────

/// Parse a canonical `"(unit)value"` dimension string (e.g. `"(px)1800"`) into
/// a [`Dimension`], preserving its unit.
///
/// Mirrors the parser used by geometry ops but keeps the unit (rather than
/// collapsing to px) because [`Page::width`]/[`Page::height`] store a full
/// `Dimension`. Returns `None` if the string is not parenthesized-unit-prefixed
/// or the numeric tail is not a finite number.
pub(super) fn parse_dimension_str(s: &str) -> Option<Dimension> {
    let rest = s.strip_prefix('(')?;
    let (unit_str, value_str) = rest.split_once(')')?;
    let unit = Unit::from_annotation(unit_str);
    let value: f64 = value_str.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(Dimension { value, unit })
}

/// The borrowed fields of an [`crate::op::Op::AddPage`], grouped so the apply
/// function stays under the argument-count lint while keeping each field named.
pub(super) struct AddPageSpec<'a> {
    /// Stable id for the new page.
    pub id: &'a str,
    /// Width dimension string, e.g. `"(px)1800"`.
    pub w: &'a str,
    /// Height dimension string, e.g. `"(px)1200"`.
    pub h: &'a str,
    /// Optional background token-ref id.
    pub background: Option<&'a str>,
    /// 0-based insert position; `None` appends.
    pub index: Option<usize>,
}

pub(super) fn apply_add_page(
    spec: &AddPageSpec<'_>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let AddPageSpec {
        id,
        w,
        h,
        background,
        index,
    } = *spec;
    // 1. Reject a page id that collides with an existing page id. (A collision
    //    with a non-page node id is also caught by post-validation's
    //    id.duplicate; the page-level check here gives a precise message.)
    if doc.body.pages.iter().any(|p| p.id == id) {
        diagnostics.push(Diagnostic::error(
            "tx.duplicate_id",
            format!("add_page: a page with id {:?} already exists", id),
            None,
            Some(id.to_owned()),
        ));
        return;
    }

    // 2. Parse the width/height dimension strings.
    let Some(width) = parse_dimension_str(w) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "add_page: width {:?} is not a valid dimension (expected e.g. \"(px)1800\")",
                w
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };
    let Some(height) = parse_dimension_str(h) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "add_page: height {:?} is not a valid dimension (expected e.g. \"(px)1200\")",
                h
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };

    // 3. Resolve the insert position. `None` appends; an explicit index must be
    //    within `0..=len` (len = append).
    let len = doc.body.pages.len();
    let at = match index {
        None => len,
        Some(i) => {
            if i > len {
                diagnostics.push(Diagnostic::error(
                    "tx.out_of_range",
                    format!(
                        "add_page: index {} is out of range (document has {} page(s))",
                        i, len
                    ),
                    None,
                    Some(id.to_owned()),
                ));
                return;
            }
            i
        }
    };

    // 4. Build the empty page with all optional fields at their defaults.
    let page = Page {
        id: id.to_owned(),
        name: None,
        width,
        height,
        background: background.map(|b| PropertyValue::TokenRef(b.to_owned())),
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        children: Vec::new(),
        source_span: None,
    };

    doc.body.pages.insert(at, page);
    record_affected(id, affected);
}

pub(super) fn apply_delete_page(
    page_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let Some(pos) = doc.body.pages.iter().position(|p| p.id == page_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("delete_page: page {:?} not found", page_id),
            None,
            Some(page_id.to_owned()),
        ));
        return;
    };
    doc.body.pages.remove(pos);
    record_affected(page_id, affected);
}

/// Resize a page by replacing its `width`/`height` dimensions.
///
/// Parses `w` and `h` via [`parse_dimension_str`], then locates the page by id
/// and overwrites its size. Children are NOT reflowed — any child that now falls
/// outside the new bounds will generate an `off_canvas` advisory at validation.
pub(super) fn apply_set_page_size(
    page_id: &str,
    w: &str,
    h: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Parse width — must be a valid dimension and strictly positive.
    let w_dim = match parse_dimension_str(w) {
        Some(d) if d.value > 0.0 => d,
        Some(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!("set_page_size: w {:?} must be a finite value > 0", w),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "set_page_size: w {:?} is not a valid dimension (expected e.g. \"(px)794\")",
                    w
                ),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
    };

    // Parse height — must be a valid dimension and strictly positive.
    let h_dim = match parse_dimension_str(h) {
        Some(d) if d.value > 0.0 => d,
        Some(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!("set_page_size: h {:?} must be a finite value > 0", h),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "set_page_size: h {:?} is not a valid dimension (expected e.g. \"(px)1123\")",
                    h
                ),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
    };

    // Locate the page by id.
    let Some(page) = doc.body.pages.iter_mut().find(|p| p.id == page_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("set_page_size: page {:?} not found", page_id),
            None,
            Some(page_id.to_owned()),
        ));
        return;
    };

    page.width = w_dim;
    page.height = h_dim;
    record_affected(page_id, affected);
}

pub(super) fn apply_reorder_pages(
    order: &[String],
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // The current page ids in document order.
    let current: Vec<String> = doc.body.pages.iter().map(|p| p.id.clone()).collect();

    // `order` must be a permutation of `current`: same length, no duplicates,
    // and every requested id must exist exactly once. We verify via sorted
    // comparison (deterministic, no HashMap).
    let mut order_sorted: Vec<&String> = order.iter().collect();
    order_sorted.sort();
    let dup = order_sorted.windows(2).any(|w| w[0] == w[1]);
    let mut current_sorted: Vec<&String> = current.iter().collect();
    current_sorted.sort();

    if dup || order_sorted != current_sorted {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "reorder_pages: order {:?} is not a permutation of the existing \
                 page ids {:?}",
                order, current
            ),
            None,
            None,
        ));
        return;
    }

    // Rebuild the page vec in the requested order. Each id resolves to exactly
    // one page (guaranteed by the permutation check above). We drain the old
    // pages into a lookup-by-index without HashMap: for each target id, find and
    // take its page from a slot-tracking vec.
    let mut slots: Vec<Option<Page>> = doc.body.pages.drain(..).map(Some).collect();
    let mut reordered: Vec<Page> = Vec::with_capacity(order.len());
    for id in order {
        // Find the first remaining slot whose page id matches. The permutation
        // check guarantees a match exists for every id.
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.as_ref().map(|p| p.id.as_str()) == Some(id.as_str()))
            && let Some(page) = slot.take()
        {
            reordered.push(page);
        }
    }
    doc.body.pages = reordered;

    // Record every page id as affected (the whole list was restructured).
    for id in order {
        record_affected(id, affected);
    }
}

// ── Group ─────────────────────────────────────────────────────────────────────

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
            _ => continue,
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

pub(super) fn apply_group(
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
        children: group_children,
        source_span: None,
        unknown_props: BTreeMap::new(),
    });

    children.insert(insert_at, group_node);
    record_affected(group_id, affected);
    // Post-validation catches group_id collision (id.duplicate).
}

// ── Ungroup ───────────────────────────────────────────────────────────────────

pub(super) fn apply_ungroup(
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
                    _ => Err("not a group"),
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
            _ => continue,
        };
        if splice_ungroup(grandchildren, group_id) {
            return true;
        }
    }
    false
}

// ── Reparent ──────────────────────────────────────────────────────────────────

pub(super) fn apply_reparent(
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
