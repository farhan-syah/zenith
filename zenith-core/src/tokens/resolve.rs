//! Token graph resolution: validates literals, follows alias chains, detects
//! cycles, and collects diagnostics — never hard-fails.
//!
//! # Algorithm overview
//!
//! 1. Build an index `id → &Token`, detecting duplicates with
//!    `token.duplicate_id` (first definition wins).
//! 2. Walk every token in source order:
//!    - If `Unknown` type → `token.unknown_type` (Warning); skip resolution.
//!    - If `Reference` → follow the alias chain iteratively with a visited set
//!      to detect cycles. Bounded by the number of distinct token IDs, so it
//!      can never loop infinitely.
//!    - If `Literal` → validate shape for the declared type.
//! 3. Emit `token.invalid_value`, `token.unknown_reference`,
//!    `token.cyclic_reference`, and `token.type_mismatch` as appropriate.
//! 4. Populate `resolved` only for tokens that passed all checks.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::token::{
    GradientLiteral, ShadowLiteral, Token, TokenBlock, TokenLiteral, TokenType, TokenValue,
};
use crate::ast::value::{Dimension, Unit};
use crate::diagnostics::Diagnostic;

// ── Public types ─────────────────────────────────────────────────────────────

/// The resolved, validated value of a single design token.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedValue {
    /// An sRGB-origin color, stored as canonical `#rrggbb`/`#rrggbbaa` hex.
    Color(String),
    /// A CMYK-origin color. `hex` is the naive device-conversion sRGB
    /// approximation (so every existing hex consumer works unchanged); `c`,
    /// `m`, `y`, `k` are the original percentages in `0.0..=100.0`, carried so a
    /// future PDF backend can emit native DeviceCMYK.
    CmykColor {
        hex: String,
        c: f32,
        m: f32,
        y: f32,
        k: f32,
    },
    Dimension(Dimension),
    Number(f64),
    FontFamily(String),
    FontWeight(u32),
    Gradient(ResolvedGradient),
    Shadow(ResolvedShadow),
}

impl ResolvedValue {
    /// The sRGB hex string for any color-origin value (`Color` or `CmykColor`),
    /// or `None` for non-color values. Lets color consumers treat both color
    /// variants uniformly without duplicating match arms.
    pub fn as_color_hex(&self) -> Option<&str> {
        match self {
            ResolvedValue::Color(hex) => Some(hex.as_str()),
            ResolvedValue::CmykColor { hex, .. } => Some(hex.as_str()),
            _ => None,
        }
    }

    /// The original CMYK channels `(c, m, y, k)` for a `CmykColor`, or `None`
    /// for sRGB-origin colors and non-color values.
    pub fn cmyk(&self) -> Option<(f32, f32, f32, f32)> {
        match self {
            ResolvedValue::CmykColor { c, m, y, k, .. } => Some((*c, *m, *y, *k)),
            _ => None,
        }
    }
}

/// A resolved gradient: an angle plus an ordered list of `(offset, color token
/// id)` stops. Offsets are clamped into `0.0..=1.0`. Stop-color existence and
/// type are checked in a second pass over the fully-resolved token map.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGradient {
    /// Angle in degrees, clockwise from +x.
    pub angle_deg: f64,
    /// Ordered `(offset, color_token_id)` stops.
    pub stops: Vec<(f64, String)>,
}

/// A resolved shadow: an ordered list of layers. Blur is clamped to `>= 0`.
/// Layer-color existence and type are checked in a second pass over the
/// fully-resolved token map (exactly like gradient stops).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShadow {
    /// Ordered list of resolved layers, in source order.
    pub layers: Vec<ResolvedShadowLayer>,
}

/// A single resolved shadow layer: offsets and blur (pixels) plus the id of the
/// color token this layer renders with.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShadowLayer {
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    pub color_token: String,
}

/// A successfully resolved token (type + value pair).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedToken {
    pub token_type: TokenType,
    pub value: ResolvedValue,
}

/// The outcome of resolving a [`TokenBlock`].
///
/// `resolved` contains only tokens that passed all validation checks.
/// `diagnostics` contains every problem found (may be non-empty even when
/// some tokens resolved successfully).
#[derive(Debug, Clone)]
pub struct TokenResolution {
    /// Successfully resolved tokens, keyed by token ID, sorted by ID.
    pub resolved: BTreeMap<String, ResolvedToken>,
    /// All diagnostics collected during resolution.
    pub diagnostics: Vec<Diagnostic>,
}

// ── Entry point ──────────────────────────────────────────────────────────────

