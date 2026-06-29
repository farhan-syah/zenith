//! Shared builder helpers for the `validate` integration test suite.
//!
//! Moved verbatim from the former in-`src` `validate/check/tests/common.rs`; the
//! body of every builder is unchanged. The AST/diagnostic types the test bodies
//! construct are re-exported here (via the crate's public surface) so a single
//! glob import suffices in each test binary.
//!
//! `tests/common/mod.rs` is compiled into EVERY integration-test binary, but
//! each binary only exercises a subset of these helpers — so the unused ones
//! trip `dead_code`/`unused_imports` in the binaries that don't call them. This
//! is the canonical shared-test-helper situation (see the Rust book, "Submodules
//! in Integration Tests"): the per-binary false positives are suppressed here.
#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;

pub use zenith_core::ast::block_style::BlockStyle;
pub use zenith_core::ast::document::Fold;
pub use zenith_core::{
    ActionDef, AssetBlock, AssetDecl, AssetKind, BrandContract, CodeNode, ConnectorNode, Dimension,
    Document, DocumentBody, EllipseNode, FieldNode, FrameNode, GroupNode, ImageNode, LibraryDef,
    LineNode, MasterDef, Node, Page, Point, PolygonNode, PolylineNode, PropertyValue,
    ProtectedRegion, ProvenanceDef, RecipeDef, RecipeParam, RectNode, SafeZone, SafeZoneType,
    SectionDef, Severity, ShapeNode, Style, StyleBlock, TableCell, TableColumn, TableNode,
    TableRow, TextNode, TextSpan, TocNode, Token, TokenBlock, TokenLiteral, TokenType, TokenValue,
    Unit, UnknownNode, UnknownStyleProp, ValidationReport, VariantDef, VariantOverride, validate,
};
pub use zenith_core::{KdlAdapter, KdlSource};

// ── Builder helpers ────────────────────────────────────────────────────────

pub fn color_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String("#112233".to_owned())),
        source_span: None,
    }
}

pub fn dim_token(id: &str) -> Token {
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

pub fn font_family_token(id: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::FontFamily,
        value: TokenValue::Literal(TokenLiteral::String("Inter".to_owned())),
        source_span: None,
    }
}

pub fn px(v: f64) -> Dimension {
    Dimension {
        value: v,
        unit: Unit::Px,
    }
}

pub fn token_ref(id: &str) -> PropertyValue {
    PropertyValue::TokenRef(id.to_owned())
}

/// A raw `(px)v` dimension wrapped as a geometry `PropertyValue`, for the
/// `x`/`y`/`w`/`h` fields that now accept a dimension literal OR a token ref.
pub fn pxv(v: f64) -> PropertyValue {
    PropertyValue::Dimension(px(v))
}

