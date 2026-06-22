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
        | Node::Connector(_) => None,
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
mod tests {
    use super::*;

    // A small doc with page → group → [rect, ellipse], plus a top-level text.
    const SMALL_DOC: &str = r##"zenith version=1 {
  project id="proj.1" name="Inspect Test"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.1" title="Inspect Test" {
    page id="page.1" w=(px)800 h=(px)600 {
      group id="group.1" x=(px)10 y=(px)20 w=(px)300 h=(px)200 {
        rect id="rect.1" x=(px)10 y=(px)20 w=(px)100 h=(px)50
        ellipse id="ellipse.1" x=(px)120 y=(px)20 w=(px)80 h=(px)80
      }
      text id="text.1" x=(px)0 y=(px)250 w=(px)400 h=(px)40
    }
  }
}
"##;

    // A doc with a hidden and locked node.
    const FLAGS_DOC: &str = r##"zenith version=1 {
  project id="proj.f" name="Flags Test"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.f" title="Flags Test" {
    page id="page.f" w=(px)400 h=(px)300 {
      rect id="rect.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 visible=#false
      rect id="rect.locked" x=(px)0 y=(px)0 w=(px)100 h=(px)100 locked=#true
    }
  }
}
"##;

    // ── build_doc_tree ────────────────────────────────────────────────────────

    #[test]
    fn doc_tree_page_count() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        assert_eq!(pages.len(), 1, "expected exactly 1 page");
    }

    #[test]
    fn doc_tree_page_dimensions() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        let page = &pages[0];
        assert_eq!(page.width, 800.0);
        assert_eq!(page.height, 600.0);
    }

    #[test]
    fn doc_tree_page_children_order() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        let children = &pages[0].children;
        // Top-level: group.1 then text.1 (source order).
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].id, "group.1");
        assert_eq!(children[0].kind, "group");
        assert_eq!(children[1].id, "text.1");
        assert_eq!(children[1].kind, "text");
    }

    #[test]
    fn doc_tree_group_children() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        let group = &pages[0].children[0];
        assert_eq!(group.children.len(), 2, "group must have 2 children");
        assert_eq!(group.children[0].id, "rect.1");
        assert_eq!(group.children[0].kind, "rect");
        assert_eq!(group.children[1].id, "ellipse.1");
        assert_eq!(group.children[1].kind, "ellipse");
    }

    #[test]
    fn doc_tree_geometry_values() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        let rect = &pages[0].children[0].children[0]; // group.1 → rect.1
        let geom = rect.geometry.as_ref().unwrap();
        assert_eq!(geom.x, Some(10.0));
        assert_eq!(geom.y, Some(20.0));
        assert_eq!(geom.w, Some(100.0));
        assert_eq!(geom.h, Some(50.0));
    }

    // ── find_node_tree ────────────────────────────────────────────────────────

    #[test]
    fn find_top_level_node() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "text.1");
        assert!(found.is_some(), "text.1 must be found");
        let e = found.unwrap();
        assert_eq!(e.id, "text.1");
        assert_eq!(e.kind, "text");
    }

    #[test]
    fn find_nested_node() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "ellipse.1");
        assert!(found.is_some(), "ellipse.1 must be found inside group");
        let e = found.unwrap();
        assert_eq!(e.id, "ellipse.1");
        assert_eq!(e.kind, "ellipse");
    }

    #[test]
    fn find_container_node_includes_children() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "group.1");
        assert!(found.is_some(), "group.1 must be found");
        let group = found.unwrap();
        assert_eq!(
            group.children.len(),
            2,
            "group subtree must include 2 children"
        );
        // Children must be in source order.
        assert_eq!(group.children[0].id, "rect.1");
        assert_eq!(group.children[1].id, "ellipse.1");
    }

    #[test]
    fn find_missing_node_returns_none() {
        let doc = KdlAdapter.parse(SMALL_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "nonexistent.node");
        assert!(found.is_none());
    }

    // ── run() integration ─────────────────────────────────────────────────────

    #[test]
    fn run_human_whole_doc() {
        let out = run(SMALL_DOC, None, false).expect("run must succeed");
        assert!(out.contains("page page.1"), "must contain page line");
        assert!(out.contains("group group.1"), "must contain group line");
        assert!(out.contains("rect rect.1"), "must contain rect line");
        assert!(out.contains("ellipse ellipse.1"), "must contain ellipse");
        assert!(out.contains("text text.1"), "must contain text");
    }

    #[test]
    fn run_human_indentation() {
        let out = run(SMALL_DOC, None, false).expect("run must succeed");
        // group is indented 2 spaces (depth 1), rect is indented 4 (depth 2).
        let group_line = out.lines().find(|l| l.contains("group.1")).unwrap();
        let rect_line = out.lines().find(|l| l.contains("rect.1")).unwrap();
        assert!(
            group_line.starts_with("  "),
            "group must be at depth 1 (2 spaces)"
        );
        assert!(
            rect_line.starts_with("    "),
            "rect must be at depth 2 (4 spaces)"
        );
    }

    #[test]
    fn run_human_flags() {
        let out = run(FLAGS_DOC, None, false).expect("run must succeed");
        assert!(
            out.contains("[hidden]"),
            "hidden node must show [hidden] flag"
        );
        assert!(
            out.contains("[locked]"),
            "locked node must show [locked] flag"
        );
    }

    #[test]
    fn run_json_whole_doc_schema() {
        let out = run(SMALL_DOC, None, true).expect("run must succeed");
        assert!(
            out.contains("zenith-inspect-v1"),
            "JSON must have schema field"
        );
    }

    #[test]
    fn run_json_has_pages_array() {
        let out = run(SMALL_DOC, None, true).expect("run must succeed");
        let v: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
        let pages = v["pages"].as_array().expect("pages must be array");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0]["id"], "page.1");
    }

    #[test]
    fn run_json_node_kinds_correct() {
        let out = run(SMALL_DOC, None, true).expect("run must succeed");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let children = v["pages"][0]["children"].as_array().unwrap();
        assert_eq!(children[0]["kind"], "group");
        assert_eq!(children[1]["kind"], "text");
        let group_children = children[0]["children"].as_array().unwrap();
        assert_eq!(group_children[0]["kind"], "rect");
        assert_eq!(group_children[1]["kind"], "ellipse");
    }

    #[test]
    fn run_node_flag_filters_subtree() {
        let out = run(SMALL_DOC, Some("group.1"), false).expect("run must succeed");
        assert!(out.contains("group group.1"), "must have root line");
        assert!(out.contains("rect rect.1"), "must include children");
        assert!(out.contains("ellipse ellipse.1"), "must include children");
        // text.1 is NOT inside group.1 so must not appear.
        assert!(
            !out.contains("text text.1"),
            "text.1 must NOT appear in group subtree"
        );
    }

    #[test]
    fn run_node_json_flag() {
        let out = run(SMALL_DOC, Some("group.1"), true).expect("run must succeed");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["schema"], "zenith-inspect-v1");
        assert_eq!(v["node"]["id"], "group.1");
        assert_eq!(v["node"]["kind"], "group");
        assert_eq!(v["node"]["children"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn run_node_missing_id_errors() {
        let result = run(SMALL_DOC, Some("does.not.exist"), false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.exit_code, 2);
        assert!(err.message.contains("does.not.exist"));
    }

    #[test]
    fn run_parse_error_returns_err() {
        let result = run("not valid kdl {{{", None, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.exit_code, 2);
    }

    // ── Table cell descent ────────────────────────────────────────────────────

    // A doc with a table whose first cell contains a rect and second cell
    // contains a text, so we can assert that inspect descends into cell children.
    const TABLE_INSPECT_DOC: &str = r##"zenith version=1 {
  project id="proj.t" name="Table Inspect"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.t" title="Table Inspect" {
    page id="page.t" w=(px)640 h=(px)400 {
      table id="tbl.1" x=(px)0 y=(px)0 w=(px)400 h=(px)200 {
        column width=(px)200
        column width=(px)200
        row {
          cell {
            rect id="cell.rect.1" x=(px)0 y=(px)0 w=(px)50 h=(px)50
          }
          cell {
            text id="cell.text.1" x=(px)0 y=(px)0 w=(px)100 h=(px)30 {
              span "hi"
            }
          }
        }
      }
    }
  }
}
"##;

    #[test]
    fn find_node_inside_table_cell_returns_entry() {
        let doc = KdlAdapter.parse(TABLE_INSPECT_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "cell.rect.1");
        assert!(
            found.is_some(),
            "cell.rect.1 inside a table cell must be findable"
        );
        let e = found.unwrap();
        assert_eq!(e.id, "cell.rect.1");
        assert_eq!(e.kind, "rect");
    }

    #[test]
    fn find_text_inside_table_cell_returns_entry() {
        let doc = KdlAdapter.parse(TABLE_INSPECT_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "cell.text.1");
        assert!(
            found.is_some(),
            "cell.text.1 inside a table cell must be findable"
        );
        let e = found.unwrap();
        assert_eq!(e.id, "cell.text.1");
        assert_eq!(e.kind, "text");
    }

    #[test]
    fn run_node_flag_table_cell_child_found() {
        // `zenith inspect --node cell.rect.1` must succeed and return that node.
        let out = run(TABLE_INSPECT_DOC, Some("cell.rect.1"), false)
            .expect("inspect of cell child must succeed");
        assert!(
            out.contains("cell.rect.1"),
            "output must mention the node id; got: {out}"
        );
        assert!(
            out.contains("rect"),
            "output must mention the node kind; got: {out}"
        );
    }

    #[test]
    fn run_node_flag_table_cell_child_not_found_errors() {
        // A nonexistent id inside a table must still return not-found.
        let result = run(TABLE_INSPECT_DOC, Some("no.such.node"), false);
        assert!(result.is_err(), "missing id must error");
        let err = result.unwrap_err();
        assert_eq!(err.exit_code, 2);
        assert!(err.message.contains("no.such.node"));
    }

    // ── Unknown (library) node descent ────────────────────────────────────────

    // A doc with an unknown node kind (`mystery`) carrying an `id` and a known
    // `rect` child, so we can assert that inspect surfaces the unknown node with
    // its id and descends into its children.
    const UNKNOWN_INSPECT_DOC: &str = r##"zenith version=1 {
  project id="proj.u" name="Unknown Inspect"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.u" title="Unknown Inspect" {
    page id="page.u" w=(px)640 h=(px)400 {
      mystery id="lib.1" {
        rect id="inner" x=(px)0 y=(px)0 w=(px)50 h=(px)50
      }
    }
  }
}
"##;

    #[test]
    fn doc_tree_unknown_node_shows_id_and_children() {
        let doc = KdlAdapter.parse(UNKNOWN_INSPECT_DOC.as_bytes()).unwrap();
        let pages = build_doc_tree(&doc.body.pages);
        let unknown = &pages[0].children[0];
        assert_eq!(unknown.id, "lib.1", "unknown node must expose its id");
        assert_eq!(unknown.kind, "mystery", "unknown node keeps its kind");
        assert_eq!(
            unknown.children.len(),
            1,
            "unknown node subtree must include its child"
        );
        assert_eq!(unknown.children[0].id, "inner");
        assert_eq!(unknown.children[0].kind, "rect");
    }

    #[test]
    fn find_unknown_node_by_id() {
        let doc = KdlAdapter.parse(UNKNOWN_INSPECT_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "lib.1");
        assert!(found.is_some(), "unknown node must be findable by id");
        let e = found.unwrap();
        assert_eq!(e.id, "lib.1");
        assert_eq!(e.kind, "mystery");
        assert_eq!(e.children.len(), 1, "subtree must include the child");
        assert_eq!(e.children[0].id, "inner");
    }

    #[test]
    fn find_known_node_inside_unknown() {
        let doc = KdlAdapter.parse(UNKNOWN_INSPECT_DOC.as_bytes()).unwrap();
        let found = find_node_tree(&doc.body.pages, "inner");
        assert!(
            found.is_some(),
            "known rect nested in an unknown node must be findable"
        );
        let e = found.unwrap();
        assert_eq!(e.id, "inner");
        assert_eq!(e.kind, "rect");
    }
}
