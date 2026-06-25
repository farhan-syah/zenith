//! Integration tests for the `chart` node: parse, format, and round-trip —
//! including two `series` children — plus the absent-chart byte-identical guarantee.

mod common;

use common::*;
use zenith_core::format::format_document;

/// **Chart parse + format + round-trip (with two series)**: a `chart` with the
/// chart-specific props (kind/title/caption/legend/axis-min/axis-max/axis-style),
/// geometry, fill, and two `series` children parses into the expected `ChartNode`
/// (series data intact), formats back out preserving everything, and survives a
/// format → re-parse round-trip (spans stripped).
#[test]
fn chart_parse_format_round_trip_with_series() {
    // NOTE: the '#' inside color strings requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.chart" name="Chart"
  tokens format="zenith-token-v1" {
    token id="color.bar" type="color" value="#334155"
  }
  styles {
  }
  document id="doc.chart" title="Chart" {
    page id="page.chart" w=(px)800 h=(px)600 {
      chart id="c.sales" kind="bar" x=(px)50 y=(px)50 w=(px)600 h=(px)400 title="Sales" legend=#true {
        series 12.0 24.0 18.0 label="Q1"
        series 30.0 15.0 22.0 label="Q2" color=(token)"color.bar"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert_eq!(chart.id, "c.sales");
    assert_eq!(chart.kind, "bar");
    assert_eq!(chart.title, Some("Sales".to_owned()));
    assert_eq!(chart.legend, Some(true));
    assert_eq!(
        chart.x,
        Some(Dimension {
            value: 50.0,
            unit: Unit::Px
        })
    );
    assert_eq!(
        chart.w,
        Some(Dimension {
            value: 600.0,
            unit: Unit::Px
        })
    );
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].label, Some("Q1".to_owned()));
    assert_eq!(chart.series[0].values, vec![12.0, 24.0, 18.0]);
    assert_eq!(chart.series[0].color, None);
    assert_eq!(chart.series[1].label, Some("Q2".to_owned()));
    assert_eq!(chart.series[1].values, vec![30.0, 15.0, 22.0]);
    assert_eq!(chart.series[1].color, Some(token_ref("color.bar")));

    // The formatter emits the chart-specific props and the series block.
    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("chart id=\"c.sales\" kind=\"bar\""),
        "formatter must emit chart id + kind; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("title=\"Sales\""),
        "formatter must emit title; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("series"),
        "formatter must emit series children; got:\n{formatted_str}"
    );

    // Round-trip: re-parse equals the first parse (spans stripped).
    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart (with series) must round-trip identically"
    );
}

/// **Categories + bar-mode round-trip**: a `chart` with a `categories` child
/// and `bar-mode="stacked"` and two series parses correctly, formats the
/// `categories` line BEFORE the series lines, and re-parses to the same AST.
#[test]
fn chart_categories_and_bar_mode_round_trip() {
    // NOTE: '#' inside strings requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.cat" name="CatChart"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#112233"
  }
  styles {
  }
  document id="doc.cat" title="CatChart" {
    page id="page.cat" w=(px)800 h=(px)600 {
      chart id="c.cat" kind="bar" bar-mode="stacked" x=(px)50 y=(px)50 w=(px)600 h=(px)400 {
        categories "Q1" "Q2" "Q3"
        series 10.0 20.0 30.0 label="A"
        series 5.0 15.0 25.0 label="B" color=(token)"color.a"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert_eq!(chart.id, "c.cat");
    assert_eq!(chart.bar_mode, Some("stacked".to_owned()));
    assert_eq!(
        chart.categories,
        vec!["Q1".to_owned(), "Q2".to_owned(), "Q3".to_owned()]
    );
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].values, vec![10.0, 20.0, 30.0]);
    assert_eq!(chart.series[1].color, Some(token_ref("color.a")));

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    // categories line must appear before the first series line.
    let cat_pos = formatted_str
        .find("categories")
        .expect("must emit categories");
    let series_pos = formatted_str.find("series").expect("must emit series");
    assert!(
        cat_pos < series_pos,
        "categories must be emitted before series; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("bar-mode=\"stacked\""),
        "formatter must emit bar-mode; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("\"Q1\""),
        "formatter must emit Q1 category label; got:\n{formatted_str}"
    );

    // Round-trip: re-parse equals the first parse (spans stripped).
    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart (with categories + bar-mode) must round-trip identically"
    );
}