pub fn minimal_rect(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(pxv(0.0)),
        y: Some(pxv(0.0)),
        w: Some(pxv(100.0)),
        h: Some(pxv(100.0)),
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
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

pub fn minimal_text(id: &str, fill: Option<PropertyValue>) -> Node {
    Node::Text(Box::new(TextNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(pxv(0.0)),
        y: Some(pxv(0.0)),
        w: Some(pxv(200.0)),
        h: Some(pxv(40.0)),
        align: None,
        v_align: None,
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
        selectable: None,
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
        content_format: None,
        src: None,
        bullet: None,
        bullet_gap: None,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        spans: vec![],
        block_styles: Vec::new(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

pub fn minimal_page(id: &str, children: Vec<Node>) -> Page {
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
        line_jumps: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        block_styles: Vec::new(),
        children,
        source_span: None,
    }
}

pub fn doc_with(tokens: Vec<Token>, pages: Vec<Page>) -> Document {
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
        variants: Vec::new(),
        recipes: Vec::new(),
        diagnostic_policy: zenith_core::DiagnosticPolicy::default(),
        brand_contract: BrandContract::default(),
        body: DocumentBody {
            id: "doc.main".to_owned(),
            title: None,
            block_styles: Vec::new(),
            pages,
        },
    }
}

pub fn has_code(report: &ValidationReport, code: &str) -> bool {
    report.diagnostics.iter().any(|d| d.code == code)
}

pub fn codes(report: &ValidationReport) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// Build an unknown node with the given id and children (no unknown props).
pub fn unknown_node(kind: &str, id: Option<&str>, children: Vec<Node>) -> Node {
    Node::Unknown(Box::new(UnknownNode {
        kind: kind.to_owned(),
        id: id.map(str::to_owned),
        unknown_props: BTreeMap::new(),
        children,
        source_span: None,
    }))
}

/// Helper: build a page with a given width/height (px) and children.
pub fn bounded_page(id: &str, w: f64, h: f64, children: Vec<Node>) -> Page {
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
        line_jumps: None,
        parity: None,
        master: None,
        safe_zones: Vec::new(),
        folds: Vec::new(),
        block_styles: Vec::new(),
        children,
        source_span: None,
    }
}

/// Helper: rect at (x, y, w, h) in px, no fill.
pub fn rect_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
    Node::Rect(Box::new(RectNode {
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        x: Some(pxv(x)),
        y: Some(pxv(y)),
        w: Some(pxv(w)),
        h: Some(pxv(h)),
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
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    }))
}

/// Build a color token with a specific hex value.
pub fn color_token_hex(id: &str, hex: &str) -> Token {
    Token {
        id: id.to_owned(),
        token_type: TokenType::Color,
        value: TokenValue::Literal(TokenLiteral::String(hex.to_owned())),
        source_span: None,
    }
}

// ── Span-stripping helpers (shared by the format/writer round-trip suite) ────
//
// Round-trip tests compare a parsed `Document` against the same document after a
// format → re-parse cycle. Source spans are byte-position metadata that
// legitimately differ between the original source and the reformatted canonical
// form, so they are cleared before comparison. These operate purely on the
// public AST surface.

/// Strip all source spans from a Document to enable span-agnostic equality.
pub fn strip_spans(mut doc: Document) -> Document {
    // Assets
    doc.assets.source_span = None;
    for decl in &mut doc.assets.assets {
        decl.source_span = None;
    }
    // Tokens
    for token in &mut doc.tokens.tokens {
        token.source_span = None;
    }
    // Styles
    doc.styles.source_span = None;
    for style in &mut doc.styles.styles {
        style.source_span = None;
    }
    // Components
    for comp in &mut doc.components {
        comp.source_span = None;
        for node in &mut comp.children {
            strip_node_span(node);
        }
    }
    // Masters
    for master in &mut doc.masters {
        master.source_span = None;
        for node in &mut master.children {
            strip_node_span(node);
        }
    }
    // Libraries
    for library in &mut doc.libraries {
        library.source_span = None;
    }
    // Actions
    for action in &mut doc.actions {
        action.source_span = None;
    }
    // Sections
    for section in &mut doc.sections {
        section.source_span = None;
    }
    // Provenance
    for prov in &mut doc.provenance {
        prov.source_span = None;
    }
    // Variants
    for variant in &mut doc.variants {
        variant.source_span = None;
        for ov in &mut variant.overrides {
            ov.source_span = None;
        }
    }
    // Recipes
    for recipe in &mut doc.recipes {
        recipe.source_span = None;
        for param in &mut recipe.params {
            param.source_span = None;
        }
    }
    // Brand contract
    doc.brand_contract.source_span = None;
    // Pages and nodes
    for page in &mut doc.body.pages {
        page.source_span = None;
        for zone in &mut page.safe_zones {
            zone.source_span = None;
        }
        for fold in &mut page.folds {
            fold.source_span = None;
        }
        for node in &mut page.children {
            strip_node_span(node);
        }
    }
    doc
}

/// Recursively clear `source_span` from a node and all its descendants.
pub fn strip_node_span(node: &mut Node) {
    match node {
        Node::Rect(r) => r.source_span = None,
        Node::Ellipse(e) => e.source_span = None,
        Node::Line(l) => l.source_span = None,
        Node::Text(t) => t.source_span = None,
        Node::Code(c) => c.source_span = None,
        Node::Frame(f) => {
            f.source_span = None;
            for child in &mut f.children {
                strip_node_span(child);
            }
        }
        Node::Group(g) => {
            g.source_span = None;
            for region in &mut g.protected_regions {
                region.source_span = None;
            }
            for child in &mut g.children {
                strip_node_span(child);
            }
        }
        Node::Image(i) => i.source_span = None,
        Node::Polygon(p) => p.source_span = None,
        Node::Polyline(p) => p.source_span = None,
        Node::Instance(i) => {
            i.source_span = None;
            for ov in &mut i.overrides {
                ov.source_span = None;
            }
        }
        Node::Field(f) => f.source_span = None,
        Node::Toc(t) => t.source_span = None,
        Node::Footnote(f) => f.source_span = None,
        Node::Table(t) => {
            t.source_span = None;
            for col in &mut t.columns {
                col.source_span = None;
            }
            for row in &mut t.rows {
                row.source_span = None;
                for cell in &mut row.cells {
                    cell.source_span = None;
                    for child in &mut cell.children {
                        strip_node_span(child);
                    }
                }
            }
        }
        Node::Shape(s) => s.source_span = None,
        Node::Connector(c) => c.source_span = None,
        Node::Pattern(p) => {
            p.source_span = None;
            // The motif is part of the AST (PartialEq compares it), so its span
            // must be stripped too for span-agnostic round-trip equality.
            strip_node_span(&mut p.motif);
        }
        Node::Chart(c) => {
            c.source_span = None;
            // Series are pure data (no sub-nodes), so no recursion needed.
        }
        Node::Light(l) => l.source_span = None,
        Node::Mesh(m) => m.source_span = None,
        Node::Unknown(u) => {
            u.source_span = None;
            for child in &mut u.children {
                strip_node_span(child);
            }
        }
    }
}
