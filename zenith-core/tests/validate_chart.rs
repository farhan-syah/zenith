//! Validation tests for the `chart` node.
//!
//! Covers valid chart kinds, `chart.invalid_kind` on bogus kinds, geometry
//! requirements, id-uniqueness participation, and the absent-chart additive
//! guarantee.

mod common;

use common::*;

/// A valid `bar` chart with geometry and series produces no chart-specific
/// diagnostics.
#[test]
fn chart_valid_bar_no_diagnostics() {
    // NOTE: '#' inside color hex requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.vb" name="ValidBar"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
  styles {
  }
  document id="doc.vb" title="ValidBar" {
    page id="page.vb" w=(px)800 h=(px)600 {
      chart id="c.bar" kind="bar" x=(px)50 y=(px)50 w=(px)600 h=(px)400 {
        series 10.0 20.0 30.0 label="A"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    let chart_diags: Vec<&str> = codes(&report)
        .into_iter()
        .filter(|c| c.starts_with("chart."))
        .collect();
    assert!(
        chart_diags.is_empty(),
        "valid bar chart must produce no chart.* diagnostics; got: {:?}",
        chart_diags
    );
}

/// A bogus `kind` value fires `chart.invalid_kind`.
#[test]
fn chart_invalid_kind_fires_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.ik" name="InvalidKind"
  styles {
  }
  document id="doc.ik" title="InvalidKind" {
    page id="page.ik" w=(px)800 h=(px)600 {
      chart id="c.bad" kind="histogram" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 2.0 label="X"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "chart.invalid_kind"),
        "bogus chart kind must fire chart.invalid_kind; got: {:?}",
        codes(&report)
    );
}

/// All five valid kind values ("bar", "line", "sparkline", "pie", "donut")
/// produce no `chart.invalid_kind`.
#[test]
fn chart_all_valid_kinds() {
    for kind in ["bar", "line", "sparkline", "pie", "donut"] {
        let src = format!(
            r##"zenith version=1 {{
  project id="proj.vk" name="ValidKind"
  styles {{
  }}
  document id="doc.vk" title="ValidKind" {{
    page id="page.vk" w=(px)800 h=(px)600 {{
      chart id="c.kind" kind="{kind}" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {{
        series 1.0 2.0 label="S"
      }}
    }}
  }}
}}
"##
        );
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let report = validate(&doc);
        assert!(
            !has_code(&report, "chart.invalid_kind"),
            "kind={kind:?} must not fire chart.invalid_kind; got: {:?}",
            codes(&report)
        );
    }
}

/// A chart whose id duplicates another node's id fires `id.duplicate` — proving
/// the chart participates in id-uniqueness checking.
#[test]
fn chart_duplicate_id_fires_id_duplicate() {
    let src = r##"zenith version=1 {
  project id="proj.dup" name="Dup"
  styles {
  }
  document id="doc.dup" title="Dup" {
    page id="page.dup" w=(px)800 h=(px)600 {
      rect id="node.dup" x=(px)0 y=(px)0 w=(px)10 h=(px)10
      chart id="node.dup" kind="bar" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 label="A"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "chart id duplicate must fire id.duplicate; got: {:?}",
        codes(&report)
    );
}

/// Valid `bar-mode` values ("grouped", "stacked") produce no `chart.invalid_bar_mode`.
#[test]
fn chart_valid_bar_mode_no_diagnostic() {
    for bar_mode in ["grouped", "stacked"] {
        let src = format!(
            r##"zenith version=1 {{
  project id="proj.bm" name="BarMode"
  styles {{
  }}
  document id="doc.bm" title="BarMode" {{
    page id="page.bm" w=(px)800 h=(px)600 {{
      chart id="c.bm" kind="bar" bar-mode="{bar_mode}" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {{
        series 1.0 2.0 label="S"
      }}
    }}
  }}
}}
"##
        );
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let report = validate(&doc);
        assert!(
            !has_code(&report, "chart.invalid_bar_mode"),
            "bar-mode={bar_mode:?} must not fire chart.invalid_bar_mode; got: {:?}",
            codes(&report)
        );
    }
}

