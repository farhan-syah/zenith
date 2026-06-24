//! Document-level inspect logic for `zenith inspect`.
//!
//! The public entry point [`run`] operates entirely on in-memory source text;
//! the caller is responsible for all filesystem I/O.
//!
//! The tree-building pass is decoupled from printing so it can be tested
//! directly: [`build_doc_tree`] / [`find_node_tree`] return [`PageEntry`] /
//! [`NodeEntry`] values that serialise to JSON and render to human-readable
//! format.

use zenith_core::{Dimension, FrameNode, GroupNode, KdlAdapter, KdlSource, Node, Page, Unit};

use crate::commands::serialize_pretty;
use crate::json_types::RecipeInspectJson;

use super::recipes;

// ── Error type ────────────────────────────────────────────────────────────────

/// Error produced by the inspect command.
#[derive(Debug)]
pub struct InspectCmdErr {
    /// Human-readable message.
    pub message: String,
    /// Recommended exit code.
    pub exit_code: u8,
}

impl InspectCmdErr {
    fn new(msg: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: msg.into(),
            exit_code,
        }
    }
}

// ── Tree representation ───────────────────────────────────────────────────────

/// The geometry summary emitted per node.  Missing fields are `None` when the
/// node kind does not carry that property (e.g. `polygon` has no bbox).
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeGeometry {
    /// Left edge (px) for bbox nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Top edge (px) for bbox nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Width (px) for bbox nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    /// Height (px) for bbox nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
    /// First endpoint x (px) for `line`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x1: Option<f64>,
    /// First endpoint y (px) for `line`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y1: Option<f64>,
    /// Second endpoint x (px) for `line`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x2: Option<f64>,
    /// Second endpoint y (px) for `line`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y2: Option<f64>,
    /// Point count for `polygon`/`polyline`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_count: Option<usize>,
}

/// A single node in the inspect tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeEntry {
    pub id: String,
    pub kind: String,
    pub geometry: Option<NodeGeometry>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub children: Vec<NodeEntry>,
}

/// A page in the inspect tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PageEntry {
    pub id: String,
    pub name: Option<String>,
    pub width: f64,
    pub height: f64,
    pub children: Vec<NodeEntry>,
}

/// The top-level JSON envelope for `inspect`.
#[derive(Debug, serde::Serialize)]
pub struct InspectOutput {
    pub schema: &'static str,
    pub pages: Vec<PageEntry>,
    /// Empty when the document has no `recipes` block.
    pub recipes: Vec<RecipeInspectJson>,
}

/// The subtree rooted at a single found node (used for `--node <ID>`).
#[derive(Debug, serde::Serialize)]
pub struct InspectNodeOutput {
    pub schema: &'static str,
    pub node: NodeEntry,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run `zenith inspect`.
///
/// - `src`      — raw `.zen` source text.
/// - `node_id`  — when `Some`, restrict output to the subtree rooted at that id.
/// - `json`     — emit JSON instead of the human-readable tree.
///
/// Returns a formatted string on success, or an [`InspectCmdErr`] on parse
/// error, not-found error, etc.
pub fn run(src: &str, node_id: Option<&str>, json: bool) -> Result<String, InspectCmdErr> {
    // Parse ─────────────────────────────────────────────────────────────────
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| InspectCmdErr::new(format!("error[parse.error]: {}", e.message), 2))?;

    if let Some(id) = node_id {
        // --node <ID>: find the subtree rooted at that node.
        let entry = find_node_tree(&doc.body.pages, id)
            .ok_or_else(|| InspectCmdErr::new(format!("error: node '{}' not found", id), 2))?;

        let out = if json {
            let output = InspectNodeOutput {
                schema: "zenith-inspect-v1",
                node: entry,
            };
            serialize_pretty(&output)
        } else {
            render_node_human(&entry, 0).trim_end().to_owned()
        };
        Ok(out)
    } else {
        // Whole document.
        let pages = build_doc_tree(&doc.body.pages);

        let out = if json {
            let recipe_entries = recipes::build_recipe_entries(&doc.recipes);
            let output = InspectOutput {
                schema: "zenith-inspect-v1",
                pages,
                recipes: recipe_entries,
            };
            serialize_pretty(&output)
        } else {
            let mut text = render_pages_human(&pages);
            let recipe_section = recipes::render_recipes_human(&doc.recipes);
            if !recipe_section.is_empty() {
                text.push('\n');
                text.push('\n');
                text.push_str(&recipe_section);
            }
            text
        };
        Ok(out)
    }
}

// ── Token-efficient summary (MCP) ───────────────────────────────────────────────