/// Resolve all tokens in `block`, collecting diagnostics without hard-failing.
///
/// Tokens that cannot be resolved (e.g., due to an unknown reference or cycle)
/// are omitted from `resolved`; all other tokens are resolved and included.
pub fn resolve_tokens(block: &TokenBlock) -> TokenResolution {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut resolved: BTreeMap<String, ResolvedToken> = BTreeMap::new();

    // ── Step 1: build index, detecting duplicate IDs ─────────────────────
    // `index` maps id → token reference (first definition wins).
    let mut index: HashMap<&str, &Token> = HashMap::new();
    // Track which IDs have been seen for deterministic duplicate detection.
    let mut seen_ids: HashSet<&str> = HashSet::new();

    for token in &block.tokens {
        if seen_ids.contains(token.id.as_str()) {
            diagnostics.push(Diagnostic::error(
                "token.duplicate_id",
                format!(
                    "token '{}' is defined more than once; the second definition is ignored",
                    token.id
                ),
                token.source_span,
                Some(token.id.clone()),
            ));
            // First definition already in index; skip.
        } else {
            seen_ids.insert(token.id.as_str());
            index.insert(token.id.as_str(), token);
        }
    }

    // ── Step 2: resolve each first-definition token ───────────────────────
    for token in &block.tokens {
        // Only process the canonical (first-definition) entry for each ID.
        // `index.get()` returns None for duplicates (which were never inserted),
        // and Some(ptr) != token for any future edge-case; neither path panics.
        let Some(canonical) = index.get(token.id.as_str()) else {
            continue;
        };
        if !std::ptr::eq(*canonical, token) {
            continue;
        }

        // Unknown type → advisory warning, skip resolution.
        if let TokenType::Unknown(ref type_name) = token.token_type {
            diagnostics.push(Diagnostic::warning(
                "token.unknown_type",
                format!(
                    "token '{}' has unrecognized type '{}' (version-relative; \
                     this type may be valid in a later schema version)",
                    token.id, type_name
                ),
                token.source_span,
                Some(token.id.clone()),
            ));
            continue;
        }

        // Resolve to a concrete literal (following aliases as needed).
        match resolve_token_to_literal(token, &index, &mut diagnostics) {
            Some((literal, resolved_type)) => {
                // Type must match the declaring token's type.
                if resolved_type != token.token_type {
                    diagnostics.push(Diagnostic::error(
                        "token.type_mismatch",
                        format!(
                            "token '{}' has declared type '{}' but its alias \
                             chain resolves to a token of type '{}'",
                            token.id,
                            type_name_of(&token.token_type),
                            type_name_of(&resolved_type),
                        ),
                        token.source_span,
                        Some(token.id.clone()),
                    ));
                    continue;
                }

                // Validate the literal's shape against the declared type.
                match validate_literal(
                    &token.id,
                    &token.token_type,
                    &literal,
                    token.source_span,
                    &mut diagnostics,
                ) {
                    Some(rv) => {
                        resolved.insert(
                            token.id.clone(),
                            ResolvedToken {
                                token_type: token.token_type.clone(),
                                value: rv,
                            },
                        );
                    }
                    None => {
                        // validate_literal already pushed a diagnostic.
                    }
                }
            }
            None => {
                // resolve_token_to_literal already pushed a diagnostic.
            }
        }
    }

    // ── Step 3: gradient stop-color cross-check ───────────────────────────
    // Now that every token's resolved value is known, verify that each
    // gradient stop references a token that exists AND resolved to a Color.
    // Iterate the resolved map (BTreeMap → deterministic id order) and clone
    // out the gradient stop lists so we don't borrow `resolved` while reading
    // other entries from it.
    let gradient_stops: Vec<(String, Vec<String>)> = resolved
        .iter()
        .filter_map(|(id, rt)| match &rt.value {
            ResolvedValue::Gradient(g) => Some((
                id.clone(),
                g.stops
                    .iter()
                    .map(|(_, color_id)| color_id.clone())
                    .collect(),
            )),
            _ => None,
        })
        .collect();

    for (id, stop_color_ids) in &gradient_stops {
        // The declaring token's span, looked up via the source index.
        let span = index.get(id.as_str()).and_then(|t| t.source_span);
        for color_token_id in stop_color_ids {
            match resolved.get(color_token_id.as_str()) {
                None => diagnostics.push(Diagnostic::error(
                    "gradient.stop_unresolved",
                    format!(
                        "gradient '{}' stop references unknown token '{}'",
                        id, color_token_id
                    ),
                    span,
                    Some(id.clone()),
                )),
                Some(rt) if rt.value.as_color_hex().is_none() => {
                    diagnostics.push(Diagnostic::error(
                        "gradient.stop_wrong_type",
                        format!(
                            "gradient '{}' stop references token '{}' of type '{}' \
                             but a color token is required",
                            id,
                            color_token_id,
                            type_name_of(&rt.token_type),
                        ),
                        span,
                        Some(id.clone()),
                    ));
                }
                Some(_) => {}
            }
        }
    }

    // ── Step 4: shadow layer-color cross-check ────────────────────────────
    // Now that every token's resolved value is known, verify that each shadow
    // layer references a token that exists AND resolved to a Color. Iterate the
    // resolved map (BTreeMap → deterministic id order) and clone out the layer
    // color lists so we don't borrow `resolved` while reading other entries.
    let shadow_layers: Vec<(String, Vec<String>)> = resolved
        .iter()
        .filter_map(|(id, rt)| match &rt.value {
            ResolvedValue::Shadow(s) => Some((
                id.clone(),
                s.layers
                    .iter()
                    .map(|layer| layer.color_token.clone())
                    .collect(),
            )),
            _ => None,
        })
        .collect();

    for (id, layer_color_ids) in &shadow_layers {
        // The declaring token's span, looked up via the source index.
        let span = index.get(id.as_str()).and_then(|t| t.source_span);
        for color_token_id in layer_color_ids {
            match resolved.get(color_token_id.as_str()) {
                None => diagnostics.push(Diagnostic::error(
                    "shadow.layer_unresolved",
                    format!(
                        "shadow '{}' layer references unknown token '{}'",
                        id, color_token_id
                    ),
                    span,
                    Some(id.clone()),
                )),
                Some(rt) if rt.value.as_color_hex().is_none() => {
                    diagnostics.push(Diagnostic::error(
                        "shadow.layer_wrong_type",
                        format!(
                            "shadow '{}' layer references token '{}' of type '{}' \
                             but a color token is required",
                            id,
                            color_token_id,
                            type_name_of(&rt.token_type),
                        ),
                        span,
                        Some(id.clone()),
                    ));
                }
                Some(_) => {}
            }
        }
    }

    TokenResolution {
        resolved,
        diagnostics,
    }
}

