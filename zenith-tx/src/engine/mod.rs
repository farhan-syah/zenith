//! Transaction engine: [`run_transaction`] and all per-op application logic.
//!
//! This module is pure: it performs no file I/O and does not mutate the input
//! document (it works on a clone). Dry-run vs. apply is the caller's concern.

use zenith_core::{
    Diagnostic, Dimension, Document, KdlAdapter, KdlSource, Node, Severity, Unit, validate,
};

use crate::op::{Op, Transaction};
use crate::result::{TxError, TxResult, TxStatus};

mod asset;
mod flags;
mod geometry;
mod pattern;
mod recipe;
pub(crate) mod structure;
mod style;
mod token;

use asset::{apply_add_asset, apply_set_asset};
use flags::{apply_set_locked, apply_set_points, apply_set_visible};
use geometry::{
    GeometryDelta, apply_align_nodes, apply_align_to_edge, apply_distribute_nodes,
    apply_set_geometry,
};
use pattern::apply_detach_pattern;
use recipe::{RecipeScalars, apply_create_recipe, apply_delete_recipe, apply_update_recipe};
use structure::{
    ReorderKind, apply_add_node, apply_add_page, apply_delete_page, apply_duplicate_node,
    apply_duplicate_page, apply_group, apply_remove_node, apply_reorder, apply_reorder_pages,
    apply_reparent, apply_set_page_size, apply_ungroup,
};
use style::{
    apply_find_replace_text, apply_replace_text, apply_set_fill, apply_set_opacity,
    apply_set_stroke, apply_set_stroke_width, apply_set_style_property, apply_set_text_align,
    apply_set_text_direction, apply_set_text_overflow,
};
use token::{apply_create_token, apply_update_token_value};

// ── Public entry point ────────────────────────────────────────────────────────

/// Apply `tx` to `doc` and return a structured [`TxResult`].
///
/// The function is **pure**: `doc` is never mutated (a clone is used for the
/// candidate), and no I/O is performed. Both dry-run and apply callers receive
/// the same result shape; the caller decides whether to persist `source_after`.
pub fn run_transaction(doc: &Document, tx: &Transaction) -> Result<TxResult, TxError> {
    let adapter = KdlAdapter;

    // 1. Format the original document → source_before.
    let source_before_bytes = adapter.format(doc).map_err(|e| TxError {
        message: format!("failed to format source document: {e}"),
    })?;
    let source_before = String::from_utf8(source_before_bytes).map_err(|e| TxError {
        message: format!("source_before is not valid UTF-8: {e}"),
    })?;

    // 2. Clone the document into a mutable candidate.
    let mut candidate = doc.clone();

    // 3. Apply each op in order, collecting diagnostics and affected ids.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut affected: Vec<String> = Vec::new(); // insertion-order, de-duplicated

    for op in &tx.ops {
        // Lock pre-check: a guarded op against a locked node is rejected unless
        // the transaction carries `permissions.allow_locked`. The check reads the
        // *candidate* state, so a `set_locked` earlier in the same transaction
        // locks the node for later ops (and `set_locked` itself is exempt, so a
        // node can always be unlocked). Targets are visited in order for
        // determinism; if any target is locked the whole op is skipped, leaving
        // the emitted `node.locked` error to reject the transaction in step 5.
        if !tx.permissions.allow_locked {
            let mut locked_hit = false;
            for target in op_lock_targets(op) {
                if node_is_locked(&candidate, target) {
                    locked_hit = true;
                    diagnostics.push(Diagnostic::error(
                        "node.locked",
                        format!(
                            "node '{}' is locked; unlock it or set \
                             permissions.allow_locked to edit it",
                            target
                        ),
                        None,
                        Some(target.to_owned()),
                    ));
                }
            }
            if locked_hit {
                continue;
            }
        }

        apply_op(op, &mut candidate, &mut diagnostics, &mut affected);
    }

    // 4. Post-apply validation.
    let report = validate(&candidate);
    diagnostics.extend(report.diagnostics);

    // 5. Determine status and source_after.
    let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);
    let has_warnings = diagnostics.iter().any(|d| d.severity == Severity::Warning);

    let (status, source_after) = if has_errors {
        // Rejected — discard candidate, source_after == source_before.
        (TxStatus::Rejected, source_before.clone())
    } else {
        let after_bytes = adapter.format(&candidate).map_err(|e| TxError {
            message: format!("failed to format candidate document: {e}"),
        })?;
        let after = String::from_utf8(after_bytes).map_err(|e| TxError {
            message: format!("source_after is not valid UTF-8: {e}"),
        })?;
        let status = if has_warnings {
            TxStatus::AcceptedWithWarnings
        } else {
            TxStatus::Accepted
        };
        (status, after)
    };

    Ok(TxResult {
        status,
        diagnostics,
        source_before,
        source_after,
        affected_node_ids: affected,
    })
}