/// Build a token-minimal structured summary of a document's node tree.
///
/// This is the shape the MCP `zenith_inspect` tool returns: instead of the full
/// recursive tree with geometry on every node, it returns a *shallow* view.
///
/// - `node`   — when `Some`, summarise only the subtree rooted at that id.
/// - `depth`  — how many node levels below each page (or below `node`) to expand.
///   Deeper children collapse to a `childCount`. `0` shows only the top level.
/// - `detail` — when `true`, re-include `geometry`/`visible`/`locked` per node.
///
/// Returns a [`serde_json::Value`] ready to embed as the tool's structured
/// result; the caller decides inline-vs-offload by serialized size.
pub fn summary(
    src: &str,
    node: Option<&str>,
    depth: usize,
    detail: bool,
) -> Result<serde_json::Value, InspectCmdErr> {
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| InspectCmdErr::new(format!("error[parse.error]: {}", e.message), 2))?;

    if let Some(id) = node {
        let entry = find_node_tree(&doc.body.pages, id)
            .ok_or_else(|| InspectCmdErr::new(format!("error: node '{id}' not found"), 2))?;
        Ok(serde_json::json!({
            "schema": "zenith-inspect-summary-v1",
            "node": trim_node(&entry, depth, detail),
        }))
    } else {
        let pages = build_doc_tree(&doc.body.pages);
        let page_values: Vec<serde_json::Value> =
            pages.iter().map(|p| trim_page(p, depth, detail)).collect();
        Ok(serde_json::json!({
            "schema": "zenith-inspect-summary-v1",
            "pages": page_values,
            "recipe_count": doc.recipes.len(),
        }))
    }
}

/// Trim a [`PageEntry`] to the shallow summary shape.
fn trim_page(p: &PageEntry, depth: usize, detail: bool) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), p.id.clone().into());
    if let Some(name) = &p.name {
        obj.insert("name".into(), name.clone().into());
    }
    obj.insert("width".into(), p.width.into());
    obj.insert("height".into(), p.height.into());
    insert_children(&mut obj, &p.children, depth, detail);
    serde_json::Value::Object(obj)
}

