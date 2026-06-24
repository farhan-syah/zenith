//! `DuplicateNode` and `DuplicatePage` application, plus the id-cloning helpers
//! they share (leaf id setter, any-variant id setter, subtree id-suffixing).

use zenith_core::ast::document::{Fold, SafeZone};
use zenith_core::{Diagnostic, Document, Node};

use super::super::{
    find_node_shared, node_id_of, node_kind_str, record_affected, subtree_contains,
};

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
        Node::Pattern(p) => {
            p.id = new_id;
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
        if let Some(src) = children.get(i) {
            let mut clone = src.clone();
            node_set_id(&mut clone, new_id.to_owned());
            children.insert(i + 1, clone);
            return true;
        }
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
            | Node::Pattern(_) => Vec::new(),
        };
        for list in lists {
            if duplicate_in_children(list, id, new_id) {
                return true;
            }
        }
    }

    false
}

pub(in crate::engine) fn apply_duplicate_node(
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

/// Recursively append `id_suffix` to the id of every node in `children`,
/// descending into `group`/`frame`/`table`/`unknown` containers.
///
/// Mirrors the ordered recursion of [`duplicate_in_children`]: a plain in-order
/// walk over the slice with no HashMap, so the result is deterministic. Ids are
/// read/written through the shared [`node_id_of`] reader and the
/// [`node_set_id_any`] setter; leaf and container nodes alike get suffixed, and
/// containers also recurse into their own children.
pub(crate) fn suffix_ids_in_children(children: &mut [Node], id_suffix: &str) {
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
            | Node::Pattern(_) => {}
        }
    }
}

/// Append `id_suffix` to every safe-zone and fold id in a cloned page and clear
/// their source spans. Page-metadata children (safe-zones, folds) carry ids in
/// the same namespace as nodes, so a deep page copy must suffix them too to stay
/// collision-free. Used by `DuplicatePage` and `merge_candidate_page`.
pub(crate) fn suffix_zone_and_fold_ids(
    safe_zones: &mut [SafeZone],
    folds: &mut [Fold],
    id_suffix: &str,
) {
    for zone in safe_zones.iter_mut() {
        zone.id.push_str(id_suffix);
        zone.source_span = None;
    }
    for fold in folds.iter_mut() {
        fold.id.push_str(id_suffix);
        fold.source_span = None;
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
pub(in crate::engine) fn node_set_id_any(node: &mut Node, new_id: String) {
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
        | Node::Connector(_)
        | Node::Pattern(_) => {
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

pub(in crate::engine) fn apply_duplicate_page(
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
    suffix_zone_and_fold_ids(&mut clone.safe_zones, &mut clone.folds, id_suffix);

    // 5. Insert the clone immediately after the source page.
    doc.body.pages.insert(position + 1, clone);

    // 6. Record the new page id. Post-validation catches any residual duplicate id.
    record_affected(new_id, affected);
}
