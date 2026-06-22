//! `table` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ── Table validation ──────────────────────────────────────────────────

/// Build a table cell holding a single text child.
fn cell_with_text(id: &str, colspan: u32) -> TableCell {
    TableCell {
        colspan,
        rowspan: 1,
        children: vec![minimal_text(id, None)],
        fill: None,
        border: None,
        border_width: None,
        h_align: None,
        v_align: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

/// Build a 2-column / 2-row table with full geometry and the given overrides.
fn table_node(
    id: &str,
    geometry: bool,
    h_align: Option<String>,
    rows: Vec<TableRow>,
    columns: Vec<TableColumn>,
) -> Node {
    Node::Table(Box::new(TableNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x: if geometry { Some(px(40.0)) } else { None },
        y: if geometry { Some(px(40.0)) } else { None },
        w: if geometry { Some(px(400.0)) } else { None },
        h: if geometry { Some(px(200.0)) } else { None },
        columns,
        rows,
        header_rows: None,
        flows: None,
        gap: None,
        cell_padding: None,
        border_collapse: None,
        fill: None,
        border: None,
        border_width: None,
        header_fill: None,
        header_style: None,
        h_align,
        v_align: None,
        style: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

fn two_cols() -> Vec<TableColumn> {
    vec![
        TableColumn {
            width: Some(px(160.0)),
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
        TableColumn {
            width: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
    ]
}

#[test]
fn table_missing_geometry_errors() {
    let rows = vec![TableRow {
        cells: vec![cell_with_text("t.c1", 1), cell_with_text("t.c2", 1)],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node("t.geom", false, None, rows, two_cols());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.missing_geometry"),
        "table without x/y/w/h must error node.missing_geometry; got {:?}",
        codes(&report)
    );
}

#[test]
fn table_colspan_overflow_errors() {
    // Single column, but a cell declares colspan=2 → overflow.
    let rows = vec![TableRow {
        cells: vec![cell_with_text("t.c1", 2)],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let columns = vec![TableColumn {
        width: Some(px(160.0)),
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node("t.overflow", true, None, rows, columns);
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "table.cell_overflow"),
        "colspan exceeding column count must error table.cell_overflow; got {:?}",
        codes(&report)
    );
}

#[test]
fn table_bad_h_align_warns() {
    let rows = vec![TableRow {
        cells: vec![cell_with_text("t.c1", 1), cell_with_text("t.c2", 1)],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node(
        "t.align",
        true,
        Some("middle".to_owned()), // invalid for h-align
        rows,
        two_cols(),
    );
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "table.invalid_h_align"),
        "bad h-align must warn table.invalid_h_align; got {:?}",
        codes(&report)
    );
}

#[test]
fn table_well_formed_is_clean() {
    let rows = vec![
        TableRow {
            cells: vec![cell_with_text("t.c11", 1), cell_with_text("t.c12", 1)],
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
        TableRow {
            cells: vec![cell_with_text("t.c21", 1), cell_with_text("t.c22", 1)],
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
    ];
    let table = table_node("t.ok", true, Some("center".to_owned()), rows, two_cols());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        !report.has_errors(),
        "well-formed table must have no errors; got {:?}",
        codes(&report)
    );
    assert!(
        !has_code(&report, "table.cell_overflow") && !has_code(&report, "table.invalid_h_align"),
        "well-formed table must not emit table.* warnings; got {:?}",
        codes(&report)
    );
}

#[test]
fn table_cell_text_without_geometry_is_clean() {
    // A cell positions and sizes its children (auto-box), so a cell text that
    // omits x/y/w/h must NOT trigger node.missing_geometry.
    let mut text = minimal_text("t.cell.txt", None);
    if let Node::Text(t) = &mut text {
        t.x = None;
        t.y = None;
        t.w = None;
        t.h = None;
    }
    let cell = TableCell {
        colspan: 1,
        rowspan: 1,
        children: vec![text],
        fill: None,
        border: None,
        border_width: None,
        h_align: None,
        v_align: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    };
    let columns = vec![TableColumn {
        width: Some(px(160.0)),
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let rows = vec![TableRow {
        cells: vec![cell],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node("t.auto", true, None, rows, columns);
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "node.missing_geometry"),
        "cell text without x/y/w/h must NOT emit node.missing_geometry; got {:?}",
        codes(&report)
    );
}

#[test]
fn table_cell_unknown_property_warns() {
    // A cell with an unrecognized property produces node.unknown_property.
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "future-cell-prop".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("yes".to_owned()),
            ty: None,
        },
    );
    let cell = TableCell {
        colspan: 1,
        rowspan: 1,
        children: vec![],
        fill: None,
        border: None,
        border_width: None,
        h_align: None,
        v_align: None,
        source_span: None,
        unknown_props,
    };
    let rows = vec![TableRow {
        cells: vec![cell],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let columns = vec![TableColumn {
        width: Some(px(160.0)),
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node("t.cell.unk", true, None, rows, columns);
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "cell with unknown property must warn node.unknown_property; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn table_row_unknown_property_warns() {
    // A row with an unrecognized property produces node.unknown_property.
    let mut row_unknown_props = BTreeMap::new();
    row_unknown_props.insert(
        "future-row-prop".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::Integer(7),
            ty: None,
        },
    );
    let rows = vec![TableRow {
        cells: vec![cell_with_text("t.r.c1", 1), cell_with_text("t.r.c2", 1)],
        source_span: None,
        unknown_props: row_unknown_props,
    }];
    let table = table_node("t.row.unk", true, None, rows, two_cols());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "row with unknown property must warn node.unknown_property; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn table_column_unknown_property_warns() {
    // A column with an unrecognized property produces node.unknown_property.
    let mut col_unknown_props = BTreeMap::new();
    col_unknown_props.insert(
        "future-col-prop".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::Bool(true),
            ty: None,
        },
    );
    let columns = vec![
        TableColumn {
            width: Some(px(160.0)),
            source_span: None,
            unknown_props: col_unknown_props,
        },
        TableColumn {
            width: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
    ];
    let rows = vec![TableRow {
        cells: vec![cell_with_text("t.col.c1", 1), cell_with_text("t.col.c2", 1)],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }];
    let table = table_node("t.col.unk", true, None, rows, columns);
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "column with unknown property must warn node.unknown_property; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn table_clean_no_unknown_property_warning() {
    // A well-formed table with no unknown props on table/columns/rows/cells
    // must NOT produce any node.unknown_property warning.
    let rows = vec![
        TableRow {
            cells: vec![
                cell_with_text("t.clean.c11", 1),
                cell_with_text("t.clean.c12", 1),
            ],
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
        TableRow {
            cells: vec![
                cell_with_text("t.clean.c21", 1),
                cell_with_text("t.clean.c22", 1),
            ],
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
    ];
    let table = table_node("t.clean", true, Some("center".to_owned()), rows, two_cols());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![table])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "node.unknown_property"),
        "clean table must not emit node.unknown_property; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}
