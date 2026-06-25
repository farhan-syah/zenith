//! Visual-property validation: token-reference integrity, type compatibility,
//! and raw-literal detection.
//!
//! [`check_visual_prop`] is the single entry point used by the node walk, the
//! page-background check, and the style-block check. It also records every
//! referenced token id (transitively for gradient/shadow tokens) so the
//! unused-token pass can diff against the defined token ids.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::block_style::BlockStyle;
use crate::ast::token::TokenType;
use crate::ast::value::PropertyValue;
use crate::diagnostics::Diagnostic;
use crate::tokens::{ResolvedToken, ResolvedValue};

/// The expected token type for a visual property.
///
/// Only the subset of visual properties that have defined expectations in v0
/// are listed here. Properties with no expectation (e.g. `line-height`,
/// `padding`, `gap`) are skipped to avoid false-positives — the contract
/// says "if a property has no defined expectation yet, skip it."
#[derive(Debug, Clone, Copy)]
pub(super) enum VisualExpect {
    Color,
    /// A fill/background slot that accepts either a color or a gradient token.
    ColorOrGradient,
    Dimension,
    FontFamily,
    FontWeight,
    /// A shadow slot that accepts a shadow token.
    Shadow,
    /// A filter slot that accepts a filter token.
    Filter,
    /// A mask slot that accepts a mask token.
    Mask,
}

/// Check a single visual property value:
/// - `None` → no-op (property is optional).
/// - `TokenRef(id)` → record the reference; check existence and type compat.
/// - `Literal(...)` → `token.raw_visual_literal` (Error).
pub(super) fn check_visual_prop(
    node_id: &str,
    prop_name: &str,
    value: Option<&PropertyValue>,
    expect: VisualExpect,
    referenced_token_ids: &mut BTreeSet<String>,
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
                         does not exist or failed resolution \
                         — check the spelling and that the token is declared in the `tokens` block",
                        node_id, prop_name, token_id
                    ),
                    None,
                    Some(node_id.to_owned()),
                ));
                return;
            };

            // If this is a gradient token, its stop color tokens are referenced
            // transitively — record them so they are not falsely flagged
            // `token.unused`.
            if let ResolvedValue::Gradient(g) = &resolved.value {
                for (_, color_id) in &g.stops {
                    referenced_token_ids.insert(color_id.clone());
                }
            }

            // Likewise, a shadow token references its per-layer color tokens
            // transitively — record them so they are not falsely flagged
            // `token.unused`.
            if let ResolvedValue::Shadow(s) = &resolved.value {
                for layer in &s.layers {
                    referenced_token_ids.insert(layer.color_token.clone());
                }
            }

            // A filter token may carry duotone ops that reference shadow/highlight
            // color tokens transitively — record them so they are not falsely
            // flagged `token.unused`.
            if let ResolvedValue::Filter(f) = &resolved.value {
                for op in &f.ops {
                    if let Some(c) = &op.shadow {
                        referenced_token_ids.insert(c.clone());
                    }
                    if let Some(c) = &op.highlight {
                        referenced_token_ids.insert(c.clone());
                    }
                }
            }

            // Type compatibility check.
            let type_ok = match expect {
                VisualExpect::Color => {
                    matches!(resolved.token_type, TokenType::Color)
                }
                VisualExpect::ColorOrGradient => {
                    matches!(resolved.token_type, TokenType::Color | TokenType::Gradient)
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
                VisualExpect::Shadow => {
                    matches!(resolved.token_type, TokenType::Shadow)
                }
                VisualExpect::Filter => {
                    matches!(resolved.token_type, TokenType::Filter)
                }
                VisualExpect::Mask => {
                    matches!(resolved.token_type, TokenType::Mask)
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
                     visual properties must reference design tokens \
                     — define a token and reference it as `(token)\"token-id\"`",
                    node_id, prop_name
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }

        // A data-binding reference is a valid future-facing value; no error is
        // emitted here — scene-side resolution emits `data.missing_field` or
        // `data.no_context` advisories at compile time when needed.
        PropertyValue::DataRef(_) => {}
    }
}

/// Check all token-referencing properties in a slice of `block role="…"` decls.
///
/// Called at the three block-style scopes (document body, page, text node) so
/// that tokens referenced ONLY via a `block` decl are recorded as used and any
/// missing or wrong-type token reference is diagnosed. Non-token fields
/// (`align`, `italic`, `space_before`, `space_after`) carry no token ref —
/// they are skipped.
pub(super) fn check_block_styles(
    scope_id: &str,
    block_styles: &[BlockStyle],
    referenced_token_ids: &mut BTreeSet<String>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for bs in block_styles {
        let label = format!("{scope_id}[block role=\"{}\"]", bs.role);
        check_visual_prop(
            &label,
            "font-family",
            bs.font_family.as_ref(),
            VisualExpect::FontFamily,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            &label,
            "font-size",
            bs.font_size.as_ref(),
            VisualExpect::Dimension,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            &label,
            "font-weight",
            bs.font_weight.as_ref(),
            VisualExpect::FontWeight,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        check_visual_prop(
            &label,
            "fill",
            bs.fill.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }
}

fn visual_expect_name(e: VisualExpect) -> &'static str {
    match e {
        VisualExpect::Color => "color",
        VisualExpect::ColorOrGradient => "color or gradient",
        VisualExpect::Dimension => "dimension",
        VisualExpect::FontFamily => "fontFamily",
        VisualExpect::FontWeight => "fontWeight",
        VisualExpect::Shadow => "shadow",
        VisualExpect::Filter => "filter",
        VisualExpect::Mask => "mask",
    }
}

fn token_type_name(t: &TokenType) -> &str {
    match t {
        TokenType::Color => "color",
        TokenType::Dimension => "dimension",
        TokenType::Number => "number",
        TokenType::FontFamily => "fontFamily",
        TokenType::FontWeight => "fontWeight",
        TokenType::Gradient => "gradient",
        TokenType::Shadow => "shadow",
        TokenType::Filter => "filter",
        TokenType::Mask => "mask",
        TokenType::Unknown(s) => s.as_str(),
    }
}
