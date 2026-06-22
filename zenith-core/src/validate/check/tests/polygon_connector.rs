//! `polygon_connector` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ══════════════════════════════════════════════════════════════════════
// Polygon / Polyline validation tests
// ══════════════════════════════════════════════════════════════════════

fn tri_points() -> Vec<Point> {
    vec![
        Point {
            x: Some(px(160.0)),
            y: Some(px(40.0)),
        },
        Point {
            x: Some(px(260.0)),
            y: Some(px(170.0)),
        },
        Point {
            x: Some(px(60.0)),
            y: Some(px(170.0)),
        },
    ]
}

fn minimal_polygon(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Polygon(PolygonNode {
        id: id.to_owned(),
        name: None,
        role: None,
        fill,
        stroke: None,
        stroke_width: None,
        stroke_alignment: None,
        fill_rule: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        style: None,
        points: tri_points(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

// ── polygon: clean doc with token fill → no errors ────────────────────

#[test]
fn polygon_clean_no_errors() {
    let doc = doc_with(
        vec![
            color_token("color.fill"),
            color_token("color.stroke"),
            dim_token("size.stroke"),
        ],
        vec![minimal_page(
            "page.one",
            vec![Node::Polygon(PolygonNode {
                id: "poly.tri".to_owned(),
                name: None,
                role: None,
                fill: Some(token_ref("color.fill")),
                stroke: Some(token_ref("color.stroke")),
                stroke_width: Some(token_ref("size.stroke")),
                stroke_alignment: None,
                fill_rule: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: tri_points(),
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean polygon, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── polygon: only 2 points → shape.insufficient_points (Error) ───────

#[test]
fn polygon_too_few_points_insufficient() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Polygon(PolygonNode {
                id: "poly.bad".to_owned(),
                name: None,
                role: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                stroke_alignment: None,
                fill_rule: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: vec![
                    Point {
                        x: Some(px(0.0)),
                        y: Some(px(0.0)),
                    },
                    Point {
                        x: Some(px(100.0)),
                        y: Some(px(0.0)),
                    },
                ],
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "shape.insufficient_points"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── polyline: only 1 point → shape.insufficient_points (Error) ───────

#[test]
fn polyline_too_few_points_insufficient() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Polyline(PolylineNode {
                id: "line.bad".to_owned(),
                name: None,
                role: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                fill_rule: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: vec![Point {
                    x: Some(px(0.0)),
                    y: Some(px(0.0)),
                }],
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "shape.insufficient_points"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── shape: unknown kind → shape.unknown_kind (Warning) ────────────────

#[test]
fn shape_invalid_kind_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_shape("s.bad", Some("bogus"), None)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "shape.unknown_kind"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn shape_valid_kind_does_not_warn() {
    for kind in ["process", "decision", "terminator", "ellipse"] {
        let doc = doc_with(
            vec![],
            vec![minimal_page(
                "page.one",
                vec![minimal_shape("s.ok", Some(kind), None)],
            )],
        );
        let report = validate(&doc);
        assert!(
            !has_code(&report, "shape.unknown_kind"),
            "kind {kind:?} must not warn; codes: {:?}",
            codes(&report)
        );
    }
}

// ── shape: invalid h-align → shape.invalid_h_align (Warning) ───────────

#[test]
fn shape_invalid_h_align_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_shape("s.bad", Some("process"), Some("sideways"))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "shape.invalid_h_align"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn shape_valid_h_align_does_not_warn() {
    for h in ["start", "center", "end"] {
        let doc = doc_with(
            vec![],
            vec![minimal_page(
                "page.one",
                vec![minimal_shape("s.ok", Some("process"), Some(h))],
            )],
        );
        let report = validate(&doc);
        assert!(
            !has_code(&report, "shape.invalid_h_align"),
            "h-align {h:?} must not warn; codes: {:?}",
            codes(&report)
        );
    }
}

// ── connector validation ──────────────────────────────────────────────────────

/// Parameters for [`make_connector`], bundled to keep the helper's arity small
/// (and to satisfy the workspace no-`#[allow]` rule on over-long arg lists).
#[derive(Clone, Copy)]
struct ConnectorSpec<'a> {
    id: &'a str,
    from: Option<&'a str>,
    to: Option<&'a str>,
    route: Option<&'a str>,
    marker_end: Option<&'a str>,
    from_anchor: Option<&'a str>,
}

/// A bare connector with caller-supplied `from`/`to` (and optional enum attrs)
/// for driving the validate-time diagnostic paths.
fn make_connector(spec: ConnectorSpec) -> Node {
    Node::Connector(Box::new(ConnectorNode {
        id: spec.id.to_owned(),
        name: None,
        role: None,
        from: spec.from.map(str::to_owned),
        to: spec.to.map(str::to_owned),
        from_anchor: spec.from_anchor.map(str::to_owned),
        to_anchor: None,
        route: spec.route.map(str::to_owned),
        marker_start: None,
        marker_end: spec.marker_end.map(str::to_owned),
        stroke: None,
        stroke_width: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        style: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

#[test]
fn connector_unknown_target_warns() {
    // `to="ghost"` names no node id → connector.unknown_target.
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: Some("ghost"),
                    route: None,
                    marker_end: None,
                    from_anchor: None,
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "connector.unknown_target"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn connector_valid_targets_do_not_warn_unknown() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                minimal_rect("b", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: Some("b"),
                    route: None,
                    marker_end: None,
                    from_anchor: None,
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "connector.unknown_target"),
        "valid from/to must not warn; codes: {:?}",
        codes(&report)
    );
    assert!(
        !has_code(&report, "connector.missing_target"),
        "both endpoints present must not warn missing; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn connector_missing_target_warns() {
    // `to` absent → connector.missing_target.
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: None,
                    route: None,
                    marker_end: None,
                    from_anchor: None,
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "connector.missing_target"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn connector_invalid_route_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                minimal_rect("b", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: Some("b"),
                    route: Some("zigzag"),
                    marker_end: None,
                    from_anchor: None,
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "connector.invalid_route"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn connector_valid_route_does_not_warn() {
    for route in ["straight", "orthogonal"] {
        let doc = doc_with(
            vec![],
            vec![minimal_page(
                "page.one",
                vec![
                    minimal_rect("a", None),
                    minimal_rect("b", None),
                    make_connector(ConnectorSpec {
                        id: "c1",
                        from: Some("a"),
                        to: Some("b"),
                        route: Some(route),
                        marker_end: None,
                        from_anchor: None,
                    }),
                ],
            )],
        );
        let report = validate(&doc);
        assert!(
            !has_code(&report, "connector.invalid_route"),
            "route {route:?} must not warn; codes: {:?}",
            codes(&report)
        );
    }
}

#[test]
fn connector_invalid_marker_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                minimal_rect("b", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: Some("b"),
                    route: None,
                    marker_end: Some("diamond"),
                    from_anchor: None,
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "connector.invalid_marker"),
        "codes: {:?}",
        codes(&report)
    );
}

#[test]
fn connector_invalid_anchor_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("a", None),
                minimal_rect("b", None),
                make_connector(ConnectorSpec {
                    id: "c1",
                    from: Some("a"),
                    to: Some("b"),
                    route: None,
                    marker_end: None,
                    from_anchor: Some("sideways"),
                }),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "connector.invalid_anchor"),
        "codes: {:?}",
        codes(&report)
    );
}

// ── polygon: point with missing y → node.missing_geometry ─────────────

#[test]
fn polygon_point_missing_coord() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Polygon(PolygonNode {
                id: "poly.missy".to_owned(),
                name: None,
                role: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                stroke_alignment: None,
                fill_rule: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: vec![
                    Point {
                        x: Some(px(0.0)),
                        y: None,
                    }, // missing y
                    Point {
                        x: Some(px(100.0)),
                        y: Some(px(0.0)),
                    },
                    Point {
                        x: Some(px(50.0)),
                        y: Some(px(100.0)),
                    },
                ],
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.missing_geometry"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── polygon: fill raw literal → token.raw_visual_literal ─────────────

#[test]
fn polygon_fill_raw_literal() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_polygon(
                "poly.lit",
                Some(PropertyValue::Literal("#ff0000".to_owned())),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── text: literal font-size dimension → token.raw_visual_literal ─────

/// A literal `font-size=(px)24` (a `PropertyValue::Dimension`, not a token)
/// must be treated as a raw visual literal — the same advisory a literal
/// color receives. It still resolves at compile time; validate just flags it.
#[test]
fn text_literal_font_size_dimension_is_raw_visual_literal() {
    let font_size = Some(PropertyValue::Dimension(px(24.0)));
    let text = match minimal_text("text.lfs", Some(token_ref("color.fill"))) {
        Node::Text(mut t) => {
            t.font_size = font_size;
            Node::Text(t)
        }
        other => other,
    };
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page("page.one", vec![text])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "a literal font-size dimension must flag token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
}

/// A literal `font-size-min="12"` (a `PropertyValue::Dimension`, not a token)
/// must be flagged as a raw visual literal, exactly like `font-size`.
#[test]
fn text_literal_font_size_min_dimension_is_raw_visual_literal() {
    let text = match minimal_text("text.lfsm", Some(token_ref("color.fill"))) {
        Node::Text(mut t) => {
            t.font_size_min = Some(PropertyValue::Dimension(px(12.0)));
            Node::Text(t)
        }
        other => other,
    };
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page("page.one", vec![text])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "a literal font-size-min dimension must flag token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
}

// ── polygon: unknown fill-rule warns ──────────────────────────────────

#[test]
fn polygon_unknown_fill_rule_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Polygon(PolygonNode {
                id: "poly.fr".to_owned(),
                name: None,
                role: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                stroke_alignment: None,
                fill_rule: Some("oddeven".to_owned()), // wrong spelling
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: tri_points(),
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "expected node.unknown_property warning for bad fill-rule; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_property")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ── polygon: invalid stroke-alignment warns; valid does not ───────────

#[test]
fn polygon_invalid_stroke_alignment_warns() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.sa",
            vec![Node::Polygon(PolygonNode {
                id: "poly.sa".to_owned(),
                name: None,
                role: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                stroke_alignment: Some("middle".to_owned()), // invalid
                fill_rule: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                style: None,
                points: tri_points(),
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_property")
        .expect("expected node.unknown_property warning for bad stroke-alignment");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(
        diag.message.contains("stroke-alignment"),
        "message must mention stroke-alignment; got: {}",
        diag.message
    );
    assert!(!report.has_errors());
}

#[test]
fn polygon_valid_stroke_alignment_no_warn() {
    for value in ["inside", "center", "outside"] {
        let doc = doc_with(
            vec![],
            vec![minimal_page(
                "page.sa",
                vec![Node::Polygon(PolygonNode {
                    id: "poly.sa".to_owned(),
                    name: None,
                    role: None,
                    fill: None,
                    stroke: None,
                    stroke_width: None,
                    stroke_alignment: Some(value.to_owned()),
                    fill_rule: None,
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
                    style: None,
                    points: tri_points(),
                    source_span: None,
                    unknown_props: BTreeMap::new(),
                })],
            )],
        );
        let report = validate(&doc);
        assert!(
            !report.diagnostics.iter().any(
                |d| d.code == "node.unknown_property" && d.message.contains("stroke-alignment")
            ),
            "valid stroke-alignment '{value}' must not warn; codes: {:?}",
            codes(&report)
        );
    }
}

/// A geometry-complete `shape` with one label span. Enum-valued attributes
/// (`kind`, `h_align`) are caller-supplied so tests can drive the enum-warning
/// paths in `compile`/`validate`.
fn minimal_shape(id: &str, kind: Option<&str>, h_align: Option<&str>) -> Node {
    Node::Shape(Box::new(ShapeNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(0.0)),
        y: Some(px(0.0)),
        w: Some(px(200.0)),
        h: Some(px(120.0)),
        kind: kind.map(str::to_owned),
        fill: None,
        stroke: None,
        stroke_width: None,
        radius: None,
        stroke_alignment: None,
        padding: None,
        h_align: h_align.map(str::to_owned),
        v_align: None,
        text_style: None,
        spans: vec![TextSpan {
            text: "Label".to_owned(),
            fill: None,
            font_weight: None,
            italic: None,
            underline: None,
            strikethrough: None,
            vertical_align: None,
            footnote_ref: None,
        }],
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
