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
