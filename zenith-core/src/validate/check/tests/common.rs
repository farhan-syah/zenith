//! Shared builder helpers for the `validate/check` test suite.
//!
//! Moved verbatim from the former single-file `validate/check/tests.rs`; the
//! body of every builder is unchanged. Tests live in sibling submodules and
//! pull these in via `use super::common::*;`. The AST/diagnostic types the test
//! bodies construct are re-exported here so a single glob import suffices.

use std::collections::BTreeMap;

pub(super) use crate::ast::action::ActionDef;
pub(super) use crate::ast::asset::{AssetBlock, AssetDecl, AssetKind};
pub(super) use crate::ast::document::{
    Document, DocumentBody, Fold, MasterDef, Page, SafeZone, SafeZoneType, SectionDef,
};
pub(super) use crate::ast::library::LibraryDef;
pub(super) use crate::ast::node::ImageNode;
pub(super) use crate::ast::node::{
    CodeNode, ConnectorNode, EllipseNode, FieldNode, FrameNode, GroupNode, LineNode, Node, Point,
    PolygonNode, PolylineNode, RectNode, ShapeNode, TableCell, TableColumn, TableNode, TableRow,
    TextNode, TextSpan, TocNode, UnknownNode,
};
pub(super) use crate::ast::provenance::ProvenanceDef;
pub(super) use crate::ast::style::{Style, StyleBlock, UnknownStyleProp};
pub(super) use crate::ast::token::{Token, TokenBlock, TokenLiteral, TokenType, TokenValue};
pub(super) use crate::ast::value::{Dimension, PropertyValue, Unit};
pub(super) use crate::diagnostics::Severity;
pub(super) use crate::validate::check::ValidationReport;
pub(super) use crate::validate::validate;

// ── Builder helpers ────────────────────────────────────────────────────────

pub(super) fn color_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String("#112233".to_owned())),
        source_span: None,
    }
}

pub(super) fn dim_token(id: &str) -> Token {
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

pub(super) fn font_family_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::FontFamily,
        value: TokenValue::Literal(TokenLiteral::String("Inter".to_owned())),
        source_span: None,
    }
}

pub(super) fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

pub(super) fn token_ref(id: &str) -> PropertyValue {
    PropertyValue::TokenRef(id.to_owned())
}

pub(super) fn minimal_rect(id: &str, fill: Option<PropertyValue>) -> Node {
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
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

pub(super) fn minimal_text(id: &str, fill: Option<PropertyValue>) -> Node {
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
        anchor: None,
        anchor_zone: None,
        spans: vec![],
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

pub(super) fn minimal_page(id: &str, children: Vec<Node>) -> Page {
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

pub(super) fn doc_with(tokens: Vec<Token>, pages: Vec<Page>) -> Document {
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

pub(super) fn has_code(report: &ValidationReport, code: &str) -> bool {
    report.diagnostics.iter().any(|d| d.code == code)
}

pub(super) fn codes(report: &ValidationReport) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// Build an unknown node with the given id and children (no unknown props).
pub(super) fn unknown_node(kind: &str, id: Option<&str>, children: Vec<Node>) -> Node {
    Node::Unknown(Box::new(UnknownNode {
        kind: kind.to_owned(),
        id: id.map(str::to_owned),
        unknown_props: BTreeMap::new(),
        children,
        source_span: None,
    }))
}

/// Helper: build a page with a given width/height (px) and children.
pub(super) fn bounded_page(id: &str, w: f64, h: f64, children: Vec<Node>) -> Page {
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
pub(super) fn rect_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
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
        anchor: None,
        anchor_zone: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// Build a color token with a specific hex value.
pub(super) fn color_token_hex(id: &str, hex: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String(hex.to_owned())),
        source_span: None,
    }
}
