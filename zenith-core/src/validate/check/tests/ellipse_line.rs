//! `ellipse_line` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ── Bonus: stroke-width with dimension token (correct type) ──────────

#[test]
fn stroke_width_with_dimension_token_is_clean() {
    let doc = doc_with(
        vec![dim_token("size.stroke")],
        vec![minimal_page(
            "page.one",
            vec![Node::Rect(Box::new(RectNode {
                shadow: None,
                filter: None,
                mask: None,
                id: "rect.one".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: Some(px(50.0)),
                h: Some(px(50.0)),
                radius: None,
                radius_tl: None,
                radius_tr: None,
                radius_br: None,
                radius_bl: None,
                style: None,
                fill: None,
                stroke: None,
                stroke_width: Some(token_ref("size.stroke")),
                stroke_alignment: None,
                stroke_dash: None,
                stroke_gap: None,
                stroke_linecap: None,
                border_top: None,
                border_bottom: None,
                border_left: None,
                border_right: None,
                border_width: None,
                stroke_outer: None,
                stroke_outer_width: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                anchor: None,
                anchor_zone: None,
                source_span: None,
                unknown_props: BTreeMap::new(),
            }))],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        codes(&report)
    );
}

// ── Bonus: font-family on text node ────────────────────────────────────

#[test]
fn text_font_family_with_font_family_token_is_clean() {
    let doc = doc_with(
        vec![font_family_token("font.body")],
        vec![minimal_page(
            "page.one",
            vec![Node::Text(Box::new(TextNode {
                shadow: None,
                filter: None,
                mask: None,
                id: "text.one".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: Some(px(200.0)),
                h: Some(px(40.0)),
                align: None,
                direction: None,
                overflow: None,
                overflow_wrap: None,
                style: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                contrast_bg: None,
                font_family: Some(token_ref("font.body")),
                font_size: None,
                font_size_min: None,
                font_weight: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                chain: None,
                drop_cap_lines: None,
                hyphenate: None,
                widow_orphan: None,
                tab_leader: None,
                text_exclusion: None,
                padding_left: None,
                text_indent: None,
                bullet: None,
                bullet_gap: None,
                anchor: None,
                anchor_zone: None,
                spans: vec![],
                source_span: None,
                unknown_props: BTreeMap::new(),
            }))],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        codes(&report)
    );
}

// ── Ellipse: clean doc produces no errors ─────────────────────────────

#[test]
fn ellipse_clean_doc_no_errors() {
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page(
            "page.one",
            vec![minimal_ellipse(
                "ellipse.one",
                Some(token_ref("color.fill")),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean ellipse doc, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── Ellipse: missing geometry → node.missing_geometry ─────────────────

#[test]
fn ellipse_missing_w_produces_node_missing_geometry() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Ellipse(EllipseNode {
                shadow: None,
                filter: None,
                mask: None,
                id: "ellipse.no-w".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: None, // missing
                h: Some(px(100.0)),
                rx: None,
                ry: None,
                style: None,
                fill: None,
                stroke: None,
                stroke_width: None,
                stroke_dash: None,
                stroke_gap: None,
                stroke_linecap: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                anchor: None,
                anchor_zone: None,
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

// ── Ellipse: raw literal fill → token.raw_visual_literal ──────────────

#[test]
fn ellipse_fill_raw_literal_produces_raw_visual_literal() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_ellipse(
                "ellipse.one",
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

// ── Ellipse: raw literal stroke → token.raw_visual_literal ────────────

#[test]
fn ellipse_stroke_raw_literal_produces_raw_visual_literal() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Ellipse(EllipseNode {
                shadow: None,
                filter: None,
                mask: None,
                id: "ellipse.stroke-lit".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: Some(px(100.0)),
                h: Some(px(100.0)),
                rx: None,
                ry: None,
                style: None,
                fill: None,
                stroke: Some(PropertyValue::Literal("#ff0000".to_owned())),
                stroke_width: None,
                stroke_dash: None,
                stroke_gap: None,
                stroke_linecap: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                anchor: None,
                anchor_zone: None,
                source_span: None,
                unknown_props: BTreeMap::new(),
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "ellipse with raw literal stroke must produce token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Line helpers ──────────────────────────────────────────────────────

fn minimal_line(id: &str, stroke: Option<PropertyValue>) -> Node {
    Node::Line(LineNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x1: Some(px(0.0)),
        y1: Some(px(0.0)),
        x2: Some(px(100.0)),
        y2: Some(px(0.0)),
        style: None,
        stroke,
        stroke_width: None,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
        opacity: None,
        visible: None,
        locked: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

// ── Line: clean doc produces no errors ───────────────────────────────

#[test]
fn line_clean_doc_no_errors() {
    let doc = doc_with(
        vec![color_token("color.rule")],
        vec![minimal_page(
            "page.one",
            vec![minimal_line("line.one", Some(token_ref("color.rule")))],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean line doc, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── Line: missing x1 → node.missing_geometry ─────────────────────────

#[test]
fn line_missing_x1_produces_node_missing_geometry() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Line(LineNode {
                id: "line.no-x1".to_owned(),
                name: None,
                role: None,
                x1: None, // missing
                y1: Some(px(0.0)),
                x2: Some(px(100.0)),
                y2: Some(px(0.0)),
                style: None,
                stroke: None,
                stroke_width: None,
                stroke_dash: None,
                stroke_gap: None,
                stroke_linecap: None,
                opacity: None,
                visible: None,
                locked: None,
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

// ── Line: stroke raw literal → token.raw_visual_literal ──────────────

#[test]
fn line_stroke_raw_literal_produces_raw_visual_literal() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_line(
                "line.one",
                Some(PropertyValue::Literal("#000000".to_owned())),
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

fn minimal_ellipse(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Ellipse(EllipseNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(0.0)),
        y: Some(px(0.0)),
        w: Some(px(100.0)),
        h: Some(px(100.0)),
        rx: None,
        ry: None,
        style: None,
        fill,
        stroke: None,
        stroke_width: None,
        stroke_dash: None,
        stroke_gap: None,
        stroke_linecap: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}