/// A bogus `bar-mode` value fires `chart.invalid_bar_mode`.
#[test]
fn chart_invalid_bar_mode_fires_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.ibm" name="InvalidBarMode"
  styles {
  }
  document id="doc.ibm" title="InvalidBarMode" {
    page id="page.ibm" w=(px)800 h=(px)600 {
      chart id="c.ibm" kind="bar" bar-mode="clustered" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 2.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "chart.invalid_bar_mode"),
        "bogus bar-mode must fire chart.invalid_bar_mode; got: {:?}",
        codes(&report)
    );
}

/// Matching categories count and series data count produces no mismatch diagnostic.
#[test]
fn chart_matching_categories_no_mismatch() {
    let src = r##"zenith version=1 {
  project id="proj.mc" name="MatchCat"
  styles {
  }
  document id="doc.mc" title="MatchCat" {
    page id="page.mc" w=(px)800 h=(px)600 {
      chart id="c.mc" kind="bar" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        categories "Q1" "Q2" "Q3"
        series 10.0 20.0 30.0 label="S1"
        series 5.0 15.0 25.0 label="S2"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "chart.category_count_mismatch"),
        "matching categories must not fire chart.category_count_mismatch; got: {:?}",
        codes(&report)
    );
}

/// Mismatched categories count vs. series data count fires `chart.category_count_mismatch`.
#[test]
fn chart_category_count_mismatch_fires_diagnostic() {
    // categories has 3 labels but series has 2 data points.
    let src = r##"zenith version=1 {
  project id="proj.cm" name="CatMismatch"
  styles {
  }
  document id="doc.cm" title="CatMismatch" {
    page id="page.cm" w=(px)800 h=(px)600 {
      chart id="c.cm" kind="bar" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        categories "Q1" "Q2" "Q3"
        series 10.0 20.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "chart.category_count_mismatch"),
        "category/series count mismatch must fire chart.category_count_mismatch; got: {:?}",
        codes(&report)
    );
}

/// An empty `categories` (absent) produces no mismatch diagnostic even when
/// series lengths differ.
#[test]
fn chart_no_categories_no_mismatch() {
    let src = r##"zenith version=1 {
  project id="proj.nc2" name="NoCatMismatch"
  styles {
  }
  document id="doc.nc2" title="NoCatMismatch" {
    page id="page.nc2" w=(px)800 h=(px)600 {
      chart id="c.nc2" kind="bar" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 2.0 3.0 label="S1"
        series 10.0 20.0 label="S2"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "chart.category_count_mismatch"),
        "absent categories must not fire chart.category_count_mismatch; got: {:?}",
        codes(&report)
    );
}

/// Valid `point-placement` values ("edge", "center") produce no diagnostic.
#[test]
fn chart_valid_point_placement_no_diagnostic() {
    for value in ["edge", "center"] {
        let src = format!(
            r##"zenith version=1 {{
  project id="proj.pp" name="PointPlacement"
  styles {{
  }}
  document id="doc.pp" title="PointPlacement" {{
    page id="page.pp" w=(px)800 h=(px)600 {{
      chart id="c.pp" kind="line" point-placement="{value}" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {{
        series 1.0 2.0 label="S"
      }}
    }}
  }}
}}
"##
        );
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let report = validate(&doc);
        assert!(
            !has_code(&report, "chart.invalid_point_placement"),
            "point-placement={value:?} must not fire chart.invalid_point_placement; got: {:?}",
            codes(&report)
        );
    }
}

/// A bogus `point-placement` value fires `chart.invalid_point_placement`.
#[test]
fn chart_invalid_point_placement_fires_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.ipp" name="InvalidPointPlacement"
  styles {
  }
  document id="doc.ipp" title="InvalidPointPlacement" {
    page id="page.ipp" w=(px)800 h=(px)600 {
      chart id="c.ipp" kind="line" point-placement="left" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 2.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "chart.invalid_point_placement"),
        "bogus point-placement must fire chart.invalid_point_placement; got: {:?}",
        codes(&report)
    );
}

