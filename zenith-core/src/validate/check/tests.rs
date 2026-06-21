//! Document-level validation tests.
//!
//! Moved verbatim from the former single-file `validate/check.rs`. The body of
//! every test is unchanged; only the surrounding module/import scaffolding was
//! adjusted for the new directory layout.

use std::collections::BTreeMap;

use super::*;
use crate::ast::asset::{AssetBlock, AssetDecl, AssetKind};
use crate::ast::document::{
    Document, DocumentBody, Fold, MasterDef, Page, SafeZone, SafeZoneType, SectionDef,
};
use crate::ast::library::LibraryDef;
use crate::ast::node::ImageNode;
use crate::ast::node::{
    CodeNode, ConnectorNode, EllipseNode, FieldNode, FrameNode, GroupNode, LineNode, Node,
    RectNode, ShapeNode, TextNode, TextSpan, TocNode, UnknownNode,
};
use crate::ast::style::StyleBlock;
use crate::ast::token::{Token, TokenBlock, TokenLiteral, TokenType, TokenValue};
use crate::ast::value::{Dimension, PropertyValue, Unit};
use crate::diagnostics::Severity;

// ── Builder helpers ───────────────────────────────────────────────────

fn color_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String("#112233".to_owned())),
        source_span: None,
    }
}

fn dim_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Dimension,
        value: TokenValue::Literal(TokenLiteral::Dimension(Dimension {
            value: 12.0,
            unit: Unit::Px,
        })),
        source_span: None,
    }
}

fn font_family_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::FontFamily,
        value: TokenValue::Literal(TokenLiteral::String("Inter".to_owned())),
        source_span: None,
    }
}

fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

fn token_ref(id: &str) -> PropertyValue {
    PropertyValue::TokenRef(id.to_owned())
}

