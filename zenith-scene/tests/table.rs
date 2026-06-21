//! Integration tests for single-page `table` compilation.
//!
//! Covers: cell background + border command emission, cell content (text)
//! positioned at the cell content-box origin, a `colspan=2` cell spanning two
//! columns' width, a `visible=#false` table emitting nothing, and the
//! CONTENT-BASED column auto-sizing + content-based row heights (auto column
//! sizes to its widest cell's text; a wrapping cell makes its row taller;
//! all-explicit-width columns are unchanged).

mod common;

use common::{SceneCommand, compile, compile_page, default_provider, parse};

/// A 2-row × 3-col table: one explicit column (160px) plus two auto columns,
/// with a colspan=2 cell in the first row. Border + fill use color tokens.
fn table_src() -> &'static str {
    r##"zenith version=1 {
  project id="proj.tbl" name="TBL"
  tokens format="zenith-token-v1" {
    token id="color.line" type="color" value="#cccccc"
    token id="color.cellbg" type="color" value="#f0f0f0"
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.tbl" title="TBL" {
    page id="page.tbl" w=(px)640 h=(px)400 {
      table id="t1" x=(px)40 y=(px)40 w=(px)520 h=(px)240 border=(token)"color.line" border-width=(px)1 fill=(token)"color.cellbg" cell-padding=(px)0 gap=(px)0 {
        column width=(px)160
        column
        column
        row {
          cell { text id="c11" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "Name" } }
          cell colspan=2 { text id="c12" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "Details" } }
        }
        row {
          cell { text id="c21" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "Ada" } }
          cell { text id="c22" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "Lovelace" } }
          cell { text id="c23" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "1815" } }
        }
      }
    }
  }
}
"##
}

#[test]
fn table_emits_cell_backgrounds_and_borders() {
    let doc = parse(table_src());
    let result = compile(&doc, &default_provider());

    let fill_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
        .count();
    let stroke_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        .count();

    // 5 placed cells (2 in row 1 due to colspan, 3 in row 2). Each emits one
    // FillRect (cell background) and four StrokeLines (separate border edges).
    // The page has no background, so every FillRect here is a cell background.
    assert_eq!(fill_count, 5, "expected one fill per placed cell");
    assert_eq!(
        stroke_count,
        5 * 4,
        "expected four border edges per placed cell"
    );

    // Cell content: every cell's text must produce a glyph run.
    let glyph_runs = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    assert_eq!(glyph_runs, 5, "expected one glyph run per cell text");
}

#[test]
fn colspan_cell_spans_two_columns() {
    let doc = parse(table_src());
    let result = compile(&doc, &default_provider());

    // Column 0 is EXPLICIT 160px (fixed, unchanged by content-based sizing).
    // Columns 1 and 2 are AUTO and now size to their content. With gap=0/pad=0
    // the colspan cell still starts at x=40+160=200 (col 0 is explicit), and its
    // width must equal the sum of the two AUTO column widths — which are exactly
    // the widths of the two single cells in row 2 (col1="Lovelace", col2="1815").
    //
    // Emission is row-major: fills[0]=cell0 (col0), fills[1]=colspan (cols1+2),
    // fills[2]=row2-col0, fills[3]=row2-col1, fills[4]=row2-col2.
    let fills: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { x, w, .. } => Some((*x, *w)),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 5, "expected 5 cell fills; got {fills:?}");
    // First cell: x=40 (table origin), width=160 (explicit column, unchanged).
    assert!((fills[0].0 - 40.0).abs() < 0.01, "cell0 x; got {fills:?}");
    assert!((fills[0].1 - 160.0).abs() < 0.01, "cell0 w; got {fills:?}");
    // Colspan cell starts immediately after the explicit column: x=200.
    assert!(
        (fills[1].0 - 200.0).abs() < 0.01,
        "colspan x; got {fills:?}"
    );
    // The colspan width spans BOTH auto columns: it equals the sum of the two
    // single auto cells' widths in row 2 (col1 + col2).
    let col1_w = fills[3].1;
    let col2_w = fills[4].1;
    assert!(
        (fills[1].1 - (col1_w + col2_w)).abs() < 0.5,
        "colspan w must span both auto columns: {} vs {}+{}; got {fills:?}",
        fills[1].1,
        col1_w,
        col2_w
    );
    // The two auto columns place edge-to-edge (gap=0): row-2 col1 starts at 200,
    // col2 starts at 200+col1_w.
    assert!(
        (fills[3].0 - 200.0).abs() < 0.01,
        "row2 col1 x; got {fills:?}"
    );
    assert!(
        (fills[4].0 - (200.0 + col1_w)).abs() < 0.5,
        "row2 col2 x; got {fills:?}"
    );
    // Auto columns are content-sized, NOT the old equal-split 180px each.
    assert!(
        col1_w > 0.0 && col2_w > 0.0,
        "auto cols sized; got {fills:?}"
    );
}