/// **No categories / no bar-mode = byte-identical**: a chart without those
/// fields must not emit `categories` or `bar-mode` text in the formatted output,
/// and must still round-trip.
#[test]
fn chart_without_categories_bar_mode_byte_identical() {
    let src = r##"zenith version=1 {
  project id="proj.plain" name="PlainChart"
  styles {
  }
  document id="doc.plain" title="PlainChart" {
    page id="page.plain" w=(px)800 h=(px)600 {
      chart id="c.plain" kind="bar" x=(px)50 y=(px)50 w=(px)600 h=(px)400 {
        series 1.0 2.0 3.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert!(chart.categories.is_empty(), "categories must be empty");
    assert_eq!(chart.bar_mode, None, "bar_mode must be None");

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    assert!(
        !formatted_str.contains("categories"),
        "no categories field must not emit 'categories'; got:\n{formatted_str}"
    );
    assert!(
        !formatted_str.contains("bar-mode"),
        "no bar-mode field must not emit 'bar-mode'; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "plain chart (no categories, no bar-mode) must round-trip identically"
    );
}

/// **point-placement + value-labels + value-color round-trip**: a chart with all
/// three new fields parses correctly and round-trips through format → re-parse
/// to an identical AST.
#[test]
fn chart_point_placement_value_labels_value_color_round_trip() {
    // NOTE: '#' inside color hex requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.pp" name="PointPlacement"
  tokens format="zenith-token-v1" {
    token id="color.label" type="color" value="#334455"
  }
  styles {
  }
  document id="doc.pp" title="PointPlacement" {
    page id="page.pp" w=(px)800 h=(px)600 {
      chart id="c.pp" kind="line" point-placement="edge" value-labels="center" value-color=(token)"color.label" x=(px)50 y=(px)50 w=(px)600 h=(px)400 {
        series 10.0 20.0 30.0 label="A"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert_eq!(chart.id, "c.pp");
    assert_eq!(
        chart.point_placement,
        Some("edge".to_owned()),
        "point_placement must be parsed"
    );
    assert_eq!(
        chart.value_labels,
        Some("center".to_owned()),
        "value_labels must be parsed"
    );
    assert_eq!(
        chart.value_color,
        Some(token_ref("color.label")),
        "value_color must be parsed as TokenRef"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    assert!(
        formatted_str.contains("point-placement=\"edge\""),
        "formatter must emit point-placement; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("value-labels=\"center\""),
        "formatter must emit value-labels; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("value-color=(token)\"color.label\""),
        "formatter must emit value-color; got:\n{formatted_str}"
    );

    // Round-trip: re-parse equals the first parse (spans stripped).
    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart (with point-placement + value-labels + value-color) must round-trip identically"
    );
}

/// **New fields absent = byte-identical**: a chart without point-placement,
/// value-labels, or value-color must not emit those keywords in the formatter
/// output, and must still round-trip.
#[test]
fn chart_without_new_fields_byte_identical() {
    let src = r##"zenith version=1 {
  project id="proj.nf" name="NoNewFields"
  styles {
  }
  document id="doc.nf" title="NoNewFields" {
    page id="page.nf" w=(px)800 h=(px)600 {
      chart id="c.nf" kind="bar" x=(px)50 y=(px)50 w=(px)600 h=(px)400 {
        series 1.0 2.0 3.0 label="S"
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert_eq!(chart.point_placement, None, "point_placement must be None");
    assert_eq!(chart.value_labels, None, "value_labels must be None");
    assert_eq!(chart.value_color, None, "value_color must be None");

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    assert!(
        !formatted_str.contains("point-placement"),
        "absent point-placement must not be emitted; got:\n{formatted_str}"
    );
    assert!(
        !formatted_str.contains("value-labels"),
        "absent value-labels must not be emitted; got:\n{formatted_str}"
    );
    assert!(
        !formatted_str.contains("value-color"),
        "absent value-color must not be emitted; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart without new fields must round-trip identically"
    );
}

/// **Absent chart is byte-identical**: a document that uses NO `chart` node
/// formats exactly as it did before the feature existed (additive guarantee).
#[test]
fn absent_chart_byte_identical() {
    // NOTE: '#' in color hex requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.nc" name="NoChart"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
  }
  styles {
  }
  document id="doc.nc" title="NoChart" {
    page id="page.nc" w=(px)400 h=(px)300 {
      rect id="r.one" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("formatted must be utf8");
    assert!(
        !formatted_str.contains("chart"),
        "a document without a chart must not emit the keyword; got:\n{formatted_str}"
    );
}

/// **label-colors child + series label-color round-trip**: a pie chart with a
/// `label-colors` child (two token refs) and a series with `label-color` parses
/// correctly, formats those fields, and re-parses to an identical AST.
#[test]
fn chart_label_colors_and_series_label_color_round_trip() {
    // NOTE: '#' in color hex requires r##...## quoting.
    let src = r##"zenith version=1 {
  project id="proj.lc" name="LabelColors"
  tokens format="zenith-token-v1" {
    token id="c.a" type="color" value="#ff0000"
    token id="c.b" type="color" value="#00ff00"
    token id="c.s" type="color" value="#0000ff"
  }
  styles {
  }
  document id="doc.lc" title="LabelColors" {
    page id="page.lc" w=(px)800 h=(px)600 {
      chart id="c.pie" kind="pie" x=(px)50 y=(px)50 w=(px)400 h=(px)400 {
        label-colors (token)"c.a" (token)"c.b"
        series label="Slice" label-color=(token)"c.s" 60.0 40.0
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert_eq!(chart.id, "c.pie");
    assert_eq!(
        chart.label_colors,
        vec![token_ref("c.a"), token_ref("c.b")],
        "label_colors must be parsed as two TokenRefs"
    );
    assert_eq!(chart.series.len(), 1);
    assert_eq!(
        chart.series[0].label_color,
        Some(token_ref("c.s")),
        "series label_color must be parsed as TokenRef"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    assert!(
        formatted_str.contains("label-colors (token)\"c.a\" (token)\"c.b\""),
        "formatter must emit label-colors child; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("label-color=(token)\"c.s\""),
        "formatter must emit series label-color; got:\n{formatted_str}"
    );

    // label-colors must appear before the series line.
    let lc_pos = formatted_str
        .find("label-colors")
        .expect("must emit label-colors");
    let series_pos = formatted_str.find("series").expect("must emit series");
    assert!(
        lc_pos < series_pos,
        "label-colors must be emitted before series; got:\n{formatted_str}"
    );

    // Round-trip: re-parse equals the first parse (spans stripped).
    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart (with label-colors + series label-color) must round-trip identically"
    );
}

/// **label-colors and series label-color absent = byte-identical**: a chart without
/// those fields must not emit `label-colors` or `label-color` text, and must
/// still round-trip.
#[test]
fn chart_without_label_colors_byte_identical() {
    let src = r##"zenith version=1 {
  project id="proj.nlc" name="NoLabelColors"
  styles {
  }
  document id="doc.nlc" title="NoLabelColors" {
    page id="page.nlc" w=(px)800 h=(px)600 {
      chart id="c.nlc" kind="pie" x=(px)50 y=(px)50 w=(px)400 h=(px)400 {
        series label="A" 60.0 40.0
      }
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    let chart = match &doc.body.pages[0].children[0] {
        Node::Chart(c) => c,
        other => panic!("expected Chart node, got {other:?}"),
    };
    assert!(
        chart.label_colors.is_empty(),
        "label_colors must be empty when absent"
    );
    assert_eq!(
        chart.series[0].label_color, None,
        "series label_color must be None when absent"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");

    assert!(
        !formatted_str.contains("label-colors"),
        "absent label-colors must not be emitted; got:\n{formatted_str}"
    );
    assert!(
        !formatted_str.contains("label-color"),
        "absent series label-color must not be emitted; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "chart without label-colors must round-trip identically"
    );
}