// ── Per-op dispatch ───────────────────────────────────────────────────────────

fn apply_op(
    op: &Op,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match op {
        Op::SetTextAlign {
            node: node_id,
            align,
        } => {
            apply_set_text_align(node_id, align, doc, diagnostics, affected);
        }
        Op::MoveForward { node: node_id } => {
            apply_reorder(node_id, ReorderKind::Forward, doc, diagnostics, affected);
        }
        Op::MoveBackward { node: node_id } => {
            apply_reorder(node_id, ReorderKind::Backward, doc, diagnostics, affected);
        }
        Op::MoveToFront { node: node_id } => {
            apply_reorder(node_id, ReorderKind::ToFront, doc, diagnostics, affected);
        }
        Op::MoveToBack { node: node_id } => {
            apply_reorder(node_id, ReorderKind::ToBack, doc, diagnostics, affected);
        }
        Op::SetFill {
            node: node_id,
            fill,
        } => {
            apply_set_fill(node_id, fill, doc, diagnostics, affected);
        }
        Op::SetStroke {
            node: node_id,
            stroke,
        } => {
            apply_set_stroke(node_id, stroke, doc, diagnostics, affected);
        }
        Op::SetStrokeWidth {
            node: node_id,
            stroke_width,
        } => {
            apply_set_stroke_width(node_id, stroke_width, doc, diagnostics, affected);
        }
        Op::SetVisible {
            node: node_id,
            visible,
        } => {
            apply_set_visible(node_id, *visible, doc, diagnostics, affected);
        }
        Op::SetLocked {
            node: node_id,
            locked,
        } => {
            apply_set_locked(node_id, *locked, doc, diagnostics, affected);
        }
        Op::SetGeometry {
            node: node_id,
            x,
            y,
            w,
            h,
            rotate,
        } => {
            apply_set_geometry(
                node_id,
                GeometryDelta {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    rotate: *rotate,
                },
                doc,
                diagnostics,
                affected,
            );
        }
        Op::SetPoints {
            node: node_id,
            points,
        } => {
            apply_set_points(node_id, points, doc, diagnostics, affected);
        }
        Op::AddNode {
            parent,
            position,
            source,
        } => {
            apply_add_node(parent, position, source, doc, diagnostics, affected);
        }
        Op::RemoveNode { node: node_id } => {
            apply_remove_node(node_id, doc, diagnostics, affected);
        }
        Op::SetOpacity {
            node: node_id,
            opacity,
        } => {
            apply_set_opacity(node_id, *opacity, doc, diagnostics, affected);
        }
        Op::ReplaceText {
            node: node_id,
            spans,
        } => {
            apply_replace_text(node_id, spans, doc, diagnostics, affected);
        }
        Op::DuplicateNode {
            node: node_id,
            new_id,
        } => {
            apply_duplicate_node(node_id, new_id, doc, diagnostics, affected);
        }
        Op::DuplicatePage {
            page,
            new_id,
            id_suffix,
        } => {
            apply_duplicate_page(page, new_id, id_suffix, doc, diagnostics, affected);
        }
        Op::Group { node_ids, group_id } => {
            apply_group(node_ids, group_id, doc, diagnostics, affected);
        }
        Op::Ungroup { group_id } => {
            apply_ungroup(group_id, doc, diagnostics, affected);
        }
        Op::Reparent {
            node: node_id,
            new_parent,
            position,
        } => {
            apply_reparent(node_id, new_parent, position, doc, diagnostics, affected);
        }
        Op::AlignNodes {
            node_ids,
            align,
            anchor,
        } => {
            apply_align_nodes(node_ids, align, anchor, doc, diagnostics, affected);
        }
        Op::SetTextOverflow { node_id, overflow } => {
            apply_set_text_overflow(node_id, overflow, doc, diagnostics, affected);
        }
        Op::DistributeNodes { node_ids, axis } => {
            apply_distribute_nodes(node_ids, axis, doc, diagnostics, affected);
        }
        Op::AddPage {
            id,
            w,
            h,
            background,
            index,
        } => {
            let spec = structure::AddPageSpec {
                id,
                w,
                h,
                background: background.as_deref(),
                index: *index,
            };
            apply_add_page(&spec, doc, diagnostics, affected);
        }
        Op::DeletePage { page } => {
            apply_delete_page(page, doc, diagnostics, affected);
        }
        Op::ReorderPages { order } => {
            apply_reorder_pages(order, doc, diagnostics, affected);
        }
        Op::AddAsset {
            id,
            kind,
            src,
            sha256,
        } => {
            apply_add_asset(id, kind, src, sha256.as_deref(), doc, diagnostics, affected);
        }
        Op::SetAsset { node_id, asset_id } => {
            apply_set_asset(node_id, asset_id, doc, diagnostics, affected);
        }
        Op::CreateToken {
            id,
            token_type,
            value,
        } => {
            apply_create_token(id, token_type, value, doc, diagnostics, affected);
        }
        Op::UpdateTokenValue { id, value } => {
            apply_update_token_value(id, value, doc, diagnostics, affected);
        }
        Op::SetStyleProperty {
            style_id,
            property,
            value,
        } => {
            apply_set_style_property(style_id, property, value, doc, diagnostics, affected);
        }
        Op::SetTextDirection { node, direction } => {
            apply_set_text_direction(node, direction, doc, diagnostics, affected);
        }
        Op::FindReplaceText {
            find,
            replace,
            node,
        } => {
            apply_find_replace_text(find, replace, node.as_deref(), doc, diagnostics, affected);
        }
        Op::SetPageSize { page, w, h } => {
            apply_set_page_size(page, w, h, doc, diagnostics, affected);
        }
        Op::AlignToEdge { node, edge, margin } => {
            apply_align_to_edge(node, edge, *margin, doc, diagnostics, affected);
        }
        Op::CreateRecipe {
            id,
            kind,
            seed,
            generator,
            bounds,
            detached,
        } => {
            apply_create_recipe(
                RecipeScalars {
                    id,
                    kind,
                    seed: *seed,
                    generator: generator.as_deref(),
                    bounds: bounds.as_deref(),
                    detached: *detached,
                },
                doc,
                diagnostics,
                affected,
            );
        }
        Op::UpdateRecipe {
            id,
            kind,
            seed,
            generator,
            bounds,
            detached,
        } => {
            apply_update_recipe(
                RecipeScalars {
                    id,
                    kind,
                    seed: *seed,
                    generator: generator.as_deref(),
                    bounds: bounds.as_deref(),
                    detached: *detached,
                },
                doc,
                diagnostics,
                affected,
            );
        }
        Op::DeleteRecipe { id } => {
            apply_delete_recipe(id, doc, diagnostics, affected);
        }
        Op::DetachPattern { node: node_id } => {
            apply_detach_pattern(node_id, doc, diagnostics, affected);
        }
    }
}

