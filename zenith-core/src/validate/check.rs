//! Document-level semantic validation checks.
//!
//! All logic is collected here. `mod.rs` only re-exports the public surface.
//!
//! Checks performed (in one document walk):
//!
//! 1. **Global ID uniqueness** — every id across tokens, styles, body, pages,
//!    and nodes must be unique. Duplicates → `id.duplicate` (Error).
//! 2. **Required geometry** — `page` requires non-`Unit::Unknown` `width`/
//!    `height`; `rect`/`text` require all four of `x`, `y`, `w`, `h` present
//!    and with known units. Missing → `node.missing_geometry` (Error);
//!    unknown unit → `node.invalid_geometry` (Error).
//! 3. **Token-reference integrity + type compatibility** — visual `TokenRef`
//!    properties that point at an unknown or wrong-type token →
//!    `token.unknown_reference` / `token.incompatible_property` (Error).
//! 4. **Raw visual literal** — a recognized visual property (fill, stroke,
//!    stroke-width, font-family, font-size, radius) whose value is a
//!    `Literal(...)` → `token.raw_visual_literal` (Error).
//! 5. **Unknown node kind** → `node.unknown_kind` (Warning).
//!    **Unknown property** → `node.unknown_property` (Warning).
//! 6. **Unused token** — a token defined but never referenced by any node
//!    visual property or style → `token.unused` (Advisory).

use std::collections::{BTreeMap, HashSet};

use crate::ast::document::Document;
use crate::ast::node::Node;
use crate::ast::token::TokenType;
use crate::ast::value::{PropertyValue, Unit};
use crate::diagnostics::{Diagnostic, Severity};
use crate::tokens::ResolvedToken;

// ── Public surface ────────────────────────────────────────────────────────────