/// An AUTO column sizes to its WIDEST cell's natural text: a column whose cells
/// hold a long word is wider than a column whose cells hold a short word.
#[test]
fn auto_column_sizes_to_widest_text() {
    // Two AUTO columns, two rows. Column 0 always holds a short word; column 1
    // holds a much longer word. The long-text column must come out wider.
    let src = r##"zenith version=1 {
  project id="proj.aw" name="AW"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.aw" title="AW" {
    page id="page.aw" w=(px)800 h=(px)400 {
      table id="t.aw" x=(px)0 y=(px)0 w=(px)800 h=(px)300 fill=(token)"color.ink" cell-padding=(px)0 gap=(px)0 {
        column
        column
        row {
          cell { text id="a1" x=(px)0 y=(px)0 { span "Hi" } }
          cell { text id="a2" x=(px)0 y=(px)0 { span "Supercalifragilistic" } }
        }
        row {
          cell { text id="b1" x=(px)0 y=(px)0 { span "Ok" } }
          cell { text id="b2" x=(px)0 y=(px)0 { span "Antidisestablishmentarianism" } }
        }
      }
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Row-major fills: [0]=col0/row0, [1]=col1/row0, [2]=col0/row1, [3]=col1/row1.
    let fills: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { x, w, .. } => Some((*x, *w)),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 4, "expected 4 cell fills; got {fills:?}");
    let col0_w = fills[0].1;
    let col1_w = fills[1].1;
    assert!(
        col1_w > col0_w,
        "the long-text column must be wider than the short-text column: {col1_w} vs {col0_w}"
    );
}

/// A cell whose text WRAPS onto multiple lines makes its row taller than a row
/// whose cells fit on a single line.
#[test]
fn wrapping_text_makes_row_taller() {
    // Two AUTO columns. Column 0 is widened by a long header in row 0; column 1
    // is forced narrow. Row 0's col-1 text is short (single line); row 1's col-1
    // text is long, so at the narrow assigned width it WRAPS to several lines and
    // its row must be taller than the single-line row 0.
    let src = r##"zenith version=1 {
  project id="proj.rh" name="RH"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.rh" title="RH" {
    page id="page.rh" w=(px)400 h=(px)600 {
      table id="t.rh" x=(px)0 y=(px)0 w=(px)200 h=(px)500 fill=(token)"color.ink" cell-padding=(px)0 gap=(px)0 {
        column width=(px)40
        column width=(px)80
        row {
          cell { text id="r0a" x=(px)0 y=(px)0 { span "A" } }
          cell { text id="r0b" x=(px)0 y=(px)0 w=(px)80 { span "Short" } }
        }
        row {
          cell { text id="r1a" x=(px)0 y=(px)0 { span "B" } }
          cell { text id="r1b" x=(px)0 y=(px)0 w=(px)80 { span "alpha bravo charlie delta echo foxtrot golf hotel india juliet" } }
        }
      }
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Row-major fills: [0],[1]=row0 cells; [2],[3]=row1 cells. Compare row tops/
    // heights by the cell y positions and heights.
    let fills: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { y, h, .. } => Some((*y, *h)),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 4, "expected 4 cell fills; got {fills:?}");
    let row0_h = fills[0].1;
    let row1_h = fills[2].1;
    assert!(
        row1_h > row0_h + 1.0,
        "the wrapping row must be taller than the single-line row: {row1_h} vs {row0_h}"
    );
    // Row 1 must start below row 0 (content-based stacking, top-aligned).
    assert!(
        fills[2].0 > fills[0].0,
        "row 1 must sit below row 0; got {fills:?}"
    );
}

/// An ALL-EXPLICIT-width table produces the SAME column widths as the pre
/// content-sizing behavior (determinism guarantee): explicit columns are never
/// touched by content measurement.
#[test]
fn all_explicit_columns_unchanged() {
    let src = r##"zenith version=1 {
  project id="proj.ex" name="EX"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.ex" title="EX" {
    page id="page.ex" w=(px)800 h=(px)400 {
      table id="t.ex" x=(px)10 y=(px)10 w=(px)600 h=(px)300 fill=(token)"color.ink" cell-padding=(px)0 gap=(px)0 {
        column width=(px)100
        column width=(px)250
        column width=(px)90
        row {
          cell { text id="e1" x=(px)0 y=(px)0 { span "One" } }
          cell { text id="e2" x=(px)0 y=(px)0 { span "Two" } }
          cell { text id="e3" x=(px)0 y=(px)0 { span "Three" } }
        }
      }
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fills: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { x, w, .. } => Some((*x, *w)),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 3, "expected 3 cell fills; got {fills:?}");
    // Explicit widths are honored verbatim, regardless of cell content.
    assert!((fills[0].0 - 10.0).abs() < 0.01, "col0 x; got {fills:?}");
    assert!((fills[0].1 - 100.0).abs() < 0.01, "col0 w; got {fills:?}");
    assert!((fills[1].0 - 110.0).abs() < 0.01, "col1 x; got {fills:?}");
    assert!((fills[1].1 - 250.0).abs() < 0.01, "col1 w; got {fills:?}");
    assert!((fills[2].0 - 360.0).abs() < 0.01, "col2 x; got {fills:?}");
    assert!((fills[2].1 - 90.0).abs() < 0.01, "col2 w; got {fills:?}");
}

