//! Transaction engine: [`run_transaction`] and all per-op application logic.
//!
//! This module is pure: it performs no file I/O and does not mutate the input
//! document (it works on a clone). Dry-run vs. apply is the caller's concern.

use zenith_core::{
    Diagnostic, Dimension, Document, KdlAdapter, KdlSource, Node, Point, PropertyValue, Severity,
    Unit, validate,
};

use crate::op::{Op, OpPoint, Transaction};
use crate::result::{TxError, TxResult, TxStatus};

// ── Valid align values ────────────────────────────────────────────────────────

const VALID_ALIGNS: &[&str] = &["start", "center", "end", "justify"];

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
        Node::Polygon(n) => Some(&mut n.fill),
        Node::Polyline(n) => Some(&mut n.fill),
        Node::Line(_) | Node::Frame(_) | Node::Group(_) | Node::Image(_) | Node::Unknown(_) => None,
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

/// Construct a [`Dimension`] with the `(px)` unit from a raw `f64` value.
fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

// ── SetFill ───────────────────────────────────────────────────────────────────

fn apply_set_fill(
    node_id: &str,
    fill_token: &str,
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
            match node_fill_mut(node) {
                Some(slot) => {
                    *slot = Some(PropertyValue::TokenRef(fill_token.to_owned()));
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("set_fill is not supported on a {} node", kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
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
        Node::Frame(f) => Some(&f.id),
        Node::Group(g) => Some(&g.id),
        Node::Image(i) => Some(&i.id),
        Node::Polygon(p) => Some(&p.id),
        Node::Polyline(p) => Some(&p.id),
        Node::Unknown(_) => None,
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
            {"op":"set_locked","node":"r","locked":true}
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
                ],
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
}
