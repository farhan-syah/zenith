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

use crate::ast::asset::{AssetDecl, AssetKind};
use crate::ast::document::Document;
use crate::ast::node::{Node, PolygonNode, PolylineNode};
use crate::ast::style::StyleBlock;
use crate::ast::token::TokenType;
use crate::ast::value::{Dimension, PropertyValue, Unit, dim_to_px};
use crate::color::{contrast_ratio, parse_rgb};
use crate::diagnostics::{Diagnostic, Severity};
use crate::tokens::{ResolvedToken, ResolvedValue};

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

    // Declared asset ids, collected once so the node walk can validate that
    // every `image.asset` reference points at a declared `AssetDecl.id`.
    let declared_asset_ids: HashSet<String> =
        doc.assets.assets.iter().map(|d| d.id.clone()).collect();

    // Declared style ids, collected once so the node walk can validate that
    // every `style="..."` node attribute references a declared style.
    let declared_style_ids: HashSet<String> =
        doc.styles.styles.iter().map(|s| s.id.clone()).collect();

    // ── Token IDs ─────────────────────────────────────────────────────────
    for token in &doc.tokens.tokens {
        register_id(&token.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Style IDs ─────────────────────────────────────────────────────────
    for style in &doc.styles.styles {
        register_id(&style.id, &mut seen_ids, &mut diagnostics);
    }

    // ── Style property validation ─────────────────────────────────────────
    validate_style_block(
        &doc.styles,
        resolved_tokens,
        &mut referenced_token_ids,
        &mut diagnostics,
    );

    // ── Asset IDs and per-declaration checks ──────────────────────────────
    for decl in &doc.assets.assets {
        register_id(&decl.id, &mut seen_ids, &mut diagnostics);
        validate_asset_decl(decl, &mut diagnostics);
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

        // ── Page background token: validate type/existence and record the
        //    reference so it is not falsely reported as an unused token.
        check_visual_prop(
            &page.id,
            "background",
            page.background.as_ref(),
            VisualExpect::Color,
            &mut referenced_token_ids,
            resolved_tokens,
            &mut diagnostics,
        );

        // ── Resolve page dimensions to px for off_canvas checks ──────────
        // If either dimension is unresolvable (e.g. Pct/Deg unit — already
        // diagnosed above as node.invalid_geometry), skip off_canvas checks
        // for this page to avoid spurious noise.
        let page_px_bounds = dim_to_px(page.width.value, &page.width.unit)
            .zip(dim_to_px(page.height.value, &page.height.unit));

        // ── Resolve page background color for contrast checks ────────────
        // Only a TokenRef → Color token produces a usable RGB triple.
        // If the page has no background or the token is unresolvable, we
        // set None and silently skip contrast checks for this page — we
        // cannot determine what the background is without it.
        let page_bg_rgb: Option<(u8, u8, u8)> = page.background.as_ref().and_then(|pv| {
            if let PropertyValue::TokenRef(id) = pv {
                resolved_tokens.get(id.as_str()).and_then(|rt| {
                    if let ResolvedValue::Color(hex) = &rt.value {
                        parse_rgb(hex)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        // ── Walk page children ────────────────────────────────────────────
        for node in &page.children {
            walk_node(
                node,
                &mut seen_ids,
                &mut referenced_token_ids,
                resolved_tokens,
                &declared_asset_ids,
                &declared_style_ids,
                page_px_bounds,
                &mut diagnostics,
            );
            // Contrast check runs after the structural walk so that
            // token-reference errors are already diagnosed and we can
            // safely skip nodes whose tokens didn't resolve.
            check_text_contrast(node, page_bg_rgb, resolved_tokens, &mut diagnostics);
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
///
/// `page_px_bounds` is `Some((page_w, page_h))` when the page's dimensions
/// resolved successfully; `None` means off_canvas checks are skipped for this
/// page (page unit was bad — already diagnosed).
///
/// # Known limitation
/// Recursion through `Node::Group` and `Node::Frame` children has no depth
/// guard.  Pathologically deep trees can overflow the stack.  This is an
/// accepted v0 limitation.
#[allow(clippy::too_many_arguments)]
fn walk_node(
    node: &Node,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_asset_ids: &HashSet<String>,
    declared_style_ids: &HashSet<String>,
    page_px_bounds: Option<(f64, f64)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── off_canvas advisory ───────────────────────────────────────────────
    // Check whether the node's authored bounding box exceeds the page rect
    // [0, 0, page_w, page_h]. This uses authored coordinates only — group
    // translation offsets are NOT accumulated (v0 advisory behavior; render-
    // time offset accumulation is a scene-compiler concern, not validation).
    if let Some((page_w, page_h)) = page_px_bounds
        && let Some((nx, ny, nw, nh)) = node_bbox(node, page_w, page_h)
        && (nx < 0.0 || ny < 0.0 || nx + nw > page_w || ny + nh > page_h)
    {
        let (node_id, node_span) = node_id_and_span(node);
        diagnostics.push(Diagnostic::advisory(
            "off_canvas",
            format!(
                "node '{}' extends outside the page bounds (0, 0, {page_w}, {page_h})",
                node_id
            ),
            node_span,
            Some(node_id.to_owned()),
        ));
    }

    match node {
        Node::Rect(r) => {
            register_id(&r.id, seen_ids, diagnostics);
            check_style_ref(
                &r.id,
                r.style.as_deref(),
                declared_style_ids,
                r.source_span,
                diagnostics,
            );

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

        Node::Ellipse(e) => {
            register_id(&e.id, seen_ids, diagnostics);
            check_style_ref(
                &e.id,
                e.style.as_deref(),
                declared_style_ids,
                e.source_span,
                diagnostics,
            );

            // Required geometry: x, y, w, h must all be present.
            check_optional_dim(&e.id, "x", e.x.as_ref(), e.source_span, diagnostics);
            check_optional_dim(&e.id, "y", e.y.as_ref(), e.source_span, diagnostics);
            check_optional_dim(&e.id, "w", e.w.as_ref(), e.source_span, diagnostics);
            check_optional_dim(&e.id, "h", e.h.as_ref(), e.source_span, diagnostics);

            // Visual properties (fill-only; no stroke/radius for ellipse).
            check_visual_prop(
                &e.id,
                "fill",
                e.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in e.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "ellipse '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        e.id, prop_name
                    ),
                    e.source_span,
                    Some(e.id.clone()),
                ));
            }
        }

        Node::Line(l) => {
            register_id(&l.id, seen_ids, diagnostics);
            check_style_ref(
                &l.id,
                l.style.as_deref(),
                declared_style_ids,
                l.source_span,
                diagnostics,
            );

            // Required geometry: x1, y1, x2, y2 must all be present.
            check_optional_dim(&l.id, "x1", l.x1.as_ref(), l.source_span, diagnostics);
            check_optional_dim(&l.id, "y1", l.y1.as_ref(), l.source_span, diagnostics);
            check_optional_dim(&l.id, "x2", l.x2.as_ref(), l.source_span, diagnostics);
            check_optional_dim(&l.id, "y2", l.y2.as_ref(), l.source_span, diagnostics);

            // Visual properties (stroke-only; no fill for line).
            // stroke is optional — only type-checked if present (a stroke-less
            // line draws nothing, but it is not an error to omit stroke).
            check_visual_prop(
                &l.id,
                "stroke",
                l.stroke.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &l.id,
                "stroke-width",
                l.stroke_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in l.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "line '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        l.id, prop_name
                    ),
                    l.source_span,
                    Some(l.id.clone()),
                ));
            }
        }

        Node::Text(t) => {
            register_id(&t.id, seen_ids, diagnostics);
            check_style_ref(
                &t.id,
                t.style.as_deref(),
                declared_style_ids,
                t.source_span,
                diagnostics,
            );

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
            check_visual_prop(
                &t.id,
                "font-weight",
                t.font_weight.as_ref(),
                VisualExpect::FontWeight,
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

        Node::Code(c) => {
            register_id(&c.id, seen_ids, diagnostics);
            check_style_ref(
                &c.id,
                c.style.as_deref(),
                declared_style_ids,
                c.source_span,
                diagnostics,
            );

            // Geometry (advisory box for v0; only unit-checked if present).
            check_optional_dim(&c.id, "x", c.x.as_ref(), c.source_span, diagnostics);
            check_optional_dim(&c.id, "y", c.y.as_ref(), c.source_span, diagnostics);
            check_optional_dim(&c.id, "w", c.w.as_ref(), c.source_span, diagnostics);
            check_optional_dim(&c.id, "h", c.h.as_ref(), c.source_span, diagnostics);

            // Visual properties (mirror text; overflow is not enum-validated,
            // matching how text.overflow is currently handled).
            check_visual_prop(
                &c.id,
                "fill",
                c.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &c.id,
                "font-family",
                c.font_family.as_ref(),
                VisualExpect::FontFamily,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &c.id,
                "font-size",
                c.font_size.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );

            // Unknown properties.
            for prop_name in c.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "code '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        c.id, prop_name
                    ),
                    c.source_span,
                    Some(c.id.clone()),
                ));
            }
        }

        Node::Frame(f) => {
            register_id(&f.id, seen_ids, diagnostics);
            check_style_ref(
                &f.id,
                f.style.as_deref(),
                declared_style_ids,
                f.source_span,
                diagnostics,
            );

            // Frames REQUIRE all four geometry dimensions (unlike groups).
            check_optional_dim(&f.id, "x", f.x.as_ref(), f.source_span, diagnostics);
            check_optional_dim(&f.id, "y", f.y.as_ref(), f.source_span, diagnostics);
            check_optional_dim(&f.id, "w", f.w.as_ref(), f.source_span, diagnostics);
            check_optional_dim(&f.id, "h", f.h.as_ref(), f.source_span, diagnostics);

            // Unknown properties.
            for prop_name in f.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "frame '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        f.id, prop_name
                    ),
                    f.source_span,
                    Some(f.id.clone()),
                ));
            }

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check.
            for child in &f.children {
                walk_node(
                    child,
                    seen_ids,
                    referenced_token_ids,
                    resolved_tokens,
                    declared_asset_ids,
                    declared_style_ids,
                    page_px_bounds,
                    diagnostics,
                );
            }
        }

        Node::Group(g) => {
            register_id(&g.id, seen_ids, diagnostics);
            check_style_ref(
                &g.id,
                g.style.as_deref(),
                declared_style_ids,
                g.source_span,
                diagnostics,
            );

            // Groups have NO required geometry — x/y/w/h are all advisory.

            // Unknown properties.
            for prop_name in g.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "group '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        g.id, prop_name
                    ),
                    g.source_span,
                    Some(g.id.clone()),
                ));
            }

            // Recurse into children, passing the SAME seen_ids so that
            // nested ids participate in the global uniqueness check.
            for child in &g.children {
                walk_node(
                    child,
                    seen_ids,
                    referenced_token_ids,
                    resolved_tokens,
                    declared_asset_ids,
                    declared_style_ids,
                    page_px_bounds,
                    diagnostics,
                );
            }
        }

        Node::Image(img) => {
            register_id(&img.id, seen_ids, diagnostics);
            check_style_ref(
                &img.id,
                img.style.as_deref(),
                declared_style_ids,
                img.source_span,
                diagnostics,
            );

            // Required geometry: x, y, w, h must all be present (mirror rect).
            check_optional_dim(&img.id, "x", img.x.as_ref(), img.source_span, diagnostics);
            check_optional_dim(&img.id, "y", img.y.as_ref(), img.source_span, diagnostics);
            check_optional_dim(&img.id, "w", img.w.as_ref(), img.source_span, diagnostics);
            check_optional_dim(&img.id, "h", img.h.as_ref(), img.source_span, diagnostics);

            // The referenced asset must exist in the document's assets block.
            if !declared_asset_ids.contains(&img.asset) {
                diagnostics.push(Diagnostic::error(
                    "asset.unknown_reference",
                    format!(
                        "image '{}': references asset '{}' which is not declared in the \
                         assets block",
                        img.id, img.asset
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // Validate fit (version-relative; forward-compat warning).
            if let Some(fit) = &img.fit
                && !matches!(fit.as_str(), "contain" | "cover" | "stretch" | "none")
            {
                diagnostics.push(Diagnostic::warning(
                    "image.invalid_fit",
                    format!(
                        "image '{}': unrecognized fit '{}' (version-relative; allowed \
                         values are contain, cover, stretch, none)",
                        img.id, fit
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }

            // Unknown properties.
            for prop_name in img.unknown_props.keys() {
                diagnostics.push(Diagnostic::warning(
                    "node.unknown_property",
                    format!(
                        "image '{}': unknown property '{}' (version-relative; \
                         may be valid in a later schema version)",
                        img.id, prop_name
                    ),
                    img.source_span,
                    Some(img.id.clone()),
                ));
            }
            // Image is a leaf — no child recursion.
        }

        Node::Polygon(poly) => {
            check_polygon(
                poly,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                diagnostics,
            );
        }

        Node::Polyline(poly) => {
            check_polyline(
                poly,
                seen_ids,
                referenced_token_ids,
                resolved_tokens,
                declared_style_ids,
                diagnostics,
            );
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

// ── WCAG 2.2 contrast advisory ────────────────────────────────────────────────

/// Recursively check text nodes against the page background for WCAG AA contrast.
///
/// # v0 Limitations
/// - Only compares against the PAGE background color; an intervening rect or
///   scrim the text visually sits on is NOT detected or used.
/// - Per-span fills (TextSpan.fill) are NOT individually checked; the node-level
///   `fill` is used as a proxy for all spans.
/// - Only `t.fill` TokenRef → Color is resolved; style-inherited fills are NOT
///   consulted (style fill resolution requires the full style block lookup, which
///   is a v1 concern; for now, a text node with no direct fill is simply skipped).
fn check_text_contrast(
    node: &Node,
    page_bg_rgb: Option<(u8, u8, u8)>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // If we don't know the page background we cannot compute contrast — bail.
    let Some(bg_rgb) = page_bg_rgb else {
        return;
    };

    match node {
        Node::Text(t) => {
            // Resolve the text fill color from a TokenRef → Color token.
            // If no fill is set or it doesn't resolve to a color, skip.
            let text_rgb = match t.fill.as_ref() {
                Some(PropertyValue::TokenRef(id)) => {
                    resolved_tokens.get(id.as_str()).and_then(|rt| {
                        if let ResolvedValue::Color(hex) = &rt.value {
                            parse_rgb(hex)
                        } else {
                            None
                        }
                    })
                }
                // Literal / Dimension fills are caught as raw_visual_literal errors
                // elsewhere; no need to chase them here.
                _ => None,
            };

            let Some(fg_rgb) = text_rgb else {
                return;
            };

            // Resolve font-size in px (default 16.0 px when absent).
            let size_px: f64 = t
                .font_size
                .as_ref()
                .and_then(|pv| {
                    if let PropertyValue::TokenRef(id) = pv {
                        resolved_tokens.get(id.as_str()).and_then(|rt| {
                            if let ResolvedValue::Dimension(dim) = &rt.value {
                                dim_to_px(dim.value, &dim.unit)
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(16.0);

            // Resolve font-weight as u32 (default 400 when absent).
            let weight: u32 = t
                .font_weight
                .as_ref()
                .and_then(|pv| {
                    if let PropertyValue::TokenRef(id) = pv {
                        resolved_tokens.get(id.as_str()).and_then(|rt| {
                            if let ResolvedValue::FontWeight(w) = &rt.value {
                                Some(*w)
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(400);

            // WCAG AA large text: >= 24 px OR >= 18.66 px bold (>= 700).
            let is_large = size_px >= 24.0 || (size_px >= 18.66 && weight >= 700);
            let threshold = if is_large { 3.0_f64 } else { 4.5_f64 };

            let ratio = contrast_ratio(fg_rgb, bg_rgb);

            if ratio < threshold {
                diagnostics.push(Diagnostic::warning(
                    "contrast.low",
                    format!(
                        "text '{}': contrast ratio {:.2}:1 of fill on page background \
                         is below WCAG AA ({:.1}:1)",
                        t.id, ratio, threshold
                    ),
                    t.source_span,
                    Some(t.id.clone()),
                ));
            }
        }

        // Recurse into container nodes, passing the same page_bg through.
        // Group and Frame children may contain text nodes.
        Node::Group(g) => {
            for child in &g.children {
                check_text_contrast(child, page_bg_rgb, resolved_tokens, diagnostics);
            }
        }
        Node::Frame(f) => {
            for child in &f.children {
                check_text_contrast(child, page_bg_rgb, resolved_tokens, diagnostics);
            }
        }

        // All other leaf node types carry no text — nothing to check.
        _ => {}
    }
}

// ── off_canvas geometry helpers ───────────────────────────────────────────────

/// Resolve a single geometry axis dimension to pixels.
///
/// `Pct` is resolved against `basis` (e.g. page_w for x/w, page_h for y/h).
/// All other convertible units delegate to [`dim_to_px`]; `None` on failure.
fn resolve_axis(dim: &Dimension, basis: f64) -> Option<f64> {
    if dim.unit == Unit::Pct {
        Some(dim.value / 100.0 * basis)
    } else {
        dim_to_px(dim.value, &dim.unit)
    }
}

/// Compute the authored bounding box `(x, y, w, h)` of a node in pixels.
///
/// Returns `None` when the node has no resolvable bounding box (Group, Unknown,
/// or any node with a missing/unresolvable required dimension). Callers should
/// treat `None` as "no check possible" and produce no advisory.
///
/// v0 NOTE: authored coordinates are used as-is. Group translation offsets are
/// NOT accumulated here (that is a scene-compiler / render-time concern). The
/// off_canvas advisory documents this v0 behavior: it checks authored geometry
/// against the page rectangle, not render-time geometry.
fn node_bbox(node: &Node, page_w: f64, page_h: f64) -> Option<(f64, f64, f64, f64)> {
    match node {
        Node::Rect(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Ellipse(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Image(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Frame(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Text(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Code(n) => {
            let x = resolve_axis(n.x.as_ref()?, page_w)?;
            let y = resolve_axis(n.y.as_ref()?, page_h)?;
            let w = resolve_axis(n.w.as_ref()?, page_w)?;
            let h = resolve_axis(n.h.as_ref()?, page_h)?;
            Some((x, y, w, h))
        }
        Node::Line(n) => {
            let x1 = resolve_axis(n.x1.as_ref()?, page_w)?;
            let y1 = resolve_axis(n.y1.as_ref()?, page_h)?;
            let x2 = resolve_axis(n.x2.as_ref()?, page_w)?;
            let y2 = resolve_axis(n.y2.as_ref()?, page_h)?;
            let bx = x1.min(x2);
            let by = y1.min(y2);
            let bw = (x2 - x1).abs();
            let bh = (y2 - y1).abs();
            Some((bx, by, bw, bh))
        }
        Node::Polygon(n) => points_bbox(&n.points, page_w, page_h),
        Node::Polyline(n) => points_bbox(&n.points, page_w, page_h),
        // Groups have no authoritative bbox in v0 — children are checked individually.
        Node::Group(_) | Node::Unknown(_) => None,
    }
}

/// Compute the bounding box of a slice of [`Point`]s, resolving each coordinate
/// against the given page axis bases.
///
/// Returns `Some((min_x, min_y, w, h))` when at least one point resolves
/// successfully, `None` when no point has resolvable coordinates.
fn points_bbox(
    points: &[crate::ast::node::Point],
    page_w: f64,
    page_h: f64,
) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;
    for pt in points {
        if let (Some(px_val), Some(py_val)) = (
            pt.x.as_ref().and_then(|d| resolve_axis(d, page_w)),
            pt.y.as_ref().and_then(|d| resolve_axis(d, page_h)),
        ) {
            min_x = min_x.min(px_val);
            min_y = min_y.min(py_val);
            max_x = max_x.max(px_val);
            max_y = max_y.max(py_val);
            any = true;
        }
    }
    if any {
        Some((min_x, min_y, max_x - min_x, max_y - min_y))
    } else {
        None
    }
}

/// Extract the string id and source span from any node variant.
fn node_id_and_span(node: &Node) -> (&str, Option<crate::ast::Span>) {
    match node {
        Node::Rect(n) => (&n.id, n.source_span),
        Node::Ellipse(n) => (&n.id, n.source_span),
        Node::Line(n) => (&n.id, n.source_span),
        Node::Text(n) => (&n.id, n.source_span),
        Node::Code(n) => (&n.id, n.source_span),
        Node::Frame(n) => (&n.id, n.source_span),
        Node::Group(n) => (&n.id, n.source_span),
        Node::Image(n) => (&n.id, n.source_span),
        Node::Polygon(n) => (&n.id, n.source_span),
        Node::Polyline(n) => (&n.id, n.source_span),
        Node::Unknown(n) => (&n.kind, n.source_span),
    }
}

// ── polygon / polyline validation ─────────────────────────────────────────────

fn check_polygon(
    poly: &PolygonNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y (both must be present with a known unit).
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            poly.source_span,
            diagnostics,
        );
    }

    // polygon requires at least 3 points.
    if poly.points.len() < 3 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polygon '{}': requires at least 3 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polygon is a LEAF: no child-node recursion (points are sub-data).
}

fn check_polyline(
    poly: &PolylineNode,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    declared_style_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y.
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            poly.source_span,
            diagnostics,
        );
    }

    // polyline requires at least 2 points.
    if poly.points.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polyline '{}': requires at least 2 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polyline is a LEAF: no child-node recursion (points are sub-data).
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
    FontWeight,
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
                VisualExpect::FontWeight => {
                    matches!(resolved.token_type, TokenType::FontWeight)
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

        PropertyValue::Literal(_) | PropertyValue::Dimension(_) => {
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

/// Validate a single [`AssetDecl`] beyond ID uniqueness:
/// - unknown kind → `asset.invalid_kind` (Error)
/// - unsafe src path → `asset.invalid_src` (Error)
/// - unknown properties → `asset.unknown_property` (Warning)
fn validate_asset_decl(decl: &AssetDecl, diagnostics: &mut Vec<Diagnostic>) {
    // ── Kind check ────────────────────────────────────────────────────────
    if let AssetKind::Unknown(unknown_kind) = &decl.kind {
        diagnostics.push(Diagnostic::error(
            "asset.invalid_kind",
            format!(
                "asset '{}': unknown kind '{}'; \
                 recognized kinds are: image, svg, font",
                decl.id, unknown_kind
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }

    // ── Src sanity check ──────────────────────────────────────────────────
    // Reject: absolute paths (starts with `/` or Windows drive `X:\`),
    // parent-traversal segments (`..`), and URLs (contain `://`).
    let src = &decl.src;
    let is_absolute_unix = src.starts_with('/');
    // Windows drive: one ASCII letter followed by `:\` or `:/`
    let is_absolute_windows = src.len() >= 3
        && src.as_bytes()[0].is_ascii_alphabetic()
        && src.as_bytes()[1] == b':'
        && (src.as_bytes()[2] == b'\\' || src.as_bytes()[2] == b'/');
    let is_url = src.contains("://");
    // Parent traversal: segment `..` in any position.
    let has_traversal = src == ".."
        || src.starts_with("../")
        || src.starts_with("..\\")
        || src.contains("/../")
        || src.contains("/..\\")
        || src.contains("\\..\\")
        || src.contains("\\../")
        || src.ends_with("/..")
        || src.ends_with("\\..");

    if is_absolute_unix || is_absolute_windows || is_url || has_traversal {
        diagnostics.push(Diagnostic::error(
            "asset.invalid_src",
            format!(
                "asset '{}': src '{}' is not a safe relative path; \
                 absolute paths, parent-traversal segments ('..'), \
                 and URLs are not allowed",
                decl.id, src
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }

    // ── Unknown properties ────────────────────────────────────────────────
    for prop_name in decl.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "asset.unknown_property",
            format!(
                "asset '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                decl.id, prop_name
            ),
            decl.source_span,
            Some(decl.id.clone()),
        ));
    }
}

// ── Style helpers ─────────────────────────────────────────────────────────────

/// Check that a node's `style` attribute references a declared style id.
///
/// Called for every node kind that carries a `style` field.
fn check_style_ref(
    node_id: &str,
    style_opt: Option<&str>,
    declared_style_ids: &HashSet<String>,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(sid) = style_opt
        && !declared_style_ids.contains(sid)
    {
        diagnostics.push(Diagnostic::error(
            "style.unknown_reference",
            format!(
                "node '{}': references style '{}' which is not declared in the styles block",
                node_id, sid
            ),
            span,
            Some(node_id.to_owned()),
        ));
    }
}

/// Validate the contents of the `styles` block:
/// - Each `(key, value)` in `Style.properties` is type-checked against the
///   expected token category and tracked as a token reference.
/// - Each entry in `Style.unknown_props` produces a `style.unknown_property`
///   Warning.
fn validate_style_block(
    block: &StyleBlock,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    referenced_token_ids: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for style in &block.styles {
        // Check recognized properties.
        for (key, value) in &style.properties {
            let expect = style_prop_expect(key);
            if let Some(expect) = expect {
                check_visual_prop(
                    &style.id,
                    key,
                    Some(value),
                    expect,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
            } else {
                // stroke-alignment and font-weight: no strict type check;
                // still track token refs so they count as used.
                if let PropertyValue::TokenRef(tid) = value {
                    referenced_token_ids.insert(tid.clone());
                }
            }
        }

        // Warn on unknown properties.
        for prop_name in style.unknown_props.keys() {
            diagnostics.push(Diagnostic::warning(
                "style.unknown_property",
                format!(
                    "style '{}': unknown property '{}' (not a recognized visual property; \
                     this property will not be applied to nodes that reference this style)",
                    style.id, prop_name
                ),
                style.source_span,
                Some(style.id.clone()),
            ));
        }
    }
}

/// Map a canonical style property key to its expected token type.
///
/// Returns `None` for keys that have no strict type expectation in v0
/// (`stroke-alignment`, `font-weight`).
fn style_prop_expect(key: &str) -> Option<VisualExpect> {
    match key {
        "fill" | "stroke" => Some(VisualExpect::Color),
        "stroke-width" | "font-size" | "line-height" | "radius" => Some(VisualExpect::Dimension),
        "font-family" => Some(VisualExpect::FontFamily),
        // stroke-alignment: plain enum string, not type-checked.
        // font-weight: fontWeight token type — no VisualExpect variant for it; skip check.
        _ => None,
    }
}

fn visual_expect_name(e: VisualExpect) -> &'static str {
    match e {
        VisualExpect::Color => "color",
        VisualExpect::Dimension => "dimension",
        VisualExpect::FontFamily => "fontFamily",
        VisualExpect::FontWeight => "fontWeight",
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
    use crate::ast::asset::{AssetBlock, AssetDecl, AssetKind};
    use crate::ast::document::{Document, DocumentBody, Page};
    use crate::ast::node::{
        CodeNode, EllipseNode, FrameNode, GroupNode, LineNode, Node, RectNode, TextNode,
        UnknownNode,
    };
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

    fn minimal_ellipse(id: &str, fill: Option<PropertyValue>) -> Node {
        Node::Ellipse(EllipseNode {
            id: id.to_owned(),
            name: None,
            role: None,
            x: Some(px(0.0)),
            y: Some(px(0.0)),
            w: Some(px(100.0)),
            h: Some(px(100.0)),
            style: None,
            fill,
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
            font_weight: None,
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            spans: vec![],
            source_span: None,
            unknown_props: BTreeMap::new(),
        })
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
            assets: AssetBlock::default(),
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

    // ── Test 4b: font-weight referencing a missing token ──────────────────

    #[test]
    fn font_weight_with_missing_token_ref_produces_unknown_reference() {
        let text = Node::Text(TextNode {
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
            style: None,
            fill: None,
            font_family: None,
            font_size: None,
            font_weight: Some(token_ref("weight.does.not.exist")),
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            spans: vec![],
            source_span: None,
            unknown_props: BTreeMap::new(),
        });
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
        let child_rect = Node::Rect(RectNode {
            id: "rect.inner".to_owned(),
            name: None,
            role: None,
            x: None, // missing — triggers node.missing_geometry
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
            unknown_props: BTreeMap::new(),
        });
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
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            style: None,
            children,
            source_span: None,
            unknown_props: BTreeMap::new(),
        })
    }

    // ── Frame: clean doc with valid frame + child rect → no diagnostics ───

    #[test]
    fn frame_clean_doc_no_errors() {
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
                    vec![minimal_rect("rect.inner", Some(token_ref("color.fill")))],
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
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
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
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
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
        let child_rect = Node::Rect(RectNode {
            id: "rect.inner".to_owned(),
            name: None,
            role: None,
            x: None, // missing
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
            unknown_props: BTreeMap::new(),
        });
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
                    opacity: None,
                    visible: None,
                    locked: None,
                    rotate: None,
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
                    font_weight: None,
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
                    id: "ellipse.no-w".to_owned(),
                    name: None,
                    role: None,
                    x: Some(px(0.0)),
                    y: Some(px(0.0)),
                    w: None, // missing
                    h: Some(px(100.0)),
                    style: None,
                    fill: None,
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
            project: None,
            assets: AssetBlock {
                assets,
                source_span: None,
            },
            tokens: TokenBlock {
                format: "zenith-token-v1".to_owned(),
                tokens: vec![],
            },
            styles: StyleBlock::default(),
            body: DocumentBody {
                id: "doc.asset-test".to_owned(),
                title: None,
                pages: vec![],
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

    use crate::ast::node::ImageNode;

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
            id: id.to_owned(),
            name: None,
            role: None,
            asset: asset.to_owned(),
            x: Some(px(40.0)),
            y: Some(px(40.0)),
            w: Some(px(160.0)),
            h: Some(px(120.0)),
            fit: fit.map(str::to_owned),
            object_position_x: None,
            object_position_y: None,
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
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

    // ── Style validation tests ─────────────────────────────────────────────

    use crate::ast::style::{Style, UnknownStyleProp};

    fn doc_with_styles(tokens: Vec<Token>, styles: Vec<Style>, pages: Vec<Page>) -> Document {
        Document {
            version: 1,
            project: None,
            assets: AssetBlock::default(),
            tokens: TokenBlock {
                format: "zenith-token-v1".to_owned(),
                tokens,
            },
            styles: StyleBlock {
                styles,
                source_span: None,
            },
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
            children,
            source_span: None,
        }
    }

    /// Helper: rect at (x, y, w, h) in px, no fill.
    fn rect_at(id: &str, x: f64, y: f64, w: f64, h: f64) -> Node {
        Node::Rect(RectNode {
            id: id.to_owned(),
            name: None,
            role: None,
            x: Some(px(x)),
            y: Some(px(y)),
            w: Some(px(w)),
            h: Some(px(h)),
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
        })
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
        Node::Text(crate::ast::node::TextNode {
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
            fill: fill_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
            font_family: None,
            font_size: font_size_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
            font_weight: font_weight_token.map(|t| PropertyValue::TokenRef(t.to_owned())),
            opacity: None,
            visible: None,
            locked: None,
            rotate: None,
            spans: vec![],
            source_span: None,
            unknown_props: BTreeMap::new(),
        })
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
}