/// Trim a [`NodeEntry`] to the shallow summary shape, recursing `depth` levels.
fn trim_node(n: &NodeEntry, depth: usize, detail: bool) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), n.id.clone().into());
    obj.insert("kind".into(), n.kind.clone().into());
    if detail {
        if let Some(g) = &n.geometry {
            obj.insert(
                "geometry".into(),
                serde_json::to_value(g).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(v) = n.visible {
            obj.insert("visible".into(), v.into());
        }
        if let Some(l) = n.locked {
            obj.insert("locked".into(), l.into());
        }
    }
    insert_children(&mut obj, &n.children, depth, detail);
    serde_json::Value::Object(obj)
}

/// Insert either an expanded `children` array (when `depth > 0`) or a collapsed
/// `child_count` (when `depth == 0`), omitting both when there are no children.
fn insert_children(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    children: &[NodeEntry],
    depth: usize,
    detail: bool,
) {
    if children.is_empty() {
        return;
    }
    if depth == 0 {
        obj.insert("child_count".into(), children.len().into());
    } else {
        let kids: Vec<serde_json::Value> = children
            .iter()
            .map(|c| trim_node(c, depth - 1, detail))
            .collect();
        obj.insert("children".into(), serde_json::Value::Array(kids));
    }
}

// ── Tree builders ─────────────────────────────────────────────────────────────

/// Build the full page tree for all pages in the document (in order).
pub fn build_doc_tree(pages: &[Page]) -> Vec<PageEntry> {
    pages.iter().map(build_page_entry).collect()
}

fn build_page_entry(page: &Page) -> PageEntry {
    PageEntry {
        id: page.id.clone(),
        name: page.name.clone(),
        width: dim_to_f64(&page.width),
        height: dim_to_f64(&page.height),
        children: page.children.iter().map(build_node_entry).collect(),
    }
}

fn build_node_entry(node: &Node) -> NodeEntry {
    match node {
        Node::Rect(n) => NodeEntry {
            id: n.id.clone(),
            kind: "rect".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Ellipse(n) => NodeEntry {
            id: n.id.clone(),
            kind: "ellipse".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Line(n) => NodeEntry {
            id: n.id.clone(),
            kind: "line".into(),
            geometry: Some(NodeGeometry {
                x: None,
                y: None,
                w: None,
                h: None,
                x1: n.x1.as_ref().map(dim_to_f64),
                y1: n.y1.as_ref().map(dim_to_f64),
                x2: n.x2.as_ref().map(dim_to_f64),
                y2: n.y2.as_ref().map(dim_to_f64),
                point_count: None,
            }),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Text(n) => NodeEntry {
            id: n.id.clone(),
            kind: "text".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Code(n) => NodeEntry {
            id: n.id.clone(),
            kind: "code".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Image(n) => NodeEntry {
            id: n.id.clone(),
            kind: "image".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Frame(n) => NodeEntry {
            id: n.id.clone(),
            kind: "frame".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: n.children.iter().map(build_node_entry).collect(),
        },
        Node::Group(n) => NodeEntry {
            id: n.id.clone(),
            kind: "group".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: n.children.iter().map(build_node_entry).collect(),
        },
        Node::Polygon(n) => NodeEntry {
            id: n.id.clone(),
            kind: "polygon".into(),
            geometry: Some(NodeGeometry {
                x: None,
                y: None,
                w: None,
                h: None,
                x1: None,
                y1: None,
                x2: None,
                y2: None,
                point_count: Some(n.points.len()),
            }),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Polyline(n) => NodeEntry {
            id: n.id.clone(),
            kind: "polyline".into(),
            geometry: Some(NodeGeometry {
                x: None,
                y: None,
                w: None,
                h: None,
                x1: None,
                y1: None,
                x2: None,
                y2: None,
                point_count: Some(n.points.len()),
            }),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Instance(n) => NodeEntry {
            id: n.id.clone(),
            kind: "instance".into(),
            // An instance carries only an x/y origin (no w/h box); report those
            // two via the bbox geometry slots with w/h left None.
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), None, None),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Field(n) => NodeEntry {
            id: n.id.clone(),
            kind: "field".into(),
            // A field carries an x/y/w/h box (any of which may be omitted, in
            // which case it defaults to the page live area at compile time).
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Toc(n) => NodeEntry {
            id: n.id.clone(),
            kind: "toc".into(),
            // A toc carries a real x/y/w/h box (it must declare its own
            // geometry for correct positioning).
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Footnote(n) => NodeEntry {
            id: n.id.clone(),
            kind: "footnote".into(),
            // A footnote has NO geometry (the renderer positions it in the
            // bottom zone); report no geometry, visible, or locked.
            geometry: None,
            visible: None,
            locked: None,
            children: vec![],
        },
        Node::Table(n) => NodeEntry {
            id: n.id.clone(),
            kind: "table".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            // Report each cell's child nodes (flattened in row→cell order) so a
            // table's content is visible in the inspect tree.
            children: n
                .rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .flat_map(|cell| cell.children.iter())
                .map(build_node_entry)
                .collect(),
        },
        Node::Shape(n) => NodeEntry {
            id: n.id.clone(),
            kind: "shape".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            // A shape owns label spans (TextSpans), not child Nodes, so it has
            // no child entries in the inspect tree.
            children: vec![],
        },
        Node::Connector(n) => NodeEntry {
            id: n.id.clone(),
            kind: "connector".into(),
            // A connector has no authored bbox — its endpoints are derived from
            // its targets' boxes at compile time.
            geometry: None,
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Pattern(n) => NodeEntry {
            id: n.id.clone(),
            kind: "pattern".into(),
            geometry: bbox_geom(n.x.as_ref(), n.y.as_ref(), n.w.as_ref(), n.h.as_ref()),
            visible: n.visible,
            locked: n.locked,
            children: vec![],
        },
        Node::Unknown(n) => NodeEntry {
            id: n.id.clone().unwrap_or_default(),
            kind: n.kind.clone(),
            geometry: None,
            visible: None,
            locked: None,
            children: n.children.iter().map(build_node_entry).collect(),
        },
    }
}

// ── Node finder ───────────────────────────────────────────────────────────────

/// Search all pages (depth-first, in source order) for a node with the given
/// id.  Returns a fully-built [`NodeEntry`] subtree when found.
pub fn find_node_tree(pages: &[Page], id: &str) -> Option<NodeEntry> {
    for page in pages {
        if let Some(entry) = search_nodes(&page.children, id) {
            return Some(entry);
        }
    }
    None
}

fn search_nodes(nodes: &[Node], id: &str) -> Option<NodeEntry> {
    for node in nodes {
        // Check if this node matches.
        let node_id = node_id_str(node);
        if node_id == id {
            return Some(build_node_entry(node));
        }
        // Recurse into Frame/Group/Unknown children via node_children.
        if let Some(children) = node_children(node)
            && let Some(found) = search_nodes(children, id)
        {
            return Some(found);
        }
        // Recurse into table cell children (node_children returns None for Table).
        if let Node::Table(t) = node {
            for row in &t.rows {
                for cell in &row.cells {
                    if let Some(found) = search_nodes(&cell.children, id) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Return the `id` field of a node as a `&str`.
fn node_id_str(node: &Node) -> &str {
    match node {
        Node::Rect(n) => &n.id,
        Node::Ellipse(n) => &n.id,
        Node::Line(n) => &n.id,
        Node::Text(n) => &n.id,
        Node::Code(n) => &n.id,
        Node::Frame(n) => &n.id,
        Node::Group(n) => &n.id,
        Node::Image(n) => &n.id,
        Node::Polygon(n) => &n.id,
        Node::Polyline(n) => &n.id,
        Node::Instance(n) => &n.id,
        Node::Field(n) => &n.id,
        Node::Toc(n) => &n.id,
        Node::Footnote(n) => &n.id,
        Node::Table(n) => &n.id,
        Node::Shape(n) => &n.id,
        Node::Connector(n) => &n.id,
        Node::Pattern(n) => &n.id,
        Node::Unknown(n) => n.id.as_deref().unwrap_or(""),
    }
}

/// Return a reference to a container node's children slice, or `None` for leaf
/// nodes.
fn node_children(node: &Node) -> Option<&[Node]> {
    match node {
        Node::Frame(FrameNode { children, .. }) | Node::Group(GroupNode { children, .. }) => {
            Some(children)
        }
        Node::Unknown(n) => Some(&n.children),
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
        | Node::Pattern(_) => None,
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

fn dim_to_f64(d: &Dimension) -> f64 {
    match d.unit {
        Unit::Pt => d.value * 96.0 / 72.0,
        Unit::Px | Unit::Pct | Unit::Deg | Unit::Unknown(_) => d.value,
    }
}

fn opt_dim_to_f64(d: Option<&Dimension>) -> Option<f64> {
    d.map(dim_to_f64)
}

fn bbox_geom(
    x: Option<&Dimension>,
    y: Option<&Dimension>,
    w: Option<&Dimension>,
    h: Option<&Dimension>,
) -> Option<NodeGeometry> {
    Some(NodeGeometry {
        x: opt_dim_to_f64(x),
        y: opt_dim_to_f64(y),
        w: opt_dim_to_f64(w),
        h: opt_dim_to_f64(h),
        x1: None,
        y1: None,
        x2: None,
        y2: None,
        point_count: None,
    })
}

// ── Human rendering ───────────────────────────────────────────────────────────

fn render_pages_human(pages: &[PageEntry]) -> String {
    let mut out = String::new();
    for page in pages {
        let name_part = page
            .name
            .as_deref()
            .map(|n| format!(" \"{}\"", n))
            .unwrap_or_default();
        out.push_str(&format!(
            "page {}{} ({}x{})\n",
            page.id, name_part, page.width, page.height
        ));
        for child in &page.children {
            out.push_str(&render_node_human(child, 1));
        }
    }
    out.trim_end().to_owned()
}

/// Render a single node (and its subtree) at the given indent depth.
/// Called by both the whole-document path and the `--node` subtree path.
fn render_node_human(node: &NodeEntry, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let geom = render_geom_summary(node);
    let flags = render_flags(node);
    let suffix = [geom, flags]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let suffix_part = if suffix.is_empty() {
        String::new()
    } else {
        format!("  {}", suffix)
    };

    let mut out = format!("{}{} {}{}\n", indent, node.kind, node.id, suffix_part);
    for child in &node.children {
        out.push_str(&render_node_human(child, depth + 1));
    }
    out
}

fn render_geom_summary(node: &NodeEntry) -> String {
    let Some(ref g) = node.geometry else {
        return String::new();
    };

    // bbox summary: x,y WxH
    if g.x.is_some() || g.y.is_some() || g.w.is_some() || g.h.is_some() {
        let x = g.x.unwrap_or(0.0);
        let y = g.y.unwrap_or(0.0);
        let w = g.w.unwrap_or(0.0);
        let h = g.h.unwrap_or(0.0);
        return format!(
            "{},{} {}x{}",
            fmt_f64(x),
            fmt_f64(y),
            fmt_f64(w),
            fmt_f64(h)
        );
    }

    // line endpoint summary
    if g.x1.is_some() || g.y1.is_some() || g.x2.is_some() || g.y2.is_some() {
        let x1 = g.x1.unwrap_or(0.0);
        let y1 = g.y1.unwrap_or(0.0);
        let x2 = g.x2.unwrap_or(0.0);
        let y2 = g.y2.unwrap_or(0.0);
        return format!(
            "({},{})→({},{})",
            fmt_f64(x1),
            fmt_f64(y1),
            fmt_f64(x2),
            fmt_f64(y2)
        );
    }

    // poly point count
    if let Some(count) = g.point_count {
        return format!("{} pts", count);
    }

    String::new()
}

fn render_flags(node: &NodeEntry) -> String {
    let mut flags = Vec::new();
    if node.visible == Some(false) {
        flags.push("[hidden]");
    }
    if node.locked == Some(true) {
        flags.push("[locked]");
    }
    flags.join(" ")
}

/// Format an `f64` without a trailing `.0` when the value is whole.
fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 {
        (v as i64).to_string()
    } else {
        v.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