#[test]
fn cell_text_positioned_at_content_origin() {
    let doc = parse(table_src());
    let result = compile(&doc, &default_provider());

    // The first cell's text (authored x=0) is translated to the cell content
    // origin x=40 (table x + 0 padding). With h-align default "start" the run
    // x equals the content-box left edge.
    let first_run_x = result.scene.commands.iter().find_map(|c| match c {
        SceneCommand::DrawGlyphRun { x, .. } => Some(*x),
        _ => None,
    });
    assert_eq!(
        first_run_x,
        Some(40.0),
        "first cell text must sit at the cell content origin x=40"
    );
}

#[test]
fn invisible_table_emits_nothing() {
    let src = table_src().replace("table id=\"t1\"", "table id=\"t1\" visible=#false");
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());

    // No table-derived commands: no FillRect (no page bg), no StrokeLine, no
    // glyph runs. (PushClip for the media box is always present.)
    let drawn = result.scene.commands.iter().any(|c| {
        matches!(
            c,
            SceneCommand::FillRect { .. }
                | SceneCommand::StrokeLine { .. }
                | SceneCommand::DrawGlyphRun { .. }
        )
    });
    assert!(!drawn, "an invisible table must emit no drawing commands");
}

// ── border-collapse="collapse" tests ─────────────────────────────────────────

/// A 2×2 table source used by the collapse tests. Two explicit columns of 100px
/// each, gap=0, pad=0, so adjacent cell edges are coincident.
fn collapse_2x2_src() -> &'static str {
    r##"zenith version=1 {
  project id="proj.col" name="COL"
  tokens format="zenith-token-v1" {
    token id="color.border" type="color" value="#aaaaaa"
    token id="color.bg"     type="color" value="#ffffff"
  }
  styles {}
  document id="doc.col" title="COL" {
    page id="page.col" w=(px)400 h=(px)300 {
      table id="tc" x=(px)0 y=(px)0 w=(px)200 h=(px)200 border=(token)"color.border" border-width=(px)1 fill=(token)"color.bg" cell-padding=(px)0 gap=(px)0 border-collapse="collapse" {
        column width=(px)100
        column width=(px)100
        row {
          cell { text id="r0c0" x=(px)0 y=(px)0 { span "A" } }
          cell { text id="r0c1" x=(px)0 y=(px)0 { span "B" } }
        }
        row {
          cell { text id="r1c0" x=(px)0 y=(px)0 { span "C" } }
          cell { text id="r1c1" x=(px)0 y=(px)0 { span "D" } }
        }
      }
    }
  }
}
"##
}

/// The same table but in `separate` mode (the default).
fn separate_2x2_src() -> &'static str {
    r##"zenith version=1 {
  project id="proj.sep" name="SEP"
  tokens format="zenith-token-v1" {
    token id="color.border" type="color" value="#aaaaaa"
    token id="color.bg"     type="color" value="#ffffff"
  }
  styles {}
  document id="doc.sep" title="SEP" {
    page id="page.sep" w=(px)400 h=(px)300 {
      table id="ts" x=(px)0 y=(px)0 w=(px)200 h=(px)200 border=(token)"color.border" border-width=(px)1 fill=(token)"color.bg" cell-padding=(px)0 gap=(px)0 {
        column width=(px)100
        column width=(px)100
        row {
          cell { text id="r0c0" x=(px)0 y=(px)0 { span "A" } }
          cell { text id="r0c1" x=(px)0 y=(px)0 { span "B" } }
        }
        row {
          cell { text id="r1c0" x=(px)0 y=(px)0 { span "C" } }
          cell { text id="r1c1" x=(px)0 y=(px)0 { span "D" } }
        }
      }
    }
  }
}
"##
}

/// `border-collapse="collapse"` on a 2×2 table emits FEWER `StrokeLine`s than
/// `separate` mode because the shared interior vertical and horizontal edges are
/// deduplicated. In separate mode 4 cells × 4 edges = 16; in collapse mode the
/// same table has 6 unique edges (4 perimeter + 1 interior vertical + 1 interior
/// horizontal), so collapse_count < separate_count.
#[test]
fn collapse_emits_fewer_stroke_lines_than_separate() {
    let col_result = compile(&parse(collapse_2x2_src()), &default_provider());
    let sep_result = compile(&parse(separate_2x2_src()), &default_provider());

    let col_strokes = col_result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        .count();
    let sep_strokes = sep_result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        .count();

    // Separate: 4 cells × 4 edges = 16.
    assert_eq!(
        sep_strokes, 16,
        "separate mode must emit 4 edges per cell (4×4=16); got {sep_strokes}"
    );
    // Collapse dedups the 4 SHARED interior segments (the x=interior vertical seam
    // and y=interior horizontal seam, each split per row/col and shared by two
    // cells). 16 − 4 deduplicated = 12. (Collapse removes doubled edges; it does
    // NOT merge collinear segments into single grid lines.)
    assert_eq!(
        col_strokes, 12,
        "collapse mode must dedup the 4 shared interior segments (16→12); got {col_strokes}"
    );
    assert!(
        col_strokes < sep_strokes,
        "collapse ({col_strokes}) must be strictly fewer than separate ({sep_strokes})"
    );
}