fn minimal_rect(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Rect(Box::new(RectNode {
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
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill,
        stroke: None,
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

fn minimal_text(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Text(Box::new(TextNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
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
        fill,
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: None,
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
        spans: vec![],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
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
        content: String::new(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

fn minimal_page(id: &str, children: Vec<Node>) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(1280.0),
        height: px(720.0),
        background: None,
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        children,
        source_span: None,
    }
}

fn doc_with(tokens: Vec<Token>, pages: Vec<Page>) -> Document {
    Document {
        version: 1,
        colorspace: None,
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
        styles: StyleBlock::default(),
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

fn has_code(report: &ValidationReport, code: &str) -> bool {
    report.diagnostics.iter().any(|d| d.code == code)
}

fn codes(report: &ValidationReport) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

// ── Test 1: clean minimal doc has no errors ───────────────────────────

#[test]
fn clean_doc_no_errors() {
    // A page with a rect and a text, both using a color token for fill.
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("rect.one", Some(token_ref("color.fill"))),
                minimal_text("text.one", Some(token_ref("color.fill"))),
            ],
        )],
    );
    let report = validate(&doc);
    // The token is used twice; no unused advisory either.
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── Test 2: duplicate id across two nodes ─────────────────────────────

#[test]
fn duplicate_node_id_produces_id_duplicate() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("node.dup", None),
                minimal_rect("node.dup", None),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Test 3: rect missing w ────────────────────────────────────────────

#[test]
fn rect_missing_w_produces_node_missing_geometry() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Rect(Box::new(RectNode {
                shadow: None,
                filter: None,
                mask: None,
                id: "rect.no-w".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: None, // missing
                h: Some(px(100.0)),
                radius: None,
                radius_tl: None,
                radius_tr: None,
                radius_br: None,
                radius_bl: None,
                style: None,
                fill: None,
                stroke: None,
                stroke_width: None,
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
                source_span: None,
                unknown_props: BTreeMap::new(),
            }))],
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

// ── Test 4: fill referencing a missing token ──────────────────────────

#[test]
fn fill_with_missing_token_ref_produces_unknown_reference() {
    let doc = doc_with(
        vec![], // no tokens defined
        vec![minimal_page(
            "page.one",
            vec![minimal_rect(
                "rect.one",
                Some(token_ref("color.does.not.exist")),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unknown_reference"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Test 4b: font-weight referencing a missing token ──────────────────

#[test]
fn font_weight_with_missing_token_ref_produces_unknown_reference() {
    let text = Node::Text(Box::new(TextNode {
        shadow: None,
        filter: None,
        mask: None,
        id: "text.fw".to_owned(),
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
        font_family: None,
        font_size: None,
        font_size_min: None,
        font_weight: Some(token_ref("weight.does.not.exist")),
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
        spans: vec![],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let doc = doc_with(vec![], vec![minimal_page("page.one", vec![text])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unknown_reference"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Test 5: fill referencing a fontFamily token (wrong type) ──────────

#[test]
fn fill_with_wrong_type_token_produces_incompatible_property() {
    let doc = doc_with(
        vec![font_family_token("font.body")],
        vec![minimal_page(
            "page.one",
            vec![minimal_rect("rect.one", Some(token_ref("font.body")))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.incompatible_property"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Test 6: fill="#ff0000" raw literal → raw_visual_literal ──────────

#[test]
fn fill_raw_literal_produces_raw_visual_literal() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_rect(
                "rect.one",
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

// ── Test 7: unknown node kind → node.unknown_kind (Warning) ──────────

#[test]
fn unknown_node_kind_produces_warning_not_error() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Unknown(Box::new(UnknownNode {
                kind: "sparkle".to_owned(),
                id: None,
                unknown_props: std::collections::BTreeMap::new(),
                children: Vec::new(),
                source_span: None,
            }))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_kind"),
        "codes: {:?}",
        codes(&report)
    );
    // Must NOT be an error.
    assert!(
        !report.has_errors(),
        "unknown_kind should be Warning, not Error. codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_kind")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
}

/// Build an unknown node with the given id and children (no unknown props).
fn unknown_node(kind: &str, id: Option<&str>, children: Vec<Node>) -> Node {
    Node::Unknown(Box::new(UnknownNode {
        kind: kind.to_owned(),
        id: id.map(str::to_owned),
        unknown_props: BTreeMap::new(),
        children,
        source_span: None,
    }))
}

/// **Unknown node id participates in duplicate-id detection**: an unknown node
/// whose `id` collides with another node's id must trigger `id.duplicate`,
/// proving the unknown node's id is registered like a known node's.
#[test]
fn unknown_node_id_participates_in_duplicate_detection() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("node.dup", None),
                unknown_node("sparkle", Some("node.dup"), vec![]),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "unknown node id must participate in duplicate detection. codes: {:?}",
        codes(&report)
    );
    // It still warns about the unknown kind.
    assert!(
        has_code(&report, "node.unknown_kind"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

/// **Known child of an unknown node is still validated**: a `rect` with a bad
/// token ref declared inside an unknown parent must still produce its
/// `token.unknown_reference` diagnostic, proving child recursion descends into
/// known children of unknown nodes.
#[test]
fn known_child_of_unknown_node_is_validated() {
    let doc = doc_with(
        vec![], // no tokens defined → the ref is dangling
        vec![minimal_page(
            "page.one",
            vec![unknown_node(
                "sparkle",
                Some("fx"),
                vec![minimal_rect(
                    "inner.rect",
                    Some(token_ref("color.does.not.exist")),
                )],
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unknown_reference"),
        "a known child's bad token ref must be validated. codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Test 8: defined-but-unreferenced token → token.unused (Advisory) ─

#[test]
fn unused_token_produces_advisory() {
    // Define two color tokens; only reference one of them.
    let doc = doc_with(
        vec![color_token("color.used"), color_token("color.unused")],
        vec![minimal_page(
            "page.one",
            vec![minimal_rect("rect.one", Some(token_ref("color.used")))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unused"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "token.unused")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Advisory);
    // Advisory only — no errors.
    assert!(
        !report.has_errors(),
        "should not be error, codes: {:?}",
        codes(&report)
    );
    // The unused subject should be the unreferenced token.
    assert_eq!(diag.subject_id.as_deref(), Some("color.unused"));
}

// ── Bonus: duplicate id between token and node ────────────────────────

#[test]
fn duplicate_id_token_vs_node() {
    // Token id collides with node id.
    let doc = doc_with(
        vec![color_token("shared.id")],
        vec![minimal_page(
            "page.one",
            vec![minimal_rect("shared.id", Some(token_ref("shared.id")))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Bonus: page with unknown unit on width ────────────────────────────

#[test]
fn page_unknown_unit_produces_invalid_geometry() {
    let doc = doc_with(
        vec![],
        vec![Page {
            id: "page.bad".to_owned(),
            name: None,
            width: Dimension {
                value: 1280.0,
                unit: Unit::Unknown("em".to_owned()),
            },
            height: px(720.0),
            background: None,
            bleed: None,
            margin_inner: None,
            margin_outer: None,
            margin_top: None,
            margin_bottom: None,
            baseline_grid: None,
            parity: None,
            master: None,
            safe_zones: Vec::new(),
            folds: Vec::new(),
            children: vec![],
            source_span: None,
        }],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.invalid_geometry"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Page dimensions must be strictly positive ────────────────────────

/// Build a single-page document whose page has the given width/height dims.
fn doc_with_page_dims(width: Dimension, height: Dimension) -> Document {
    let mut page = minimal_page("page.dim", vec![]);
    page.width = width;
    page.height = height;
    doc_with(vec![], vec![page])
}

#[test]
fn page_zero_width_produces_out_of_range() {
    let report = validate(&doc_with_page_dims(px(0.0), px(720.0)));
    assert!(
        has_code(&report, "value.out_of_range"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn page_negative_height_produces_out_of_range() {
    let report = validate(&doc_with_page_dims(px(1280.0), px(-100.0)));
    assert!(
        has_code(&report, "value.out_of_range"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn page_positive_dims_no_out_of_range() {
    let report = validate(&doc_with_page_dims(px(1280.0), px(720.0)));
    assert!(
        !has_code(&report, "value.out_of_range"),
        "positive dims must not trip value.out_of_range; codes: {:?}",
        codes(&report)
    );
}

// ── A document must contain at least one page ────────────────────────

#[test]
fn empty_document_produces_no_pages_error() {
    let report = validate(&doc_with(vec![], vec![]));
    assert!(
        has_code(&report, "document.no_pages"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Bonus: node with unknown property → node.unknown_property ─────────

#[test]
fn unknown_property_on_rect_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "magic-glow".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("true".to_owned()),
            ty: None,
        },
    );
    let doc = doc_with(
        vec![],
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
                stroke_width: None,
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
                source_span: None,
                unknown_props,
            }))],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ── Group helpers ─────────────────────────────────────────────────────

fn minimal_group(id: &str, children: Vec<Node>) -> Node {
    Node::Group(GroupNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x: None,
        y: None,
        w: None,
        h: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        children,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

// ── Group: no required geometry — clean group has no errors ──────────

#[test]
fn group_with_children_no_errors() {
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page(
            "page.one",
            vec![minimal_group(
                "group.one",
                vec![minimal_rect("rect.inner", Some(token_ref("color.fill")))],
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean group doc, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── Group: nested id duplicate with page sibling → id.duplicate ──────

#[test]
fn group_nested_id_duplicate_with_page_sibling() {
    // Page has a rect "shared" and a group containing another node "shared".
    // The walk must share seen_ids across page-level and group-children,
    // so the second "shared" triggers id.duplicate.
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("shared", None),
                minimal_group("group.one", vec![minimal_rect("shared", None)]),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Group: child with missing geometry surfaces → node.missing_geometry

#[test]
fn group_child_missing_geometry_surfaces() {
    // A rect nested inside a group has no `x` property; walk_node must
    // recurse into group children and report the missing geometry.
    let child_rect = Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: "rect.inner".to_owned(),
        name: None,
        role: None,
        x: None, // missing — triggers node.missing_geometry
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
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_group("group.one", vec![child_rect])],
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

// ── Group: unknown property → node.unknown_property (Warning) ─────────

#[test]
fn group_unknown_property_warns() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "future-blend".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("multiply".to_owned()),
            ty: None,
        },
    );
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Group(GroupNode {
                id: "group.one".to_owned(),
                name: None,
                role: None,
                x: None,
                y: None,
                w: None,
                h: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                style: None,
                children: vec![],
                source_span: None,
                unknown_props,
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ── Frame helpers ─────────────────────────────────────────────────────

fn minimal_frame(id: &str, x: f64, y: f64, w: f64, h: f64, children: Vec<Node>) -> Node {
    Node::Frame(FrameNode {
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        layout: None,
        columns: None,
        rows: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        children,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

// ── Frame: clean doc with valid frame + child rect → no diagnostics ───

#[test]
fn frame_clean_doc_no_errors() {
    // Child rect sits fully inside the frame box (40,40,120,100), so neither
    // off_canvas nor frame.child_overflow fire.
    let inner = Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: "rect.inner".to_owned(),
        name: None,
        role: None,
        x: Some(px(50.0)),
        y: Some(px(50.0)),
        w: Some(px(40.0)),
        h: Some(px(40.0)),
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: Some(token_ref("color.fill")),
        stroke: None,
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let doc = doc_with(
        vec![color_token("color.fill")],
        vec![minimal_page(
            "page.one",
            vec![minimal_frame(
                "frame.clip",
                40.0,
                40.0,
                120.0,
                100.0,
                vec![inner],
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean frame doc, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── Frame: missing x → node.missing_geometry ──────────────────────────

#[test]
fn frame_missing_x_produces_node_missing_geometry() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Frame(FrameNode {
                id: "frame.nox".to_owned(),
                name: None,
                role: None,
                x: None, // missing
                y: Some(px(0.0)),
                w: Some(px(100.0)),
                h: Some(px(100.0)),
                layout: None,
                columns: None,
                rows: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                style: None,
                children: vec![],
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

// ── Frame: missing h → node.missing_geometry ──────────────────────────

#[test]
fn frame_missing_h_produces_node_missing_geometry() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Frame(FrameNode {
                id: "frame.noh".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: Some(px(100.0)),
                h: None, // missing
                layout: None,
                columns: None,
                rows: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                style: None,
                children: vec![],
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

// ── Frame: child rect with no x → node.missing_geometry (recursion) ───

#[test]
fn frame_child_missing_geometry_surfaces() {
    // A rect nested inside a frame has no `x`; walk_node must recurse
    // into frame children and report the missing geometry.
    let child_rect = Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: "rect.inner".to_owned(),
        name: None,
        role: None,
        x: None, // missing
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
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![minimal_frame(
                "frame.clip",
                0.0,
                0.0,
                100.0,
                100.0,
                vec![child_rect],
            )],
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

// ── Frame: child overflow advisories ──────────────────────────────────

/// A frame child whose `x + w` exceeds the frame's right edge → advisory
/// `frame.child_overflow`.
#[test]
fn frame_child_overflowing_right_edge_advises() {
    // Frame box: x=40 y=40 w=120 h=100 → right edge at 160.
    // Child rect: x=100 w=100 → right edge at 200 > 160 → protrudes.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            1000.0,
            1000.0,
            vec![minimal_frame(
                "frame.clip",
                40.0,
                40.0,
                120.0,
                100.0,
                vec![rect_at("rect.over", 100.0, 50.0, 100.0, 40.0)],
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "frame.child_overflow"),
        "expected frame.child_overflow; codes: {:?}",
        codes(&report)
    );
}

/// A frame child fully inside the frame box → no overflow advisory.
#[test]
fn frame_child_fully_inside_is_clean() {
    // Frame box: x=40 y=40 w=120 h=100. Child rect fully inside.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            1000.0,
            1000.0,
            vec![minimal_frame(
                "frame.clip",
                40.0,
                40.0,
                120.0,
                100.0,
                vec![rect_at("rect.in", 50.0, 50.0, 40.0, 40.0)],
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "frame.child_overflow"),
        "inside child must not overflow; codes: {:?}",
        codes(&report)
    );
}

/// A flow-frame child with no explicit geometry → no overflow advisory
/// (node_bbox is None, so the child is naturally skipped).
#[test]
fn flow_frame_child_without_geometry_is_skipped() {
    let child_rect = Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: "rect.flow".to_owned(),
        name: None,
        role: None,
        x: None,
        y: None,
        w: None,
        h: None,
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: None,
        stroke: None,
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let flow_frame = Node::Frame(FrameNode {
        id: "frame.flow".to_owned(),
        name: None,
        role: None,
        x: Some(px(40.0)),
        y: Some(px(40.0)),
        w: Some(px(120.0)),
        h: Some(px(100.0)),
        layout: Some("flow".to_owned()),
        columns: None,
        rows: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        children: vec![child_rect],
        source_span: None,
        unknown_props: BTreeMap::new(),
    });
    let doc = doc_with(
        vec![],
        vec![bounded_page("page.one", 1000.0, 1000.0, vec![flow_frame])],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "frame.child_overflow"),
        "flow child without geometry must be skipped; codes: {:?}",
        codes(&report)
    );
}

// ── Frame: nested id duplicate with page sibling → id.duplicate ───────

#[test]
fn frame_nested_id_duplicate_with_page_sibling() {
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![
                minimal_rect("shared", None),
                minimal_frame(
                    "frame.clip",
                    0.0,
                    0.0,
                    100.0,
                    100.0,
                    vec![minimal_rect("shared", None)],
                ),
            ],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── Frame: unknown property → node.unknown_property (Warning) ─────────

#[test]
fn frame_unknown_property_warns() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "future-scroll".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::Bool(true),
            ty: None,
        },
    );
    let doc = doc_with(
        vec![],
        vec![minimal_page(
            "page.one",
            vec![Node::Frame(FrameNode {
                id: "frame.one".to_owned(),
                name: None,
                role: None,
                x: Some(px(0.0)),
                y: Some(px(0.0)),
                w: Some(px(100.0)),
                h: Some(px(100.0)),
                layout: None,
                columns: None,
                rows: None,
                opacity: None,
                visible: None,
                locked: None,
                rotate: None,
                blend_mode: None,
                blur: None,
                style: None,
                children: vec![],
                source_span: None,
                unknown_props,
            })],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.unknown_property"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "node.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

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

// ══════════════════════════════════════════════════════════════════════
// Asset validation tests
// ══════════════════════════════════════════════════════════════════════

/// Build a Document that has an AssetBlock but no content nodes.
fn doc_with_assets(assets: Vec<AssetDecl>) -> Document {
    Document {
        version: 1,
        colorspace: None,
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
        assets: AssetBlock {
            assets,
            source_span: None,
        },
        libraries: Vec::new(),
        actions: Vec::new(),
        tokens: TokenBlock {
            format: "zenith-token-v1".to_owned(),
            tokens: vec![],
        },
        styles: StyleBlock::default(),
        components: Vec::new(),
        masters: Vec::new(),
        sections: Vec::new(),
        provenance: Vec::new(),
        body: DocumentBody {
            id: "doc.asset-test".to_owned(),
            title: None,
            // A valid document needs ≥1 page; these asset tests don't care about
            // page content, so use a single minimal page.
            pages: vec![minimal_page("page.one", vec![])],
        },
    }
}

fn image_asset(id: &str, src: &str) -> AssetDecl {
    AssetDecl {
        id: id.to_owned(),
        kind: AssetKind::Image,
        src: src.to_owned(),
        sha256: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

// ── asset.clean: a well-formed assets block produces no diagnostics ───

#[test]
fn asset_clean_block_no_diagnostics() {
    let doc = doc_with_assets(vec![
        AssetDecl {
            id: "asset.logo".to_owned(),
            kind: AssetKind::Svg,
            src: "assets/logo.svg".to_owned(),
            sha256: Some("deadbeef".to_owned()),
            source_span: None,
            unknown_props: BTreeMap::new(),
        },
        image_asset("asset.hero", "assets/hero.png"),
    ]);
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean asset block, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── asset.duplicate_id: duplicate asset id → id.duplicate ────────────

#[test]
fn asset_duplicate_id_produces_id_duplicate() {
    let doc = doc_with_assets(vec![
        image_asset("asset.dup", "a.png"),
        image_asset("asset.dup", "b.png"),
    ]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.cross_type_duplicate: asset id clashes with token id ────────

#[test]
fn asset_id_clashes_with_token_id_produces_id_duplicate() {
    let mut doc = doc_with(vec![color_token("shared.id")], vec![]);
    doc.assets = AssetBlock {
        assets: vec![image_asset("shared.id", "img.png")],
        source_span: None,
    };
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.invalid_kind: unknown kind → asset.invalid_kind (Error) ─────

#[test]
fn asset_unknown_kind_produces_invalid_kind() {
    let doc = doc_with_assets(vec![AssetDecl {
        id: "asset.movie".to_owned(),
        kind: AssetKind::Unknown("movie".to_owned()),
        src: "clips/intro.mp4".to_owned(),
        sha256: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.invalid_kind"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.invalid_src: absolute path → asset.invalid_src (Error) ──────

#[test]
fn asset_absolute_src_unix_produces_invalid_src() {
    let doc = doc_with_assets(vec![image_asset("asset.abs", "/etc/x.png")]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.invalid_src"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.invalid_src: parent traversal → asset.invalid_src (Error) ───

#[test]
fn asset_parent_traversal_src_produces_invalid_src() {
    let doc = doc_with_assets(vec![image_asset("asset.trav", "../x.png")]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.invalid_src"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.invalid_src: URL → asset.invalid_src (Error) ────────────────

#[test]
fn asset_url_src_produces_invalid_src() {
    let doc = doc_with_assets(vec![image_asset("asset.url", "https://example.com/x.png")]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.invalid_src"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── asset.unknown_property: unknown prop → asset.unknown_property ─────

#[test]
fn asset_unknown_property_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "dpi".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::Integer(96),
            ty: None,
        },
    );
    let doc = doc_with_assets(vec![AssetDecl {
        id: "asset.hi-res".to_owned(),
        kind: AssetKind::Image,
        src: "img/hi.png".to_owned(),
        sha256: None,
        source_span: None,
        unknown_props,
    }]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.unknown_property"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "asset.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ══════════════════════════════════════════════════════════════════════
// Image node validation tests
// ══════════════════════════════════════════════════════════════════════

/// Build a Document with an assets block and a single page of nodes.
fn doc_with_assets_and_nodes(assets: Vec<AssetDecl>, children: Vec<Node>) -> Document {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", children)]);
    doc.assets = AssetBlock {
        assets,
        source_span: None,
    };
    doc
}

fn full_image(id: &str, asset: &str, fit: Option<&str>) -> ImageNode {
    ImageNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        asset: asset.to_owned(),
        x: Some(px(40.0)),
        y: Some(px(40.0)),
        w: Some(px(160.0)),
        h: Some(px(120.0)),
        src_x: None,
        src_y: None,
        src_w: None,
        src_h: None,
        fit: fit.map(str::to_owned),
        clip: None,
        clip_radius: None,
        object_position_x: None,
        object_position_y: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

// ── image.clean: well-formed image with declared asset → no errors ────

#[test]
fn image_clean_no_errors() {
    let doc = doc_with_assets_and_nodes(
        vec![image_asset("asset.swatch", "assets/swatch.png")],
        vec![Node::Image(full_image(
            "img.swatch",
            "asset.swatch",
            Some("contain"),
        ))],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics for clean image doc, got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

// ── image.missing_x → node.missing_geometry ───────────────────────────

#[test]
fn image_missing_x_node_missing_geometry() {
    let mut img = full_image("img.nox", "asset.swatch", None);
    img.x = None;
    let doc = doc_with_assets_and_nodes(
        vec![image_asset("asset.swatch", "assets/swatch.png")],
        vec![Node::Image(img)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "node.missing_geometry"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── image referencing an undeclared asset → asset.unknown_reference ───

#[test]
fn image_unknown_asset_reference() {
    let doc = doc_with_assets_and_nodes(
        vec![image_asset("asset.swatch", "assets/swatch.png")],
        vec![Node::Image(full_image(
            "img.x",
            "asset.does-not-exist",
            None,
        ))],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "asset.unknown_reference"),
        "codes: {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

// ── image with an unknown fit → image.invalid_fit (Warning) ───────────

#[test]
fn image_invalid_fit_warns() {
    let doc = doc_with_assets_and_nodes(
        vec![image_asset("asset.swatch", "assets/swatch.png")],
        vec![Node::Image(full_image(
            "img.squish",
            "asset.swatch",
            Some("squish"),
        ))],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "image.invalid_fit"),
        "codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "image.invalid_fit")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    // invalid_fit is forward-compat: a Warning, not an Error.
    assert!(!report.has_errors());
}

// ══════════════════════════════════════════════════════════════════════
// Polygon / Polyline validation tests
// ══════════════════════════════════════════════════════════════════════

use crate::ast::node::{Point, PolygonNode, PolylineNode};

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

/// A bare connector with caller-supplied `from`/`to` (and optional enum attrs)
/// for driving the validate-time diagnostic paths.
#[allow(clippy::too_many_arguments)]
fn make_connector(
    id: &str,
    from: Option<&str>,
    to: Option<&str>,
    route: Option<&str>,
    marker_end: Option<&str>,
    from_anchor: Option<&str>,
) -> Node {
    Node::Connector(Box::new(ConnectorNode {
        id: id.to_owned(),
        name: None,
        role: None,
        from: from.map(str::to_owned),
        to: to.map(str::to_owned),
        from_anchor: from_anchor.map(str::to_owned),
        to_anchor: None,
        route: route.map(str::to_owned),
        marker_start: None,
        marker_end: marker_end.map(str::to_owned),
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
                make_connector("c1", Some("a"), Some("ghost"), None, None, None),
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
                make_connector("c1", Some("a"), Some("b"), None, None, None),
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
                make_connector("c1", Some("a"), None, None, None, None),
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
                make_connector("c1", Some("a"), Some("b"), Some("zigzag"), None, None),
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
                    make_connector("c1", Some("a"), Some("b"), Some(route), None, None),
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
                make_connector("c1", Some("a"), Some("b"), None, Some("diamond"), None),
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
                make_connector("c1", Some("a"), Some("b"), None, None, Some("sideways")),
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

// ── Style validation tests ─────────────────────────────────────────────

use crate::ast::style::{Style, UnknownStyleProp};

fn doc_with_styles(tokens: Vec<Token>, styles: Vec<Style>, pages: Vec<Page>) -> Document {
    Document {
        version: 1,
        colorspace: None,
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

// ══════════════════════════════════════════════════════════════════════
// off_canvas advisory tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a page with a given width/height (px) and children.
fn bounded_page(id: &str, w: f64, h: f64, children: Vec<Node>) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(w),
        height: px(h),
        background: None,
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        children,
        source_span: None,
    }
}

/// Helper: rect at (x, y, w, h) in px, no fill.
fn rect_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
    Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: None,
        stroke: None,
        stroke_width: None,
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
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// A rect with x=-20 on a 100×100 page → off_canvas advisory.
#[test]
fn rect_negative_x_is_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.out", -20.0, 0.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "expected off_canvas advisory; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert_eq!(diag.subject_id.as_deref(), Some("rect.out"));
    // off_canvas is advisory only — no errors.
    assert!(!report.has_errors());
}

/// A rect fully inside the page → NO off_canvas advisory.
#[test]
fn rect_fully_inside_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.in", 10.0, 10.0, 80.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "rect fully inside should NOT get off_canvas; codes: {:?}",
        codes(&report)
    );
}

/// A rect at x=80, w=40 (right edge=120 > page_w=100) → off_canvas.
#[test]
fn rect_overflowing_right_edge_is_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.wide", 80.0, 0.0, 40.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "rect extending past right edge should be off_canvas; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert!(!report.has_errors());
}

/// A rect exactly touching the page edges (x=0,y=0,w=100,h=100) → no off_canvas.
#[test]
fn rect_exactly_on_page_edge_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.one",
            100.0,
            100.0,
            vec![rect_at("rect.edge", 0.0, 0.0, 100.0, 100.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "rect exactly on page boundary should NOT be off_canvas; codes: {:?}",
        codes(&report)
    );
}

/// Helper: rect at (x, y, w, h) in px with an optional rotation in degrees.
fn rect_at_rotated(id: &str, x: f64, y: f64, w: f64, h: f64, rotate_deg: Option<f64>) -> Node {
    let rotate = rotate_deg.map(|deg| Dimension {
        value: deg,
        unit: Unit::Deg,
    });
    Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: None,
        stroke: None,
        stroke_width: None,
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
        rotate,
        blend_mode: None,
        blur: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// A rect centered on the page, small enough that its authored bbox is fully
/// inside, but rotated 45° so its rotated AABB extends beyond the page edge →
/// off_canvas advisory fires.
///
/// Page: 100×100. Rect: x=40, y=40, w=20, h=20 (authored bbox fully inside,
/// center at (50,50)). At 45° the AABB half-extents are
/// hw_rot = (10+10)*cos(45°) ≈ 14.14, so AABB = (35.86..64.14, 35.86..64.14)
/// which is still inside. Use a more extreme rect: x=35, y=35, w=30, h=30,
/// center (50,50). AABB half-extent = (15+15)/sqrt(2)*sqrt(2) = 15*sqrt(2) ≈
/// 21.2. AABB: (28.8..71.2, 28.8..71.2) — still inside.
///
/// Simpler: rect x=0, y=40, w=80, h=20 centered at (40, 50). At 45° the
/// AABB half-extents: x-half = |40*cos45 - 10*sin45| + ... use the standard
/// formula: hw = (|w/2|*|cos| + |h/2|*|sin|) = 40*cos45 + 10*sin45 ≈ 35.36.
/// hh = 40*sin45 + 10*cos45 ≈ 35.36. AABB: (40-35.36, 50-35.36) = (4.64, 14.64)
/// to (75.36, 85.36) — still inside. Need larger rect.
///
/// Rect x=0, y=0, w=80, h=20, center (40, 10). At 45°: hw=40*cos45+10*sin45≈35.36,
/// hh=40*sin45+10*cos45≈35.36. AABB: (4.64-35.36=-30.72, ...) → off_canvas!
#[test]
fn rotated_aabb_off_canvas_fires() {
    // A wide, short rect in the top-left: authored bbox inside the page, but
    // its 45° rotated AABB extends outside.
    // Page: 200×200. Rect: x=0, y=0, w=160, h=20 → center (80, 10).
    // At 45°: hw = 80*cos45 + 10*sin45 ≈ 63.6, hh = 80*sin45 + 10*cos45 ≈ 63.6.
    // AABB: (80-63.6, 10-63.6) = (16.4, -53.6) → ay < 0 → off_canvas.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.rot",
            200.0,
            200.0,
            vec![rect_at_rotated(
                "rect.rot",
                0.0,
                0.0,
                160.0,
                20.0,
                Some(45.0),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "off_canvas"),
        "rotated rect whose AABB exits page should fire off_canvas; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "off_canvas")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Advisory);
    assert_eq!(diag.subject_id.as_deref(), Some("rect.rot"));
}

/// Same authored box as above but unrotated (rotate=None) → authored bbox is
/// fully inside the page → no off_canvas advisory.
#[test]
fn unrotated_inside_page_no_off_canvas() {
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.norot",
            200.0,
            200.0,
            vec![rect_at_rotated("rect.norot", 0.0, 0.0, 160.0, 20.0, None)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "off_canvas"),
        "unrotated rect inside page should NOT fire off_canvas; codes: {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// WCAG 2.2 contrast advisory tests
// ══════════════════════════════════════════════════════════════════════

/// Build a color token with a specific hex value.
fn color_token_hex(id: &str, hex: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String(hex.to_owned())),
        source_span: None,
    }
}

/// Build a dimension token in pt.
fn dim_token_pt(id: &str, value: f64) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Dimension,
        value: TokenValue::Literal(TokenLiteral::Dimension(Dimension {
            value,
            unit: Unit::Pt,
        })),
        source_span: None,
    }
}

/// Build a font-weight token.
fn fw_token(id: &str, weight: f64) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::FontWeight,
        value: TokenValue::Literal(TokenLiteral::Number(weight)),
        source_span: None,
    }
}

/// Helper: build a page with a background color token reference.
fn page_with_bg(id: &str, bg_token_id: &str, children: Vec<Node>) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(1280.0),
        height: px(720.0),
        background: Some(PropertyValue::TokenRef(bg_token_id.to_owned())),
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        children,
        source_span: None,
    }
}

/// Build a text node with explicit fill and optional font-size / font-weight.
fn text_with_fill_and_size(
    id: &str,
    fill_token: Option<&str>,
    font_size_token: Option<&str>,
    font_weight_token: Option<&str>,
) -> Node {
    Node::Text(Box::new(crate::ast::node::TextNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
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
        fill: fill_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: None,
        font_size: font_size_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
        font_size_min: None,
        font_weight: font_weight_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
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
        spans: vec![],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// Light gray (#aaaaaa) text on white page at 16 px → contrast ~2.32:1 < 4.5
/// → `contrast.low` warning.
#[test]
fn low_contrast_normal_text_warns() {
    let doc = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#aaaaaa"),
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_size(
                "text.one",
                Some("color.text"),
                None,
                None,
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "contrast.low"),
        "light gray on white should warn contrast.low; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "contrast.low")
        .expect("must exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors(), "contrast.low must not be an error");
}

/// Black (#000000) text on white page → contrast 21:1 → NO warning.
#[test]
fn high_contrast_text_no_warning() {
    let doc = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#000000"),
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_size(
                "text.one",
                Some("color.text"),
                None,
                None,
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "contrast.low"),
        "black on white must NOT warn contrast.low; codes: {:?}",
        codes(&report)
    );
}

/// Large text (20 pt ≈ 26.67 px, which is >= 24 px) with a mid-contrast
/// color (#777777 ≈ 4.48:1) passes the large-text threshold (3.0) but would
/// fail the normal threshold (4.5) → NO warning.
///
/// Note: 20 pt × (4/3) = 26.67 px, which exceeds the 24 px large-text cut-off.
#[test]
fn large_text_passes_lower_threshold_no_warning() {
    let doc = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#777777"), // ~4.48:1 — passes 3.0, fails 4.5
            dim_token_pt("size.large", 20.0),         // 20pt ≈ 26.67px >= 24px → large
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_size(
                "text.one",
                Some("color.text"),
                Some("size.large"),
                None,
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "contrast.low"),
        "large text at ~4.48:1 should pass the 3.0 threshold; codes: {:?}",
        codes(&report)
    );
}

/// Small bold text (18 pt ≈ 24 px, which is exactly 24 px → large) with
/// mid-contrast (#777777 ≈ 4.48:1) → passes 3.0 → NO warning.
#[test]
fn bold_large_text_passes_lower_threshold() {
    let doc = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#777777"),
            dim_token_pt("size.18pt", 18.0), // 18pt ≈ 24px → exactly at large boundary
            fw_token("weight.bold", 700.0),
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_size(
                "text.one",
                Some("color.text"),
                Some("size.18pt"),
                Some("weight.bold"),
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "contrast.low"),
        "18pt bold (large text) at ~4.48:1 should pass 3.0; codes: {:?}",
        codes(&report)
    );
}

/// Text node with no fill → no contrast check → no warning.
#[test]
fn text_without_fill_skips_contrast_check() {
    let doc = doc_with(
        vec![color_token_hex("color.bg", "#ffffff")],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_size("text.one", None, None, None)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "contrast.low"),
        "text with no fill must not produce contrast.low; codes: {:?}",
        codes(&report)
    );
}

/// Page with no background token → contrast checks are skipped entirely.
#[test]
fn no_page_background_skips_contrast_check() {
    let doc = doc_with(
        vec![color_token_hex("color.text", "#aaaaaa")],
        vec![minimal_page(
            "page.one",
            vec![text_with_fill_and_size(
                "text.one",
                Some("color.text"),
                None,
                None,
            )],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "contrast.low"),
        "page with no background must not produce contrast.low; codes: {:?}",
        codes(&report)
    );
}

/// Build a text node with an explicit fill token AND a `contrast-bg` hint token.
fn text_with_fill_and_contrast_bg(id: &str, fill_token: &str, contrast_bg_token: &str) -> Node {
    Node::Text(Box::new(crate::ast::node::TextNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
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
        fill: Some(PropertyValue::TokenRef(fill_token.to_owned())),
        stroke: None,
        stroke_width: None,
        contrast_bg: Some(PropertyValue::TokenRef(contrast_bg_token.to_owned())),
        font_family: None,
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
        spans: vec![],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// A `contrast-bg` hint takes TOP priority over the page background: a dark fill
/// with a dark `contrast-bg` on a WHITE page must still warn `contrast.low`
/// (judged against the hint, not the page bg), and the message names the hint.
#[test]
fn contrast_bg_hint_used_as_background() {
    // Dark hint + dark fill → low contrast despite the white page bg.
    let dark = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#222222"),
            color_token_hex("color.photo.shadow", "#101010"),
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_contrast_bg(
                "coverline",
                "color.text",
                "color.photo.shadow",
            )],
        )],
    );
    let report = validate(&dark);
    assert!(
        has_code(&report, "contrast.low"),
        "dark fill on a dark contrast-bg hint must warn contrast.low; codes: {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "contrast.low")
        .expect("must exist");
    assert!(
        diag.message.contains("contrast-bg hint"),
        "message must name the contrast-bg hint as the bg source; got: {}",
        diag.message
    );

    // Light hint + dark fill → high contrast → NO warning (hint overrides bg).
    let light = doc_with(
        vec![
            color_token_hex("color.bg", "#000000"),
            color_token_hex("color.text", "#111111"),
            color_token_hex("color.photo.light", "#fafafa"),
        ],
        vec![page_with_bg(
            "page.one",
            "color.bg",
            vec![text_with_fill_and_contrast_bg(
                "coverline",
                "color.text",
                "color.photo.light",
            )],
        )],
    );
    let report = validate(&light);
    assert!(
        !has_code(&report, "contrast.low"),
        "dark fill on a light contrast-bg hint must NOT warn contrast.low; codes: {:?}",
        codes(&report)
    );
}

/// A raw literal `contrast-bg` value is rejected as `token.raw_visual_literal`,
/// consistent with `fill`/`stroke`.
#[test]
fn contrast_bg_literal_rejected() {
    let mut text = match text_with_fill_and_contrast_bg("t", "color.text", "color.bg") {
        Node::Text(t) => t,
        _ => unreachable!(),
    };
    // Overwrite the hint with a RAW literal.
    text.contrast_bg = Some(PropertyValue::Literal("#000000".to_owned()));
    let doc = doc_with(
        vec![
            color_token_hex("color.bg", "#ffffff"),
            color_token_hex("color.text", "#000000"),
        ],
        vec![page_with_bg("page.one", "color.bg", vec![Node::Text(text)])],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.raw_visual_literal"),
        "a raw-literal contrast-bg must flag token.raw_visual_literal; codes: {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// safe-zone advisory tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a page with explicit safe-zones and children (px page rect).
fn page_with_zones(
    id: &str,
    w: f64,
    h: f64,
    safe_zones: Vec<SafeZone>,
    children: Vec<Node>,
) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(w),
        height: px(h),
        background: None,
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones,
        folds: Vec::new(),
        children,
        source_span: None,
    }
}

/// Helper: build a safe-zone rect of the given type at (x, y, w, h) px.
fn zone(id: &str, zone_type: SafeZoneType, x: f64, y: f64, w: f64, h: f64) -> SafeZone {
    SafeZone {
        id: id.to_owned(),
        zone_type,
        x: px(x),
        y: px(y),
        w: px(w),
        h: px(h),
        label: None,
        source_span: None,
    }
}

/// Helper: a full-bleed background image covering the whole page rect.
fn image_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
    Node::Image(ImageNode {
        id: id.to_owned(),
        name: None,
        role: None,
        asset: "asset.bg".to_owned(),
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: Some(px(h)),
        src_x: None,
        src_y: None,
        src_w: None,
        src_h: None,
        fit: None,
        clip: None,
        clip_radius: None,
        object_position_x: None,
        object_position_y: None,
        opacity: None,
        shadow: None,
        filter: None,
        mask: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    })
}

/// An exclusion zone overlapped by a content node → `safe_zone.violation`.
#[test]
fn exclusion_zone_overlapping_node_violates() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.bad", 50.0, 380.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "safe_zone.violation"),
        "expected safe_zone.violation; codes: {:?}",
        codes(&report)
    );
}

/// An exclusion zone NOT overlapped by a content node → no violation.
#[test]
fn exclusion_zone_non_overlapping_node_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.ok", 600.0, 40.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "non-overlapping node must not violate; codes: {:?}",
        codes(&report)
    );
}

/// A full-bleed background image overlapping an exclusion zone → no violation
/// (full-bleed nodes are exempt).
#[test]
fn full_bleed_background_is_exempt_from_exclusion_zone() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![image_at("img.bg", 0.0, 0.0, 1500.0, 500.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "full-bleed background must be exempt; codes: {:?}",
        codes(&report)
    );
}

/// A required zone with a node fully outside → `safe_zone.violation`.
#[test]
fn required_zone_node_fully_outside_violates() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.title",
                SafeZoneType::Required,
                600.0,
                40.0,
                300.0,
                100.0,
            )],
            vec![rect_at("rect.out", 0.0, 400.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "safe_zone.violation"),
        "node outside required zone must violate; codes: {:?}",
        codes(&report)
    );
}

/// A required zone with a node overlapping it → no violation (lenient).
#[test]
fn required_zone_overlapping_node_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.title",
                SafeZoneType::Required,
                600.0,
                40.0,
                300.0,
                100.0,
            )],
            vec![rect_at("rect.in", 650.0, 50.0, 100.0, 40.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "safe_zone.violation"),
        "node overlapping required zone must not violate; codes: {:?}",
        codes(&report)
    );
}

/// A safe-zone violation is ADVISORY — it must not flag the report as errored.
#[test]
fn safe_zone_violation_is_advisory_not_error() {
    let doc = doc_with(
        vec![],
        vec![page_with_zones(
            "page.one",
            1500.0,
            500.0,
            vec![zone(
                "sz.avatar",
                SafeZoneType::Exclusion,
                0.0,
                358.0,
                175.0,
                142.0,
            )],
            vec![rect_at("rect.bad", 50.0, 380.0, 100.0, 80.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "safe_zone.violation" && d.severity == Severity::Advisory),
        "safe_zone.violation must be Advisory severity; codes: {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "safe_zone.violation must not make the report errored"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Fold content-crossing advisories
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a page with explicit folds and children (px page rect).
fn page_with_folds(id: &str, w: f64, h: f64, folds: Vec<Fold>, children: Vec<Node>) -> Page {
    Page {
        id: id.to_owned(),
        name: None,
        width: px(w),
        height: px(h),
        background: None,
        bleed: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        baseline_grid: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds,
        children,
        source_span: None,
    }
}

/// Helper: build a fold of the given orientation at the given px position.
fn fold(id: &str, orientation: &str, position: f64) -> Fold {
    Fold {
        id: id.to_owned(),
        orientation: orientation.to_owned(),
        position: Some(px(position)),
        source_span: None,
    }
}

/// A vertical fold at x=1169 with a node spanning x=80..2430 → crossing.
#[test]
fn vertical_fold_crossed_by_node_advises() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "fold.content_crossing"),
        "expected fold.content_crossing; codes: {:?}",
        codes(&report)
    );
}

/// A node entirely left of the vertical fold → no crossing.
#[test]
fn vertical_fold_not_crossed_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            // Right edge at 80+200 = 280 < 1169 → fully left of the fold.
            vec![rect_at("rect.left", 80.0, 100.0, 200.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "node left of fold must not cross; codes: {:?}",
        codes(&report)
    );
}

/// A horizontal fold at y=500 with a node spanning y=100..900 → crossing.
#[test]
fn horizontal_fold_crossed_by_node_advises() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.h", "horizontal", 500.0)],
            vec![rect_at("rect.tall", 100.0, 100.0, 200.0, 800.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "fold.content_crossing"),
        "expected fold.content_crossing for horizontal fold; codes: {:?}",
        codes(&report)
    );
}

/// A node entirely above the horizontal fold → no crossing.
#[test]
fn horizontal_fold_not_crossed_is_clean() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.h", "horizontal", 500.0)],
            // Bottom edge at 100+200 = 300 < 500 → fully above the fold.
            vec![rect_at("rect.top", 100.0, 100.0, 200.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "node above fold must not cross; codes: {:?}",
        codes(&report)
    );
}

/// A fold content-crossing is ADVISORY — it must not flag the report errored.
#[test]
fn fold_content_crossing_is_advisory_not_error() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![fold("fold.1", "vertical", 1169.0)],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fold.content_crossing" && d.severity == Severity::Advisory),
        "fold.content_crossing must be Advisory; codes: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

/// A fold with no resolvable position → no crossing advisory (skipped).
#[test]
fn fold_without_position_is_skipped() {
    let doc = doc_with(
        vec![],
        vec![page_with_folds(
            "page.one",
            2480.0,
            1000.0,
            vec![Fold {
                id: "fold.none".to_owned(),
                orientation: "vertical".to_owned(),
                position: None,
                source_span: None,
            }],
            vec![rect_at("rect.wide", 80.0, 100.0, 2350.0, 200.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "fold.content_crossing"),
        "fold without position must be skipped; codes: {:?}",
        codes(&report)
    );
}

// ── Component / instance validation ───────────────────────────────────────────

mod component_validation {
    use crate::parse::{KdlAdapter, KdlSource};
    use crate::validate::validate;

    fn parse_doc(src: &str) -> crate::ast::Document {
        KdlAdapter.parse(src.as_bytes()).expect("must parse")
    }

    fn has_code(report: &crate::validate::ValidationReport, code: &str) -> bool {
        report.diagnostics.iter().any(|d| d.code == code)
    }

    const BASE_TOKENS: &str = r##"  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
    token id="color.fg" type="color" value="#fafafa"
  }
  styles {}"##;

    #[test]
    fn unknown_component_reference_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="real.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="missing" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_reference"),
            "expected component.unknown_reference: {:?}",
            report.diagnostics
        );
        assert!(report.has_errors());
    }

    #[test]
    fn unknown_override_target_is_warning() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="c.one" x=(px)0 y=(px)0 {{
        override ref="does.not.exist" {{ span "X" }}
      }}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_override_target"),
            "expected component.unknown_override_target: {:?}",
            report.diagnostics
        );
        // It is a Warning, not a hard error.
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "component.unknown_override_target"
                    && d.severity == crate::diagnostics::Severity::Error)
        );
    }

    #[test]
    fn duplicate_component_id_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="dup" {{
      rect id="a" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
    component id="dup" {{
      rect id="b" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "duplicate component id must be id.duplicate: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn local_child_ids_do_not_collide_across_components() {
        // Two components both declare a child id "bg" and "label" — this must
        // NOT trigger id.duplicate because component child ids are local.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.a" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "A" }}
    }}
    component id="c.b" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "B" }}
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            !has_code(&report, "id.duplicate"),
            "component-local ids must not collide across components: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn instance_id_participates_in_global_uniqueness() {
        // An instance id that collides with a page node id → id.duplicate.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      rect id="dup.id" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      instance id="dup.id" component="c.one" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "instance id must participate in global uniqueness: {:?}",
            report.diagnostics
        );
    }
}

// ── Page bleed validation ─────────────────────────────────────────────

/// A page with a valid positive px bleed produces no bleed warning.
#[test]
fn valid_bleed_no_warning() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(px(35.0));
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(
        !has_code(&report, "page.invalid_bleed"),
        "valid bleed must not warn: {:?}",
        codes(&report)
    );
}

/// A bleed declared with a non-resolvable unit (pct) warns but is not an error.
#[test]
fn bleed_bad_unit_warns_not_errors() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(Dimension {
        value: 5.0,
        unit: Unit::Pct,
    });
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(
        has_code(&report, "page.invalid_bleed"),
        "bad-unit bleed must warn: {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "bad-unit bleed must NOT be a hard error: {:?}",
        codes(&report)
    );
}

/// A negative bleed warns but is not an error.
#[test]
fn bleed_negative_warns_not_errors() {
    let mut page = minimal_page("page.bleed", vec![]);
    page.bleed = Some(px(-10.0));
    let report = validate(&doc_with(vec![], vec![page]));
    assert!(has_code(&report, "page.invalid_bleed"));
    assert!(!report.has_errors());
}

// ══════════════════════════════════════════════════════════════════════
// margin.violation advisory tests (book live-area)
// ══════════════════════════════════════════════════════════════════════

/// Helper: a book page with the standard four margins set
/// (inner 225, outer 150, top 210, bottom 240 on a 1240×1754 spread).
fn book_page(id: &str, children: Vec<Node>) -> Page {
    let mut page = bounded_page(id, 1240.0, 1754.0, children);
    page.margin_inner = Some(px(225.0));
    page.margin_outer = Some(px(150.0));
    page.margin_top = Some(px(210.0));
    page.margin_bottom = Some(px(240.0));
    page
}

/// Returns `true` when a `margin.violation` advisory names `node_id`.
fn has_margin_violation_for(report: &ValidationReport, node_id: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.code == "margin.violation" && d.subject_id.as_deref() == Some(node_id))
}

#[test]
fn margin_recto_node_inside_live_area_no_violation() {
    // recto live area: x∈[225, 1090], y∈[210, 1514]. A rect fully inside.
    let doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("ok", 300.0, 300.0, 400.0, 400.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "node inside the live area must not trip margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_recto_node_left_of_inner_violates() {
    // mirror on, page 1 = recto → inner (225) insets the LEFT. A rect at x=100
    // crosses the left margin edge.
    let mut doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "a recto node left of margin-inner must trip margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_verso_parity_flips_inner_side() {
    // A rect at x=160 sits BETWEEN outer (150) and inner (225).
    // mirror on:
    //   - page 1 (recto): left inset = inner = 225 → 160 < 225 → VIOLATION.
    //   - page 2 (verso): left inset = outer = 150 → 160 ≥ 150 → NO violation.
    let recto_rect = rect_at("r.node", 160.0, 300.0, 400.0, 400.0);
    let verso_rect = rect_at("v.node", 160.0, 300.0, 400.0, 400.0);
    let mut doc = doc_with(
        vec![],
        vec![
            book_page("page.recto", vec![recto_rect]),
            book_page("page.verso", vec![verso_rect]),
        ],
    );
    doc.mirror_margins = Some(true);
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "r.node"),
        "recto node at x=160 (< inner 225) must violate; got {:?}",
        codes(&report)
    );
    assert!(
        !has_margin_violation_for(&report, "v.node"),
        "verso node at x=160 (≥ outer 150) must NOT violate (inner side flipped); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_rtl_parity_is_mirror_of_ltr() {
    // page-progression="rtl" mirrors the spread: recto binding is on the RIGHT
    // (left inset = outer = 150), verso binding on the LEFT (left inset = inner
    // = 225) — the exact opposite of the LTR parity above. A rect at x=160:
    //   - page 1 (recto, RTL): left inset = outer = 150 → 160 ≥ 150 → NO violation.
    //   - page 2 (verso, RTL): left inset = inner = 225 → 160 < 225 → VIOLATION.
    let recto_rect = rect_at("r.node", 160.0, 300.0, 400.0, 400.0);
    let verso_rect = rect_at("v.node", 160.0, 300.0, 400.0, 400.0);
    let mut doc = doc_with(
        vec![],
        vec![
            book_page("page.recto", vec![recto_rect]),
            book_page("page.verso", vec![verso_rect]),
        ],
    );
    doc.mirror_margins = Some(true);
    doc.page_progression = Some("rtl".to_owned());
    let report = validate(&doc);
    assert!(
        !has_margin_violation_for(&report, "r.node"),
        "RTL recto node at x=160 (≥ outer 150) must NOT violate (inner on right); got {:?}",
        codes(&report)
    );
    assert!(
        has_margin_violation_for(&report, "v.node"),
        "RTL verso node at x=160 (< inner 225) must violate (inner on left); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_guide_role_is_exempt() {
    // A node with role="guide" intentionally lives in the margins → exempt.
    let mut guide = rect_at("guide.line", 0.0, 300.0, 50.0, 50.0);
    if let Node::Rect(r) = &mut guide {
        r.role = Some("guide".to_owned());
    }
    let doc = doc_with(vec![], vec![book_page("page.recto", vec![guide])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "a role=guide node must be exempt from margin.violation; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_absent_skips_check() {
    // A plain page with no margins → the check is skipped entirely.
    let doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.plain",
            1240.0,
            1754.0,
            vec![rect_at("any", 0.0, 0.0, 50.0, 50.0)],
        )],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "margin.violation"),
        "absent margins must skip the margin check; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_document_default_cascades_to_bare_page() {
    // The page declares NO margins, but the DOCUMENT sets all four defaults
    // (inner 225, outer 150, top 210, bottom 240). The bare page inherits them,
    // so its live area is computed and a node outside it trips margin.violation.
    // recto live area: x∈[225, 1090]. A rect at x=100 crosses the left edge.
    let mut doc = doc_with(
        vec![],
        vec![bounded_page(
            "page.bare",
            1240.0,
            1754.0,
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    doc.margin_inner = Some(px(225.0));
    doc.margin_outer = Some(px(150.0));
    doc.margin_top = Some(px(210.0));
    doc.margin_bottom = Some(px(240.0));
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "a bare page must inherit the document default margins; got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_page_inner_overrides_doc_default() {
    // Doc default inner = 225; the page overrides inner = 100 (keeps doc
    // outer/top/bottom). recto left inset becomes 100, so a rect at x=120 is now
    // INSIDE the live area and must NOT violate — proving the per-page override
    // wins over the doc default for inner only.
    let mut page = bounded_page(
        "page.over",
        1240.0,
        1754.0,
        vec![rect_at("ok", 120.0, 300.0, 50.0, 50.0)],
    );
    page.margin_inner = Some(px(100.0));
    let mut doc = doc_with(vec![], vec![page]);
    doc.mirror_margins = Some(true);
    doc.margin_inner = Some(px(225.0));
    doc.margin_outer = Some(px(150.0));
    doc.margin_top = Some(px(210.0));
    doc.margin_bottom = Some(px(240.0));
    let report = validate(&doc);
    assert!(
        !has_margin_violation_for(&report, "ok"),
        "the page's own inner margin (100) must override the doc default (225); got {:?}",
        codes(&report)
    );
}

#[test]
fn margin_doc_default_off_is_byte_identical_to_page_only() {
    // Regression guard for the default-off path: a doc with page margins but NO
    // document margins must produce EXACTLY the diagnostics it did before the
    // cascade existed. We assert against an explicit per-page book page with no
    // doc-level margins set.
    let mut doc = doc_with(
        vec![],
        vec![book_page(
            "page.recto",
            vec![rect_at("bleeds", 100.0, 300.0, 50.0, 50.0)],
        )],
    );
    doc.mirror_margins = Some(true);
    // No doc-level margins set — the cascade reads the page's own values verbatim.
    assert!(doc.margin_inner.is_none());
    let report = validate(&doc);
    assert!(
        has_margin_violation_for(&report, "bleeds"),
        "page-only margins must behave exactly as before; got {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// document.invalid_page_progression warning tests
// ══════════════════════════════════════════════════════════════════════

#[test]
fn page_progression_rtl_is_valid() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_progression = Some("rtl".to_owned());
    let report = validate(&doc);
    assert!(!has_code(&report, "document.invalid_page_progression"));
}

#[test]
fn page_progression_invalid_warns() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_progression = Some("sideways".to_owned());
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_page_progression"),
        "an unrecognized page-progression must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page-progression warning must not be a hard error"
    );
}

// ══════════════════════════════════════════════════════════════════════
// page-parity-start / page parity warning tests
// ══════════════════════════════════════════════════════════════════════

#[test]
fn page_parity_start_verso_is_valid() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_parity_start = Some("verso".to_owned());
    let report = validate(&doc);
    assert!(!has_code(&report, "document.invalid_page_parity_start"));
    assert!(!report.has_errors());
}

#[test]
fn page_parity_start_invalid_warns() {
    let mut doc = doc_with(vec![], vec![minimal_page("page.one", vec![])]);
    doc.page_parity_start = Some("sideways".to_owned());
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_page_parity_start"),
        "an unrecognized page-parity-start must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page-parity-start warning must not be a hard error"
    );
}

#[test]
fn page_parity_override_valid_does_not_warn() {
    let mut page = minimal_page("page.one", vec![]);
    page.parity = Some("verso".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(!has_code(&report, "page.invalid_parity"));
    assert!(!report.has_errors());
}

#[test]
fn page_parity_override_invalid_warns() {
    let mut page = minimal_page("page.one", vec![]);
    page.parity = Some("upside-down".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "page.invalid_parity"),
        "an unrecognized per-page parity must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "page parity warning must not be a hard error"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Configurable parity drives the mirrored-margin live area
// ══════════════════════════════════════════════════════════════════════

/// With `mirror-margins`, `page-parity-start="verso"` makes page 1 a VERSO, so
/// its binding (inner) margin moves to the right and the left inset becomes the
/// OUTER margin — flipping the `margin.violation` advisory's named parity and
/// live-area x relative to the default (page 1 = recto).
#[test]
fn page_parity_start_verso_flips_page_one_live_area() {
    // book_page: inner=225, outer=150 on a 1240-wide page.
    // Default (recto): live x = inner = 225. A node at x=160 crosses the LEFT.
    // start=verso (page 1 = verso): live x = outer = 150. The SAME node at x=160
    // is now INSIDE on the left, but a node at x=140 would cross.
    let probe = rect_at("probe", 160.0, 300.0, 400.0, 400.0);

    // Default: page 1 recto, inner=225 → node at 160 is left of the live area.
    let mut doc_default = doc_with(vec![], vec![book_page("p1", vec![probe.clone()])]);
    doc_default.mirror_margins = Some(true);
    let report_default = validate(&doc_default);
    assert!(
        has_margin_violation_for(&report_default, "probe"),
        "recto page-1 default: node at x=160 must violate the inner(225) live edge; got {:?}",
        codes(&report_default)
    );

    // start=verso: page 1 verso, outer=150 → node at 160 is now inside on the left.
    let mut doc_verso = doc_with(vec![], vec![book_page("p1", vec![probe.clone()])]);
    doc_verso.mirror_margins = Some(true);
    doc_verso.page_parity_start = Some("verso".to_owned());
    let report_verso = validate(&doc_verso);
    assert!(
        !has_margin_violation_for(&report_verso, "probe"),
        "verso page-1: node at x=160 must sit inside the outer(150) live edge; got {:?}",
        codes(&report_verso)
    );
}

/// An explicit per-page `parity="recto"` override flips a page back even when
/// `page-parity-start="verso"` would otherwise make it a verso.
#[test]
fn page_parity_override_flips_one_page_live_area() {
    let probe = rect_at("probe", 160.0, 300.0, 400.0, 400.0);

    let mut page = book_page("p1", vec![probe]);
    page.parity = Some("recto".to_owned());
    let mut doc = doc_with(vec![], vec![page]);
    doc.mirror_margins = Some(true);
    doc.page_parity_start = Some("verso".to_owned());
    let report = validate(&doc);
    // Override forces recto → inner=225 live edge → node at x=160 violates again.
    assert!(
        has_margin_violation_for(&report, "probe"),
        "explicit parity=recto must restore the inner(225) live edge; got {:?}",
        codes(&report)
    );
}

// ── Master-page + field validation ────────────────────────────────────────

/// Build a `field` node with the given id and type; all other props default.
fn field_node(id: &str, field_type: &str) -> FieldNode {
    FieldNode {
        id: id.to_owned(),
        name: None,
        role: None,
        field_type: field_type.to_owned(),
        recto: None,
        verso: None,
        target: None,
        folio_style: None,
        suppress_first: None,
        x: None,
        y: Some(px(80.0)),
        h: Some(px(40.0)),
        w: None,
        style: None,
        fill: None,
        font_family: None,
        font_size: None,
        opacity: None,
        visible: None,
        locked: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

/// `doc_with` plus a masters block.
fn doc_with_masters(tokens: Vec<Token>, masters: Vec<MasterDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(tokens, pages);
    doc.masters = masters;
    doc
}

#[test]
fn unknown_master_reference_is_error() {
    let mut page = minimal_page("p1", vec![]);
    page.master = Some("m.missing".to_owned());
    let doc = doc_with(vec![], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "master.unknown_reference"),
        "an unknown master reference must be a hard error; got {:?}",
        codes(&report)
    );
}

#[test]
fn declared_master_reference_is_accepted() {
    let master = MasterDef {
        id: "m.body".to_owned(),
        children: vec![],
        source_span: None,
    };
    let mut page = minimal_page("p1", vec![]);
    page.master = Some("m.body".to_owned());
    let doc = doc_with_masters(vec![], vec![master], vec![page]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "master.unknown_reference"),
        "a declared master reference must validate cleanly; got {:?}",
        codes(&report)
    );
}

#[test]
fn unknown_field_type_is_warning() {
    let field = Node::Field(field_node("f.bad", "marquee"));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![field])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "field.unknown_type"),
        "an unknown field type must be a warning; got {:?}",
        codes(&report)
    );
}

#[test]
fn known_field_types_have_no_unknown_type_warning() {
    for ty in ["running-head", "page-number", "page-ref", "page-count"] {
        let mut f = field_node("f", ty);
        if ty == "page-ref" {
            // give it a resolvable target so we isolate the type check
            f.target = Some("p1".to_owned());
        }
        let doc = doc_with(vec![], vec![minimal_page("p1", vec![Node::Field(f)])]);
        let report = validate(&doc);
        assert!(
            !has_code(&report, "field.unknown_type"),
            "{ty} is a known field type; got {:?}",
            codes(&report)
        );
    }
}

#[test]
fn unresolved_page_ref_target_is_warning() {
    let mut f = field_node("f.ref", "page-ref");
    f.target = Some("nowhere".to_owned());
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![Node::Field(f)])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "field.unresolved_ref"),
        "a page-ref to a missing target must warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn resolved_page_ref_target_does_not_warn() {
    // The page contains a node with id "anchor"; a page-ref to it resolves.
    let anchor = Node::Rect(Box::new(RectNode {
        id: "anchor".to_owned(),
        name: None,
        role: None,
        x: Some(px(0.0)),
        y: Some(px(0.0)),
        w: Some(px(10.0)),
        h: Some(px(10.0)),
        radius: None,
        radius_tl: None,
        radius_tr: None,
        radius_br: None,
        radius_bl: None,
        style: None,
        fill: None,
        stroke: None,
        stroke_width: None,
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
        shadow: None,
        filter: None,
        mask: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }));
    let mut f = field_node("f.ref", "page-ref");
    f.target = Some("anchor".to_owned());
    let doc = doc_with(
        vec![],
        vec![minimal_page("p1", vec![anchor, Node::Field(f)])],
    );
    let report = validate(&doc);
    assert!(
        !has_code(&report, "field.unresolved_ref"),
        "a page-ref to a present target must not warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn unresolved_footnote_ref_is_warning() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "Dangling" footnote-ref="fn.missing"
      }
    }
  }
}
"##;
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "footnote.unresolved_ref"),
        "a span footnote-ref to a missing footnote must warn; got {:?}",
        codes(&report)
    );
}

#[test]
fn resolved_footnote_ref_does_not_warn_and_id_is_unique() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      text id="body" x=(px)10 y=(px)10 w=(px)300 h=(px)100 {
        span "Evidence" footnote-ref="fn.1"
      }
      footnote id="fn.1" {
        span "See Chapter 4."
      }
    }
  }
}
"##;
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "footnote.unresolved_ref"),
        "a span footnote-ref to a present footnote must not warn; got {:?}",
        codes(&report)
    );
    // The footnote id participates in global id-uniqueness: no duplicate is
    // flagged for a unique id, but a colliding id would be.
    assert!(
        !has_code(&report, "id.duplicate"),
        "a unique footnote id must not be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn duplicate_footnote_id_is_flagged() {
    let src = r##"zenith version=1 {
  project id="p" name="P"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="d" {
    page id="pg" w=(px)400 h=(px)600 {
      footnote id="dup" {
        span "First."
      }
      footnote id="dup" {
        span "Second."
      }
    }
  }
}
"##;
    let doc = <crate::parse::KdlAdapter as crate::parse::KdlSource>::parse(
        &crate::parse::KdlAdapter,
        src.as_bytes(),
    )
    .expect("parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a footnote id colliding with another node must be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn master_id_participates_in_global_uniqueness() {
    // A master id colliding with a page id is a duplicate-id error.
    let master = MasterDef {
        id: "dup".to_owned(),
        children: vec![],
        source_span: None,
    };
    let mut page = minimal_page("dup", vec![]);
    page.master = Some("dup".to_owned());
    let doc = doc_with_masters(vec![], vec![master], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a master id colliding with a page id must be a duplicate; got {:?}",
        codes(&report)
    );
}

#[test]
fn master_local_ids_are_scoped_per_master() {
    // The same local id may appear in two different masters without colliding.
    let m1 = MasterDef {
        id: "m1".to_owned(),
        children: vec![Node::Field(field_node("shared", "page-number"))],
        source_span: None,
    };
    let m2 = MasterDef {
        id: "m2".to_owned(),
        children: vec![Node::Field(field_node("shared", "page-number"))],
        source_span: None,
    };
    let doc = doc_with_masters(vec![], vec![m1, m2], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "id.duplicate"),
        "the same local id in two masters must not collide; got {:?}",
        codes(&report)
    );
}

// ══════════════════════════════════════════════════════════════════════
// Section validation tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a document with the given sections appended to a
/// single-page doc.
fn doc_with_sections(sections: Vec<SectionDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(vec![], pages);
    doc.sections = sections;
    doc
}

fn minimal_section(id: &str, start_page: &str) -> SectionDef {
    SectionDef {
        id: id.to_owned(),
        name: id.to_owned(),
        folio_start: None,
        folio_style: None,
        start_page: start_page.to_owned(),
        source_span: None,
    }
}

#[test]
fn clean_sections_block_no_diagnostics() {
    let page = minimal_page("p1", vec![]);
    let sec = minimal_section("sec.front", "p1");
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a clean sections block must produce no diagnostics; got: {:?}",
        codes(&report)
    );
}

#[test]
fn section_unknown_start_page_is_error() {
    let page = minimal_page("p1", vec![]);
    let sec = minimal_section("sec.x", "page.does.not.exist");
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.unknown_start_page"),
        "an unknown start-page reference must be a hard error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_duplicate_start_page_is_error() {
    let p1 = minimal_page("p1", vec![]);
    let p2 = minimal_page("p2", vec![]);
    let sec_a = minimal_section("sec.a", "p1");
    let sec_b = minimal_section("sec.b", "p1"); // same start_page
    let doc = doc_with_sections(vec![sec_a, sec_b], vec![p1, p2]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.duplicate_start_page"),
        "two sections sharing a start-page must be a hard error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_invalid_folio_style_is_warning() {
    let page = minimal_page("p1", vec![]);
    let mut sec = minimal_section("sec.bad", "p1");
    sec.folio_style = Some("arabic".to_owned()); // unrecognized
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "section.invalid_folio_style"),
        "an unknown folio-style must be a Warning; got {:?}",
        codes(&report)
    );
    // A Warning must NOT be counted as an error.
    assert!(
        !report.has_errors(),
        "section.invalid_folio_style must not be a hard error; got {:?}",
        codes(&report)
    );
}

#[test]
fn section_id_colliding_with_page_id_is_duplicate() {
    let page = minimal_page("shared", vec![]);
    let sec = minimal_section("shared", "shared"); // id == page id
    let doc = doc_with_sections(vec![sec], vec![page]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "a section id colliding with a page id must be an id.duplicate error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn section_valid_folio_styles_produce_no_warning() {
    for style in ["decimal", "lower-roman", "upper-roman"] {
        let page = minimal_page("p1", vec![]);
        let mut sec = minimal_section("sec.ok", "p1");
        sec.folio_style = Some(style.to_owned());
        let doc = doc_with_sections(vec![sec], vec![page]);
        let report = validate(&doc);
        assert!(
            !has_code(&report, "section.invalid_folio_style"),
            "folio-style \"{style}\" must not warn; got {:?}",
            codes(&report)
        );
    }
}

// ── facing-pages / spread-gutter ─────────────────────────────────────────────

/// Parse + round-trip test: `facing-pages` and `spread-gutter` survive a
/// parse → format → parse cycle unchanged.
#[test]
fn facing_pages_and_spread_gutter_parse_and_round_trip() {
    use crate::format::format_document;
    use crate::parse::{KdlAdapter, KdlSource};

    let src = r#"zenith version=1 facing-pages=#true spread-gutter=(px)40 {
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" {
    page id="p1" w=(px)400 h=(px)600 {}
  }
}
"#;
    let doc1 = KdlAdapter.parse(src.as_bytes()).expect("must parse");
    assert_eq!(
        doc1.facing_pages,
        Some(true),
        "facing-pages must parse to Some(true)"
    );
    assert_eq!(
        doc1.spread_gutter,
        Some(Dimension {
            value: 40.0,
            unit: Unit::Px
        }),
        "spread-gutter must parse to (px)40"
    );

    // Round-trip: format → re-parse, fields must survive unchanged.
    let formatted = format_document(&doc1).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("utf-8");
    let doc2 = KdlAdapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse must succeed");
    assert_eq!(
        doc2.facing_pages, doc1.facing_pages,
        "facing-pages must round-trip"
    );
    assert_eq!(
        doc2.spread_gutter, doc1.spread_gutter,
        "spread-gutter must round-trip"
    );
}

/// A `spread-gutter` with a non-px/pt unit (pct) → `document.invalid_spread_gutter` Warning.
#[test]
fn spread_gutter_pct_emits_invalid_spread_gutter_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: 10.0,
        unit: Unit::Pct,
    });
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_spread_gutter"),
        "pct spread-gutter must warn with document.invalid_spread_gutter; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "document.invalid_spread_gutter must not be a hard error; got {:?}",
        codes(&report)
    );
}

/// A negative `spread-gutter` → `document.invalid_spread_gutter` Warning.
#[test]
fn spread_gutter_negative_emits_invalid_spread_gutter_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: -5.0,
        unit: Unit::Px,
    });
    let report = validate(&doc);
    assert!(
        has_code(&report, "document.invalid_spread_gutter"),
        "negative spread-gutter must warn; got {:?}",
        codes(&report)
    );
    assert!(
        !report.has_errors(),
        "document.invalid_spread_gutter must not be a hard error; got {:?}",
        codes(&report)
    );
}

/// A valid (px, non-negative) `spread-gutter` → no diagnostic.
#[test]
fn spread_gutter_valid_px_no_warning() {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    doc.spread_gutter = Some(Dimension {
        value: 40.0,
        unit: Unit::Px,
    });
    let report = validate(&doc);
    assert!(
        !has_code(&report, "document.invalid_spread_gutter"),
        "valid px spread-gutter must not warn; got {:?}",
        codes(&report)
    );
}

/// When `spread_gutter` is `None` (absent), no diagnostic should be emitted.
#[test]
fn spread_gutter_absent_no_warning() {
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "document.invalid_spread_gutter"),
        "absent spread-gutter must not warn; got {:?}",
        codes(&report)
    );
}

// ── Toc node validation ───────────────────────────────────────────────────────

/// Build a minimal `toc` node (no geometry, no styling).
fn toc_node_bare(id: &str, match_role: Option<&str>, match_style: Option<&str>) -> TocNode {
    TocNode {
        id: id.to_owned(),
        name: None,
        role: None,
        match_role: match_role.map(str::to_owned),
        match_style: match_style.map(str::to_owned),
        leader: None,
        folio_style: None,
        x: Some(px(50.0)),
        y: Some(px(100.0)),
        w: Some(px(400.0)),
        h: Some(px(200.0)),
        style: None,
        fill: None,
        font_family: None,
        font_size: None,
        opacity: None,
        visible: None,
        locked: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn toc_with_match_role_does_not_warn_no_selector() {
    let toc = Node::Toc(toc_node_bare("toc.1", Some("heading"), None));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "toc.no_selector"),
        "toc with match-role must not emit toc.no_selector; got {:?}",
        codes(&report)
    );
}

#[test]
fn toc_with_match_style_does_not_warn_no_selector() {
    let toc = Node::Toc(toc_node_bare("toc.2", None, Some("Heading 1")));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        !has_code(&report, "toc.no_selector"),
        "toc with match-style must not emit toc.no_selector; got {:?}",
        codes(&report)
    );
}

#[test]
fn toc_with_no_selector_warns() {
    let toc = Node::Toc(toc_node_bare("toc.3", None, None));
    let doc = doc_with(vec![], vec![minimal_page("p1", vec![toc])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "toc.no_selector"),
        "toc without selector must warn toc.no_selector; got {:?}",
        codes(&report)
    );
}

// ── Table validation ──────────────────────────────────────────────────

use crate::ast::node::{TableCell, TableColumn, TableNode, TableRow};

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

// ══════════════════════════════════════════════════════════════════════
// Library block validation tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: build a document with the given libraries appended to a
/// single-page doc.
fn doc_with_libraries(libraries: Vec<LibraryDef>, pages: Vec<Page>) -> Document {
    let mut doc = doc_with(vec![], pages);
    doc.libraries = libraries;
    doc
}

fn minimal_library(id: &str) -> LibraryDef {
    LibraryDef {
        id: id.to_owned(),
        version: None,
        hash: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn clean_libraries_block_no_diagnostics() {
    let lib = LibraryDef {
        id: "@acme/brand-kit".to_owned(),
        version: Some("1.4.0".to_owned()),
        hash: Some("sha256-abc".to_owned()),
        source_span: None,
        unknown_props: BTreeMap::new(),
    };
    let doc = doc_with_libraries(vec![lib], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a well-formed libraries block must produce no diagnostics; got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn library_duplicate_id_is_error() {
    let a = minimal_library("@acme/brand-kit");
    let b = minimal_library("@acme/brand-kit"); // duplicate id
    let doc = doc_with_libraries(vec![a, b], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "two libraries sharing an id must trigger id.duplicate; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn library_unknown_property_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "registry".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("x".to_owned()),
            ty: Some("token".to_owned()),
        },
    );
    let lib = LibraryDef {
        id: "@acme/brand-kit".to_owned(),
        version: None,
        hash: None,
        source_span: None,
        unknown_props,
    };
    let doc = doc_with_libraries(vec![lib], vec![minimal_page("p1", vec![])]);
    let report = validate(&doc);
    assert!(
        has_code(&report, "library.unknown_property"),
        "an unknown prop on a library must warn; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "library.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(!report.has_errors());
}

// ── provenance: cross-reference validation ────────────────────────────

use crate::ast::action::ActionDef;
use crate::ast::provenance::ProvenanceDef;

/// A document with the given provenance records, libraries, and page children.
/// The page (id "p1") carries `children`, so node refs can resolve.
fn doc_with_provenance(
    provenance: Vec<ProvenanceDef>,
    libraries: Vec<LibraryDef>,
    children: Vec<Node>,
) -> Document {
    let mut doc = doc_with(vec![], vec![minimal_page("p1", children)]);
    doc.libraries = libraries;
    doc.provenance = provenance;
    doc
}

fn minimal_provenance(id: &str, node: &str, library: &str) -> ProvenanceDef {
    ProvenanceDef {
        id: id.to_owned(),
        node: node.to_owned(),
        library: library.to_owned(),
        item: None,
        linked: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }
}

#[test]
fn clean_provenance_record_no_diagnostics() {
    // node "btn" exists on the page; library "@acme/brand-kit" is declared.
    let prov = ProvenanceDef {
        id: "prov.btn".to_owned(),
        node: "btn".to_owned(),
        library: "@acme/brand-kit".to_owned(),
        item: Some("button".to_owned()),
        linked: Some(true),
        source_span: None,
        unknown_props: BTreeMap::new(),
    };
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        report.diagnostics.is_empty(),
        "a fully-resolved provenance record must produce no diagnostics; got: {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn provenance_unknown_node_is_error() {
    let prov = minimal_provenance("prov.ghost", "ghost", "@acme/brand-kit");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_node"),
        "a provenance record referencing a non-existent node must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_node_may_be_a_declared_token() {
    // A provenance record whose `node` is a declared TOKEN id (a token imported
    // from a library) validates clean — the target need not be a node.
    let prov = minimal_provenance("prov.noir", "noir", "@zenith/filters");
    let mut doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@zenith/filters")],
        vec![minimal_rect("btn", None)],
    );
    doc.tokens.tokens.push(color_token_hex("noir", "#000000"));
    let report = validate(&doc);
    assert!(
        !has_code(&report, "provenance.unknown_node"),
        "a provenance record targeting a declared token must not error; got {:?}",
        codes(&report)
    );
}

#[test]
fn provenance_unknown_library_is_error() {
    let prov = minimal_provenance("prov.btn", "btn", "@nope/x");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_library"),
        "a provenance record referencing an undeclared library must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_duplicate_id_is_error() {
    let a = minimal_provenance("prov.dup", "btn", "@acme/brand-kit");
    let b = minimal_provenance("prov.dup", "btn", "@acme/brand-kit"); // duplicate id
    let doc = doc_with_provenance(
        vec![a, b],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "id.duplicate"),
        "two provenance records sharing an id must trigger id.duplicate; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}

#[test]
fn provenance_unknown_property_produces_warning() {
    let mut unknown_props = BTreeMap::new();
    unknown_props.insert(
        "registry".to_owned(),
        crate::ast::node::UnknownProperty {
            value: crate::ast::node::UnknownValue::String("x".to_owned()),
            ty: Some("token".to_owned()),
        },
    );
    let prov = ProvenanceDef {
        id: "prov.btn".to_owned(),
        node: "btn".to_owned(),
        library: "@acme/brand-kit".to_owned(),
        item: None,
        linked: None,
        source_span: None,
        unknown_props,
    };
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_property"),
        "an unknown prop on an origin must warn; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "provenance.unknown_property")
        .expect("should exist");
    assert_eq!(diag.severity, Severity::Warning);
    // The unknown-property warning is not itself an error; node + library resolve.
    assert!(!report.has_errors());
}

#[test]
fn provenance_node_may_be_a_declared_action() {
    // A provenance record whose `node` is a declared ACTION id validates clean —
    // an action imported from a library is a valid provenance target.
    let prov = minimal_provenance("prov.brand", "apply-brand-kit", "@acme/brand-kit");
    let mut doc = doc_with_provenance(vec![prov], vec![minimal_library("@acme/brand-kit")], vec![]);
    doc.actions.push(ActionDef {
        id: "apply-brand-kit".to_owned(),
        label: Some("Apply Brand Kit".to_owned()),
        version: None,
        tx_json: "{}".to_owned(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    });
    let report = validate(&doc);
    assert!(
        !has_code(&report, "provenance.unknown_node"),
        "a provenance record targeting a declared action must not fire unknown_node; got {:?}",
        codes(&report)
    );
    assert!(!report.has_errors());
}

#[test]
fn provenance_node_nonexistent_id_still_errors() {
    // Negative case: a provenance `node` that matches neither any node id, nor
    // any declared token id, nor any declared action id must still fire
    // `provenance.unknown_node`.
    let prov = minimal_provenance("prov.ghost2", "does-not-exist", "@acme/brand-kit");
    let doc = doc_with_provenance(
        vec![prov],
        vec![minimal_library("@acme/brand-kit")],
        vec![minimal_rect("btn", None)],
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "provenance.unknown_node"),
        "a provenance record with a non-existent node/token/action id must error; got {:?}",
        codes(&report)
    );
    assert!(report.has_errors());
}
