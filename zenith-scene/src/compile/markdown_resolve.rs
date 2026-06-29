//! Markdown-resolution pass: for each `text` node whose `content_format` is
//! `"markdown"`, concatenate the current span texts (AFTER data-binding
//! substitution) into one string and replace `node.spans` with the styled spans
//! produced by [`zenith_core::parse_inline_markdown`].
//!
//! This pass runs right after [`super::data_resolve::substitute_data_refs`] in
//! the compile entry ([`super::compile_page`]) so that data-bound spans (whose
//! text was substituted from the `DataContext` in the previous pass) are also
//! parsed as markdown when `format="markdown"` is set.
//!
//! ## Byte-identical guarantee
//!
//! A `text` node WITHOUT `content_format` (or with `content_format = Some("plain")`)
//! is skipped entirely. Its spans are not touched, so all existing documents
//! (and any authored without `format=`) produce exactly the same output as before.
//!
//! ## Determinism
//!
//! Pure transform: iterates pages and nodes in document order, no HashMap, no
//! time, no randomness. Same input → same output.

use std::collections::BTreeMap;

use zenith_core::{Document, MdBlock, Node, TextNode, parse_block_markdown, parse_inline_markdown};

/// Side-channel map from `text` node id to its parsed block-level markdown.
///
/// Populated by [`resolve_markdown`] for every `text` node with
/// `content_format == Some("markdown")`. Keyed by node id and ordered
/// ([`BTreeMap`]) for determinism. The block-layout path in the text compiler
/// (`compile_markdown_blocks`) reads this map; a node absent from it falls
/// through to the historical inline path (byte-identical).
pub(super) type MdBlockMap = BTreeMap<String, Vec<MdBlock>>;

/// Whether `doc` contains any `text` node with `content_format == Some("markdown")`,
/// anywhere in the page tree. Used by the `data = None` compile path to decide
/// whether to clone the document for the markdown-resolution pass.
pub(super) fn scan_for_markdown_text(doc: &Document) -> bool {
    for page in &doc.body.pages {
        for node in &page.children {
            if node_has_markdown_text(node) {
                return true;
            }
        }
    }
    false
}

/// Recursively test whether a node (or any descendant) is a `text` node with
/// `content_format == Some("markdown")`. EXHAUSTIVE over the [`Node`] enum.
fn node_has_markdown_text(node: &Node) -> bool {
    match node {
        Node::Text(n) => n.content_format.as_deref() == Some("markdown"),
        Node::Frame(n) => n.children.iter().any(node_has_markdown_text),
        Node::Group(n) => n.children.iter().any(node_has_markdown_text),
        Node::Table(n) => n.rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| cell.children.iter().any(node_has_markdown_text))
        }),
        Node::Unknown(n) => n.children.iter().any(node_has_markdown_text),
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Code(_)
        | Node::Image(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Field(_)
        | Node::Footnote(_)
        | Node::Toc(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Instance(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_) => false,
    }
}

/// Apply the markdown-resolution pass to every page in `doc` in document order.
///
/// For each `text` node with `content_format == Some("markdown")`:
/// - the span texts are concatenated and re-parsed as INLINE markdown, and the
///   node's spans are replaced with the parsed result (kept so the chain path
///   and the single-paragraph fallback render unchanged);
/// - the SAME concatenated text is parsed as BLOCK-level markdown and the
///   resulting `Vec<MdBlock>` is stored in the returned [`MdBlockMap`], keyed by
///   node id. The block-layout path in the text compiler consumes this map.
///
/// All other nodes are left untouched. When no markdown node exists the returned
/// map is empty and the document is unmodified (byte-identity).
pub(super) fn resolve_markdown(doc: &mut Document) -> MdBlockMap {
    let mut blocks: MdBlockMap = MdBlockMap::new();
    for page in &mut doc.body.pages {
        for node in &mut page.children {
            resolve_node(node, &mut blocks);
        }
    }
    blocks
}

/// Recursively walk a node, applying markdown resolution to any `text` node
/// with `content_format == Some("markdown")`, and descending into containers.
///
/// EXHAUSTIVE over the [`Node`] enum so a new node kind forces a compile error
/// here (the coverage guarantee, mirroring `data_resolve`).
fn resolve_node(node: &mut Node, blocks: &mut MdBlockMap) {
    match node {
        Node::Text(n) => resolve_text(n, blocks),
        Node::Frame(n) => {
            for child in &mut n.children {
                resolve_node(child, blocks);
            }
        }
        Node::Group(n) => {
            for child in &mut n.children {
                resolve_node(child, blocks);
            }
        }
        Node::Table(n) => {
            for row in &mut n.rows {
                for cell in &mut row.cells {
                    for child in &mut cell.children {
                        resolve_node(child, blocks);
                    }
                }
            }
        }
        Node::Unknown(n) => {
            for child in &mut n.children {
                resolve_node(child, blocks);
            }
        }
        // Leaf nodes that are not `text` carry no spans to markdown-parse.
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Code(_)
        | Node::Image(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Field(_)
        | Node::Footnote(_)
        | Node::Toc(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Instance(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_) => {}
    }
}

/// Resolve markdown on a single `TextNode` when opted in.
///
/// When `content_format == Some("markdown")`, concatenates all current span
/// texts (left-to-right in source order), parses the result as inline
/// markdown, and replaces `node.spans` with the parsed styled spans.
///
/// When `content_format` is `None`, `Some("plain")`, or any other value,
/// the node's spans are left untouched (byte-identical).
fn resolve_text(node: &mut TextNode, blocks: &mut MdBlockMap) {
    if node.content_format.as_deref() != Some("markdown") {
        return;
    }
    // Concatenate the current span texts in source order. After the
    // data-binding pre-pass, each span's text has already been resolved
    // from any `data-ref` binding. `span.text` is the authoritative content.
    let content: String = node.spans.iter().map(|s| s.text.as_str()).collect();
    // Parse the concatenated string as BLOCK-level markdown and record it in the
    // side-channel keyed by node id (consumed by the block-layout path). The
    // node id may be empty; the block-layout activation is gated on a non-empty
    // match, so an id-less node simply falls through to the inline path below.
    blocks.insert(node.id.clone(), parse_block_markdown(&content));
    // Parse the concatenated string as inline markdown and replace spans. This
    // is preserved so chained markdown nodes and the single-paragraph fallback
    // render byte-identically to before.
    node.spans = parse_inline_markdown(&content);
}