// ── Lock enforcement ──────────────────────────────────────────────────────────

/// Return the node id(s) a *mutating* op would edit, for the lock-guarded ops
/// only. Exempt ops return an empty `Vec`.
///
/// Guarded (return target id(s)): the property/geometry/text setters, removal,
/// the z-order reorders, `reparent` (its `node`), and `align_nodes` (every id,
/// in source order).
///
/// Exempt (empty): `set_locked` (must be able to *unlock* a locked node),
/// `set_visible` (visibility is a view toggle), `add_node`, `duplicate_node`
/// (the source is read-only), `group`, and `ungroup`.
fn op_lock_targets(op: &Op) -> Vec<&str> {
    match op {
        Op::SetTextAlign { node, .. }
        | Op::SetFill { node, .. }
        | Op::SetStroke { node, .. }
        | Op::SetStrokeWidth { node, .. }
        | Op::SetGeometry { node, .. }
        | Op::SetPoints { node, .. }
        | Op::SetOpacity { node, .. }
        | Op::ReplaceText { node, .. }
        | Op::RemoveNode { node }
        | Op::MoveForward { node }
        | Op::MoveBackward { node }
        | Op::MoveToFront { node }
        | Op::MoveToBack { node }
        | Op::Reparent { node, .. }
        | Op::SetTextOverflow { node_id: node, .. }
        | Op::SetTextDirection { node, .. }
        | Op::AlignToEdge { node, .. }
        | Op::DetachPattern { node } => vec![node.as_str()],
        // Doc-wide mode returns empty (lock handling is inside apply_find_replace_text).
        // Scoped mode: guard the named node.
        Op::FindReplaceText { node, .. } => {
            node.as_deref().map(|n| vec![n]).unwrap_or_default()
        }
        Op::AlignNodes { node_ids, .. } | Op::DistributeNodes { node_ids, .. } => {
            node_ids.iter().map(String::as_str).collect()
        }
        Op::SetAsset { node_id, .. } => vec![node_id.as_str()],
        Op::SetLocked { .. }
        | Op::SetVisible { .. }
        | Op::AddNode { .. }
        | Op::DuplicateNode { .. }
        | Op::DuplicatePage { .. }
        | Op::Group { .. }
        | Op::Ungroup { .. }
        // Page-structure ops act on `Page` structs, which have no `locked`
        // dimension (locking is a per-`Node` property). There is no node-level
        // lock target to enforce here, so these are exempt (empty).
        | Op::AddPage { .. }
        | Op::DeletePage { .. }
        | Op::ReorderPages { .. }
        | Op::SetPageSize { .. }
        // AddAsset creates new content and never mutates a node; exempt like AddNode.
        | Op::AddAsset { .. }
        // Token ops mutate the token block, not the node tree; no per-node lock target.
        | Op::CreateToken { .. }
        | Op::UpdateTokenValue { .. }
        // Style ops mutate the styles block, not the node tree; no per-node lock target.
        | Op::SetStyleProperty { .. }
        // Recipe ops mutate the recipes block, not the node tree; no per-node lock target.
        | Op::CreateRecipe { .. }
        | Op::UpdateRecipe { .. }
        | Op::DeleteRecipe { .. } => Vec::new(),
    }
}