// ── Alias-chain resolution ────────────────────────────────────────────────────

/// Follow the alias chain from `start` until a literal is reached, or until a
/// cycle / missing reference is detected.
///
/// Returns `Some((literal, type_of_literal_token))` on success.
/// Pushes exactly one diagnostic and returns `None` on failure.
///
/// The walk is **iterative** and terminates in at most `index.len()` steps,
/// so it is safe against arbitrarily long or cyclic chains.
fn resolve_token_to_literal<'a>(
    start: &'a Token,
    index: &HashMap<&str, &'a Token>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(TokenLiteral, TokenType)> {
    // visited tracks IDs we've stepped through, used for cycle detection.
    let mut visited: HashSet<&str> = HashSet::new();
    let mut current: &Token = start;

    loop {
        match &current.value {
            TokenValue::Literal(lit) => {
                return Some((lit.clone(), current.token_type.clone()));
            }
            TokenValue::Reference { token_id } => {
                // Check for a cycle: if we've seen this target before we're
                // in a cycle.
                if visited.contains(token_id.as_str()) {
                    diagnostics.push(Diagnostic::error(
                        "token.cyclic_reference",
                        format!(
                            "token '{}' participates in a cyclic alias chain \
                             (cycle detected at '{}')",
                            start.id, token_id
                        ),
                        start.source_span,
                        Some(start.id.clone()),
                    ));
                    return None;
                }

                // Check for self-reference before we insert current into
                // visited, so `a → a` is caught on the first step.
                if token_id == &current.id {
                    diagnostics.push(Diagnostic::error(
                        "token.cyclic_reference",
                        format!("token '{}' references itself", current.id),
                        current.source_span,
                        Some(current.id.clone()),
                    ));
                    return None;
                }

                // Record that we've visited the current node *before* following
                // the reference, so we detect `a → b → a` correctly.
                visited.insert(current.id.as_str());

                // Resolve the reference target.
                match index.get(token_id.as_str()) {
                    Some(next) => {
                        current = next;
                    }
                    None => {
                        diagnostics.push(Diagnostic::error(
                            "token.unknown_reference",
                            format!(
                                "token '{}' references '{}' which does not exist",
                                start.id, token_id
                            ),
                            start.source_span,
                            Some(start.id.clone()),
                        ));
                        return None;
                    }
                }
            }
        }
    }
}

// ── Literal validation ────────────────────────────────────────────────────────

/// Validate `literal` against `token_type`. Returns the [`ResolvedValue`] on
/// success, or pushes `token.invalid_value` and returns `None` on failure.
fn validate_literal(
    token_id: &str,
    token_type: &TokenType,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match token_type {
        TokenType::Color => validate_color(token_id, literal, span, diagnostics),
        TokenType::Dimension => validate_dimension(token_id, literal, span, diagnostics),
        TokenType::Number => validate_number(token_id, literal, span, diagnostics),
        TokenType::FontFamily => validate_font_family(token_id, literal, span, diagnostics),
        TokenType::FontWeight => validate_font_weight(token_id, literal, span, diagnostics),
        TokenType::Gradient => validate_gradient(token_id, literal, span, diagnostics),
        TokenType::Shadow => validate_shadow(token_id, literal, span, diagnostics),
        TokenType::Unknown(_) => {
            // Already handled upstream; should not reach here.
            None
        }
    }
}

