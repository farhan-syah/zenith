//! External-file text-source resolution for `text` nodes with `src="path"`.
//!
//! This module is part of the CLI render layer (not `zenith-scene`) because
//! file I/O must not happen inside the pure scene compile. The single public
//! entry point [`resolve_text_sources`] is called in every render entry point
//! after `parse_validate` and before `compile_page`.

use std::path::Path;

use zenith_core::{Diagnostic, Document, Node, TextNode, TextSpan};

/// Walk every page's node tree and, for each `text` node with `src = Some(path)`,
/// read the file (resolved relative to `project_dir`) and replace the node's
/// `spans` with a single plain span carrying the file's raw UTF-8 text.
///
/// When `project_dir` is `None`, all `src`-bearing text nodes receive a
/// `text.src_missing` Error diagnostic and their spans are left unchanged
/// (there is no directory to resolve the path against).
///
/// A successful read produces a single [`TextSpan`] with all style fields set to
/// `None` — the node's own fill/font apply at compile time, and the
/// `markdown_resolve` pass will re-parse the content if `format="markdown"`.
///
/// On read failure (file missing, unreadable, or not valid UTF-8), a
/// `text.src_missing` Error diagnostic is pushed naming the node id and the
/// resolved path; the node's existing spans are left unchanged so the document
/// degrades gracefully in advisory contexts. The Error severity ensures the
/// same render gate used for `asset.missing` blocks output.
///
/// Determinism: given the same file contents the output is always the same
/// single span — no timestamps or randomness are introduced.
pub(crate) fn resolve_text_sources(
    doc: &mut Document,
    project_dir: Option<&Path>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for page in &mut doc.body.pages {
        resolve_in_nodes(&mut page.children, project_dir, diagnostics);
    }
}

/// Recursively walk `nodes`, resolving `src` on every `Node::Text`.
///
/// Every node variant is listed explicitly (exhaustive match) so that a future
/// container kind forces a deliberate decision here and never silently skips
/// text nodes it might contain.
fn resolve_in_nodes(
    nodes: &mut [Node],
    project_dir: Option<&Path>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in nodes.iter_mut() {
        match node {
            Node::Text(text) => {
                // Clone the path so the immutable borrow of `text.src` ends before
                // `resolve_one` takes `text` mutably.
                if let Some(rel_path) = text.src.clone() {
                    resolve_one(text, &rel_path, project_dir, diagnostics);
                }
            }
            Node::Frame(f) => {
                resolve_in_nodes(&mut f.children, project_dir, diagnostics);
            }
            Node::Group(g) => {
                resolve_in_nodes(&mut g.children, project_dir, diagnostics);
            }
            Node::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        resolve_in_nodes(&mut cell.children, project_dir, diagnostics);
                    }
                }
            }
            // Leaf nodes that cannot contain children — explicit for exhaustiveness:
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Toc(_)
            | Node::Footnote(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Mesh(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Attempt to load the file at `rel_path` (relative to `project_dir`) and
/// replace `text.spans` with a single plain span carrying the file contents.
///
/// Pushes a `text.src_missing` Error diagnostic and leaves `text.spans`
/// unchanged when:
/// - `project_dir` is `None`.
/// - The resolved path cannot be read as UTF-8.
fn resolve_one(
    text: &mut TextNode,
    rel_path: &str,
    project_dir: Option<&Path>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let dir = match project_dir {
        Some(d) => d,
        None => {
            diagnostics.push(Diagnostic::error(
                "text.src_missing",
                format!(
                    "text node '{}': src=\"{}\" cannot be resolved — no project directory available",
                    text.id, rel_path
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
            return;
        }
    };

    let full_path = dir.join(rel_path);
    match std::fs::read_to_string(&full_path) {
        Ok(contents) => {
            text.spans = vec![TextSpan {
                text: contents,
                fill: None,
                font_weight: None,
                italic: None,
                underline: None,
                strikethrough: None,
                vertical_align: None,
                footnote_ref: None,
                data_ref: None,
                data_format: None,
                highlight: None,
                code: None,
                link: None,
            }];
        }
        Err(e) => {
            diagnostics.push(Diagnostic::error(
                "text.src_missing",
                format!(
                    "text node '{}': src=\"{}\" could not be read from '{}': {}",
                    text.id,
                    rel_path,
                    full_path.display(),
                    e
                ),
                text.source_span,
                Some(text.id.clone()),
            ));
        }
    }
}