/// Return `true` if the node with `id` exists and has `locked == Some(true)`.
///
/// Missing nodes and nodes with `locked` absent/`Some(false)` return `false`;
/// the missing-node case is left for the op's own `tx.unknown_node` path.
/// Mirrors the variant coverage of [`node_locked_mut`] via a shared scan.
fn node_is_locked(doc: &Document, id: &str) -> bool {
    fn locked_of(node: &Node) -> Option<bool> {
        match node {
            Node::Rect(n) => n.locked,
            Node::Ellipse(n) => n.locked,
            Node::Line(n) => n.locked,
            Node::Text(n) => n.locked,
            Node::Code(n) => n.locked,
            Node::Frame(n) => n.locked,
            Node::Group(n) => n.locked,
            Node::Image(n) => n.locked,
            Node::Polygon(n) => n.locked,
            Node::Polyline(n) => n.locked,
            Node::Instance(n) => n.locked,
            Node::Field(n) => n.locked,
            Node::Toc(n) => n.locked,
            Node::Table(n) => n.locked,
            Node::Shape(n) => n.locked,
            Node::Connector(n) => n.locked,
            Node::Pattern(n) => n.locked,
            // A footnote has no `locked` field; treat as unlocked.
            Node::Footnote(_) => None,
            Node::Unknown(_) => None,
        }
    }

    doc.body
        .pages
        .iter()
        .find_map(|page| find_node_shared(&page.children, id))
        .and_then(locked_of)
        == Some(true)
}

