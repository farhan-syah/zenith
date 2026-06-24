mod common;
use common::*;
use zenith_tx::{Op, Permissions, Transaction, TxStatus, run_transaction};

// ── Fixture ───────────────────────────────────────────────────────────────────
//
// An 800×600 page with:
//   - "box": a 60×40 rect at (100, 80)  — used for align_to_edge tests
//   - "far": a 50×50 rect at (750, 580) — lies near the edge; used for the
//     shrink-page / off_canvas test
//
// All coords and dims are (px). No tokens needed for geometry-only ops.
const LAYOUT_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" { }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)800 h=(px)600 {
      rect id="box" x=(px)100 y=(px)80 w=(px)60 h=(px)40
      rect id="far" x=(px)750 y=(px)580 w=(px)50 h=(px)50
    }
  }
}"##;

// ── set_page_size: happy path ─────────────────────────────────────────────────

#[test]
fn set_page_size_accepted_updates_dimensions() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "pg1".to_owned(),
            w: "(px)794".to_owned(),
            h: "(px)1123".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    // The transaction must be accepted (or accepted with warnings from off_canvas).
    assert!(
        matches!(
            result.status,
            TxStatus::Accepted | TxStatus::AcceptedWithWarnings
        ),
        "expected Accepted or AcceptedWithWarnings; got {:?}, diagnostics: {:?}",
        result.status,
        result.diagnostics
    );

    // The page id must appear in affected_node_ids.
    assert!(
        result.affected_node_ids.contains(&"pg1".to_owned()),
        "pg1 must be in affected_node_ids; got {:?}",
        result.affected_node_ids
    );

    // The serialised source must reflect the new dimensions.
    assert!(
        result.source_after.contains("w=(px)794"),
        "source_after must contain w=(px)794; got:\n{}",
        result.source_after
    );
    assert!(
        result.source_after.contains("h=(px)1123"),
        "source_after must contain h=(px)1123; got:\n{}",
        result.source_after
    );
}

// ── set_page_size: unknown page → tx.unknown_node ────────────────────────────

#[test]
fn set_page_size_unknown_page_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "ghost.page".to_owned(),
            w: "(px)400".to_owned(),
            h: "(px)300".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
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

// ── set_page_size: unparseable w → tx.invalid_value ──────────────────────────

#[test]
fn set_page_size_bad_w_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "pg1".to_owned(),
            w: "notadimension".to_owned(),
            h: "(px)300".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── set_page_size: zero h → tx.invalid_value ─────────────────────────────────

#[test]
fn set_page_size_zero_h_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "pg1".to_owned(),
            w: "(px)800".to_owned(),
            h: "(px)0".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value for zero h; got: {:?}",
        result.diagnostics
    );
}

// ── set_page_size: negative w → tx.invalid_value ─────────────────────────────

#[test]
fn set_page_size_negative_w_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "pg1".to_owned(),
            w: "(px)-100".to_owned(),
            h: "(px)300".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value for negative w; got: {:?}",
        result.diagnostics
    );
}

// ── set_page_size: shrink so "far" falls off-canvas ──────────────────────────
//
// "far" at (750,580) with size 50×50 on the original 800×600 page.
// After shrinking to 400×300 it lies entirely outside the new bounds.
// Verdict: Accepted — off_canvas is an Advisory (not a Warning), so it does NOT
// flip the status to AcceptedWithWarnings; the advisory diagnostic is still
// emitted. The resize is applied AND child coordinates are unchanged (no reflow).
#[test]
fn set_page_size_shrink_child_falls_off_canvas() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::SetPageSize {
            page: "pg1".to_owned(),
            w: "(px)400".to_owned(),
            h: "(px)300".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    // The resize itself is valid → not Rejected. off_canvas is an Advisory
    // (below Warning), so the status remains Accepted.
    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "expected Accepted (off_canvas is an advisory, not a warning); got {:?}, diagnostics: {:?}",
        result.status,
        result.diagnostics
    );

    // At least one off_canvas advisory must be present.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "layout.off_canvas"),
        "expected an off_canvas advisory; got: {:?}",
        result.diagnostics
    );

    // The page resize must have been applied.
    assert!(
        result.source_after.contains("w=(px)400"),
        "source_after must contain w=(px)400"
    );
    assert!(
        result.source_after.contains("h=(px)300"),
        "source_after must contain h=(px)300"
    );

    // Child node "far" must retain its original coordinates (no reflow).
    let far_x = extract_px_attr(&result.source_after, "far", "x").expect("far.x");
    let far_y = extract_px_attr(&result.source_after, "far", "y").expect("far.y");
    assert!(
        (far_x - 750.0).abs() < 1e-9,
        "far.x must be unchanged (750); got {far_x}"
    );
    assert!(
        (far_y - 580.0).abs() < 1e-9,
        "far.y must be unchanged (580); got {far_y}"
    );

    // "pg1" is in affected.
    assert!(
        result.affected_node_ids.contains(&"pg1".to_owned()),
        "pg1 must be in affected_node_ids"
    );
}