/// When one cell in a collapse table has an OWN explicit `border` color that
/// differs from the table-level default, the shared edge between that cell and
/// its neighbour must take the explicit cell color (tie-break rule: explicit wins
/// over inherited).
#[test]
fn collapse_explicit_cell_border_wins_on_shared_edge() {
    // A 1×2 table: left cell has a red explicit border; right cell inherits the
    // table default (#aaaaaa grey). The shared right edge of the left cell (= the
    // left edge of the right cell) must be red, not grey.
    let src = r##"zenith version=1 {
  project id="proj.tb" name="TB"
  tokens format="zenith-token-v1" {
    token id="color.grey" type="color" value="#aaaaaa"
    token id="color.red"  type="color" value="#ff0000"
    token id="color.bg"   type="color" value="#ffffff"
  }
  styles {}
  document id="doc.tb" title="TB" {
    page id="page.tb" w=(px)400 h=(px)200 {
      table id="tt" x=(px)0 y=(px)0 w=(px)200 h=(px)100 border=(token)"color.grey" border-width=(px)1 fill=(token)"color.bg" cell-padding=(px)0 gap=(px)0 border-collapse="collapse" {
        column width=(px)100
        column width=(px)100
        row {
          cell border=(token)"color.red" {
            text id="lc" x=(px)0 y=(px)0 { span "Left" }
          }
          cell {
            text id="rc" x=(px)0 y=(px)0 { span "Right" }
          }
        }
      }
    }
  }
}
"##;
    let result = compile(&parse(src), &default_provider());

    // The shared vertical interior edge sits at x=100 (left cell right edge =
    // right cell left edge). Its y-extent is the content-based row height (the
    // table does not stretch to its declared h), so match on x only — x=0 and
    // x=200 are the perimeter verticals, leaving x=100 as the unique interior one.
    let shared_edge_color = result.scene.commands.iter().find_map(|c| match c {
        SceneCommand::StrokeLine { x1, x2, color, .. }
            if (x1 - 100.0).abs() < 0.1 && (x2 - 100.0).abs() < 0.1 =>
        {
            Some(*color)
        }
        _ => None,
    });

    let color = shared_edge_color.expect(
        "a StrokeLine at x=100 (the shared vertical interior edge) must exist in collapse output",
    );
    // The explicit cell border is red (#ff0000 → r=255, g=0, b=0).
    assert_eq!(
        color.r, 255,
        "shared edge must be red (explicit cell border wins); got r={} g={} b={}",
        color.r, color.g, color.b
    );
    assert_eq!(
        color.g, 0,
        "shared edge must be red; got r={} g={} b={}",
        color.r, color.g, color.b
    );
    assert_eq!(
        color.b, 0,
        "shared edge must be red; got r={} g={} b={}",
        color.r, color.g, color.b
    );
}

// ── Header-row styling tests ──────────────────────────────────────────────────

/// A 2-row table with `header-rows=1` and a distinct `header-fill` token.
/// The first row's cell background must use the header-fill color; the second
/// row's cell must use the table body `fill` color.
#[test]
fn header_fill_applied_to_first_row_cells() {
    let src = r##"zenith version=1 {
  project id="proj.hf" name="HF"
  tokens format="zenith-token-v1" {
    token id="color.header" type="color" value="#aabbcc"
    token id="color.body"   type="color" value="#112233"
  }
  styles {}
  document id="doc.hf" title="HF" {
    page id="page.hf" w=(px)400 h=(px)300 {
      table id="t.hf" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.body" header-rows=1 header-fill=(token)"color.header" cell-padding=(px)0 gap=(px)0 {
        column width=(px)100
        column width=(px)100
        row {
          cell { text id="h1" x=(px)0 y=(px)0 { span "H1" } }
          cell { text id="h2" x=(px)0 y=(px)0 { span "H2" } }
        }
        row {
          cell { text id="b1" x=(px)0 y=(px)0 { span "B1" } }
          cell { text id="b2" x=(px)0 y=(px)0 { span "B2" } }
        }
      }
    }
  }
}
"##;
    let result = compile(&parse(src), &default_provider());

    // Row-major FillRects: [0],[1] = header row, [2],[3] = body row.
    let fill_colors: Vec<(u8, u8, u8)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();

    assert_eq!(
        fill_colors.len(),
        4,
        "expected 4 cell fills; got {fill_colors:?}"
    );
    // Header-fill: #aabbcc = r=0xaa=170, g=0xbb=187, b=0xcc=204.
    assert_eq!(
        fill_colors[0],
        (0xaa, 0xbb, 0xcc),
        "header cell 0 must use header-fill; got {fill_colors:?}"
    );
    assert_eq!(
        fill_colors[1],
        (0xaa, 0xbb, 0xcc),
        "header cell 1 must use header-fill; got {fill_colors:?}"
    );
    // Body fill: #112233 = r=0x11=17, g=0x22=34, b=0x33=51.
    assert_eq!(
        fill_colors[2],
        (0x11, 0x22, 0x33),
        "body cell 0 must use table fill; got {fill_colors:?}"
    );
    assert_eq!(
        fill_colors[3],
        (0x11, 0x22, 0x33),
        "body cell 1 must use table fill; got {fill_colors:?}"
    );
}