// ── Shared tree-walk helpers ──────────────────────────────────────────────────

/// Returns true if `node` is, or transitively contains, a node with `id`.
pub(super) fn subtree_contains(node: &Node, id: &str) -> bool {
    if node_id_of(node) == Some(id) {
        return true;
    }
    match node {
        Node::Frame(f) => f.children.iter().any(|c| subtree_contains(c, id)),
        Node::Group(g) => g.children.iter().any(|c| subtree_contains(c, id)),
        Node::Table(t) => t.rows.iter().any(|r| {
            r.cells
                .iter()
                .any(|c| c.children.iter().any(|ch| subtree_contains(ch, id)))
        }),
        Node::Unknown(u) => u.children.iter().any(|c| subtree_contains(c, id)),
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
        | Node::Pattern(_) => false,
    }
}

/// Walk the document tree and return a mutable reference to the node with
/// the given `id`, or `None` if not found.
///
/// Two-phase approach: shared scan first (to find the page index), then a
/// single targeted mutable borrow. This pattern avoids the borrow-checker
/// conflict that would arise if we tried to return a mutable reference from
/// within an `&mut`-iterating for loop.
pub(super) fn find_node_any_mut<'doc>(doc: &'doc mut Document, id: &str) -> Option<&'doc mut Node> {
    // Phase 1: find which page (shared borrow only).
    let page_index = doc.body.pages.iter().enumerate().find_map(|(pi, page)| {
        let found = page.children.iter().any(|n| subtree_contains(n, id));
        if found { Some(pi) } else { None }
    });

    // Phase 2: act on the found page with an exclusive borrow.
    match page_index {
        None => None,
        Some(pi) => match doc.body.pages.get_mut(pi) {
            None => None,
            Some(page) => find_in_children_any_mut(&mut page.children, id),
        },
    }
}

/// Descend into a children slice and return a mutable reference to the node
/// with `id`. Returns `None` if the id is not present in this subtree.
///
/// Two-phase: shared scan to find the index, then exclusive borrow to act.
///
/// No recursion-depth guard (accepted v0 limit, consistent with
/// `reorder_in` and `subtree_contains`).
fn find_in_children_any_mut<'a>(children: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    // Phase 1: find the index and how to reach it.
    // `Direct(i)` — id matches children[i] itself.
    // `Descend(i)` — id lives somewhere inside the container at children[i].
    enum Hit {
        Direct(usize),
        Descend(usize),
    }

    let hit = children.iter().enumerate().find_map(|(i, node)| {
        if node_id_of(node) == Some(id) {
            return Some(Hit::Direct(i));
        }
        match node {
            Node::Frame(f) if f.children.iter().any(|c| subtree_contains(c, id)) => {
                Some(Hit::Descend(i))
            }
            Node::Group(g) if g.children.iter().any(|c| subtree_contains(c, id)) => {
                Some(Hit::Descend(i))
            }
            Node::Table(t)
                if t.rows.iter().any(|r| {
                    r.cells
                        .iter()
                        .any(|c| c.children.iter().any(|ch| subtree_contains(ch, id)))
                }) =>
            {
                Some(Hit::Descend(i))
            }
            Node::Unknown(u) if u.children.iter().any(|c| subtree_contains(c, id)) => {
                Some(Hit::Descend(i))
            }
            Node::Frame(_)
            | Node::Group(_)
            | Node::Table(_)
            | Node::Unknown(_)
            | Node::Rect(_)
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
            | Node::Pattern(_) => None,
        }
    });

    // Phase 2: take the exclusive borrow we deferred.
    match hit {
        None => None,
        Some(Hit::Direct(i)) => children.get_mut(i),
        Some(Hit::Descend(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => find_in_children_any_mut(&mut f.children, id),
            Some(Node::Group(g)) => find_in_children_any_mut(&mut g.children, id),
            Some(Node::Table(t)) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        if let Some(found) = find_in_children_any_mut(&mut cell.children, id) {
                            return Some(found);
                        }
                    }
                }
                None
            }
            Some(Node::Unknown(u)) => find_in_children_any_mut(&mut u.children, id),
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
            | Some(Node::Shape(_))
            | Some(Node::Connector(_))
            | Some(Node::Pattern(_))
            | None => None,
        },
    }
}

