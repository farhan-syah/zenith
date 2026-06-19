//! Transaction engine: [`run_transaction`] and all per-op application logic.
//!
//! This module is pure: it performs no file I/O and does not mutate the input
//! document (it works on a clone). Dry-run vs. apply is the caller's concern.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, Document, GroupNode, KdlAdapter, KdlSource, Node, Point, PropertyValue,
    Severity, TextSpan, Unit, dim_to_px, validate,
};

use crate::op::{Op, OpPoint, OpSpan, Position, Transaction};
use crate::result::{TxError, TxResult, TxStatus};

// ── Valid align values ────────────────────────────────────────────────────────

const VALID_ALIGNS: &[&str] = &["start", "center", "end", "justify"];

/// Valid alignment directions for [`Op::AlignNodes`].
const VALID_ALIGN_DIRS: &[&str] = &["left", "hcenter", "right", "top", "vcenter", "bottom"];

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
        } => {
            apply_set_geometry(node_id, *x, *y, *w, *h, doc, diagnostics, affected);
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
    }
}

// ── SetTextAlign ──────────────────────────────────────────────────────────────

fn apply_set_text_align(
    node_id: &str,
    align: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate align value before touching the tree.
    if !VALID_ALIGNS.contains(&align) {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "invalid align value {:?}; must be one of: {}",
                align,
                VALID_ALIGNS.join(", ")
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    // Walk the tree looking for `node_id`.
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(Node::Text(text_node)) => {
            text_node.align = Some(align.to_owned());
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.wrong_node_type",
                format!(
                    "set_text_align requires a text node but {:?} is a {}",
                    node_id, kind
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}

// ── Reorder ops ───────────────────────────────────────────────────────────────

fn apply_reorder(
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

// ── Tree walk helpers ─────────────────────────────────────────────────────────

/// Returns true if `node` is, or transitively contains, a node with `id`.
fn subtree_contains(node: &Node, id: &str) -> bool {
    if node_id_of(node) == Some(id) {
        return true;
    }
    match node {
        Node::Frame(f) => f.children.iter().any(|c| subtree_contains(c, id)),
        Node::Group(g) => g.children.iter().any(|c| subtree_contains(c, id)),
        _ => false,
    }
}

/// Walk the document tree and return a mutable reference to the node with
/// the given `id`, or `None` if not found.
///
/// Two-phase approach: shared scan first (to find the page index), then a
/// single targeted mutable borrow. This pattern avoids the borrow-checker
/// conflict that would arise if we tried to return a mutable reference from
/// within an `&mut`-iterating for loop.
fn find_node_any_mut<'doc>(doc: &'doc mut Document, id: &str) -> Option<&'doc mut Node> {
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
            _ => None,
        }
    });

    // Phase 2: take the exclusive borrow we deferred.
    match hit {
        None => None,
        Some(Hit::Direct(i)) => children.get_mut(i),
        Some(Hit::Descend(i)) => match children.get_mut(i) {
            Some(Node::Frame(f)) => find_in_children_any_mut(&mut f.children, id),
            Some(Node::Group(g)) => find_in_children_any_mut(&mut g.children, id),
            _ => None, // unreachable: phase-1 confirmed a container at i
        },
    }
}

// ── Node-kind string ──────────────────────────────────────────────────────────

/// Return a static string naming the variant kind of a [`Node`].
fn node_kind_str(node: &Node) -> &'static str {
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
        Node::Unknown(_) => "unknown",
    }
}

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
        Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `fill` field of a node, or `None` for
/// node variants that do not have a `fill` property
/// (`Line`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_fill_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.fill),
        Node::Ellipse(n) => Some(&mut n.fill),
        Node::Text(n) => Some(&mut n.fill),
        Node::Code(n) => Some(&mut n.fill),
        Node::Polygon(n) => Some(&mut n.fill),
        Node::Polyline(n) => Some(&mut n.fill),
        Node::Line(_) | Node::Frame(_) | Node::Group(_) | Node::Image(_) | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `stroke` field of a node, or `None` for
/// variants that do not have a `stroke` property
/// (`Text`, `Code`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_stroke_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.stroke),
        Node::Line(n) => Some(&mut n.stroke),
        Node::Polygon(n) => Some(&mut n.stroke),
        Node::Polyline(n) => Some(&mut n.stroke),
        Node::Ellipse(n) => Some(&mut n.stroke),
        Node::Text(_)
        | Node::Code(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `stroke_width` field of a node, or `None`
/// for variants that do not have a `stroke_width` property
/// (`Text`, `Code`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_stroke_width_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.stroke_width),
        Node::Line(n) => Some(&mut n.stroke_width),
        Node::Polygon(n) => Some(&mut n.stroke_width),
        Node::Polyline(n) => Some(&mut n.stroke_width),
        Node::Ellipse(n) => Some(&mut n.stroke_width),
        Node::Text(_)
        | Node::Code(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Unknown(_) => None,
    }
}

/// Mutable references to a node's four bbox geometry slots `(x, y, w, h)`.
type GeometryMut<'a> = (
    &'a mut Option<Dimension>,
    &'a mut Option<Dimension>,
    &'a mut Option<Dimension>,
    &'a mut Option<Dimension>,
);

