//! `styles` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

use std::collections::BTreeMap;

use super::common::*;

// ── Style validation tests ─────────────────────────────────────────────

fn doc_with_styles(tokens: Vec<Token>, styles: Vec<Style>, pages: Vec<Page>) -> Document {
    Document {
        version: 1,
        colorspace: None,
        doc_id: None,
        mirror_margins: None,
        facing_pages: None,
        spread_gutter: None,
        page_progression: None,
        page_parity_start: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        project: None,
        assets: AssetBlock::default(),
        libraries: Vec::new(),
        actions: Vec::new(),
        tokens: TokenBlock {
            format: "zenith-token-v1".to_owned(),
            tokens,
        },
        styles: StyleBlock {
            styles,
            source_span: None,
        },
        components: Vec::new(),
        masters: Vec::new(),
        sections: Vec::new(),
        provenance: Vec::new(),
        body: DocumentBody {
            id: "doc.main".to_owned(),
            title: None,
            pages,
        },
    }
}

fn style_with_props(id: &str, props: Vec<(&str, PropertyValue)>) -> Style {
    Style {
        id: id.to_owned(),
        properties: props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        unknown_props: BTreeMap::new(),
        source_span: None,
    }
}

/// A node that references a non-declared style id → `style.unknown_reference` error.
#[test]
fn node_unknown_style_reference() {
    let rect = match minimal_rect("rect.one", None) {
        Node::Rect(mut r) => {
            r.style = Some("style.missing".to_owned());
            Node::Rect(r)
        }
        other => other,
    };
    let doc = doc_with_styles(
        vec![],
        vec![], // no styles declared
        vec![minimal_page("page.one", vec![rect])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "style.unknown_reference"),
        "expected style.unknown_reference; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// A clean `code` node referencing a declared color token passes validation.
#[test]
fn clean_code_node_no_errors() {
    let doc = doc_with(
        vec![color_token("color.fg")],
        vec![minimal_page(
            "page.one",
            vec![minimal_code("code.one", Some(token_ref("color.fg")))],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

/// A `code` node referencing a non-declared style id → `style.unknown_reference`.
#[test]
fn code_node_unknown_style_reference() {
    let code = match minimal_code("code.one", None) {
        Node::Code(mut c) => {
            c.style = Some("style.missing".to_owned());
            Node::Code(c)
        }
        other => other,
    };
    let doc = doc_with_styles(
        vec![],
        vec![], // no styles declared
        vec![minimal_page("page.one", vec![code])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "style.unknown_reference"),
        "expected style.unknown_reference; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// An unknown property on a `code` node → `node.unknown_property` warning.
#[test]
fn code_node_unknown_property_warns() {
    let code = match minimal_code("code.one", None) {
        Node::Code(mut c) => {
            c.unknown_props.insert(
                "future-prop".to_owned(),
                crate::ast::UnknownProperty {
                    value: crate::ast::UnknownValue::String("x".to_owned()),
                    ty: None,
                },
            );
            Node::Code(c)
        }
        other => other,
    };
    let doc = doc_with(vec![], vec![minimal_page("page.one", vec![code])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "expected node.unknown_property; codes: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

/// A style property that references a missing token → `token.unknown_reference` error.
#[test]
fn style_prop_unknown_token() {
    let style = style_with_props(
        "style.s",
        vec![("fill", PropertyValue::TokenRef("color.missing".to_owned()))],
    );
    let doc = doc_with_styles(
        vec![], // no tokens declared
        vec![style],
        vec![minimal_page("page.one", vec![])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unknown_reference"),
        "expected token.unknown_reference; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// A style property with a raw literal → `token.raw_visual_literal` error.
#[test]
fn style_raw_literal_fill() {
    let style = style_with_props(
        "style.s",
        vec![("fill", PropertyValue::Literal("#ff0000".to_owned()))],
    );
    let doc = doc_with_styles(vec![], vec![style], vec![minimal_page("page.one", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "expected token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// A style `padding` with a raw literal dimension → `token.raw_visual_literal`.
///
/// `padding` is a token-only visual dimension prop, identical to `font-size` /
/// `stroke-width`: a raw `(px)N` literal (a `PropertyValue::Dimension`, not a
/// token) MUST be flagged, never silently accepted.
#[test]
fn style_padding_raw_literal_rejected() {
    let style = style_with_props(
        "style.flow",
        vec![("padding", PropertyValue::Dimension(px(16.0)))],
    );
    let doc = doc_with_styles(vec![], vec![style], vec![minimal_page("page.one", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "a raw-literal padding must flag token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// A style `gap` with a raw literal dimension → `token.raw_visual_literal`.
#[test]
fn style_gap_raw_literal_rejected() {
    let style = style_with_props(
        "style.flow",
        vec![("gap", PropertyValue::Dimension(px(8.0)))],
    );
    let doc = doc_with_styles(vec![], vec![style], vec![minimal_page("page.one", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "a raw-literal gap must flag token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// Unknown style property children → `style.unknown_property` warning.
#[test]
fn style_unknown_property_warns() {
    let style = Style {
        id: "style.s".to_owned(),
        properties: BTreeMap::new(),
        unknown_props: {
            let mut m = BTreeMap::new();
            m.insert(
                "bogus-prop".to_owned(),
                UnknownStyleProp {
                    raw: "whatever".to_owned(),
                },
            );
            m
        },
        source_span: None,
    };
    let doc = doc_with_styles(vec![], vec![style], vec![minimal_page("page.one", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "style.unknown_property"),
        "expected style.unknown_property warning; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "style.unknown_property")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(
        !report.has_errors(),
        "unknown prop must only warn, not error"
    );
}

/// A token referenced ONLY by a style (not by any node) must NOT be flagged `token.unused`.
#[test]
fn token_used_only_by_style_not_unused() {
    let style = style_with_props(
        "style.s",
        vec![("fill", PropertyValue::TokenRef("color.used".to_owned()))],
    );
    let doc = doc_with_styles(
        vec![color_token("color.used")],
        vec![style],
        // No nodes reference color.used — only the style does.
        vec![minimal_page("page.one", vec![])],
    );
    let report = validate(&doc);
    // Should NOT contain token.unused for color.used.
    let unused: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "token.unused")
        .collect();
    assert!(
        unused.is_empty(),
        "token referenced by style must not be flagged token.unused; codes: {:?}",
        codes(&report)
    );
}

fn minimal_code(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Code(CodeNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(0.0)),
        y: Some(px(0.0)),
        w: Some(px(200.0)),
        h: Some(px(80.0)),
        overflow: None,
        language: None,
        line_numbers: None,
        tab_width: None,
        style: None,
        fill,
        font_family: None,
        font_size: None,
        font_weight: None,
        syntax_theme: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        anchor: None,
        anchor_zone: None,
        content: String::new(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}