fn validate_color(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match literal {
        TokenLiteral::String(s) => {
            if is_valid_hex_color(s) {
                Some(ResolvedValue::Color(s.clone()))
            } else if s.starts_with("cmyk(") {
                match crate::color::parse_cmyk(s) {
                    Some(cmyk) => Some(ResolvedValue::CmykColor {
                        hex: crate::color::cmyk_to_hex(cmyk),
                        c: cmyk.c,
                        m: cmyk.m,
                        y: cmyk.y,
                        k: cmyk.k,
                    }),
                    None => {
                        diagnostics.push(invalid_value(
                            token_id,
                            &format!(
                                "color token '{}' has value '{}' which is not a valid \
                                 CMYK color; expected 'cmyk(c,m,y,k)' with each channel \
                                 a percentage in 0..=100",
                                token_id, s
                            ),
                            span,
                        ));
                        None
                    }
                }
            } else {
                diagnostics.push(invalid_value(
                    token_id,
                    &format!(
                        "color token '{}' has value '{}' which is not a valid \
                         color; expected sRGB hex '#rrggbb'/'#rrggbbaa' \
                         (lowercase hex digits) or 'cmyk(c,m,y,k)'",
                        token_id, s
                    ),
                    span,
                ));
                None
            }
        }
        other => {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "color token '{}' must have a string literal value (e.g. \"#rrggbb\"), \
                     got {}",
                    token_id,
                    literal_kind_name(other),
                ),
                span,
            ));
            None
        }
    }
}

/// Returns `true` if `s` matches `#[0-9a-fA-F]{6}` or `#[0-9a-fA-F]{8}`.
fn is_valid_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'#') {
        return false;
    }
    let hex = &bytes[1..];
    if hex.len() != 6 && hex.len() != 8 {
        return false;
    }
    hex.iter().all(|b| b.is_ascii_hexdigit())
}

fn validate_dimension(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match literal {
        TokenLiteral::Dimension(dim) => {
            if matches!(dim.unit, Unit::Unknown(_)) {
                diagnostics.push(invalid_value(
                    token_id,
                    &format!(
                        "dimension token '{}' uses an unrecognized unit; \
                         allowed units are px, pt, pct, deg",
                        token_id
                    ),
                    span,
                ));
                None
            } else {
                // Negative values are allowed at the token layer (doc 11:
                // "Negative dimensions are invalid unless the consuming
                // property explicitly allows negative values" — that check
                // belongs to the property/node validation layer, not here).
                Some(ResolvedValue::Dimension(dim.clone()))
            }
        }
        other => {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "dimension token '{}' must have a dimension literal value \
                     (e.g. (px)28), got {}",
                    token_id,
                    literal_kind_name(other),
                ),
                span,
            ));
            None
        }
    }
}

fn validate_number(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match literal {
        TokenLiteral::Number(n) => {
            if n.is_finite() {
                Some(ResolvedValue::Number(*n))
            } else {
                diagnostics.push(invalid_value(
                    token_id,
                    &format!(
                        "number token '{}' has non-finite value '{}'; \
                         NaN and ±inf are invalid",
                        token_id, n
                    ),
                    span,
                ));
                None
            }
        }
        other => {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "number token '{}' must have a numeric literal value, got {}",
                    token_id,
                    literal_kind_name(other),
                ),
                span,
            ));
            None
        }
    }
}

fn validate_font_family(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match literal {
        TokenLiteral::String(s) => {
            if s.is_empty() {
                diagnostics.push(invalid_value(
                    token_id,
                    &format!(
                        "fontFamily token '{}' must not be an empty string",
                        token_id
                    ),
                    span,
                ));
                None
            } else {
                Some(ResolvedValue::FontFamily(s.clone()))
            }
        }
        other => {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "fontFamily token '{}' must have a string literal value, got {}",
                    token_id,
                    literal_kind_name(other),
                ),
                span,
            ));
            None
        }
    }
}

fn validate_font_weight(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    match literal {
        TokenLiteral::Number(n) => {
            // Must be an integer in [100, 900] in multiples of 1 (the contract
            // says "integer weight, initially 100 through 900").
            let truncated = n.trunc();
            // Check integral (no fractional part) and in-range.
            if (n - truncated).abs() > f64::EPSILON || !(100.0..=900.0).contains(&truncated) {
                diagnostics.push(invalid_value(
                    token_id,
                    &format!(
                        "fontWeight token '{}' has value '{}'; expected an \
                         integer in 100..=900",
                        token_id, n
                    ),
                    span,
                ));
                None
            } else {
                Some(ResolvedValue::FontWeight(truncated as u32))
            }
        }
        other => {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "fontWeight token '{}' must have a numeric literal value \
                     (e.g. 700), got {}",
                    token_id,
                    literal_kind_name(other),
                ),
                span,
            ));
            None
        }
    }
}

/// Validate a gradient literal: require ≥2 stops and finite offsets. Offsets are
/// clamped into `0.0..=1.0`. Stop-color existence/type are NOT checked here —
/// that requires the full resolved map and runs as a second pass.
fn validate_gradient(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    let TokenLiteral::Gradient(GradientLiteral { angle_deg, stops }) = literal else {
        diagnostics.push(invalid_value(
            token_id,
            &format!(
                "gradient token '{}' must be defined by `stop` child nodes, got {}",
                token_id,
                literal_kind_name(literal),
            ),
            span,
        ));
        return None;
    };

    if stops.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "gradient.too_few_stops",
            format!(
                "gradient token '{}' has {} stop(s); at least 2 are required",
                token_id,
                stops.len()
            ),
            span,
            Some(token_id.to_owned()),
        ));
        return None;
    }

    let mut resolved_stops: Vec<(f64, String)> = Vec::with_capacity(stops.len());
    for stop in stops {
        if !stop.offset.is_finite() {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "gradient token '{}' has a non-finite stop offset; \
                     NaN and ±inf are invalid",
                    token_id
                ),
                span,
            ));
            return None;
        }
        let clamped = stop.offset.clamp(0.0, 1.0);
        resolved_stops.push((clamped, stop.color_token.clone()));
    }

    Some(ResolvedValue::Gradient(ResolvedGradient {
        angle_deg: *angle_deg,
        stops: resolved_stops,
    }))
}

