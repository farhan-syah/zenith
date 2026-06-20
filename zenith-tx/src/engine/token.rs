//! Token op application: [`apply_create_token`] and [`apply_update_token_value`].

use zenith_core::{Diagnostic, Document, Token, TokenLiteral, TokenType, TokenValue};

use super::record_affected;
use super::structure::parse_dimension_str;

// ── Shared value-parsing helper ───────────────────────────────────────────────

/// Parse a literal value string against the given [`TokenType`], producing a
/// [`TokenLiteral`] on success or `None` on failure.
///
/// - `Color` / `FontFamily` → [`TokenLiteral::String`] (verbatim, including any
///   leading `#`).
/// - `Dimension` → [`TokenLiteral::Dimension`] via the canonical `"(unit)value"`
///   parser (e.g. `"(px)40"`).  Returns `None` if the string is not that form or
///   the number is not finite.
/// - `Number` / `FontWeight` → [`TokenLiteral::Number`] via `f64` parse; must be
///   finite.
/// - `Gradient` / `Shadow` / `Unknown(_)` → `None` (unsupported scalar form;
///   caller emits `tx.invalid_value`).
fn parse_token_literal(token_type: &TokenType, value: &str) -> Option<TokenLiteral> {
    match token_type {
        TokenType::Color | TokenType::FontFamily => Some(TokenLiteral::String(value.to_owned())),
        TokenType::Dimension => {
            let dim = parse_dimension_str(value)?;
            Some(TokenLiteral::Dimension(dim))
        }
        TokenType::Number | TokenType::FontWeight => {
            let n: f64 = value.trim().parse().ok()?;
            if n.is_finite() {
                Some(TokenLiteral::Number(n))
            } else {
                None
            }
        }
        TokenType::Gradient | TokenType::Shadow | TokenType::Unknown(_) => None,
    }
}

/// Return a human-readable name for a [`TokenType`] suitable for diagnostic
/// messages.
fn token_type_name(token_type: &TokenType) -> &str {
    match token_type {
        TokenType::Color => "color",
        TokenType::Dimension => "dimension",
        TokenType::Number => "number",
        TokenType::FontFamily => "fontFamily",
        TokenType::FontWeight => "fontWeight",
        TokenType::Gradient => "gradient",
        TokenType::Shadow => "shadow",
        TokenType::Unknown(s) => s.as_str(),
    }
}

// ── CreateToken ───────────────────────────────────────────────────────────────

/// Create a new design token in `doc.tokens.tokens`.
///
/// Eagerly rejects with `tx.duplicate_id` if a token with `id` already exists.
/// Rejects with `tx.invalid_value` if `token_type` maps to a gradient, shadow,
/// or unknown type (v0: scalar types only), or if `value` does not parse for
/// the given type.  On success pushes the new token and records `id` in
/// `affected`.
pub(super) fn apply_create_token(
    id: &str,
    token_type: &str,
    value: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Eager duplicate-id check.
    if doc.tokens.tokens.iter().any(|t| t.id == id) {
        diagnostics.push(Diagnostic::error(
            "tx.duplicate_id",
            format!("create_token: a token with id {:?} already exists", id),
            None,
            Some(id.to_owned()),
        ));
        return;
    }

    let ty = TokenType::from_type_name(token_type);

    // Reject unsupported complex types (gradient / shadow / unknown).
    match &ty {
        TokenType::Gradient | TokenType::Shadow | TokenType::Unknown(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "create_token: token type {:?} is not supported via this op \
                     (gradient/shadow/unknown tokens must be authored in source)",
                    token_type_name(&ty)
                ),
                None,
                Some(id.to_owned()),
            ));
            return;
        }
        TokenType::Color
        | TokenType::Dimension
        | TokenType::Number
        | TokenType::FontFamily
        | TokenType::FontWeight => {}
    }

    // Parse the value against the resolved type.
    let Some(lit) = parse_token_literal(&ty, value) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "create_token: value {:?} is not valid for token type {:?}",
                value,
                token_type_name(&ty)
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };

    doc.tokens.tokens.push(Token {
        id: id.to_owned(),
        token_type: ty,
        value: TokenValue::Literal(lit),
        source_span: None,
    });

    record_affected(id, affected);
}

// ── UpdateTokenValue ──────────────────────────────────────────────────────────

/// Replace the literal value of an existing token, preserving its declared type.
///
/// Rejects with `tx.unknown_token` if no token with `id` exists.  Rejects with
/// `tx.invalid_value` if the token is a gradient or shadow type (unsupported via
/// this op), or if `value` does not parse for the token's existing type.  On
/// success replaces `token.value` and records `id` in `affected`.
pub(super) fn apply_update_token_value(
    id: &str,
    value: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Find the token index first (shared borrow), then mutate.
    let Some(idx) = doc.tokens.tokens.iter().position(|t| t.id == id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_token",
            format!("update_token_value: no token with id {:?} exists", id),
            None,
            Some(id.to_owned()),
        ));
        return;
    };

    // Clone the type so we can release the shared borrow before mutating.
    // SAFETY: idx came from .position() on the same Vec with no intervening
    // mutation; .get() is used here to satisfy the no-unchecked-index rule.
    let Some(ty) = doc.tokens.tokens.get(idx).map(|t| t.token_type.clone()) else {
        return; // unreachable: idx is valid for this Vec
    };

    // Reject unsupported complex types.
    match &ty {
        TokenType::Gradient | TokenType::Shadow | TokenType::Unknown(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "update_token_value: token {:?} has type {:?} which cannot be \
                     updated via this op",
                    id,
                    token_type_name(&ty)
                ),
                None,
                Some(id.to_owned()),
            ));
            return;
        }
        TokenType::Color
        | TokenType::Dimension
        | TokenType::Number
        | TokenType::FontFamily
        | TokenType::FontWeight => {}
    }

    // Parse the new value against the token's existing type.
    let Some(lit) = parse_token_literal(&ty, value) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "update_token_value: value {:?} is not valid for token type {:?}",
                value,
                token_type_name(&ty)
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };

    // SAFETY: idx is still valid — no insertions/removals since .position().
    if let Some(t) = doc.tokens.tokens.get_mut(idx) {
        t.value = TokenValue::Literal(lit);
    }

    record_affected(id, affected);
}