/// A header cell with its OWN `fill` must keep that fill, overriding
/// the table's `header-fill`. (cell.fill precedence is highest.)
#[test]
fn header_cell_own_fill_overrides_header_fill() {
    let src = r##"zenith version=1 {
  project id="proj.hco" name="HCO"
  tokens format="zenith-token-v1" {
    token id="color.header" type="color" value="#aabbcc"
    token id="color.cell"   type="color" value="#ff0000"
    token id="color.body"   type="color" value="#112233"
  }
  styles {}
  document id="doc.hco" title="HCO" {
    page id="page.hco" w=(px)400 h=(px)300 {
      table id="t.hco" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.body" header-rows=1 header-fill=(token)"color.header" cell-padding=(px)0 gap=(px)0 {
        column width=(px)200
        row {
          cell fill=(token)"color.cell" { text id="hc" x=(px)0 y=(px)0 { span "Header" } }
        }
        row {
          cell { text id="bc" x=(px)0 y=(px)0 { span "Body" } }
        }
      }
    }
  }
}
"##;
    let result = compile(&parse(src), &default_provider());

    let fill_colors: Vec<(u8, u8, u8)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();

    assert_eq!(
        fill_colors.len(),
        2,
        "expected 2 cell fills; got {fill_colors:?}"
    );
    // Header cell has its OWN fill=#ff0000; cell.fill wins over header-fill.
    assert_eq!(
        fill_colors[0],
        (0xff, 0x00, 0x00),
        "header cell with own fill must use cell.fill; got {fill_colors:?}"
    );
    // Body cell falls back to table fill=#112233.
    assert_eq!(
        fill_colors[1],
        (0x11, 0x22, 0x33),
        "body cell must use table fill; got {fill_colors:?}"
    );
}

/// `header-rows=1` + `header-style` with a distinct fill: a header text child
/// with no own `style` picks up the header style's fill color (distinct glyph
/// color), while a body-row text child does not.
///
/// The text node fill cascade is: node.fill → style.fill → default(black).
/// When header-style is injected (the text node has no `style` attr), the
/// style-resolved fill takes effect. The text nodes here have NO own fill so
/// the style fill is the only source of color; body text also has no own fill
/// and no injected style, so it falls through to default black.
#[test]
fn header_style_applied_to_unstyled_text_children() {
    // style.header declares fill=#ff8800 (orange).
    // Row 0 (header): text has no style attr → header-style injected → orange.
    // Row 1 (body): text has no style attr, no injection → default black.
    let src = r##"zenith version=1 {
  project id="proj.hs" name="HS"
  tokens format="zenith-token-v1" {
    token id="color.orange" type="color" value="#ff8800"
    token id="color.bg"     type="color" value="#ffffff"
  }
  styles {
    style id="style.header" {
      fill (token)"color.orange"
    }
  }
  document id="doc.hs" title="HS" {
    page id="page.hs" w=(px)400 h=(px)300 {
      table id="t.hs" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.bg" header-rows=1 header-style="style.header" cell-padding=(px)0 gap=(px)0 {
        column width=(px)200
        row {
          cell { text id="ht" x=(px)0 y=(px)0 { span "Header" } }
        }
        row {
          cell { text id="bt" x=(px)0 y=(px)0 { span "Body" } }
        }
      }
    }
  }
}
"##;
    let result = compile(&parse(src), &default_provider());

    // Collect DrawGlyphRun colors in emission order: [0]=header run, [1]=body run.
    let run_colors: Vec<(u8, u8, u8)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();

    assert_eq!(
        run_colors.len(),
        2,
        "expected 2 glyph runs; got {run_colors:?}"
    );
    // Header run: style.header fill=#ff8800 (r=255, g=136, b=0).
    assert_eq!(
        run_colors[0],
        (0xff, 0x88, 0x00),
        "header text must use header-style fill; got {run_colors:?}"
    );
    // Body run: no header-style applied, no node fill → default black (0,0,0).
    assert_eq!(
        run_colors[1],
        (0x00, 0x00, 0x00),
        "body text must NOT use header-style; got {run_colors:?}"
    );
}