/// Valid `value-labels` values ("auto", "none", "top", "center") produce no diagnostic.
#[test]
fn chart_valid_value_labels_no_diagnostic() {
    for value in ["auto", "none", "top", "center"] {
        let src = format!(
            r##"zenith version=1 {{
  project id="proj.vl" name="ValueLabels"
  styles {{
  }}
  document id="doc.vl" title="ValueLabels" {{
    page id="page.vl" w=(px)800 h=(px)600 {{
      chart id="c.vl" kind="bar" value-labels="{value}" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {{
        series 1.0 2.0 label="S"
      }}
    }}
  }}
}}
"##
        );
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let report = validate(&doc);
        assert!(
            !has_code(&report, "chart.invalid_value_labels"),
            "value-labels={value:?} must not fire chart.invalid_value_labels; got: {:?}",
            codes(&report)
        );
    }
}

/// A bogus `value-labels` value fires `chart.invalid_value_labels`.
#[test]
fn chart_invalid_value_labels_fires_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.ivl" name="InvalidValueLabels"
  styles {
  }
  document id="doc.ivl" title="InvalidValueLabels" {
    page id="page.ivl" w=(px)800 h=(px)600 {
      chart id="c.ivl" kind="bar" value-labels="outside" x=(px)0 y=(px)0 w=(px)400 h=(px)300 {
        series 1.0 2.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    assert!(
        has_code(&report, "chart.invalid_value_labels"),
        "bogus value-labels must fire chart.invalid_value_labels; got: {:?}",
        codes(&report)
    );
}

/// A chart missing geometry (x/y/w/h absent) outside a flow parent fires
/// `node.missing_geometry`, proving that geometry validation runs on chart nodes.
#[test]
fn chart_missing_geometry_fires_geometry_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.mg" name="MissingGeom"
  styles {
  }
  document id="doc.mg" title="MissingGeom" {
    page id="page.mg" w=(px)800 h=(px)600 {
      chart id="c.nogeom" kind="line" {
        series 1.0 2.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    let geom_diags: Vec<&str> = codes(&report)
        .into_iter()
        .filter(|c| *c == "node.missing_geometry")
        .collect();
    assert!(
        !geom_diags.is_empty(),
        "chart missing geometry must fire node.missing_geometry; got: {:?}",
        codes(&report)
    );
}

/// `label-colors` token refs are counted as referenced — they must NOT produce
/// an "unused token" advisory when the tokens are declared. This mirrors the
/// series-color and value-color token ref collection tests above.
#[test]
fn chart_label_colors_token_refs_are_collected() {
    // NOTE: '#' in color hex requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.lct" name="LabelColorTokens"
  tokens format="zenith-token-v1" {
    token id="c.a" type="color" value="#aa0000"
    token id="c.b" type="color" value="#00aa00"
    token id="c.s" type="color" value="#0000aa"
  }
  styles {
  }
  document id="doc.lct" title="LabelColorTokens" {
    page id="page.lct" w=(px)800 h=(px)600 {
      chart id="c.lct" kind="pie" x=(px)0 y=(px)0 w=(px)400 h=(px)400 {
        label-colors (token)"c.a" (token)"c.b"
        series label="S" label-color=(token)"c.s" 60.0 40.0
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let report = validate(&doc);
    // All three token ids (c.a, c.b, c.s) must be referenced — no token.unused_token.
    let unused_diags: Vec<&str> = codes(&report)
        .into_iter()
        .filter(|c| c.starts_with("token."))
        .collect();
    assert!(
        unused_diags.is_empty(),
        "label-colors and series label-color tokens must be counted as referenced \
         (no token.* diagnostics); got: {:?}",
        unused_diags
    );
}