/// The outcome of a full document validation pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// All diagnostics collected during validation (token resolution +
    /// document-level checks). Never causes a hard panic; always complete.
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// Returns `true` if any diagnostic has [`Severity::Error`].
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Run the full document validation pass.
///
/// Internally runs `resolve_tokens` on `doc.tokens`, merges those diagnostics,
/// then walks the full document collecting all semantic diagnostics.
/// Never hard-fails; all findings are returned in the [`ValidationReport`].
pub fn validate(doc: &Document) -> ValidationReport {
    // ── Step 1: token resolution ──────────────────────────────────────────
    let token_resolution = crate::tokens::resolve_tokens(&doc.tokens);
    let resolved_tokens: &BTreeMap<String, ResolvedToken> = &token_resolution.resolved;

    let mut diagnostics: Vec<Diagnostic> = token_resolution.diagnostics;

    // ── Step 2: collect all IDs and gather referenced token ids ──────────
    // `seen_ids` accumulates every id encountered across the whole document.
    // When we encounter a duplicate we push `id.duplicate`.
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut referenced_token_ids: HashSet<String> = HashSet::new();

    // ── Token IDs ─────────────────────────────────────────────────────────
    for token in &doc.tokens.tokens {
        register_id(&token.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Style IDs ─────────────────────────────────────────────────────────
    for style in &doc.styles.styles {
        register_id(&style.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Document body id ──────────────────────────────────────────────────
    register_id(&doc.body.id, &mut seen_ids, &mut diagnostics);

    // ── Pages and their children ──────────────────────────────────────────
    for page in &doc.body.pages {
        register_id(&page.id, &mut seen_ids, &mut diagnostics);

        // ── Check page geometry (unit must be known) ──────────────────────
        if matches!(page.width.unit, Unit::Unknown(_)) {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "page '{}': property 'width' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }
        if matches!(page.height.unit, Unit::Unknown(_)) {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "page '{}': property 'height' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
        }

        // ── Walk page children ────────────────────────────────────────────
        for node in &page.children {
            walk_node(
                node,
                &mut seen_ids,
                &mut referenced_token_ids,
                resolved_tokens,
                &mut diagnostics,
            );
        }
    }

    // ── Step 3: unused token check ────────────────────────────────────────
    // Every token id that appears in `doc.tokens` but is not in
    // `referenced_token_ids` → advisory `token.unused`.
    for token in &doc.tokens.tokens {
        if !referenced_token_ids.contains(&token.id) {
            diagnostics.push(Diagnostic::advisory(
                "token.unused",
                format!(
                    "token '{}' is defined but never referenced by any node \
                     visual property or style in this document",
                    token.id
                ),
                token.source_span,
                Some(token.id.clone()),
            ));
        }
    }

    ValidationReport { diagnostics }
}

// ── Node walk ─────────────────────────────────────────────────────────────────

/// Recursively walk a [`Node`], collecting all diagnostics.
///
/// `referenced_token_ids` accumulates every token id actually used so that
/// the unused-token check (done after the walk) can diff against defined ids.
fn walk_node(
    node: &Node,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match node {
        Node::Rect(r) => {
            register_id(&r.id, seen_ids, diagnostics);

            // Required geometry: x, y, w, h must all be present.
            check_optional_dim(&r.id, "x", r.x.as_ref(), r.source_span, diagnostics);
            check_optional_dim(&r.id, "y", r.y.as_ref(), r.source_span, diagnostics);
            check_optional_dim(&r.id, "w", r.w.as_ref(), r.source_span, diagnostics);
            check_optional_dim(&r.id, "h", r.h.as_ref(), r.source_span, diagnostics);

            // Visual properties.
            check_visual_prop(
                &r.id,
                "fill",
                r.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "stroke",
                r.stroke.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "stroke-width",
                r.stroke_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &r.id,
                "radius",
                r.radius.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in r.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "rect '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        r.id, prop_name
                    ),
                    r.source_span,
                    Some(r.id.clone()),
                ));
            }
        }

        Node::Text(t) => {
            register_id(&t.id, seen_ids, diagnostics);

            // Required geometry.
            check_optional_dim(&t.id, "x", t.x.as_ref(), t.source_span, diagnostics);
            check_optional_dim(&t.id, "y", t.y.as_ref(), t.source_span, diagnostics);
            check_optional_dim(&t.id, "w", t.w.as_ref(), t.source_span, diagnostics);
            check_optional_dim(&t.id, "h", t.h.as_ref(), t.source_span, diagnostics);

            // Visual properties.
            check_visual_prop(
                &t.id,
                "fill",
                t.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-family",
                t.font_family.as_ref(),
                VisualExpect::FontFamily,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "font-size",
                t.font_size.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in t.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "text '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        t.id, prop_name
                    ),
                    t.source_span,
                    Some(t.id.clone()),
                ));
            }
        }

        Node::Unknown(u) => {
            diagnostics.push(Diagnostic::warning(
                "node.unknown_kind",
                format!(
                    "unknown node kind '{}' (forward-compatibility; \
                     this kind may be valid in a later schema version)",
                    u.kind
                ),
                u.source_span,
                None,
            ));
            // Unknown nodes have no children in the v0 AST; nothing to recurse.
        }
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Check a single optional geometry dimension (`x`, `y`, `w`, `h`):
/// - absent → `node.missing_geometry` (Error).
/// - present but `Unit::Unknown` → `node.invalid_geometry` (Error).
fn check_optional_dim(
    node_id: &str,
    prop: &str,
    dim: Option<&crate::ast::value::Dimension>,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match dim {
        None => {
            diagnostics.push(Diagnostic::error(
                "node.missing_geometry",
                format!(
                    "node '{}': required geometry property '{}' is missing",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        Some(d) if matches!(d.unit, Unit::Unknown(_)) => {
            diagnostics.push(Diagnostic::error(
                "node.invalid_geometry",
                format!(
                    "node '{}': geometry property '{}' has an unrecognized unit; \
                     allowed units are px, pt, pct, deg",
                    node_id, prop
                ),
                span,
                Some(node_id.to_owned()),
            ));
        }
        Some(_) => {
            // valid
        }
    }
}

// ── Visual property helpers ───────────────────────────────────────────────────

/// The expected token type for a visual property.
///
/// Only the subset of visual properties that have defined expectations in v0
/// are listed here. Properties with no expectation (e.g. `line-height`,
/// `padding`, `gap`) are skipped to avoid false-positives — the contract
/// says "if a property has no defined expectation yet, skip it."
#[derive(Debug, Clone, Copy)]
enum VisualExpect {
    Color,
    Dimension,
    FontFamily,
}

/// Check a single visual property value:
/// - `None` → no-op (property is optional).
/// - `TokenRef(id)` → record the reference; check existence and type compat.
/// - `Literal(...)` → `token.raw_visual_literal` (Error).
fn check_visual_prop(
    node_id: &str,
    prop_name: &str,
    value: Option<&PropertyValue>,
    expect: VisualExpect,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(pv) = value else {
        return;
    };

    match pv {
        PropertyValue::TokenRef(token_id) => {
            // Record as referenced (for unused-token check).
            referenced_token_ids.insert(token_id.clone());

            // Existence check.
            let Some(resolved) = resolved_tokens.get(token_id.as_str()) else {
                diagnostics.push(Diagnostic::error(
                    "token.unknown_reference",
                    format!(
                        "node '{}': property '{}' references token '{}' which \
                         does not exist or failed resolution",
                        node_id, prop_name, token_id
                    ),
                    None,
                    Some(node_id.to_owned()),
                ));
                return;
            };

            // Type compatibility check.
            let type_ok = match expect {
                VisualExpect::Color => {
                    matches!(resolved.token_type, TokenType::Color)
                }
                VisualExpect::Dimension => {
                    matches!(resolved.token_type, TokenType::Dimension)
                }
                VisualExpect::FontFamily => {
                    matches!(resolved.token_type, TokenType::FontFamily)
                }
            };

            if !type_ok {
                diagnostics.push(Diagnostic::error(
                    "token.incompatible_property",
                    format!(
                        "node '{}': property '{}' expects a {} token but \
                         '{}' is of type '{}'",
                        node_id,
                        prop_name,
                        visual_expect_name(expect),
                        token_id,
                        token_type_name(&resolved.token_type),
                    ),
                    None,
                    Some(node_id.to_owned()),
                ));
            }
        }

        PropertyValue::Literal(_) => {
            diagnostics.push(Diagnostic::error(
                "token.raw_visual_literal",
                format!(
                    "node '{}': visual property '{}' has a raw literal value; \
                     visual properties must reference design tokens",
                    node_id, prop_name
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}

// ── Tiny helpers ──────────────────────────────────────────────────────────────

/// Register a single id; push `id.duplicate` if already seen.
///
/// Used for tokens, styles, body, pages, and all node kinds — any id-bearing
/// element in the document participates in the same global uniqueness check.
fn register_id(id: &str, seen: &mut HashSet<String>, diagnostics: &mut Vec<Diagnostic>) {
    if !seen.insert(id.to_owned()) {
        diagnostics.push(Diagnostic::error(
            "id.duplicate",
            format!(
                "id '{}' is declared more than once; IDs must be globally unique",
                id
            ),
            None,
            Some(id.to_owned()),
        ));
    }
}

fn visual_expect_name(e: VisualExpect) -> &'static str {
    match e {
        VisualExpect::Color => "color",
        VisualExpect::Dimension => "dimension",
        VisualExpect::FontFamily => "fontFamily",
    }
}

fn token_type_name(t: &TokenType) -> &str {
    match t {
        TokenType::Color => "color",
        TokenType::Dimension => "dimension",
        TokenType::Number => "number",
        TokenType::FontFamily => "fontFamily",
        TokenType::FontWeight => "fontWeight",
        TokenType::Unknown(s) => s.as_str(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ast::document::{Document, DocumentBody, Page};
    use crate::ast::node::{Node, RectNode, TextNode, UnknownNode};
    use crate::ast::style::StyleBlock;
    use crate::ast::token::{Token, TokenBlock, TokenLiteral, TokenType, TokenValue};
    use crate::ast::value::{Dimension, PropertyValue, Unit};

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
        Node::Rect(RectNode {
            id: id.to_owned(),
            name: None,
            role: None,
            x: Some(px(0.0)),
            y: Some(px(0.0)),
            w: Some(px(100.0)),
            h: Some(px(100.0)),
            radius: None,
            style: None,
            fill,
            stroke: None,
            stroke_width: None,
            stroke_alignment: None,
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        })
    }

    fn minimal_text(id: &str, fill: Option<PropertyValue>) -> Node {
        Node::Text(TextNode {
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
            style: None,
            fill,
            font_family: None,
            font_size: None,
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            spans: vec![],
            source_span: None,
            unknown_props: BTreeMap::new(),
        })
    }

    fn minimal_page(id: &str, children: Vec<Node>) -> Page {
        Page {
            id: id.to_owned(),
            name: None,
            width: px(1280.0),
            height: px(720.0),
            background: None,
            children,
            source_span: None,
        }
    }

    fn doc_with(tokens: Vec<Token>, pages: Vec<Page>) -> Document {
        Document {
            version: 1,
            project: None,
            tokens: TokenBlock {
                format: "zenith-token-v1".to_owned(),
                tokens,
            },
            styles: StyleBlock::default(),
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
                vec![Node::Rect(RectNode {
                    id: "rect.no-w".to_owned(),
                    name: None,
                    role: None,
                    x: Some(px(0.0)),
                    y: Some(px(0.0)),
                    w: None, // missing
                    h: Some(px(100.0)),
                    radius: None,
                    style: None,
                    fill: None,
                    stroke: None,
                    stroke_width: None,
                    stroke_alignment: None,
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
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
                vec![Node::Unknown(UnknownNode {
                    kind: "sparkle".to_owned(),
                    source_span: None,
                })],
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

    // ── Bonus: node with unknown property → node.unknown_property ─────────

    #[test]
    fn unknown_property_on_rect_produces_warning() {
        let mut unknown_props = BTreeMap::new();
        unknown_props.insert(
            "magic-glow".to_owned(),
            crate::ast::node::UnknownProperty {
                value: crate::ast::node::UnknownValue::String("true".to_owned()),
            },
        );
        let doc = doc_with(
            vec![],
            vec![minimal_page(
                "page.one",
                vec![Node::Rect(RectNode {
                    id: "rect.one".to_owned(),
                    name: None,
                    role: None,
                    x: Some(px(0.0)),
                    y: Some(px(0.0)),
                    w: Some(px(50.0)),
                    h: Some(px(50.0)),
                    radius: None,
                    style: None,
                    fill: None,
                    stroke: None,
                    stroke_width: None,
                    stroke_alignment: None,
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
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
                vec![Node::Rect(RectNode {
                    id: "rect.one".to_owned(),
                    name: None,
                    role: None,
                    x: Some(px(0.0)),
                    y: Some(px(0.0)),
                    w: Some(px(50.0)),
                    h: Some(px(50.0)),
                    radius: None,
                    style: None,
                    fill: None,
                    stroke: None,
                    stroke_width: Some(token_ref("size.stroke")),
                    stroke_alignment: None,
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
                    source_span: None,
                    unknown_props: BTreeMap::new(),
                })],
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
                vec![Node::Text(TextNode {
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
                    style: None,
                    fill: None,
                    font_family: Some(token_ref("font.body")),
                    font_size: None,
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
                    spans: vec![],
                    source_span: None,
                    unknown_props: BTreeMap::new(),
                })],
            )],
        );
        let report = validate(&doc);
        assert!(
            report.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}",
            codes(&report)
        );
    }
}