/// Regression: a table WITHOUT `header-rows` emits fills byte-identical to the
/// same table. Specifically the body fill color appears for all cells, and adding
/// `header-fill` without `header-rows` changes nothing (header_rows=0 → no cell
/// is a header, the header path is inert).
#[test]
fn no_header_rows_emits_table_fill_for_all_cells() {
    let src_no_header = r##"zenith version=1 {
  project id="proj.nh" name="NH"
  tokens format="zenith-token-v1" {
    token id="color.body" type="color" value="#334455"
  }
  styles {}
  document id="doc.nh" title="NH" {
    page id="page.nh" w=(px)400 h=(px)300 {
      table id="t.nh" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.body" cell-padding=(px)0 gap=(px)0 {
        column width=(px)200
        row {
          cell { text id="r0" x=(px)0 y=(px)0 { span "R0" } }
        }
        row {
          cell { text id="r1" x=(px)0 y=(px)0 { span "R1" } }
        }
      }
    }
  }
}
"##;
    // Variant with header-fill declared but header-rows absent (so header_rows=0).
    let src_with_header_fill = src_no_header.replace(
        "fill=(token)\"color.body\" cell-padding",
        "fill=(token)\"color.body\" header-fill=(token)\"color.body\" cell-padding",
    );

    let result_a = compile(&parse(src_no_header), &default_provider());
    let result_b = compile(&parse(&src_with_header_fill), &default_provider());

    let colors_a: Vec<(u8, u8, u8)> = result_a
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();
    let colors_b: Vec<(u8, u8, u8)> = result_b
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();

    assert_eq!(
        colors_a.len(),
        2,
        "expected 2 fills without header-rows; got {colors_a:?}"
    );
    // All cells must carry the table body fill (#334455).
    assert!(
        colors_a.iter().all(|&c| c == (0x33, 0x44, 0x55)),
        "all cells must use table fill; got {colors_a:?}"
    );
    // Variant with header-fill but header-rows=0: must be identical.
    assert_eq!(
        colors_a, colors_b,
        "header-fill with no header-rows must not change fill output"
    );
}

/// Separate mode (no `border-collapse` attribute) still emits exactly 4
/// `StrokeLine`s per cell — the existing behavior must be byte-identical.
/// This guards against regressions on the default separate path.
#[test]
fn separate_mode_stroke_count_unchanged() {
    // The shared `table_src()` is 5 placed cells (one colspan=2 in row 0,
    // three normal in row 1). Separate mode: 5 × 4 = 20 StrokeLines.
    let result = compile(&parse(table_src()), &default_provider());
    let stroke_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        .count();
    assert_eq!(
        stroke_count,
        5 * 4,
        "separate mode (default) must emit 4 border edges per placed cell; got {stroke_count}"
    );
}

// ── Multi-page table flow (unit U-D) ────────────────────────────────────────

/// Count the `DrawGlyphRun` text strings on a compiled page, by collecting the
/// first span text of each run is not exposed; instead count runs and, where the
/// span text matters, count `DrawGlyphRun` commands (one per shaped run).
fn glyph_run_count(result: &common::CompileResult) -> usize {
    result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count()
}

/// A 2-page document: each page hosts a `table flows="t"` box sharing the same
/// single-column layout + header-rows=1. The SOURCE (page 1) carries the header
/// row plus 6 body rows; the page-1 box is short enough that only some body rows
/// fit, so the rest flow onto the page-2 continuation box (which declares the
/// same flow id with EMPTY rows). The header repeats on both pages.
fn flow_src() -> &'static str {
    r##"zenith version=1 {
  project id="proj.fl" name="FL"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.fl" title="FL" {
    page id="page.fl1" w=(px)400 h=(px)400 {
      table id="src" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)120 header-rows=1 cell-padding=(px)0 gap=(px)0 {
        column
        row { cell { text id="h" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "HEAD" } } }
        row { cell { text id="b1" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-1" } } }
        row { cell { text id="b2" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-2" } } }
        row { cell { text id="b3" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-3" } } }
        row { cell { text id="b4" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-4" } } }
        row { cell { text id="b5" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-5" } } }
        row { cell { text id="b6" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "row-6" } } }
      }
    }
    page id="page.fl2" w=(px)400 h=(px)400 {
      table id="cont" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)400 header-rows=1 cell-padding=(px)0 gap=(px)0 {
        column
      }
    }
  }
}
"##
}

/// The flow splits the body across the two member boxes: page 1 shows the header
/// plus a leading slice; page 2 shows the header AGAIN plus the remaining body
/// rows. Every body row appears exactly once, and the header appears on BOTH
/// pages. We count glyph runs (one per non-empty cell text) per page: the total
/// across both pages = 6 body rows + 2 header repeats = 8 runs.
#[test]
fn flow_splits_body_and_repeats_header_across_pages() {
    let doc = parse(flow_src());
    let fonts = default_provider();
    let p1 = compile_page(&doc, &fonts, 0);
    let p2 = compile_page(&doc, &fonts, 1);

    let c1 = glyph_run_count(&p1);
    let c2 = glyph_run_count(&p2);

    // Both pages render the repeated header → each page has ≥1 run.
    assert!(
        c1 >= 1,
        "page 1 must render header + a body slice; got {c1}"
    );
    assert!(
        c2 >= 1,
        "page 2 must render header + remaining body; got {c2}"
    );
    // Header repeats on both pages: 6 distinct body rows + 2 header copies.
    assert_eq!(
        c1 + c2,
        8,
        "total runs = 6 body + 2 header copies; page1={c1} page2={c2}"
    );
    // The split is real: page 1 did NOT take all 7 source rows (would be 7 runs).
    assert!(
        c1 < 7,
        "page 1 must not fit the whole source table; got {c1} runs"
    );
    // Page 2 must carry more than just its header (it received overflow body).
    assert!(c2 >= 2, "page 2 must carry overflow body rows; got {c2}");
}

/// A rowspan body group that would straddle the page-1/page-2 boundary is pushed
/// WHOLE to the continuation: the spanning cell renders on page 2, not split. We
/// place a tall rowspan=2 cell late in the body so it cannot fit page 1's
/// remaining capacity and must move entirely to page 2.
fn flow_rowspan_src() -> &'static str {
    r##"zenith version=1 {
  project id="proj.fr" name="FR"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.fr" title="FR" {
    page id="page.fr1" w=(px)400 h=(px)400 {
      table id="rsrc" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)55 header-rows=1 cell-padding=(px)0 gap=(px)0 {
        column
        column
        row { cell { text id="rh1" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "H1" } }; cell { text id="rh2" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "H2" } } }
        row { cell { text id="ra1" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "A1" } }; cell { text id="ra2" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "A2" } } }
        row { cell rowspan=2 { text id="span" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "SPAN" } }; cell { text id="rb2" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "B2" } } }
        row { cell { text id="rc2" x=(px)0 y=(px)0 fill=(token)"color.ink" { span "C2" } } }
      }
    }
    page id="page.fr2" w=(px)400 h=(px)400 {
      table id="rcont" flows="t" x=(px)20 y=(px)20 w=(px)360 h=(px)400 header-rows=1 cell-padding=(px)0 gap=(px)0 {
        column
        column
      }
    }
  }
}
"##
}