/// Validate a shadow literal: require ≥1 layer, each dx/dy/blur finite, with
/// blur clamped to `>= 0`. Layer-color existence/type are NOT checked here —
/// that requires the full resolved map and runs as a second pass.
fn validate_shadow(
    token_id: &str,
    literal: &TokenLiteral,
    span: Option<crate::ast::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedValue> {
    let TokenLiteral::Shadow(ShadowLiteral { layers }) = literal else {
        diagnostics.push(invalid_value(
            token_id,
            &format!(
                "shadow token '{}' must be defined by `layer` child nodes, got {}",
                token_id,
                literal_kind_name(literal),
            ),
            span,
        ));
        return None;
    };

    if layers.is_empty() {
        diagnostics.push(Diagnostic::error(
            "shadow.no_layers",
            format!(
                "shadow token '{}' has no layers; at least 1 is required",
                token_id
            ),
            span,
            Some(token_id.to_owned()),
        ));
        return None;
    }

    let mut resolved_layers: Vec<ResolvedShadowLayer> = Vec::with_capacity(layers.len());
    for layer in layers {
        if !layer.dx.is_finite() || !layer.dy.is_finite() || !layer.blur.is_finite() {
            diagnostics.push(invalid_value(
                token_id,
                &format!(
                    "shadow token '{}' has a non-finite layer dx/dy/blur; \
                     NaN and ±inf are invalid",
                    token_id
                ),
                span,
            ));
            return None;
        }
        resolved_layers.push(ResolvedShadowLayer {
            dx: layer.dx,
            dy: layer.dy,
            blur: layer.blur.max(0.0),
            color_token: layer.color_token.clone(),
        });
    }

    Some(ResolvedValue::Shadow(ResolvedShadow {
        layers: resolved_layers,
    }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn invalid_value(token_id: &str, message: &str, span: Option<crate::ast::Span>) -> Diagnostic {
    Diagnostic::error(
        "token.invalid_value",
        message,
        span,
        Some(token_id.to_owned()),
    )
}

fn literal_kind_name(lit: &TokenLiteral) -> &'static str {
    match lit {
        TokenLiteral::String(_) => "a string literal",
        TokenLiteral::Dimension(_) => "a dimension literal",
        TokenLiteral::Number(_) => "a number literal",
        TokenLiteral::Gradient(_) => "a gradient literal",
        TokenLiteral::Shadow(_) => "a shadow literal",
    }
}

fn type_name_of(t: &TokenType) -> &str {
    match t {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::token::{Token, TokenBlock, TokenLiteral, TokenType, TokenValue};
    use crate::ast::value::{Dimension, Unit};

    // ── Builder helpers ───────────────────────────────────────────────────

    fn literal_token(id: &str, token_type: TokenType, literal: TokenLiteral) -> Token {
        Token {
            id: id.to_owned(),
            token_type,
            value: TokenValue::Literal(literal),
            source_span: None,
        }
    }

    fn alias_token(id: &str, token_type: TokenType, target: &str) -> Token {
        Token {
            id: id.to_owned(),
            token_type,
            value: TokenValue::Reference {
                token_id: target.to_owned(),
            },
            source_span: None,
        }
    }

    fn block(tokens: Vec<Token>) -> TokenBlock {
        TokenBlock {
            format: "zenith-token-v1".to_owned(),
            tokens,
        }
    }

    fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
        diagnostics.iter().any(|d| d.code == code)
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics.iter().map(|d| d.code.as_str()).collect()
    }

    // ── Literal resolution tests ──────────────────────────────────────────

    #[test]
    fn resolves_color_literal() {
        let b = block(vec![literal_token(
            "color.text.primary",
            TokenType::Color,
            TokenLiteral::String("#111827".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert_eq!(
            r.resolved["color.text.primary"].value,
            ResolvedValue::Color("#111827".to_owned())
        );
    }

    #[test]
    fn resolves_color_with_alpha() {
        let b = block(vec![literal_token(
            "color.bg",
            TokenType::Color,
            TokenLiteral::String("#ffffff80".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(r.diagnostics.is_empty());
        assert!(matches!(
            r.resolved["color.bg"].value,
            ResolvedValue::Color(_)
        ));
    }

    #[test]
    fn resolves_dimension_literal() {
        let b = block(vec![literal_token(
            "size.text.title",
            TokenType::Dimension,
            TokenLiteral::Dimension(Dimension {
                value: 48.0,
                unit: Unit::Pt,
            }),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert_eq!(
            r.resolved["size.text.title"].value,
            ResolvedValue::Dimension(Dimension {
                value: 48.0,
                unit: Unit::Pt
            })
        );
    }

    #[test]
    fn resolves_number_literal() {
        let b = block(vec![literal_token(
            "lineheight.title",
            TokenType::Number,
            TokenLiteral::Number(1.05),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert_eq!(
            r.resolved["lineheight.title"].value,
            ResolvedValue::Number(1.05)
        );
    }

    #[test]
    fn resolves_font_family_literal() {
        let b = block(vec![literal_token(
            "font.family.body",
            TokenType::FontFamily,
            TokenLiteral::String("Inter".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert_eq!(
            r.resolved["font.family.body"].value,
            ResolvedValue::FontFamily("Inter".to_owned())
        );
    }

    #[test]
    fn resolves_font_weight_literal() {
        let b = block(vec![literal_token(
            "font.weight.bold",
            TokenType::FontWeight,
            TokenLiteral::Number(700.0),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert_eq!(
            r.resolved["font.weight.bold"].value,
            ResolvedValue::FontWeight(700)
        );
    }

    // ── Alias chain resolution ────────────────────────────────────────────

    #[test]
    fn alias_chain_resolves_to_literal() {
        // a → b → "#aabbcc" literal
        let b = block(vec![
            alias_token("color.a", TokenType::Color, "color.b"),
            alias_token("color.b", TokenType::Color, "color.c"),
            literal_token(
                "color.c",
                TokenType::Color,
                TokenLiteral::String("#aabbcc".to_owned()),
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        // All three should be present in the resolved map.
        assert!(r.resolved.contains_key("color.a"), "color.a missing");
        assert!(r.resolved.contains_key("color.b"), "color.b missing");
        assert!(r.resolved.contains_key("color.c"), "color.c missing");
        assert_eq!(
            r.resolved["color.a"].value,
            ResolvedValue::Color("#aabbcc".to_owned())
        );
    }

    // ── Cycle detection ───────────────────────────────────────────────────

    #[test]
    fn self_cycle_produces_diagnostic_and_terminates() {
        let b = block(vec![alias_token(
            "color.self",
            TokenType::Color,
            "color.self",
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.cyclic_reference"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("color.self"));
    }

    #[test]
    fn two_cycle_produces_diagnostic_and_terminates() {
        // a → b → a
        let b = block(vec![
            alias_token("color.a", TokenType::Color, "color.b"),
            alias_token("color.b", TokenType::Color, "color.a"),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.cyclic_reference"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        // Neither should be resolved.
        assert!(!r.resolved.contains_key("color.a"));
        assert!(!r.resolved.contains_key("color.b"));
    }

    // ── Unknown reference ─────────────────────────────────────────────────

    #[test]
    fn unknown_reference_produces_diagnostic() {
        let b = block(vec![alias_token(
            "color.missing-target",
            TokenType::Color,
            "color.does.not.exist",
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.unknown_reference"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("color.missing-target"));
    }

    // ── Type mismatch ─────────────────────────────────────────────────────

    #[test]
    fn cross_type_alias_produces_type_mismatch() {
        // color.bad → size.text.title (dimension) — type mismatch
        let b = block(vec![
            alias_token("color.bad", TokenType::Color, "size.text.title"),
            literal_token(
                "size.text.title",
                TokenType::Dimension,
                TokenLiteral::Dimension(Dimension {
                    value: 48.0,
                    unit: Unit::Pt,
                }),
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.type_mismatch"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("color.bad"));
        // size.text.title itself should resolve fine.
        assert!(r.resolved.contains_key("size.text.title"));
    }

    // ── Duplicate ID ──────────────────────────────────────────────────────

    #[test]
    fn duplicate_id_produces_diagnostic_and_first_wins() {
        let b = block(vec![
            literal_token(
                "color.dup",
                TokenType::Color,
                TokenLiteral::String("#111111".to_owned()),
            ),
            literal_token(
                "color.dup",
                TokenType::Color,
                TokenLiteral::String("#222222".to_owned()),
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.duplicate_id"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        // First definition (#111111) should win.
        assert_eq!(
            r.resolved["color.dup"].value,
            ResolvedValue::Color("#111111".to_owned())
        );
    }

    // ── Invalid value ─────────────────────────────────────────────────────

    #[test]
    fn invalid_color_hex_produces_diagnostic() {
        let b = block(vec![literal_token(
            "color.bad",
            TokenType::Color,
            TokenLiteral::String("#xyz".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("color.bad"));
    }

    #[test]
    fn resolves_cmyk_color_to_hex_and_carries_channels() {
        let b = block(vec![literal_token(
            "color.accent.violet",
            TokenType::Color,
            TokenLiteral::String("cmyk(59,85,0,7)".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        match &r.resolved["color.accent.violet"].value {
            ResolvedValue::CmykColor { hex, c, m, y, k } => {
                assert_eq!(hex, "#6124ed");
                assert_eq!((*c, *m, *y, *k), (59.0, 85.0, 0.0, 7.0));
            }
            other => panic!("expected CmykColor, got {other:?}"),
        }
    }

    #[test]
    fn cmyk_zero_resolves_to_white_hex() {
        let b = block(vec![literal_token(
            "color.white",
            TokenType::Color,
            TokenLiteral::String("cmyk(0,0,0,0)".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(r.diagnostics.is_empty());
        assert_eq!(
            r.resolved["color.white"].value.as_color_hex(),
            Some("#ffffff")
        );
    }

    #[test]
    fn malformed_cmyk_produces_invalid_value() {
        let b = block(vec![literal_token(
            "color.bad-cmyk",
            TokenType::Color,
            TokenLiteral::String("cmyk(59,85,0,200)".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("color.bad-cmyk"));
    }

    #[test]
    fn hex_color_still_resolves_to_color_variant_unchanged() {
        // Regression guard: an sRGB hex token must remain a plain `Color`,
        // carrying no CMYK, byte-for-byte as before.
        let b = block(vec![literal_token(
            "color.hex",
            TokenType::Color,
            TokenLiteral::String("#112233".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(r.diagnostics.is_empty());
        assert_eq!(
            r.resolved["color.hex"].value,
            ResolvedValue::Color("#112233".to_owned())
        );
        assert_eq!(r.resolved["color.hex"].value.cmyk(), None);
    }

    #[test]
    fn cmyk_color_works_as_gradient_stop() {
        let b = block(vec![
            literal_token(
                "color.top",
                TokenType::Color,
                TokenLiteral::String("cmyk(59,85,0,7)".to_owned()),
            ),
            literal_token(
                "color.bottom",
                TokenType::Color,
                TokenLiteral::String("#334455".to_owned()),
            ),
            gradient_token(
                "gradient.bg",
                90.0,
                vec![(0.0, "color.top"), (1.0, "color.bottom")],
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "a CMYK color must be a valid gradient stop; got: {:?}",
            r.diagnostics
        );
    }

    #[test]
    fn font_weight_out_of_range_produces_diagnostic() {
        let b = block(vec![literal_token(
            "font.weight.heavy",
            TokenType::FontWeight,
            TokenLiteral::Number(1000.0),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("font.weight.heavy"));
    }

    #[test]
    fn font_weight_fractional_produces_diagnostic() {
        let b = block(vec![literal_token(
            "font.weight.frac",
            TokenType::FontWeight,
            TokenLiteral::Number(450.5),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn number_nan_produces_diagnostic() {
        let b = block(vec![literal_token(
            "lineheight.nan",
            TokenType::Number,
            TokenLiteral::Number(f64::NAN),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn number_inf_produces_diagnostic() {
        let b = block(vec![literal_token(
            "lineheight.inf",
            TokenType::Number,
            TokenLiteral::Number(f64::INFINITY),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn dimension_wrong_literal_type_produces_diagnostic() {
        // A color token given a Dimension literal should produce invalid_value.
        let b = block(vec![literal_token(
            "color.bad-shape",
            TokenType::Color,
            TokenLiteral::Dimension(Dimension {
                value: 10.0,
                unit: Unit::Px,
            }),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    // ── Unknown type ──────────────────────────────────────────────────────

    #[test]
    fn unknown_type_produces_warning_and_is_not_resolved() {
        let b = block(vec![literal_token(
            "gradient.hero",
            TokenType::Unknown("gradient".to_owned()),
            TokenLiteral::String("linear-gradient(...)".to_owned()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.unknown_type"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        let unknown_diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == "token.unknown_type")
            .expect("should exist");
        assert_eq!(unknown_diag.severity, crate::diagnostics::Severity::Warning);
        assert!(!r.resolved.contains_key("gradient.hero"));
    }

    // ── Negative dimension allowed ────────────────────────────────────────

    #[test]
    fn negative_dimension_is_allowed_at_token_layer() {
        let b = block(vec![literal_token(
            "size.offset",
            TokenType::Dimension,
            TokenLiteral::Dimension(Dimension {
                value: -4.0,
                unit: Unit::Px,
            }),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        assert!(r.resolved.contains_key("size.offset"));
    }

    // ── Dimension unknown unit ────────────────────────────────────────────

    #[test]
    fn dimension_unknown_unit_produces_invalid_value() {
        let b = block(vec![literal_token(
            "size.bad-unit",
            TokenType::Dimension,
            TokenLiteral::Dimension(Dimension {
                value: 10.0,
                unit: Unit::Unknown("em".to_owned()),
            }),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    // ── Font family empty ─────────────────────────────────────────────────

    // ── Gradient resolution ───────────────────────────────────────────────

    use crate::ast::token::{GradientLiteral, GradientStopRef};

    fn gradient_token(id: &str, angle_deg: f64, stops: Vec<(f64, &str)>) -> Token {
        Token {
            id: id.to_owned(),
            token_type: TokenType::Gradient,
            value: TokenValue::Literal(TokenLiteral::Gradient(GradientLiteral {
                angle_deg,
                stops: stops
                    .into_iter()
                    .map(|(offset, color)| GradientStopRef {
                        offset,
                        color_token: color.to_owned(),
                    })
                    .collect(),
            })),
            source_span: None,
        }
    }

    #[test]
    fn resolves_gradient_with_clamped_offsets() {
        let b = block(vec![
            literal_token(
                "color.top",
                TokenType::Color,
                TokenLiteral::String("#001122".to_owned()),
            ),
            literal_token(
                "color.bottom",
                TokenType::Color,
                TokenLiteral::String("#334455".to_owned()),
            ),
            // Offsets out of range get clamped into 0.0..=1.0.
            gradient_token(
                "gradient.bg.hero",
                90.0,
                vec![(-0.5, "color.top"), (1.5, "color.bottom")],
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        match &r.resolved["gradient.bg.hero"].value {
            ResolvedValue::Gradient(g) => {
                assert_eq!(g.angle_deg, 90.0);
                assert_eq!(
                    g.stops,
                    vec![
                        (0.0, "color.top".to_owned()),
                        (1.0, "color.bottom".to_owned()),
                    ]
                );
            }
            other => panic!("expected gradient, got {other:?}"),
        }
    }

    #[test]
    fn gradient_with_one_stop_produces_too_few_stops() {
        let b = block(vec![
            literal_token(
                "color.top",
                TokenType::Color,
                TokenLiteral::String("#001122".to_owned()),
            ),
            gradient_token("gradient.bad", 90.0, vec![(0.0, "color.top")]),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "gradient.too_few_stops"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("gradient.bad"));
    }

    #[test]
    fn gradient_stop_missing_token_produces_stop_unresolved() {
        let b = block(vec![
            literal_token(
                "color.top",
                TokenType::Color,
                TokenLiteral::String("#001122".to_owned()),
            ),
            gradient_token(
                "gradient.bg",
                90.0,
                vec![(0.0, "color.top"), (1.0, "color.does.not.exist")],
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "gradient.stop_unresolved"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn gradient_stop_wrong_type_produces_stop_wrong_type() {
        let b = block(vec![
            literal_token(
                "color.top",
                TokenType::Color,
                TokenLiteral::String("#001122".to_owned()),
            ),
            literal_token(
                "size.not-a-color",
                TokenType::Dimension,
                TokenLiteral::Dimension(Dimension {
                    value: 4.0,
                    unit: Unit::Px,
                }),
            ),
            gradient_token(
                "gradient.bg",
                90.0,
                vec![(0.0, "color.top"), (1.0, "size.not-a-color")],
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "gradient.stop_wrong_type"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn empty_font_family_produces_invalid_value() {
        let b = block(vec![literal_token(
            "font.family.empty",
            TokenType::FontFamily,
            TokenLiteral::String(String::new()),
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "token.invalid_value"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    // ── Shadow resolution ─────────────────────────────────────────────────

    use crate::ast::token::{ShadowLayerRef, ShadowLiteral};

    fn shadow_token(id: &str, layers: Vec<(f64, f64, f64, &str)>) -> Token {
        Token {
            id: id.to_owned(),
            token_type: TokenType::Shadow,
            value: TokenValue::Literal(TokenLiteral::Shadow(ShadowLiteral {
                layers: layers
                    .into_iter()
                    .map(|(dx, dy, blur, color)| ShadowLayerRef {
                        dx,
                        dy,
                        blur,
                        color_token: color.to_owned(),
                    })
                    .collect(),
            })),
            source_span: None,
        }
    }

    #[test]
    fn resolves_shadow_with_clamped_blur() {
        let b = block(vec![
            literal_token(
                "color.shadow.black",
                TokenType::Color,
                TokenLiteral::String("#000000".to_owned()),
            ),
            // Negative blur is clamped to 0; offsets pass through.
            shadow_token(
                "shadow.headline",
                vec![(8.0, 8.0, -4.0, "color.shadow.black")],
            ),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            r.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            r.diagnostics
        );
        match &r.resolved["shadow.headline"].value {
            ResolvedValue::Shadow(s) => {
                assert_eq!(s.layers.len(), 1);
                let layer = &s.layers[0];
                assert_eq!(layer.dx, 8.0);
                assert_eq!(layer.dy, 8.0);
                assert_eq!(layer.blur, 0.0);
                assert_eq!(layer.color_token, "color.shadow.black");
            }
            other => panic!("expected shadow, got {other:?}"),
        }
    }

    #[test]
    fn empty_shadow_produces_no_layers() {
        let b = block(vec![shadow_token("shadow.empty", vec![])]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "shadow.no_layers"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
        assert!(!r.resolved.contains_key("shadow.empty"));
    }

    #[test]
    fn shadow_layer_missing_token_produces_layer_unresolved() {
        let b = block(vec![shadow_token(
            "shadow.bad",
            vec![(0.0, 0.0, 20.0, "color.does.not.exist")],
        )]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "shadow.layer_unresolved"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }

    #[test]
    fn shadow_layer_wrong_type_produces_layer_wrong_type() {
        let b = block(vec![
            literal_token(
                "size.not-a-color",
                TokenType::Dimension,
                TokenLiteral::Dimension(Dimension {
                    value: 4.0,
                    unit: Unit::Px,
                }),
            ),
            shadow_token("shadow.bad", vec![(0.0, 0.0, 20.0, "size.not-a-color")]),
        ]);
        let r = resolve_tokens(&b);
        assert!(
            has_code(&r.diagnostics, "shadow.layer_wrong_type"),
            "codes: {:?}",
            codes(&r.diagnostics)
        );
    }
}