// ── align_to_edge: right with margin 20 ──────────────────────────────────────
//
// "box": w=60, page_w=800 → x = 800 - 60 - 20 = 720

#[test]
fn align_to_edge_right_margin() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "box".to_owned(),
            edge: "right".to_owned(),
            margin: 20.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert!(result.affected_node_ids.contains(&"box".to_owned()));

    let x = extract_px_attr(&result.source_after, "box", "x").expect("box.x");
    // 800 - 60 - 20 = 720
    assert!(
        (x - 720.0).abs() < 1e-9,
        "expected x=720 for right+margin=20; got {x}"
    );

    // y must be unchanged.
    let y = extract_px_attr(&result.source_after, "box", "y").expect("box.y");
    assert!((y - 80.0).abs() < 1e-9, "y must be unchanged (80); got {y}");
}

// ── align_to_edge: bottom with margin 20 ─────────────────────────────────────
//
// "box": h=40, page_h=600 → y = 600 - 40 - 20 = 540

#[test]
fn align_to_edge_bottom_margin() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "box".to_owned(),
            edge: "bottom".to_owned(),
            margin: 20.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "diagnostics: {:?}",
        result.diagnostics
    );

    let y = extract_px_attr(&result.source_after, "box", "y").expect("box.y");
    // 600 - 40 - 20 = 540
    assert!(
        (y - 540.0).abs() < 1e-9,
        "expected y=540 for bottom+margin=20; got {y}"
    );

    // x must be unchanged.
    let x = extract_px_attr(&result.source_after, "box", "x").expect("box.x");
    assert!(
        (x - 100.0).abs() < 1e-9,
        "x must be unchanged (100); got {x}"
    );
}

// ── align_to_edge: two ops in one transaction → bottom-right corner ───────────
//
// "box": w=60, h=40, page 800×600, margin=20 on both axes.
// right: x = 800 - 60 - 20 = 720
// bottom: y = 600 - 40 - 20 = 540

#[test]
fn align_to_edge_right_and_bottom_corner() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![
            Op::AlignToEdge {
                node: "box".to_owned(),
                edge: "right".to_owned(),
                margin: 20.0,
            },
            Op::AlignToEdge {
                node: "box".to_owned(),
                edge: "bottom".to_owned(),
                margin: 20.0,
            },
        ],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "diagnostics: {:?}",
        result.diagnostics
    );

    let x = extract_px_attr(&result.source_after, "box", "x").expect("box.x");
    let y = extract_px_attr(&result.source_after, "box", "y").expect("box.y");
    assert!(
        (x - 720.0).abs() < 1e-9,
        "expected x=720 (right corner); got {x}"
    );
    assert!(
        (y - 540.0).abs() < 1e-9,
        "expected y=540 (bottom corner); got {y}"
    );
}

// ── align_to_edge: hcenter ────────────────────────────────────────────────────
//
// "box": w=60, page_w=800 → x = (800 - 60) / 2 = 370

#[test]
fn align_to_edge_hcenter() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "box".to_owned(),
            edge: "hcenter".to_owned(),
            margin: 0.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "diagnostics: {:?}",
        result.diagnostics
    );

    let x = extract_px_attr(&result.source_after, "box", "x").expect("box.x");
    // (800 - 60) / 2 = 370
    assert!(
        (x - 370.0).abs() < 1e-9,
        "expected x=370 for hcenter; got {x}"
    );
}

// ── align_to_edge: invalid edge → tx.unsupported_property ────────────────────

#[test]
fn align_to_edge_invalid_edge_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "box".to_owned(),
            edge: "diagonal".to_owned(),
            margin: 0.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
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

// ── align_to_edge: unknown node → tx.unknown_node ────────────────────────────

#[test]
fn align_to_edge_unknown_node_rejected() {
    let doc = parse(LAYOUT_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "ghost".to_owned(),
            edge: "right".to_owned(),
            margin: 0.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
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

// ── align_to_edge: node without x/y/w/h geometry → tx.unsupported_property ───
//
// A `line` node carries x1/y1/x2/y2, not x/y/w/h, so read_geometry_px
// returns None for it. We use LINE_DOC from the common fixtures.

#[test]
fn align_to_edge_no_geometry_rejected() {
    let doc = parse(LINE_DOC);
    let tx = Transaction {
        ops: vec![Op::AlignToEdge {
            node: "ln1".to_owned(),
            edge: "right".to_owned(),
            margin: 0.0,
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction must not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.unsupported_property"),
        "expected tx.unsupported_property for line (no bbox); got: {:?}",
        result.diagnostics
    );
}