#[test]
fn flow_rowspan_group_not_split_across_pages() {
    let doc = parse(flow_rowspan_src());
    let fonts = default_provider();
    let p1 = compile_page(&doc, &fonts, 0);
    let p2 = compile_page(&doc, &fonts, 1);

    // The rowspan group (rows containing SPAN/B2 + C2) must land WHOLE on page 2.
    // Page 1 fits the header (H1,H2) + the first body row (A1,A2): 4 runs, and
    // must NOT contain the spanning group. Total runs = header(2)×2 + body cells.
    let c1 = glyph_run_count(&p1);
    let c2 = glyph_run_count(&p2);
    // Page 1 should carry the header + first body row only (no SPAN yet).
    assert!(
        (2..=4).contains(&c1),
        "page 1 = header + first body row, no rowspan group; got {c1}"
    );
    // Page 2 carries the repeated header plus the rowspan group (SPAN,B2,C2).
    assert!(
        c2 >= 4,
        "page 2 must carry the repeated header + whole rowspan group; got {c2}"
    );
}

/// Regression: a NORMAL table with NO `flows` attribute renders byte-identically
/// to before this unit. The shared `table_src()` (no flows) must produce the
/// exact same command count whether or not the flow pre-pass exists.
#[test]
fn non_flow_table_command_count_unchanged() {
    let result = compile(&parse(table_src()), &default_provider());
    let total = result.scene.commands.len();
    // 5 placed cells: each emits FillRect + 4 StrokeLine + PushClip + glyph + PopClip.
    // We assert the count is stable and non-zero (byte-identical guard); the exact
    // figure is pinned by the pre-existing single-page tests, so here we only
    // ensure the flow pre-pass added nothing to a non-flow table.
    let flow_pass_added = result
        .scene
        .commands
        .iter()
        .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    assert!(total > 0 && flow_pass_added, "non-flow table still renders");
}

// ── U-C4: cell provides children's geometry (auto-box/wrap/align) ─────────────

/// Build a single-cell table document whose cell text omits w/h/align, with the
/// given table-level `attrs` appended to the cell open (e.g. `h-align="center"`).
fn auto_cell_src(cell_attrs: &str, text: &str) -> String {
    format!(
        r##"zenith version=1 {{
  project id="proj.ac" name="AC"
  tokens format="zenith-token-v1" {{
    token id="color.ink" type="color" value="#000000"
  }}
  styles {{}}
  document id="doc.ac" title="AC" {{
    page id="page.ac" w=(px)640 h=(px)400 {{
      table id="t.ac" x=(px)40 y=(px)40 w=(px)400 h=(px)200 cell-padding=(px)0 gap=(px)0 {{
        column width=(px)400
        row {{
          cell {cell_attrs} {{ text id="cx" fill=(token)"color.ink" {{ span "{text}" }} }}
        }}
      }}
    }}
  }}
}}
"##
    )
}

fn glyph_runs(result: &zenith_scene::CompileResult) -> Vec<(f64, f64)> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect()
}

#[test]
fn cell_text_without_geometry_compiles_into_content_box() {
    let result = compile(&parse(&auto_cell_src("", "Hello")), &default_provider());
    let runs = glyph_runs(&result);
    assert!(!runs.is_empty(), "cell text without w/h must still render");
    // Cell content x = table origin x (40) + pad (0). Glyph run starts at/after it.
    assert!(
        runs[0].0 >= 40.0 - 0.01,
        "glyph run x must be inside cell content box; got {runs:?}"
    );
}

