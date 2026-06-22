//! `AddNode` / `RemoveNode` application: build a node from a `.zen` fragment via
//! the canonical parser, locate the parent container, and insert/remove.

use zenith_core::{Diagnostic, Document, KdlAdapter, KdlSource, Node};

use crate::op::Position;

use super::super::{node_id_of, record_affected};
use super::finders::{find_container_children_mut, remove_node_by_id, resolve_position};

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

pub(in crate::engine) fn apply_add_node(
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

pub(in crate::engine) fn apply_remove_node(
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