/// Return mutable references to the four bbox geometry fields `(x, y, w, h)`,
/// or `None` for node variants excluded from `set_geometry`.
///
/// `Line` is excluded because it uses `x1/y1/x2/y2` endpoints, not a bbox.
/// `Polygon` and `Polyline` are excluded because they carry no `x/y/w/h` fields.
/// `Text` and `Group` are excluded by spec even though their structs carry `x/y/w/h`
/// fields: those fields are advisory/layout hints, not a canonical bbox to set.
/// `Unknown` is excluded because its schema is opaque.
fn node_geometry_mut(node: &mut Node) -> Option<GeometryMut<'_>> {
    match node {
        Node::Rect(r) => Some((&mut r.x, &mut r.y, &mut r.w, &mut r.h)),
        Node::Ellipse(e) => Some((&mut e.x, &mut e.y, &mut e.w, &mut e.h)),
        Node::Frame(f) => Some((&mut f.x, &mut f.y, &mut f.w, &mut f.h)),
        Node::Image(i) => Some((&mut i.x, &mut i.y, &mut i.w, &mut i.h)),
        Node::Line(_)
        | Node::Text(_)
        | Node::Code(_)
        | Node::Group(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `points` field of a `polygon` or
/// `polyline` node, or `None` for all other variants.
fn node_points_mut(node: &mut Node) -> Option<&mut Vec<Point>> {
    match node {
        Node::Polygon(p) => Some(&mut p.points),
        Node::Polyline(p) => Some(&mut p.points),
        _ => None,
    }
}

/// Return a mutable reference to the `opacity` field of a node, or `None`
/// for `Node::Unknown` which carries no `opacity` field.
fn node_opacity_mut(node: &mut Node) -> Option<&mut Option<f64>> {
    match node {
        Node::Rect(n) => Some(&mut n.opacity),
        Node::Ellipse(n) => Some(&mut n.opacity),
        Node::Line(n) => Some(&mut n.opacity),
        Node::Text(n) => Some(&mut n.opacity),
        Node::Code(n) => Some(&mut n.opacity),
        Node::Frame(n) => Some(&mut n.opacity),
        Node::Group(n) => Some(&mut n.opacity),
        Node::Image(n) => Some(&mut n.opacity),
        Node::Polygon(n) => Some(&mut n.opacity),
        Node::Polyline(n) => Some(&mut n.opacity),
        Node::Unknown(_) => None,
    }
}

/// Construct a [`Dimension`] with the `(px)` unit from a raw `f64` value.
fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

// ── SetFill / SetStroke / SetStrokeWidth ──────────────────────────────────────

/// Shared driver for token-valued property setters (`set_fill`, `set_stroke`,
/// `set_stroke_width`). Finds the node, fetches the `Option<PropertyValue>`
/// slot via `accessor`, and sets it to `TokenRef(token)`. Emits `tx.unknown_node`
/// if the node is missing, or `tx.unsupported_property` (naming `op_label`) if the
/// node kind lacks the property.
fn apply_set_property_token(
    node_id: &str,
    token: &str,
    op_label: &str,
    accessor: fn(&mut Node) -> Option<&mut Option<PropertyValue>>,
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
            // node_kind_str returns &'static str, so there is no live borrow
            // of `node` after this let binding — the mutable borrow below is fine.
            let kind = node_kind_str(node);
            match accessor(node) {
                Some(slot) => {
                    *slot = Some(PropertyValue::TokenRef(token.to_owned()));
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

fn apply_set_fill(
    node_id: &str,
    fill_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        fill_token,
        "set_fill",
        node_fill_mut,
        doc,
        diagnostics,
        affected,
    );
}

fn apply_set_stroke(
    node_id: &str,
    stroke_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        stroke_token,
        "set_stroke",
        node_stroke_mut,
        doc,
        diagnostics,
        affected,
    );
}

fn apply_set_stroke_width(
    node_id: &str,
    stroke_width_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        stroke_width_token,
        "set_stroke_width",
        node_stroke_width_mut,
        doc,
        diagnostics,
        affected,
    );
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

fn apply_set_visible(
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

fn apply_set_locked(
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

// ── SetGeometry ───────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn apply_set_geometry(
    node_id: &str,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Early-out: if every field is None this is a no-op — emit advisory.
    if x.is_none() && y.is_none() && w.is_none() && h.is_none() {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            format!(
                "set_geometry on {:?} specified no fields; document is unchanged",
                node_id
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

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
            match node_geometry_mut(node) {
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!(
                            "set_geometry is not supported on a {} node (no x/y/w/h)",
                            kind
                        ),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
                Some((nx, ny, nw, nh)) => {
                    if let Some(v) = x {
                        *nx = Some(px(v));
                    }
                    if let Some(v) = y {
                        *ny = Some(px(v));
                    }
                    if let Some(v) = w {
                        *nw = Some(px(v));
                    }
                    if let Some(v) = h {
                        *nh = Some(px(v));
                    }
                    record_affected(node_id, affected);
                }
            }
        }
    }
}

// ── SetPoints ─────────────────────────────────────────────────────────────────

fn apply_set_points(
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

/// Remove the node with `id` from `children` or any nested `group`/`frame`
/// container within it, returning the removed node, or `None` if absent.
fn remove_node_by_id(children: &mut Vec<Node>, id: &str) -> Option<Node> {
    if let Some(i) = children.iter().position(|n| node_id_of(n) == Some(id)) {
        return Some(children.remove(i));
    }
    for child in children.iter_mut() {
        let nested = match child {
            Node::Frame(f) => remove_node_by_id(&mut f.children, id),
            Node::Group(g) => remove_node_by_id(&mut g.children, id),
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
    }
    None
}

fn apply_add_node(
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

fn apply_remove_node(
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
    matches!(node, Node::Frame(_) | Node::Group(_))
}

/// Set the `id` field on a leaf [`Node`] variant to `new_id`.
///
/// Mirrors [`node_id_of`] (the shared-borrow id reader). Only leaf variants
/// are covered; `Frame` and `Group` are deliberately excluded because
/// [`apply_duplicate_node`] rejects containers before calling this helper.
/// Returns `false` only if called on an `Unknown` node (which has no id field)
/// — that path is also unreachable from `apply_duplicate_node` because an
/// `Unknown` node cannot be found by `node_id_of`, so it can never be the
/// source.
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
        // Containers handled by the v0 guard in apply_duplicate_node; Unknown
        // nodes have no id field and are never reached here.
        Node::Frame(_) | Node::Group(_) | Node::Unknown(_) => false,
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
        let grandchildren = match child {
            Node::Frame(f) => &mut f.children,
            Node::Group(g) => &mut g.children,
            _ => continue,
        };
        if duplicate_in_children(grandchildren, id, new_id) {
            return true;
        }
    }

    false
}

fn apply_duplicate_node(
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

// ── SetOpacity ────────────────────────────────────────────────────────────────

fn apply_set_opacity(
    node_id: &str,
    opacity: f64,
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
            match node_opacity_mut(node) {
                Some(slot) => {
                    *slot = Some(opacity.clamp(0.0, 1.0));
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("set_opacity is not supported on a {} node", kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
    }
}

// ── ReplaceText ───────────────────────────────────────────────────────────────

fn apply_replace_text(
    node_id: &str,
    spans: &[OpSpan],
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
        Some(Node::Text(text_node)) => {
            text_node.spans = spans
                .iter()
                .map(|s| TextSpan {
                    text: s.text.clone(),
                    fill: s
                        .fill
                        .as_ref()
                        .map(|id| PropertyValue::TokenRef(id.clone())),
                    font_weight: s
                        .font_weight
                        .as_ref()
                        .map(|id| PropertyValue::TokenRef(id.clone())),
                    italic: s.italic,
                    underline: s.underline,
                    strikethrough: s.strikethrough,
                })
                .collect();
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.unsupported_property",
                format!("replace_text is not supported on a {} node", kind),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}

// ── Reorder internals ────────────────────────────────────────────────────────

/// Which z-order reorder to perform.
#[derive(Copy, Clone)]
enum ReorderKind {
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

/// Reorder the node with `id` within whatever children slice directly contains
/// it, according to `kind`. Recurses into `Group` and `Frame` containers.
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
            _ => {}
        }
    }
    MoveOutcome::NotFound
}

/// Extract the stable id string from any [`Node`] variant, if it has one.
fn node_id_of(node: &Node) -> Option<&str> {
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
        Node::Unknown(_) => None,
    }
}

// ── Group ─────────────────────────────────────────────────────────────────────

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

fn apply_group(
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
        style: None,
        children: group_children,
        source_span: None,
        unknown_props: BTreeMap::new(),
    });

    children.insert(insert_at, group_node);
    record_affected(group_id, affected);
    // Post-validation catches group_id collision (id.duplicate).
}

// ── Ungroup ───────────────────────────────────────────────────────────────────

fn apply_ungroup(
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

/// Shared-borrow tree walk: find a node with `id` anywhere in `children`.
fn find_node_shared<'a>(children: &'a [Node], id: &str) -> Option<&'a Node> {
    for node in children {
        if node_id_of(node) == Some(id) {
            return Some(node);
        }
        let grandchildren = match node {
            Node::Frame(f) => f.children.as_slice(),
            Node::Group(g) => g.children.as_slice(),
            _ => continue,
        };
        if let Some(found) = find_node_shared(grandchildren, id) {
            return Some(found);
        }
    }
    None
}

// ── Reparent ──────────────────────────────────────────────────────────────────

fn apply_reparent(
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

// ── AlignNodes ────────────────────────────────────────────────────────────────

/// Read the four bbox dimensions of a node as px values, if the node kind
/// supports geometry and all four fields are present and resolvable.
///
/// Returns `Some((x, y, w, h))` or `None` if the node is unsupported, any
/// field is absent, or any unit cannot be converted to px (e.g. `%`, `deg`).
fn read_geometry_px(node: &Node) -> Option<(f64, f64, f64, f64)> {
    let (x, y, w, h) = match node {
        Node::Rect(r) => (r.x.as_ref(), r.y.as_ref(), r.w.as_ref(), r.h.as_ref()),
        Node::Ellipse(e) => (e.x.as_ref(), e.y.as_ref(), e.w.as_ref(), e.h.as_ref()),
        Node::Frame(f) => (f.x.as_ref(), f.y.as_ref(), f.w.as_ref(), f.h.as_ref()),
        Node::Image(i) => (i.x.as_ref(), i.y.as_ref(), i.w.as_ref(), i.h.as_ref()),
        _ => return None,
    };
    let resolve = |d: Option<&Dimension>| -> Option<f64> {
        d.and_then(|dim| dim_to_px(dim.value, &dim.unit))
    };
    Some((resolve(x)?, resolve(y)?, resolve(w)?, resolve(h)?))
}

fn apply_align_nodes(
    node_ids: &[String],
    align: &str,
    anchor: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate align value.
    if !VALID_ALIGN_DIRS.contains(&align) {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!("align_nodes: unknown align {:?}", align),
            None,
            None,
        ));
        return;
    }

    // Validate anchor value.
    if anchor != "selection" && anchor != "page" {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!(
                "align_nodes: unknown anchor {:?}; must be \"selection\" or \"page\"",
                anchor
            ),
            None,
            None,
        ));
        return;
    }

    // ── Phase 1: shared scan — gather bbox and check existence ────────────────
    //
    // We need a shared borrow to read geometry before taking the exclusive borrow
    // to write back. Collect (id, x, y, w, h) for every alignable node.
    // Nodes that are not found or lack resolvable geometry are skipped with
    // advisories; the loop continues so the remaining nodes are still aligned.

    struct NodeBbox {
        id: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    let mut alignable: Vec<NodeBbox> = Vec::new();

    for node_id in node_ids {
        // Shared-borrow scan across all pages.
        let found: Option<Option<(f64, f64, f64, f64)>> = 'page_scan: {
            for page in doc.body.pages.iter() {
                if let Some(node) = find_node_shared(&page.children, node_id) {
                    break 'page_scan Some(read_geometry_px(node));
                }
            }
            None // not found in any page
        };

        match found {
            None => {
                // Node not found at all.
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("align_nodes: node {:?} not found in document", node_id),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(None) => {
                // Node found but geometry is unresolvable (wrong kind or missing/pct field).
                // Use Warning so the caller sees AcceptedWithWarnings and knows a
                // node was silently skipped.
                diagnostics.push(Diagnostic::warning(
                    "tx.unsupported_property",
                    format!(
                        "align_nodes: node {:?} has no resolvable x/y/w/h geometry; skipped",
                        node_id
                    ),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(Some((x, y, w, h))) => {
                alignable.push(NodeBbox {
                    id: node_id.clone(),
                    x,
                    y,
                    w,
                    h,
                });
            }
        }
    }

    // Need at least one alignable node to proceed.
    if alignable.is_empty() {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            "align_nodes: no alignable nodes with resolvable geometry; document is unchanged"
                .to_owned(),
            None,
            None,
        ));
        return;
    }

    // ── Compute the reference rectangle ───────────────────────────────────────

    let (ref_left, ref_right, ref_top, ref_bottom) = if anchor == "page" {
        // Find the page that contains the first alignable node.
        let first_id = &alignable[0].id;
        let page_opt = doc
            .body
            .pages
            .iter()
            .find(|page| page.children.iter().any(|n| subtree_contains(n, first_id)));
        match page_opt {
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.invalid_parent",
                    format!(
                        "align_nodes: could not locate page containing node {:?}",
                        first_id
                    ),
                    None,
                    Some(first_id.clone()),
                ));
                return;
            }
            Some(page) => {
                let pw = dim_to_px(page.width.value, &page.width.unit);
                let ph = dim_to_px(page.height.value, &page.height.unit);
                match (pw, ph) {
                    (Some(w), Some(h)) => (0.0_f64, w, 0.0_f64, h),
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            "tx.invalid_parent",
                            "align_nodes: page width/height cannot be resolved to px".to_owned(),
                            None,
                            None,
                        ));
                        return;
                    }
                }
            }
        }
    } else {
        // anchor == "selection": union bbox of all alignable nodes.
        let ref_left = alignable.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let ref_right = alignable
            .iter()
            .map(|n| n.x + n.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let ref_top = alignable.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let ref_bottom = alignable
            .iter()
            .map(|n| n.y + n.h)
            .fold(f64::NEG_INFINITY, f64::max);
        (ref_left, ref_right, ref_top, ref_bottom)
    };

    // ── Phase 2: exclusive borrow — write new x or y for each node ───────────
    //
    // Compute the new position per node from the captured bbox, then apply via
    // find_node_any_mut + node_geometry_mut, mirroring apply_set_geometry's
    // write path.

    for bbox in &alignable {
        let new_x = match align {
            "left" => Some(ref_left),
            "hcenter" => Some((ref_left + ref_right) / 2.0 - bbox.w / 2.0),
            "right" => Some(ref_right - bbox.w),
            _ => None,
        };
        let new_y = match align {
            "top" => Some(ref_top),
            "vcenter" => Some((ref_top + ref_bottom) / 2.0 - bbox.h / 2.0),
            "bottom" => Some(ref_bottom - bbox.h),
            _ => None,
        };

        // At least one of new_x/new_y is Some (align was validated above).
        match find_node_any_mut(doc, &bbox.id) {
            None => {
                // Should not happen: we found it in phase 1, but guard anyway.
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("align_nodes: node {:?} disappeared between phases", bbox.id),
                    None,
                    Some(bbox.id.clone()),
                ));
            }
            Some(node) => {
                // node_geometry_mut is guaranteed Some here: we filtered on
                // read_geometry_px which uses the same set of node kinds.
                if let Some((nx, ny, _, _)) = node_geometry_mut(node) {
                    if let Some(v) = new_x {
                        *nx = Some(px(v));
                    }
                    if let Some(v) = new_y {
                        *ny = Some(px(v));
                    }
                    record_affected(&bbox.id, affected);
                }
            }
        }
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Append `id` to `affected` only if it is not already present.
/// Uses a linear scan to maintain deterministic first-seen insertion order
/// without HashMap (which has non-deterministic iteration).
fn record_affected(id: &str, affected: &mut Vec<String>) {
    if !affected.iter().any(|s| s == id) {
        affected.push(id.to_owned());
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::Transaction;
    use zenith_core::{KdlAdapter, KdlSource};

    /// Minimal valid document with a `text` node (align `start`) and a `rect`.
    fn parse(src: &str) -> Document {
        KdlAdapter
            .parse(src.as_bytes())
            .expect("test doc must parse")
    }

    // ── Test documents ────────────────────────────────────────────────────────

    const TEXT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      text id="label" x=(px)10 y=(px)10 w=(px)200 h=(px)40 align="start" {
        span "Hello"
      }
    }
  }
}"##;

    const TWO_RECT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="b" x=(px)0 y=(px)0 w=(px)100 h=(px)100
    }
  }
}"##;

    const MIXED_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="box1" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      text id="lbl" x=(px)10 y=(px)10 w=(px)200 h=(px)40 {
        span "Hi"
      }
    }
  }
}"##;

    const ELLIPSE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      text id="lbl" x=(px)10 y=(px)10 w=(px)200 h=(px)40 {
        span "Hi"
      }
    }
  }
}"##;

    const IMAGE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  assets {
    asset id="asset.pic" kind="image" src="pic.png"
  }
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      image id="pic" asset="asset.pic" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      text id="lbl" x=(px)10 y=(px)10 w=(px)200 h=(px)40 {
        span "Hi"
      }
    }
  }
}"##;

    // ── 1. SetTextAlign: accepted, affected ids, source diff ──────────────────

    #[test]
    fn set_text_align_accepted() {
        let doc = parse(TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "label".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["label".to_owned()]);
        assert!(
            result.source_after.contains("center"),
            "source_after should contain align=\"center\""
        );
        assert!(
            !result.source_before.contains("center"),
            "source_before should not contain center"
        );
        assert_ne!(result.source_before, result.source_after);
    }

    // ── 2. from_json round-trip ───────────────────────────────────────────────

    #[test]
    fn from_json_round_trip() {
        let json = r#"{"ops":[{"op":"set_text_align","node":"label","align":"center"},{"op":"move_forward","node":"accent"}]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::SetTextAlign {
                        node: "label".to_owned(),
                        align: "center".to_owned(),
                    },
                    Op::MoveForward {
                        node: "accent".to_owned()
                    },
                ],
            }
        );
    }

    // ── 3. MoveForward: a moves after b ──────────────────────────────────────

    #[test]
    fn move_forward_reorders() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveForward {
                node: "a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);

        // In source_after, "b" should appear before "a" (a is now last).
        let pos_a = result
            .source_after
            .find("id=\"a\"")
            .expect("a in source_after");
        let pos_b = result
            .source_after
            .find("id=\"b\"")
            .expect("b in source_after");
        assert!(pos_b < pos_a, "b should appear before a in source_after");

        // source_before has a before b.
        let pb_a = result
            .source_before
            .find("id=\"a\"")
            .expect("a in source_before");
        let pb_b = result
            .source_before
            .find("id=\"b\"")
            .expect("b in source_before");
        assert!(pb_a < pb_b, "a should appear before b in source_before");
    }

    // ── 4. Unknown node id → Rejected ────────────────────────────────────────

    #[test]
    fn unknown_node_rejected() {
        let doc = parse(TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "does_not_exist".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node diagnostic"
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── 5. SetTextAlign on a rect → wrong_node_type, Rejected ────────────────

    #[test]
    fn set_text_align_wrong_node_type() {
        let doc = parse(MIXED_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "box1".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.wrong_node_type"),
            "expected tx.wrong_node_type diagnostic"
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── 5b. SetTextAlign on an ellipse → wrong_node_type, Rejected ───────────

    #[test]
    fn set_text_align_on_ellipse_wrong_node_type() {
        let doc = parse(ELLIPSE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "dot".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.wrong_node_type" && d.message.contains("ellipse")),
            "expected tx.wrong_node_type diagnostic naming the ellipse kind"
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── 5c. SetTextAlign on an image → wrong_node_type, Rejected ─────────────

    #[test]
    fn set_text_align_on_image_wrong_node_type() {
        let doc = parse(IMAGE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "pic".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.wrong_node_type" && d.message.contains("image")),
            "expected tx.wrong_node_type diagnostic naming the image kind"
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── SetTextAlign: recursion into group children ───────────────────────────

    const GROUP_TEXT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Nest"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" {
        text id="nested.label" x=(px)10 y=(px)10 w=(px)200 h=(px)40 align="start" {
          span "Hello"
        }
      }
    }
  }
}"##;

    #[test]
    fn tx_set_text_align_targets_nested_text() {
        // A text node nested inside a group should now be reachable via
        // recursive descent; the tx engine is no longer limited to top-level
        // page children.
        let doc = parse(GROUP_TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "nested.label".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["nested.label".to_owned()]);
        assert!(
            result.source_after.contains("center"),
            "source_after should contain align=\"center\""
        );
        assert!(!result.source_before.contains("center"));
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn tx_set_text_align_on_group_itself_wrong_type() {
        // Targeting the group's own id with SetTextAlign must yield
        // tx.wrong_node_type mentioning "group".
        let doc = parse(GROUP_TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "grp1".to_owned(),
                align: "center".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.wrong_node_type" && d.message.contains("group")),
            "expected tx.wrong_node_type diagnostic naming \"group\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── MoveForward: reorder among group siblings ─────────────────────────────

    const GROUP_TWO_RECT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" {
        rect id="a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
        rect id="b" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      }
    }
  }
}"##;

    #[test]
    fn tx_move_forward_reorders_nested_child() {
        // Two rects (a then b) nested inside a group. MoveForward on "a"
        // should reorder them so b appears before a in source_after.
        let doc = parse(GROUP_TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveForward {
                node: "a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);

        // In source_after, "b" should appear before "a".
        let pos_a = result
            .source_after
            .find("id=\"a\"")
            .expect("a in source_after");
        let pos_b = result
            .source_after
            .find("id=\"b\"")
            .expect("b in source_after");
        assert!(pos_b < pos_a, "b should appear before a in source_after");

        // source_before has a before b.
        let pb_a = result
            .source_before
            .find("id=\"a\"")
            .expect("a in source_before");
        let pb_b = result
            .source_before
            .find("id=\"b\"")
            .expect("b in source_before");
        assert!(pb_a < pb_b, "a should appear before b in source_before");
    }

    // ── SetFill / SetVisible / SetLocked test documents ───────────────────────

    /// Rect with fill token A; token B also declared so post-validate passes.
    const FILL_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ff0000"
    token id="color.b" type="color" value="#0000ff"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.a"
    }
  }
}"##;

    /// Line node (no fill field).
    const LINE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ff0000"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      line id="ln1" x1=(px)0 y1=(px)0 x2=(px)100 y2=(px)100 stroke=(token)"color.a"
    }
  }
}"##;

    /// Page with one code node (fill via a declared token).
    const CODE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ff0000"
    token id="color.b" type="color" value="#0000ff"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      code id="snip" x=(px)0 y=(px)0 w=(px)200 h=(px)100 fill=(token)"color.a" {
        content "fn main() {}"
      }
    }
  }
}"##;

    /// Rect inside a group.
    const NESTED_RECT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" {
        rect id="inner" x=(px)0 y=(px)0 w=(px)50 h=(px)50
      }
    }
  }
}"##;

    // ── SetFill tests ─────────────────────────────────────────────────────────

    #[test]
    fn set_fill_recolors_rect() {
        let doc = parse(FILL_DOC);
        let tx = Transaction {
            ops: vec![Op::SetFill {
                node: "r1".to_owned(),
                fill: "color.b".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["r1".to_owned()]);
        // TokenRef("color.b") serialises as fill=(token)"color.b"
        assert!(
            result.source_after.contains("(token)\"color.b\""),
            "source_after must reference color.b; got:\n{}",
            result.source_after
        );
        assert!(
            !result.source_after.contains("(token)\"color.a\""),
            "old token must not appear in source_after"
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn set_fill_unsupported_on_line() {
        let doc = parse(LINE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetFill {
                node: "ln1".to_owned(),
                fill: "color.a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("line")),
            "expected tx.unsupported_property mentioning \"line\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn set_fill_unknown_token_rejected() {
        // color.nope is not declared → post-validate emits token.unknown_reference → Rejected
        let doc = parse(FILL_DOC);
        let tx = Transaction {
            ops: vec![Op::SetFill {
                node: "r1".to_owned(),
                fill: "color.nope".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "token.unknown_reference"),
            "expected token.unknown_reference diagnostic; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── SetStroke / SetStrokeWidth tests ──────────────────────────────────────

    /// Rect, line, polygon carrying valid color + dimension tokens.
    const STROKE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.rule" type="color" value="#334155"
    token id="size.stroke" type="dimension" value=(px)2
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 stroke=(token)"color.rule" stroke-width=(token)"size.stroke"
      line id="ln1" x1=(px)0 y1=(px)0 x2=(px)100 y2=(px)100 stroke=(token)"color.rule"
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.rule"
      text id="lbl" x=(px)10 y=(px)10 w=(px)200 h=(px)40 {
        span "Hi"
      }
      polygon id="poly1" stroke=(token)"color.rule" stroke-width=(token)"size.stroke" {
        point x=(px)10 y=(px)10
        point x=(px)90 y=(px)10
        point x=(px)50 y=(px)90
      }
    }
  }
}"##;

    #[test]
    fn set_stroke_recolors_rect() {
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStroke {
                node: "r1".to_owned(),
                stroke: "color.rule".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["r1".to_owned()]);
        assert!(
            result.source_after.contains("stroke=(token)\"color.rule\""),
            "source_after must reference color.rule as stroke; got:\n{}",
            result.source_after
        );
    }

    #[test]
    fn set_stroke_unknown_token_rejected() {
        // color.nope is not declared → post-validate emits token.unknown_reference.
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStroke {
                node: "r1".to_owned(),
                stroke: "color.nope".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "token.unknown_reference"),
            "expected token.unknown_reference diagnostic; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn set_stroke_accepted_on_ellipse() {
        // Ellipse now supports stroke — set_stroke must be Accepted.
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStroke {
                node: "dot".to_owned(),
                stroke: "color.rule".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "set_stroke on an ellipse must be Accepted; got: {:?}",
            result.diagnostics
        );
        assert!(
            result.source_after.contains("stroke=(token)\"color.rule\""),
            "formatted source must contain the new stroke property; got:\n{}",
            result.source_after
        );
    }

    #[test]
    fn set_stroke_unknown_node_rejected() {
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStroke {
                node: "nope".to_owned(),
                stroke: "color.rule".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node diagnostic; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn set_stroke_width_on_polygon() {
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStrokeWidth {
                node: "poly1".to_owned(),
                stroke_width: "size.stroke".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["poly1".to_owned()]);
        assert!(
            result
                .source_after
                .contains("stroke-width=(token)\"size.stroke\""),
            "source_after must reference size.stroke as stroke-width; got:\n{}",
            result.source_after
        );
    }

    #[test]
    fn set_stroke_width_unsupported_on_text() {
        let doc = parse(STROKE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetStrokeWidth {
                node: "lbl".to_owned(),
                stroke_width: "size.stroke".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property"
                    && d.message
                        .contains("set_stroke_width is not supported on a text node")),
            "expected tx.unsupported_property naming text; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── SetVisible tests ──────────────────────────────────────────────────────

    #[test]
    fn set_visible_hides_node() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetVisible {
                node: "a".to_owned(),
                visible: false,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);
        assert!(
            result.source_after.contains("visible=#false"),
            "source_after must contain visible=#false; got:\n{}",
            result.source_after
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn set_visible_on_nested_node() {
        let doc = parse(NESTED_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetVisible {
                node: "inner".to_owned(),
                visible: false,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["inner".to_owned()]);
        assert!(
            result.source_after.contains("visible=#false"),
            "source_after must contain visible=#false for nested node; got:\n{}",
            result.source_after
        );
    }

    // ── SetLocked tests ───────────────────────────────────────────────────────

    #[test]
    fn set_locked_sets_lock() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetLocked {
                node: "b".to_owned(),
                locked: true,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["b".to_owned()]);
        assert!(
            result.source_after.contains("locked=#true"),
            "source_after must contain locked=#true; got:\n{}",
            result.source_after
        );
        assert_ne!(result.source_before, result.source_after);
    }

    // ── Unknown node targeting ────────────────────────────────────────────────

    // `UnknownNode` has no `id` field, so `node_id_of` returns `None` for it.
    // `subtree_contains` will never match an unknown node by id, and
    // `find_node_any_mut` returns `None` → tx.unknown_node.
    // We verify this by targeting a non-existent id that would match an unknown
    // node if it had an id; since it doesn't, we just get tx.unknown_node.
    #[test]
    fn set_visible_on_nonexistent_id_is_unknown_node() {
        // Using TEXT_DOC — there is no node with id "does_not_exist".
        // The important thing: we get tx.unknown_node, not a panic or
        // tx.unsupported_property.
        let doc = parse(TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetVisible {
                node: "does_not_exist".to_owned(),
                visible: false,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node; got: {:?}",
            result.diagnostics
        );
    }

    // ── from_json round-trip: new op variants ─────────────────────────────────

    #[test]
    fn from_json_new_ops_round_trip() {
        let json = r#"{"ops":[
            {"op":"set_fill","node":"r","fill":"c"},
            {"op":"set_visible","node":"r","visible":false},
            {"op":"set_locked","node":"r","locked":true},
            {"op":"set_stroke","node":"r","stroke":"s"},
            {"op":"set_stroke_width","node":"r","stroke_width":"sw"},
            {"op":"add_node","parent":"pg1","position":{"at":"after","id":"r"},"source":"rect id=\"r2\""},
            {"op":"remove_node","node":"r"}
        ]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::SetFill {
                        node: "r".to_owned(),
                        fill: "c".to_owned(),
                    },
                    Op::SetVisible {
                        node: "r".to_owned(),
                        visible: false,
                    },
                    Op::SetLocked {
                        node: "r".to_owned(),
                        locked: true,
                    },
                    Op::SetStroke {
                        node: "r".to_owned(),
                        stroke: "s".to_owned(),
                    },
                    Op::SetStrokeWidth {
                        node: "r".to_owned(),
                        stroke_width: "sw".to_owned(),
                    },
                    Op::AddNode {
                        parent: "pg1".to_owned(),
                        position: Position::After { id: "r".to_owned() },
                        source: "rect id=\"r2\"".to_owned(),
                    },
                    Op::RemoveNode {
                        node: "r".to_owned(),
                    },
                ],
            }
        );
    }

    #[test]
    fn from_json_add_node_position_defaults_to_last() {
        // `position` omitted → serde default → Position::Last.
        let json = r#"{"ops":[{"op":"add_node","parent":"pg1","source":"rect id=\"r2\""}]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![Op::AddNode {
                    parent: "pg1".to_owned(),
                    position: Position::Last,
                    source: "rect id=\"r2\"".to_owned(),
                }],
            }
        );
    }

    // ── SetGeometry / SetPoints test documents ────────────────────────────────

    /// Rect at origin, 100×100. No tokens needed for geometry ops.
    const RECT_GEOM_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="rect" x=(px)0 y=(px)0 w=(px)100 h=(px)100
    }
  }
}"##;

    /// Polygon with exactly 3 points and a fill token (to keep post-validate happy).
    const POLY_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ff0000"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      polygon id="poly" fill=(token)"color.fill" {
        point x=(px)0 y=(px)0
        point x=(px)100 y=(px)0
        point x=(px)50 y=(px)80
      }
    }
  }
}"##;

    // ── SetGeometry tests ─────────────────────────────────────────────────────

    #[test]
    fn set_geometry_moves_rect() {
        let doc = parse(RECT_GEOM_DOC);
        let tx = Transaction {
            ops: vec![Op::SetGeometry {
                node: "rect".to_owned(),
                x: Some(50.0),
                y: None,
                w: Some(200.0),
                h: None,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["rect".to_owned()]);

        // Changed fields appear in source_after.
        assert!(
            result.source_after.contains("x=(px)50"),
            "source_after must contain x=(px)50; got:\n{}",
            result.source_after
        );
        assert!(
            result.source_after.contains("w=(px)200"),
            "source_after must contain w=(px)200; got:\n{}",
            result.source_after
        );
        // Untouched fields stay at their original values.
        assert!(
            result.source_after.contains("y=(px)0"),
            "source_after must retain y=(px)0; got:\n{}",
            result.source_after
        );
        assert!(
            result.source_after.contains("h=(px)100"),
            "source_after must retain h=(px)100; got:\n{}",
            result.source_after
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn set_geometry_unsupported_on_line() {
        let doc = parse(LINE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetGeometry {
                node: "ln1".to_owned(),
                x: Some(10.0),
                y: None,
                w: None,
                h: None,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("line")),
            "expected tx.unsupported_property mentioning \"line\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn set_geometry_no_fields_is_noop() {
        let doc = parse(RECT_GEOM_DOC);
        let tx = Transaction {
            ops: vec![Op::SetGeometry {
                node: "rect".to_owned(),
                x: None,
                y: None,
                w: None,
                h: None,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        // All-None must produce Accepted (advisory is not an error/warning) with
        // no affected nodes and identical source.
        assert_eq!(result.status, TxStatus::Accepted);
        assert!(
            result.affected_node_ids.is_empty(),
            "affected must be empty for a noop; got: {:?}",
            result.affected_node_ids
        );
        assert!(
            result.diagnostics.iter().any(|d| d.code == "tx.noop"),
            "expected tx.noop advisory; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── Code node tx tests ────────────────────────────────────────────────────

    #[test]
    fn set_visible_on_code_accepted() {
        let doc = parse(CODE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetVisible {
                node: "snip".to_owned(),
                visible: false,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["snip".to_owned()]);
        assert!(
            result.source_after.contains("visible=#false"),
            "source_after must contain visible=#false; got:\n{}",
            result.source_after
        );
        // Content blob must survive the edit untouched.
        assert!(result.source_after.contains("content \"fn main() {}\""));
    }

    #[test]
    fn set_fill_on_code_accepted() {
        let doc = parse(CODE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetFill {
                node: "snip".to_owned(),
                fill: "color.b".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["snip".to_owned()]);
        assert!(
            result.source_after.contains("(token)\"color.b\""),
            "source_after must reference color.b; got:\n{}",
            result.source_after
        );
    }

    #[test]
    fn set_geometry_unsupported_on_code() {
        let doc = parse(CODE_DOC);
        let tx = Transaction {
            ops: vec![Op::SetGeometry {
                node: "snip".to_owned(),
                x: Some(10.0),
                y: None,
                w: None,
                h: None,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("code")),
            "expected tx.unsupported_property mentioning \"code\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn add_code_node_into_page_accepted() {
        let doc = parse(CODE_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "pg1".to_owned(),
                position: Position::Last,
                source:
                    r#"code id="snip2" x=(px)0 y=(px)0 w=(px)100 h=(px)40 { content "let x = 1;" }"#
                        .to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["snip2".to_owned()]);
        assert!(
            result.source_after.contains("id=\"snip2\""),
            "source_after must contain the new code node; got:\n{}",
            result.source_after
        );
        assert!(result.source_after.contains("content \"let x = 1;\""));
    }

    // ── SetPoints tests ───────────────────────────────────────────────────────

    #[test]
    fn set_points_replaces_polygon() {
        let doc = parse(POLY_DOC);
        // Replace the 3 original points with 3 different ones.
        let tx = Transaction {
            ops: vec![Op::SetPoints {
                node: "poly".to_owned(),
                points: vec![
                    crate::op::OpPoint { x: 10.0, y: 20.0 },
                    crate::op::OpPoint { x: 90.0, y: 20.0 },
                    crate::op::OpPoint { x: 50.0, y: 70.0 },
                ],
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["poly".to_owned()]);

        // New coordinates appear in source_after.
        assert!(
            result.source_after.contains("x=(px)10"),
            "source_after must contain x=(px)10; got:\n{}",
            result.source_after
        );
        assert!(
            result.source_after.contains("y=(px)20"),
            "source_after must contain y=(px)20; got:\n{}",
            result.source_after
        );
        // Old distinctive coordinate (x=50, y=80) from original must be gone.
        assert!(
            !result.source_after.contains("y=(px)80"),
            "old y=(px)80 must not appear in source_after"
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn set_points_too_few_rejected() {
        // Start from a valid 3-point polygon; replace with only 2 points →
        // post-validation rejects with shape.insufficient_points.
        let doc = parse(POLY_DOC);
        let tx = Transaction {
            ops: vec![Op::SetPoints {
                node: "poly".to_owned(),
                points: vec![
                    crate::op::OpPoint { x: 0.0, y: 0.0 },
                    crate::op::OpPoint { x: 100.0, y: 0.0 },
                ],
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "shape.insufficient_points"),
            "expected shape.insufficient_points diagnostic; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn set_points_unsupported_on_rect() {
        let doc = parse(RECT_GEOM_DOC);
        let tx = Transaction {
            ops: vec![Op::SetPoints {
                node: "rect".to_owned(),
                points: vec![
                    crate::op::OpPoint { x: 0.0, y: 0.0 },
                    crate::op::OpPoint { x: 100.0, y: 0.0 },
                    crate::op::OpPoint { x: 50.0, y: 80.0 },
                ],
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("rect")),
            "expected tx.unsupported_property mentioning \"rect\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── JSON round-trip: reshape ops ─────────────────────────────────────────

    #[test]
    fn from_json_reshape_ops_round_trip() {
        use crate::op::OpPoint;

        let json_geo = r#"{"ops":[{"op":"set_geometry","node":"r","x":10.0,"w":200.0}]}"#;
        let tx_geo = Transaction::from_json(json_geo).expect("parse set_geometry JSON");
        assert_eq!(
            tx_geo,
            Transaction {
                ops: vec![Op::SetGeometry {
                    node: "r".to_owned(),
                    x: Some(10.0),
                    y: None,
                    w: Some(200.0),
                    h: None,
                }],
            }
        );

        let json_pts = r#"{"ops":[{"op":"set_points","node":"p","points":[{"x":0.0,"y":0.0},{"x":1.0,"y":1.0}]}]}"#;
        let tx_pts = Transaction::from_json(json_pts).expect("parse set_points JSON");
        assert_eq!(
            tx_pts,
            Transaction {
                ops: vec![Op::SetPoints {
                    node: "p".to_owned(),
                    points: vec![OpPoint { x: 0.0, y: 0.0 }, OpPoint { x: 1.0, y: 1.0 },],
                }],
            }
        );
    }

    // ── 6. Invalid align value → tx.invalid_value, Rejected ──────────────────

    #[test]
    fn invalid_align_value_rejected() {
        let doc = parse(TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetTextAlign {
                node: "label".to_owned(),
                align: "middle".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_value"),
            "expected tx.invalid_value diagnostic"
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── MoveBackward / MoveToFront / MoveToBack test documents ───────────────

    /// Three rects a (index 0, bottom), b (index 1), c (index 2, top).
    const THREE_RECT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="b" x=(px)10 y=(px)0 w=(px)100 h=(px)100
      rect id="c" x=(px)20 y=(px)0 w=(px)100 h=(px)100
    }
  }
}"##;

    /// Group containing two rects: x (bottom) then y (top).
    const GROUP_TWO_RECT_BACKWARD_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" {
        rect id="x" x=(px)0 y=(px)0 w=(px)100 h=(px)100
        rect id="y" x=(px)10 y=(px)0 w=(px)100 h=(px)100
      }
    }
  }
}"##;

    // ── MoveBackward tests ────────────────────────────────────────────────────

    #[test]
    fn move_backward_reorders() {
        // Doc: a (bottom) then b (top). MoveBackward on b → b moves before a.
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveBackward {
                node: "b".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["b".to_owned()]);

        // In source_after, b should appear before a.
        let pos_a = result
            .source_after
            .find("id=\"a\"")
            .expect("a in source_after");
        let pos_b = result
            .source_after
            .find("id=\"b\"")
            .expect("b in source_after");
        assert!(pos_b < pos_a, "b should appear before a in source_after");
    }

    #[test]
    fn move_backward_already_at_back_noop() {
        // Doc: a (bottom) then b. MoveBackward on "a" → already at back → noop advisory.
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveBackward {
                node: "a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert!(
            result.affected_node_ids.is_empty(),
            "affected must be empty for noop; got: {:?}",
            result.affected_node_ids
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.noop" && d.message.contains("back")),
            "expected tx.noop advisory mentioning \"back\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn move_backward_nested_child() {
        // Group with x (bottom) then y (top). MoveBackward on y → recursion into
        // group, y swaps with x.
        let doc = parse(GROUP_TWO_RECT_BACKWARD_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveBackward {
                node: "y".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["y".to_owned()]);

        // In source_after, y should appear before x.
        let pos_x = result
            .source_after
            .find("id=\"x\"")
            .expect("x in source_after");
        let pos_y = result
            .source_after
            .find("id=\"y\"")
            .expect("y in source_after");
        assert!(pos_y < pos_x, "y should appear before x in source_after");
    }

    // ── MoveToFront tests ─────────────────────────────────────────────────────

    #[test]
    fn move_to_front_moves_to_top() {
        // THREE_RECT_DOC: a (0), b (1), c (2). MoveToFront on "a" → order becomes b, c, a.
        let doc = parse(THREE_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveToFront {
                node: "a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);

        // In source_after: b appears before c, c appears before a.
        let pos_a = result
            .source_after
            .find("id=\"a\"")
            .expect("a in source_after");
        let pos_b = result
            .source_after
            .find("id=\"b\"")
            .expect("b in source_after");
        let pos_c = result
            .source_after
            .find("id=\"c\"")
            .expect("c in source_after");
        assert!(pos_b < pos_c, "b should appear before c in source_after");
        assert!(pos_c < pos_a, "c should appear before a in source_after");
    }

    #[test]
    fn move_to_front_already_front_noop() {
        // THREE_RECT_DOC: c is already the last (topmost). MoveToFront on "c" → noop.
        let doc = parse(THREE_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveToFront {
                node: "c".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert!(
            result.affected_node_ids.is_empty(),
            "affected must be empty for noop; got: {:?}",
            result.affected_node_ids
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.noop" && d.message.contains("front")),
            "expected tx.noop advisory mentioning \"front\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── MoveToBack tests ──────────────────────────────────────────────────────

    #[test]
    fn move_to_back_moves_to_bottom() {
        // THREE_RECT_DOC: a (0), b (1), c (2). MoveToBack on "c" → order becomes c, a, b.
        let doc = parse(THREE_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::MoveToBack {
                node: "c".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["c".to_owned()]);

        // In source_after: c appears before a, a appears before b.
        let pos_a = result
            .source_after
            .find("id=\"a\"")
            .expect("a in source_after");
        let pos_b = result
            .source_after
            .find("id=\"b\"")
            .expect("b in source_after");
        let pos_c = result
            .source_after
            .find("id=\"c\"")
            .expect("c in source_after");
        assert!(pos_c < pos_a, "c should appear before a in source_after");
        assert!(pos_a < pos_b, "a should appear before b in source_after");
    }

    // ── from_json round-trip: reorder ops ─────────────────────────────────────

    #[test]
    fn from_json_reorder_ops_round_trip() {
        let json = r#"{"ops":[
            {"op":"move_backward","node":"x"},
            {"op":"move_to_front","node":"x"},
            {"op":"move_to_back","node":"x"}
        ]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::MoveBackward {
                        node: "x".to_owned()
                    },
                    Op::MoveToFront {
                        node: "x".to_owned()
                    },
                    Op::MoveToBack {
                        node: "x".to_owned()
                    },
                ],
            }
        );
    }

    // ── AddNode / RemoveNode test documents ───────────────────────────────────

    /// Page with one rect; an accent color token declared so added rects that
    /// reference it pass post-validation.
    const ADD_BASE_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.accent" type="color" value="#3b82f6"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)320 h=(px)200 {
      rect id="base" x=(px)0 y=(px)0 w=(px)100 h=(px)100
    }
  }
}"##;

    /// Page with a group that contains two rects.
    const ADD_GROUP_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)320 h=(px)200 {
      group id="grp1" {
        rect id="g.a" x=(px)0 y=(px)0 w=(px)50 h=(px)50
        rect id="g.b" x=(px)0 y=(px)0 w=(px)50 h=(px)50
      }
    }
  }
}"##;

    // ── AddNode tests ─────────────────────────────────────────────────────────

    #[test]
    fn add_node_into_page_last() {
        let doc = parse(ADD_BASE_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "pg1".to_owned(),
                position: Position::Last,
                source: r#"rect id="box" x=(px)10 y=(px)10 w=(px)100 h=(px)80 fill=(token)"color.accent""#
                    .to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["box".to_owned()]);
        assert!(
            result.source_after.contains("id=\"box\""),
            "source_after must contain the new rect; got:\n{}",
            result.source_after
        );
        // "box" inserted last → appears after "base".
        let pos_base = result.source_after.find("id=\"base\"").expect("base");
        let pos_box = result.source_after.find("id=\"box\"").expect("box");
        assert!(pos_base < pos_box, "box should come after base");
    }

    #[test]
    fn add_node_into_group_first() {
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "grp1".to_owned(),
                position: Position::First,
                source: r#"rect id="g.new" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["g.new".to_owned()]);
        // First child of the group → appears before g.a.
        let pos_new = result.source_after.find("id=\"g.new\"").expect("g.new");
        let pos_a = result.source_after.find("id=\"g.a\"").expect("g.a");
        assert!(pos_new < pos_a, "g.new should be first in the group");
    }

    #[test]
    fn add_node_before_and_after_sibling() {
        // Insert before g.b.
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "grp1".to_owned(),
                position: Position::Before {
                    id: "g.b".to_owned(),
                },
                source: r#"rect id="g.mid" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");
        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        let pos_a = result.source_after.find("id=\"g.a\"").expect("g.a");
        let pos_mid = result.source_after.find("id=\"g.mid\"").expect("g.mid");
        let pos_b = result.source_after.find("id=\"g.b\"").expect("g.b");
        assert!(
            pos_a < pos_mid && pos_mid < pos_b,
            "order should be a, mid, b"
        );

        // Insert after g.a.
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "grp1".to_owned(),
                position: Position::After {
                    id: "g.a".to_owned(),
                },
                source: r#"rect id="g.mid" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");
        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        let pos_a = result.source_after.find("id=\"g.a\"").expect("g.a");
        let pos_mid = result.source_after.find("id=\"g.mid\"").expect("g.mid");
        let pos_b = result.source_after.find("id=\"g.b\"").expect("g.b");
        assert!(
            pos_a < pos_mid && pos_mid < pos_b,
            "order should be a, mid, b"
        );
    }

    #[test]
    fn add_node_index_clamped() {
        // index well beyond len → clamped to last.
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "grp1".to_owned(),
                position: Position::Index { index: 99 },
                source: r#"rect id="g.tail" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");
        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        let pos_b = result.source_after.find("id=\"g.b\"").expect("g.b");
        let pos_tail = result.source_after.find("id=\"g.tail\"").expect("g.tail");
        assert!(pos_b < pos_tail, "clamped insert should be last");
    }

    #[test]
    fn add_node_duplicate_id_rejected() {
        let doc = parse(ADD_BASE_DOC);
        let before = run_transaction(
            &doc,
            &Transaction {
                ops: vec![Op::AddNode {
                    parent: "pg1".to_owned(),
                    position: Position::Last,
                    source: r#"rect id="base" x=(px)0 y=(px)0 w=(px)20 h=(px)20"#.to_owned(),
                }],
            },
        )
        .expect("run_transaction should not error");

        assert_eq!(before.status, TxStatus::Rejected);
        assert_eq!(before.source_after, before.source_before);
    }

    #[test]
    fn add_node_malformed_fragment_rejected() {
        let doc = parse(ADD_BASE_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "pg1".to_owned(),
                position: Position::Last,
                source: "not valid kdl {{{".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_node_spec"),
            "expected tx.invalid_node_spec; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn add_node_unknown_parent_rejected() {
        let doc = parse(ADD_BASE_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "nope".to_owned(),
                position: Position::Last,
                source: r#"rect id="box" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_parent"),
            "expected tx.invalid_parent; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn add_node_parent_is_leaf_rejected() {
        // "base" is a rect (a leaf) — not a valid container.
        let doc = parse(ADD_BASE_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "base".to_owned(),
                position: Position::Last,
                source: r#"rect id="box" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_parent"),
            "expected tx.invalid_parent; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn add_node_before_missing_sibling_rejected() {
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AddNode {
                parent: "grp1".to_owned(),
                position: Position::Before {
                    id: "nope".to_owned(),
                },
                source: r#"rect id="g.new" x=(px)0 y=(px)0 w=(px)10 h=(px)10"#.to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── RemoveNode tests ──────────────────────────────────────────────────────

    #[test]
    fn remove_node_top_level() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::RemoveNode {
                node: "a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);
        assert!(
            !result.source_after.contains("id=\"a\""),
            "node a must be gone from source_after; got:\n{}",
            result.source_after
        );
        assert!(result.source_after.contains("id=\"b\""), "b must remain");
    }

    #[test]
    fn remove_node_nested_in_group() {
        let doc = parse(ADD_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::RemoveNode {
                node: "g.a".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["g.a".to_owned()]);
        assert!(
            !result.source_after.contains("id=\"g.a\""),
            "nested node g.a must be gone; got:\n{}",
            result.source_after
        );
        assert!(
            result.source_after.contains("id=\"g.b\""),
            "g.b must remain"
        );
    }

    #[test]
    fn remove_node_unknown_rejected() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::RemoveNode {
                node: "nope".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── from_json round-trip: crud ops ────────────────────────────────────────

    #[test]
    fn from_json_crud_ops_round_trip() {
        let json = r#"{"ops":[
            {"op":"add_node","parent":"pg1","source":"rect id=\"r\""},
            {"op":"add_node","parent":"pg1","position":{"at":"index","index":2},"source":"rect id=\"r2\""},
            {"op":"remove_node","node":"r"}
        ]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::AddNode {
                        parent: "pg1".to_owned(),
                        position: Position::Last,
                        source: r#"rect id="r""#.to_owned(),
                    },
                    Op::AddNode {
                        parent: "pg1".to_owned(),
                        position: Position::Index { index: 2 },
                        source: r#"rect id="r2""#.to_owned(),
                    },
                    Op::RemoveNode {
                        node: "r".to_owned(),
                    },
                ],
            }
        );
    }

    // ── SetOpacity tests ──────────────────────────────────────────────────────

    #[test]
    fn set_opacity_on_rect() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetOpacity {
                node: "a".to_owned(),
                opacity: 0.5,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);
        assert!(
            result.source_after.contains("opacity=0.5"),
            "source_after must contain opacity=0.5; got:\n{}",
            result.source_after
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn set_opacity_clamped_above_one() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetOpacity {
                node: "a".to_owned(),
                opacity: 1.5,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        // 1.5 clamped to 1.0; formatter writes "1" (or "1.0") — just verify source
        // changed and the candidate has Some(1.0) by checking node in candidate.
        // We check the diagnostic list is clean (no errors) and affected is recorded.
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.severity != zenith_core::Severity::Error),
            "no errors expected; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);
    }

    #[test]
    fn set_opacity_clamped_below_zero() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetOpacity {
                node: "a".to_owned(),
                opacity: -0.5,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["a".to_owned()]);
        assert!(
            result.source_after.contains("opacity=0"),
            "clamped-to-0 opacity must appear in source_after; got:\n{}",
            result.source_after
        );
    }

    #[test]
    fn set_opacity_unknown_node_rejected() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::SetOpacity {
                node: "nope".to_owned(),
                opacity: 0.5,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── from_json round-trip: set_opacity + replace_text ─────────────────────

    #[test]
    fn from_json_set_opacity_replace_text_round_trip() {
        use crate::op::OpSpan;
        let json = r#"{"ops":[
            {"op":"set_opacity","node":"box","opacity":0.75},
            {"op":"replace_text","node":"lbl","spans":[
                {"text":"Hello","fill":"color.brand","italic":true},
                {"text":" world","underline":false}
            ]}
        ]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::SetOpacity {
                        node: "box".to_owned(),
                        opacity: 0.75,
                    },
                    Op::ReplaceText {
                        node: "lbl".to_owned(),
                        spans: vec![
                            OpSpan {
                                text: "Hello".to_owned(),
                                fill: Some("color.brand".to_owned()),
                                font_weight: None,
                                italic: Some(true),
                                underline: None,
                                strikethrough: None,
                            },
                            OpSpan {
                                text: " world".to_owned(),
                                fill: None,
                                font_weight: None,
                                italic: None,
                                underline: Some(false),
                                strikethrough: None,
                            },
                        ],
                    },
                ],
            }
        );
    }

    // ── ReplaceText tests ─────────────────────────────────────────────────────

    #[test]
    fn replace_text_updates_spans() {
        use crate::op::OpSpan;
        let doc = parse(TEXT_DOC);
        let tx = Transaction {
            ops: vec![Op::ReplaceText {
                node: "label".to_owned(),
                spans: vec![OpSpan {
                    text: "Goodbye".to_owned(),
                    fill: None,
                    font_weight: None,
                    italic: None,
                    underline: None,
                    strikethrough: None,
                }],
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Accepted);
        assert_eq!(result.affected_node_ids, vec!["label".to_owned()]);
        assert!(
            result.source_after.contains("Goodbye"),
            "source_after must contain new text; got:\n{}",
            result.source_after
        );
        assert!(
            !result.source_after.contains("Hello"),
            "old text must not appear in source_after"
        );
        assert_ne!(result.source_before, result.source_after);
    }

    #[test]
    fn replace_text_on_rect_unsupported() {
        use crate::op::OpSpan;
        let doc = parse(MIXED_DOC);
        let tx = Transaction {
            ops: vec![Op::ReplaceText {
                node: "box1".to_owned(),
                spans: vec![OpSpan {
                    text: "hi".to_owned(),
                    fill: None,
                    font_weight: None,
                    italic: None,
                    underline: None,
                    strikethrough: None,
                }],
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("rect")),
            "expected tx.unsupported_property naming rect; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    #[test]
    fn replace_text_span_with_fill_token() {
        use crate::op::OpSpan;
        // A doc that has both color tokens and a text node.
        const TEXT_WITH_TOKEN_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ff0000"
    token id="color.b" type="color" value="#0000ff"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      text id="lbl" x=(px)10 y=(px)10 w=(px)200 h=(px)40 {
        span "Original"
      }
    }
  }
}"##;
        let doc2 = parse(TEXT_WITH_TOKEN_DOC);
        let tx = Transaction {
            ops: vec![Op::ReplaceText {
                node: "lbl".to_owned(),
                spans: vec![OpSpan {
                    text: "Branded".to_owned(),
                    fill: Some("color.a".to_owned()),
                    font_weight: None,
                    italic: None,
                    underline: None,
                    strikethrough: None,
                }],
            }],
        };
        let result = run_transaction(&doc2, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "expected Accepted; diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["lbl".to_owned()]);
        // The formatter should emit the span's fill token ref in source_after.
        assert!(
            result.source_after.contains("Branded"),
            "new text must appear in source_after; got:\n{}",
            result.source_after
        );
    }

    // ── DuplicateNode tests ───────────────────────────────────────────────────

    /// Document with a single rect and a fill token (needed for post-validate).
    const DUP_RECT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#ff0000"
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="orig" x=(px)10 y=(px)20 w=(px)80 h=(px)60 fill=(token)"color.a"
    }
  }
}"##;

    /// Document with a group containing a rect (for container-rejection test).
    const DUP_GROUP_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp" {
        rect id="inner" x=(px)0 y=(px)0 w=(px)50 h=(px)50
      }
    }
  }
}"##;

    /// Duplicate a leaf rect: parent now has 2 rects, clone right after original,
    /// clone has new_id and same geometry/fill.
    #[test]
    fn duplicate_node_leaf_rect_accepted() {
        let doc = parse(DUP_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::DuplicateNode {
                node: "orig".to_owned(),
                new_id: "orig-copy".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "expected Accepted; diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.affected_node_ids, vec!["orig-copy".to_owned()]);

        // Both ids must appear in source_after.
        assert!(
            result.source_after.contains("id=\"orig\""),
            "original must still be present; got:\n{}",
            result.source_after
        );
        assert!(
            result.source_after.contains("id=\"orig-copy\""),
            "clone must be present; got:\n{}",
            result.source_after
        );

        // Clone must appear AFTER the original in source text.
        let pos_orig = result
            .source_after
            .find("id=\"orig\"")
            .expect("orig in source_after");
        let pos_copy = result
            .source_after
            .find("id=\"orig-copy\"")
            .expect("orig-copy in source_after");
        assert!(
            pos_orig < pos_copy,
            "clone should appear after original in source_after"
        );

        // Clone must carry the same geometry and fill as the original.
        // Count occurrences: both nodes should have x=(px)10, y=(px)20, etc.
        assert_eq!(
            result.source_after.matches("x=(px)10").count(),
            2,
            "both orig and clone should have x=(px)10; got:\n{}",
            result.source_after
        );
        assert_eq!(
            result.source_after.matches("w=(px)80").count(),
            2,
            "both orig and clone should have w=(px)80; got:\n{}",
            result.source_after
        );
        assert_eq!(
            result.source_after.matches("(token)\"color.a\"").count(),
            2,
            "both orig and clone should reference color.a; got:\n{}",
            result.source_after
        );

        // source_before has only one rect.
        assert_eq!(
            result.source_before.matches("id=\"orig").count(),
            1,
            "source_before should have only one orig* node"
        );
    }

    /// Duplicate with a new_id that already exists → post-validate rejects (id.duplicate).
    #[test]
    fn duplicate_node_colliding_new_id_rejected() {
        // TWO_RECT_DOC has rect "a" and rect "b"; duplicating "a" with new_id="b"
        // creates a second node with id "b" → id.duplicate from post-validate.
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::DuplicateNode {
                node: "a".to_owned(),
                new_id: "b".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(
            result.status,
            TxStatus::Rejected,
            "colliding new_id must be rejected; diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result.diagnostics.iter().any(|d| d.code == "id.duplicate"),
            "expected id.duplicate diagnostic; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    /// Attempting to duplicate a group → tx.unsupported_property (v0 scope).
    #[test]
    fn duplicate_node_container_group_rejected() {
        let doc = parse(DUP_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::DuplicateNode {
                node: "grp".to_owned(),
                new_id: "grp-copy".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| { d.code == "tx.unsupported_property" && d.message.contains("group") }),
            "expected tx.unsupported_property mentioning group; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    /// Attempting to duplicate an unknown node id → tx.unknown_node.
    #[test]
    fn duplicate_node_unknown_id_rejected() {
        let doc = parse(TWO_RECT_DOC);
        let tx = Transaction {
            ops: vec![Op::DuplicateNode {
                node: "does_not_exist".to_owned(),
                new_id: "copy".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unknown_node"),
            "expected tx.unknown_node; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    /// Extend the ops serde round-trip to include duplicate_node.
    #[test]
    fn from_json_duplicate_node_round_trip() {
        let json = r#"{"ops":[{"op":"duplicate_node","node":"orig","new_id":"orig-copy"}]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![Op::DuplicateNode {
                    node: "orig".to_owned(),
                    new_id: "orig-copy".to_owned(),
                }],
            }
        );
    }

    // ── Group / Ungroup / Reparent test documents ─────────────────────────────

    /// Two sibling rects on a page; used for group/reparent tests.
    const TWO_SIBLING_RECTS: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="r2" x=(px)0 y=(px)0 w=(px)100 h=(px)100
    }
  }
}"##;

    /// A page with a group that already exists (for ungroup / reparent tests).
    const PAGE_WITH_GROUP: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" {
        rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100
        rect id="r2" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      }
      rect id="r3" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}"##;

    /// A page with a group that has a non-zero x/y offset (advisory test).
    const PAGE_WITH_OFFSET_GROUP: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="grp1" x=(px)50 y=(px)20 {
        rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      }
    }
  }
}"##;

    /// A page with a group nested inside another group (cycle check + reparent).
    const NESTED_GROUPS: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      group id="outer" {
        group id="inner" {
          rect id="r1" x=(px)0 y=(px)0 w=(px)50 h=(px)50
        }
      }
    }
  }
}"##;

    // ── Group tests ───────────────────────────────────────────────────────────

    /// Group two sibling rects → parent now has one group containing both,
    /// inserted at the position of the first (r1's original index = 0).
    #[test]
    fn group_two_sibling_rects() {
        let doc = parse(TWO_SIBLING_RECTS);
        let tx = Transaction {
            ops: vec![Op::Group {
                node_ids: vec!["r1".to_owned(), "r2".to_owned()],
                group_id: "grp-new".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result.affected_node_ids.contains(&"grp-new".to_owned()),
            "grp-new must be in affected_node_ids"
        );
        // The page should now contain exactly one top-level node: the group.
        assert!(
            result.source_after.contains("id=\"grp-new\""),
            "source_after must contain the new group id"
        );
        // r1 and r2 are inside the group, not at the page level.
        // Both ids should still appear in the source (as group children).
        assert!(
            result.source_after.contains("id=\"r1\""),
            "r1 must appear inside the group"
        );
        assert!(
            result.source_after.contains("id=\"r2\""),
            "r2 must appear inside the group"
        );
        // r1 must appear before r2 in source_after (relative order preserved).
        let pos_r1 = result
            .source_after
            .find("id=\"r1\"")
            .expect("r1 in source_after");
        let pos_r2 = result
            .source_after
            .find("id=\"r2\"")
            .expect("r2 in source_after");
        assert!(pos_r1 < pos_r2, "r1 must precede r2 inside the group");
        // The group must appear before both rects in source_after (group wraps them).
        let pos_grp = result
            .source_after
            .find("id=\"grp-new\"")
            .expect("grp-new in source_after");
        assert!(pos_grp < pos_r1, "group node must open before its children");
    }

    /// Attempting to group nodes that do not share a parent → tx.invalid_parent.
    #[test]
    fn group_non_siblings_rejected() {
        let doc = parse(PAGE_WITH_GROUP);
        // r1 is inside grp1, r3 is a top-level sibling of grp1 → different parents.
        let tx = Transaction {
            ops: vec![Op::Group {
                node_ids: vec!["r1".to_owned(), "r3".to_owned()],
                group_id: "grp-bad".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_parent"),
            "expected tx.invalid_parent; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── Ungroup tests ─────────────────────────────────────────────────────────

    /// Ungroup a group → its children move up to the parent in order, group gone.
    #[test]
    fn ungroup_splices_children_in_place() {
        let doc = parse(PAGE_WITH_GROUP);
        let tx = Transaction {
            ops: vec![Op::Ungroup {
                group_id: "grp1".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        // AcceptedWithWarnings is fine; exact status depends on post-validate.
        assert_ne!(
            result.status,
            TxStatus::Rejected,
            "ungroup must not be rejected; diagnostics: {:?}",
            result.diagnostics
        );
        // The group id should no longer appear in source_after.
        assert!(
            !result.source_after.contains("id=\"grp1\""),
            "group grp1 must be gone from source_after;\n{}",
            result.source_after
        );
        // r1 and r2 must still be present (now at page level).
        assert!(
            result.source_after.contains("id=\"r1\""),
            "r1 must appear in source_after"
        );
        assert!(
            result.source_after.contains("id=\"r2\""),
            "r2 must appear in source_after"
        );
        // r1 must appear before r2 (order preserved).
        let pos_r1 = result
            .source_after
            .find("id=\"r1\"")
            .expect("r1 in source_after");
        let pos_r2 = result
            .source_after
            .find("id=\"r2\"")
            .expect("r2 in source_after");
        assert!(pos_r1 < pos_r2, "r1 must precede r2 after ungroup");
        // r3 must still be present.
        assert!(
            result.source_after.contains("id=\"r3\""),
            "r3 must remain in source_after"
        );
    }

    /// Ungrouping a node that is not a group → tx.unsupported_property.
    #[test]
    fn ungroup_non_group_rejected() {
        let doc = parse(PAGE_WITH_GROUP);
        let tx = Transaction {
            ops: vec![Op::Ungroup {
                group_id: "r1".to_owned(), // r1 is a rect, not a group
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property"),
            "expected tx.unsupported_property; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    /// Ungrouping a group with non-zero x/y emits an advisory but still applies.
    #[test]
    fn ungroup_with_offset_emits_advisory() {
        let doc = parse(PAGE_WITH_OFFSET_GROUP);
        let tx = Transaction {
            ops: vec![Op::Ungroup {
                group_id: "grp1".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        // Must not be rejected.
        assert_ne!(
            result.status,
            TxStatus::Rejected,
            "ungroup with offset must not be rejected; diagnostics: {:?}",
            result.diagnostics
        );
        // Advisory (tx.noop) must be present.
        assert!(
            result.diagnostics.iter().any(|d| d.code == "tx.noop"),
            "expected tx.noop advisory for offset group; got: {:?}",
            result.diagnostics
        );
        // Group must be gone; r1 must remain.
        assert!(
            !result.source_after.contains("id=\"grp1\""),
            "group must be dissolved"
        );
        assert!(
            result.source_after.contains("id=\"r1\""),
            "r1 must survive ungroup"
        );
    }

    // ── Reparent tests ────────────────────────────────────────────────────────

    /// Move a top-level rect into an existing group.
    #[test]
    fn reparent_rect_into_group() {
        let doc = parse(PAGE_WITH_GROUP);
        // r3 is a top-level rect; move it into grp1.
        let tx = Transaction {
            ops: vec![Op::Reparent {
                node: "r3".to_owned(),
                new_parent: "grp1".to_owned(),
                position: Position::Last,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result.affected_node_ids.contains(&"r3".to_owned()),
            "r3 must be in affected_node_ids"
        );
        // r3 must still be present somewhere in the output.
        assert!(
            result.source_after.contains("id=\"r3\""),
            "r3 must appear in source_after"
        );
        // grp1 must contain r3 (grp1 opens before r3 in the serialised form).
        let pos_grp = result
            .source_after
            .find("id=\"grp1\"")
            .expect("grp1 in source_after");
        let pos_r3 = result
            .source_after
            .find("id=\"r3\"")
            .expect("r3 in source_after");
        assert!(
            pos_grp < pos_r3,
            "r3 must appear after grp1 opens (inside it)"
        );
    }

    /// Reparent into a non-container (a rect) → tx.invalid_parent.
    #[test]
    fn reparent_into_non_container_rejected() {
        let doc = parse(PAGE_WITH_GROUP);
        let tx = Transaction {
            ops: vec![Op::Reparent {
                node: "r3".to_owned(),
                new_parent: "r1".to_owned(), // r1 is a rect, not a container
                position: Position::Last,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_parent"),
            "expected tx.invalid_parent; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    /// Reparent a group into its own child group → cycle → tx.invalid_parent.
    #[test]
    fn reparent_into_own_subtree_rejected() {
        let doc = parse(NESTED_GROUPS);
        // Try to move `outer` into `inner` (inner is a descendant of outer).
        let tx = Transaction {
            ops: vec![Op::Reparent {
                node: "outer".to_owned(),
                new_parent: "inner".to_owned(),
                position: Position::Last,
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.invalid_parent"),
            "expected tx.invalid_parent (cycle); got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── Serde round-trip: group / ungroup / reparent ──────────────────────────

    #[test]
    fn from_json_group_ungroup_reparent_round_trip() {
        let json = r#"{"ops":[
            {"op":"group","node_ids":["r1","r2"],"group_id":"grp-new"},
            {"op":"ungroup","group_id":"grp1"},
            {"op":"reparent","node":"r3","new_parent":"grp1","position":{"at":"first"}}
        ]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![
                    Op::Group {
                        node_ids: vec!["r1".to_owned(), "r2".to_owned()],
                        group_id: "grp-new".to_owned(),
                    },
                    Op::Ungroup {
                        group_id: "grp1".to_owned(),
                    },
                    Op::Reparent {
                        node: "r3".to_owned(),
                        new_parent: "grp1".to_owned(),
                        position: Position::First,
                    },
                ],
            }
        );
    }

    // ── AlignNodes tests ──────────────────────────────────────────────────────

    /// Three sibling rects at different x positions (10, 50, 90) on a 400×300
    /// page; all have the same width (80px).
    const THREE_RECTS_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)10 y=(px)20 w=(px)80 h=(px)50
      rect id="r2" x=(px)50 y=(px)60 w=(px)80 h=(px)50
      rect id="r3" x=(px)90 y=(px)100 w=(px)80 h=(px)50
    }
  }
}"##;

    /// Parse a node's x value from `source_after` by looking for
    /// `id="<id>"` and then the first `x=(px)<value>` on the same node line.
    /// This is intentionally naive — sufficient for deterministic test docs.
    fn extract_px_attr(source: &str, node_id: &str, attr: &str) -> Option<f64> {
        // Find the line containing this node id.
        source
            .lines()
            .find(|line| line.contains(&format!("id=\"{node_id}\"")))
            .and_then(|line| {
                let needle = format!("{attr}=(px)");
                let start = line.find(&needle)? + needle.len();
                let rest = &line[start..];
                let end = rest
                    .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                    .unwrap_or(rest.len());
                rest[..end].parse::<f64>().ok()
            })
    }

    // ── align "left" anchor "selection" → all get x = min(x) = 10 ───────────

    #[test]
    fn align_left_selection() {
        let doc = parse(THREE_RECTS_DOC);
        let tx = Transaction {
            ops: vec![Op::AlignNodes {
                node_ids: vec!["r1".to_owned(), "r2".to_owned(), "r3".to_owned()],
                align: "left".to_owned(),
                anchor: "selection".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "diagnostics: {:?}",
            result.diagnostics
        );
        // All three nodes must be affected.
        assert!(result.affected_node_ids.contains(&"r1".to_owned()));
        assert!(result.affected_node_ids.contains(&"r2".to_owned()));
        assert!(result.affected_node_ids.contains(&"r3".to_owned()));

        // All three must have x = 10 (the minimum original x).
        for id in ["r1", "r2", "r3"] {
            let x = extract_px_attr(&result.source_after, id, "x")
                .unwrap_or_else(|| panic!("could not extract x for {id}"));
            assert!((x - 10.0).abs() < 1e-9, "expected x=10 for {id}, got {x}");
        }
    }

    // ── align "right" anchor "selection" → all right edges equal max(x+w) ───

    #[test]
    fn align_right_selection() {
        let doc = parse(THREE_RECTS_DOC);
        // ref_right = max(x+w) = max(90, 130, 170) = 170
        // each node: x = 170 - 80 = 90
        let tx = Transaction {
            ops: vec![Op::AlignNodes {
                node_ids: vec!["r1".to_owned(), "r2".to_owned(), "r3".to_owned()],
                align: "right".to_owned(),
                anchor: "selection".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "diagnostics: {:?}",
            result.diagnostics
        );
        for id in ["r1", "r2", "r3"] {
            let x = extract_px_attr(&result.source_after, id, "x")
                .unwrap_or_else(|| panic!("could not extract x for {id}"));
            // ref_right=170, w=80 → x = 90
            assert!((x - 90.0).abs() < 1e-9, "expected x=90 for {id}, got {x}");
        }
    }

    // ── align "hcenter" anchor "page" → x = page_w/2 − w/2 ──────────────────

    #[test]
    fn align_hcenter_page() {
        let doc = parse(THREE_RECTS_DOC);
        // page_w=400, each rect w=80 → centered x = 400/2 − 80/2 = 200 − 40 = 160
        let tx = Transaction {
            ops: vec![Op::AlignNodes {
                node_ids: vec!["r1".to_owned(), "r2".to_owned(), "r3".to_owned()],
                align: "hcenter".to_owned(),
                anchor: "page".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(
            result.status,
            TxStatus::Accepted,
            "diagnostics: {:?}",
            result.diagnostics
        );
        for id in ["r1", "r2", "r3"] {
            let x = extract_px_attr(&result.source_after, id, "x")
                .unwrap_or_else(|| panic!("could not extract x for {id}"));
            assert!((x - 160.0).abs() < 1e-9, "expected x=160 for {id}, got {x}");
        }
    }

    // ── node without geometry (group) in the set → skipped, others aligned ───

    /// Doc with two rects and one group; the group has no resolvable bbox.
    const RECTS_AND_GROUP_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)20 y=(px)0 w=(px)60 h=(px)40
      rect id="r2" x=(px)80 y=(px)0 w=(px)60 h=(px)40
      group id="grp1" { }
    }
  }
}"##;

    #[test]
    fn align_skips_non_geometry_node() {
        let doc = parse(RECTS_AND_GROUP_DOC);
        let tx = Transaction {
            ops: vec![Op::AlignNodes {
                node_ids: vec!["r1".to_owned(), "grp1".to_owned(), "r2".to_owned()],
                align: "left".to_owned(),
                anchor: "selection".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        // grp1 skipped → advisory, but the tx is still accepted.
        assert_eq!(
            result.status,
            TxStatus::AcceptedWithWarnings,
            "diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("grp1")),
            "expected tx.unsupported_property advisory for grp1; got: {:?}",
            result.diagnostics
        );
        // r1 and r2 must still have been aligned (x=20, the minimum).
        for id in ["r1", "r2"] {
            let x = extract_px_attr(&result.source_after, id, "x")
                .unwrap_or_else(|| panic!("could not extract x for {id}"));
            assert!((x - 20.0).abs() < 1e-9, "expected x=20 for {id}, got {x}");
        }
        // grp1 must not appear in affected.
        assert!(
            !result.affected_node_ids.contains(&"grp1".to_owned()),
            "grp1 must not be in affected_node_ids"
        );
    }

    // ── unknown align value → tx.unsupported_property, rejected ──────────────

    #[test]
    fn align_nodes_unknown_align_rejected() {
        let doc = parse(THREE_RECTS_DOC);
        let tx = Transaction {
            ops: vec![Op::AlignNodes {
                node_ids: vec!["r1".to_owned()],
                align: "diagonal".to_owned(),
                anchor: "selection".to_owned(),
            }],
        };
        let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

        assert_eq!(result.status, TxStatus::Rejected);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "tx.unsupported_property" && d.message.contains("diagonal")),
            "expected tx.unsupported_property naming \"diagonal\"; got: {:?}",
            result.diagnostics
        );
        assert_eq!(result.source_after, result.source_before);
    }

    // ── serde round-trip: align_nodes with default anchor ────────────────────

    #[test]
    fn from_json_align_nodes_round_trip() {
        // anchor is omitted → should deserialize to "selection" via serde default.
        let json = r#"{"ops":[{"op":"align_nodes","node_ids":["r1","r2"],"align":"left"}]}"#;
        let tx = Transaction::from_json(json).expect("parse JSON");
        assert_eq!(
            tx,
            Transaction {
                ops: vec![Op::AlignNodes {
                    node_ids: vec!["r1".to_owned(), "r2".to_owned()],
                    align: "left".to_owned(),
                    anchor: "selection".to_owned(),
                }],
            }
        );
    }
}