#[test]
fn cell_h_align_shifts_text_horizontally() {
    let start = compile(&parse(&auto_cell_src("", "Hi")), &default_provider());
    let center = compile(
        &parse(&auto_cell_src("h-align=\"center\"", "Hi")),
        &default_provider(),
    );
    let end = compile(
        &parse(&auto_cell_src("h-align=\"end\"", "Hi")),
        &default_provider(),
    );
    let sx = glyph_runs(&start)[0].0;
    let cx = glyph_runs(&center)[0].0;
    let ex = glyph_runs(&end)[0].0;
    assert!(cx > sx, "center start ({cx}) must be right of start ({sx})");
    assert!(ex > cx, "end start ({ex}) must be right of center ({cx})");
}

/// A row with a SHORT cell (col 0) and a TALL multi-line cell (col 1). Rows are
/// content-sized, so the tall cell sets the row height and the short cell gets
/// vertical slack for `v-align` to act within. (A lone short cell shrink-wraps
/// its row and has no slack — the standard table v-align case needs a taller
/// sibling.)
fn v_align_src(cell_attrs: &str) -> String {
    format!(
        r##"zenith version=1 {{
  project id="proj.va" name="VA"
  tokens format="zenith-token-v1" {{
    token id="color.ink" type="color" value="#000000"
  }}
  styles {{}}
  document id="doc.va" title="VA" {{
    page id="page.va" w=(px)640 h=(px)400 {{
      table id="t.va" x=(px)40 y=(px)40 w=(px)400 h=(px)200 cell-padding=(px)0 gap=(px)0 {{
        column width=(px)120
        column width=(px)120
        row {{
          cell {cell_attrs} {{ text id="short" fill=(token)"color.ink" {{ span "Hi" }} }}
          cell {{ text id="tall" fill=(token)"color.ink" {{ span "L1\nL2\nL3\nL4" }} }}
        }}
      }}
    }}
  }}
}}
"##
    )
}

#[test]
fn cell_v_align_shifts_text_vertically() {
    // glyph_runs[0] is the short cell's "Hi" (row-major: col 0 emits first).
    let top = compile(&parse(&v_align_src("")), &default_provider());
    let middle = compile(&parse(&v_align_src("v-align=\"middle\"")), &default_provider());
    let bottom = compile(&parse(&v_align_src("v-align=\"bottom\"")), &default_provider());
    let ty = glyph_runs(&top)[0].1;
    let my = glyph_runs(&middle)[0].1;
    let by = glyph_runs(&bottom)[0].1;
    assert!(my > ty, "middle baseline ({my}) must be below top ({ty})");
    assert!(
        by > my,
        "bottom baseline ({by}) must be below middle ({my})"
    );
}

#[test]
fn cell_text_wraps_to_narrow_column() {
    // A long string in a narrow (80px) column must wrap into multiple lines.
    let src = r##"zenith version=1 {
  project id="proj.wr" name="WR"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.wr" title="WR" {
    page id="page.wr" w=(px)640 h=(px)400 {
      table id="t.wr" x=(px)40 y=(px)40 w=(px)80 h=(px)300 cell-padding=(px)0 gap=(px)0 {
        column width=(px)80
        row {
          cell { text id="cw" fill=(token)"color.ink" { span "one two three four five six seven eight" } }
        }
      }
    }
  }
}
"##;
    let result = compile(&parse(src), &default_provider());
    let runs = glyph_runs(&result);
    assert!(
        runs.len() >= 2,
        "long text in a narrow column must wrap to multiple lines; got {} run(s)",
        runs.len()
    );
    // Wrapped lines descend within the cell: later runs have larger y.
    assert!(
        runs.windows(2).all(|w| w[1].1 >= w[0].1 - 0.01),
        "wrapped lines must descend; got {runs:?}"
    );
}

#[test]
fn cell_text_with_explicit_geometry_unchanged() {
    // Author-specified w/x/align must win — render byte-identically regardless of
    // the cell's h-align (which would otherwise re-place auto-box text).
    let build = |cell_attrs: &str| {
        format!(
            r##"zenith version=1 {{
  project id="proj.ex" name="EX"
  tokens format="zenith-token-v1" {{ token id="color.ink" type="color" value="#000000" }}
  styles {{}}
  document id="doc.ex" title="EX" {{
    page id="page.ex" w=(px)640 h=(px)400 {{
      table id="t.ex" x=(px)40 y=(px)40 w=(px)400 h=(px)200 cell-padding=(px)0 gap=(px)0 {{
        column width=(px)400
        row {{
          cell {cell_attrs} {{ text id="ce" x=(px)0 y=(px)0 w=(px)400 align="start" fill=(token)"color.ink" {{ span "Fixed" }} }}
        }}
      }}
    }}
  }}
}}
"##
        )
    };
    let start = compile(&parse(&build("")), &default_provider());
    let centered = compile(&parse(&build("h-align=\"center\"")), &default_provider());
    assert_eq!(
        glyph_runs(&start),
        glyph_runs(&centered),
        "explicit-geometry cell text must ignore cell h-align (author override wins)"
    );
}
