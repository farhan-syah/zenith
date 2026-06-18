//! Transaction engine: [`run_transaction`] and all per-op application logic.
//!
//! This module is pure: it performs no file I/O and does not mutate the input
//! document (it works on a clone). Dry-run vs. apply is the caller's concern.

use zenith_core::{Diagnostic, Document, KdlAdapter, KdlSource, Node, Severity, validate};

use crate::op::{Op, Transaction};
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
            apply_move_forward(node_id, doc, diagnostics, affected);
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
    match find_node_mut(doc, node_id) {
        FindResult::NotFound => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        FindResult::WrongType { kind } => {
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
        FindResult::TextNode(text_node) => {
            text_node.align = Some(align.to_owned());
            record_affected(node_id, affected);
        }
    }
}

// ── MoveForward ───────────────────────────────────────────────────────────────

fn apply_move_forward(
    node_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    for page in doc.body.pages.iter_mut() {
        match move_forward_in(&mut page.children, node_id) {
            MoveOutcome::NotFound => {
                // Try the next page.
            }
            MoveOutcome::Moved => {
                record_affected(node_id, affected);
                return;
            }
            MoveOutcome::AlreadyFront => {
                // Already last (front) — no-op; emit an advisory.
                diagnostics.push(Diagnostic::advisory(
                    "tx.noop",
                    format!("node {:?} is already at the front of its parent", node_id),
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

/// Result of a node lookup for mutation.
enum FindResult<'a> {
    NotFound,
    WrongType { kind: &'static str },
    TextNode(&'a mut zenith_core::TextNode),
}

/// Returns true if `node` is, or transitively contains, a node with `id`.
fn subtree_contains(node: &Node, id: &str) -> bool {
    if node_id_of(node) == Some(id) {
        return true;
    }
    if let Node::Group(g) = node {
        return g.children.iter().any(|c| subtree_contains(c, id));
    }
    false
}

/// Walk the document tree and return a mutable reference to a `TextNode` with
/// the given id, or indicate not-found / wrong-type.
///
/// Two-phase approach: shared scan first (to find the page index), then a
/// single targeted mutable borrow. This pattern avoids the borrow-checker
/// conflict that would arise if we tried to return a mutable reference from
/// within an `&mut`-iterating for loop.
fn find_node_mut<'doc>(doc: &'doc mut Document, id: &str) -> FindResult<'doc> {
    // Phase 1: find which page (shared borrow only).
    // `subtree_contains` recurses into groups at any depth, so a node nested
    // inside one or more groups is correctly located on its containing page.
    let page_index = doc.body.pages.iter().enumerate().find_map(|(pi, page)| {
        let found = page.children.iter().any(|n| subtree_contains(n, id));
        if found { Some(pi) } else { None }
    });

    // Phase 2: act on the found page with an exclusive borrow.
    // `pi` came from iterating the same `doc.body.pages`; no intervening
    // mutation, so `.get_mut()` will always be `Some`. We use it instead of
    // the indexing operator so the engine can never panic.
    match page_index {
        None => FindResult::NotFound,
        Some(pi) => match doc.body.pages.get_mut(pi) {
            None => FindResult::NotFound,
            Some(page) => {
                find_in_children_mut(&mut page.children, id).unwrap_or(FindResult::NotFound)
            }
        },
    }
}

fn find_in_children_mut<'a>(children: &'a mut [Node], id: &str) -> Option<FindResult<'a>> {
    // Two-phase: first find the index (shared borrow), then mutate (exclusive
    // borrow). This avoids a simultaneous shared + mutable borrow of `children`.

    // Phase 1: find the index and record what kind of node it is.
    // `Descend(i)` means the id is nested inside the group at index `i`.
    enum Hit {
        Text(usize),
        WrongType(&'static str),
        Descend(usize),
    }

    // The scan checks direct children first. If the id matches a direct child,
    // we record the node kind. If the id is nested inside a group child, we
    // record `Descend` so phase 2 can recurse into that group's children vec.
    let hit = children
        .iter()
        .enumerate()
        .find_map(|(i, node)| match node {
            Node::Text(t) if t.id == id => Some(Hit::Text(i)),
            Node::Rect(r) if r.id == id => Some(Hit::WrongType("rect")),
            Node::Ellipse(e) if e.id == id => Some(Hit::WrongType("ellipse")),
            Node::Line(l) if l.id == id => Some(Hit::WrongType("line")),
            Node::Group(g) if g.id == id => Some(Hit::WrongType("group")),
            Node::Group(g) if g.children.iter().any(|c| subtree_contains(c, id)) => {
                Some(Hit::Descend(i))
            }
            // All other variants without a matching id, and Unknown: skip.
            _ => None,
        });

    // Phase 2: act on the hit (if any).
    match hit {
        None => None,
        Some(Hit::WrongType(kind)) => Some(FindResult::WrongType { kind }),
        Some(Hit::Text(i)) => {
            // SAFETY: `i` came from the same `children` slice above; it is
            // within bounds. We replace the shared borrow with an exclusive one.
            match children.get_mut(i) {
                Some(Node::Text(t)) => Some(FindResult::TextNode(t)),
                // Unreachable: we just confirmed it's Text in phase 1.
                _ => None,
            }
        }
        Some(Hit::Descend(i)) => match children.get_mut(i) {
            Some(Node::Group(g)) => find_in_children_mut(&mut g.children, id),
            _ => None, // unreachable: phase-1 confirmed a group at i
        },
    }
}

/// Outcome of attempting to move a node one step forward (up in z-order)
/// within its parent's children list, searching recursively through groups.
enum MoveOutcome {
    NotFound,
    AlreadyFront,
    Moved,
}

/// Move the node with `id` one step later in whatever children vec directly
/// contains it (page children or a group's children). Recurses into groups.
fn move_forward_in(children: &mut [Node], id: &str) -> MoveOutcome {
    if let Some(i) = children.iter().position(|n| node_id_of(n) == Some(id)) {
        if i + 1 >= children.len() {
            return MoveOutcome::AlreadyFront;
        }
        children.swap(i, i + 1);
        return MoveOutcome::Moved;
    }
    for child in children.iter_mut() {
        if let Node::Group(g) = child {
            match move_forward_in(&mut g.children, id) {
                MoveOutcome::NotFound => {}
                other => return other,
            }
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
        Node::Group(g) => Some(&g.id),
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
}