/// Shared-borrow tree walk: find a node with `id` anywhere in `children`.
pub(super) fn find_node_shared<'a>(children: &'a [Node], id: &str) -> Option<&'a Node> {
    for node in children {
        if node_id_of(node) == Some(id) {
            return Some(node);
        }
        match node {
            Node::Frame(f) => {
                if let Some(found) = find_node_shared(&f.children, id) {
                    return Some(found);
                }
            }
            Node::Group(g) => {
                if let Some(found) = find_node_shared(&g.children, id) {
                    return Some(found);
                }
            }
            Node::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        if let Some(found) = find_node_shared(&cell.children, id) {
                            return Some(found);
                        }
                    }
                }
            }
            Node::Unknown(u) => {
                if let Some(found) = find_node_shared(&u.children, id) {
                    return Some(found);
                }
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
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_) => {}
        }
    }
    None
}

/// Extract the stable id string from any [`Node`] variant, if it has one.
pub(super) fn node_id_of(node: &Node) -> Option<&str> {
    match node {
        Node::Rect(r) => Some(&r.id),
        Node::Ellipse(e) => Some(&e.id),
        Node::Line(l) => Some(&l.id),
        Node::Text(t) => Some(&t.id),
        Node::Code(c) => Some(&c.id),
        Node::Frame(f) => Some(&f.id),
        Node::Group(g) => Some(&g.id),
        Node::Image(i) => Some(&i.id),
        Node::Polygon(p) => Some(&p.id),
        Node::Polyline(p) => Some(&p.id),
        Node::Instance(i) => Some(&i.id),
        Node::Field(f) => Some(&f.id),
        Node::Toc(t) => Some(&t.id),
        Node::Footnote(f) => Some(&f.id),
        Node::Table(t) => Some(&t.id),
        Node::Shape(s) => Some(&s.id),
        Node::Connector(c) => Some(&c.id),
        Node::Pattern(p) => Some(&p.id),
        Node::Unknown(u) => u.id.as_deref(),
    }
}

// ── Node-kind string ──────────────────────────────────────────────────────────

/// Return a static string naming the variant kind of a [`Node`].
pub(super) fn node_kind_str(node: &Node) -> &'static str {
    match node {
        Node::Rect(_) => "rect",
        Node::Ellipse(_) => "ellipse",
        Node::Line(_) => "line",
        Node::Text(_) => "text",
        Node::Code(_) => "code",
        Node::Frame(_) => "frame",
        Node::Group(_) => "group",
        Node::Image(_) => "image",
        Node::Polygon(_) => "polygon",
        Node::Polyline(_) => "polyline",
        Node::Instance(_) => "instance",
        Node::Field(_) => "field",
        Node::Toc(_) => "toc",
        Node::Footnote(_) => "footnote",
        Node::Table(_) => "table",
        Node::Shape(_) => "shape",
        Node::Connector(_) => "connector",
        Node::Pattern(_) => "pattern",
        Node::Unknown(_) => "unknown",
    }
}

/// Construct a [`Dimension`] with the `(px)` unit from a raw `f64` value.
pub(super) fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Append `id` to `affected` only if it is not already present.
/// Uses a linear scan to maintain deterministic first-seen insertion order
/// without HashMap (which has non-deterministic iteration).
pub(super) fn record_affected(id: &str, affected: &mut Vec<String>) {
    if !affected.iter().any(|s| s == id) {
        affected.push(id.to_owned());
    }
}
