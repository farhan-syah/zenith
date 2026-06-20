//! Pure logic for `zenith tokens`.
//!
//! The public entry point [`list`] operates entirely on in-memory source text;
//! the caller is responsible for all filesystem I/O.

use zenith_core::{KdlAdapter, KdlSource, ResolvedValue, resolve_tokens};

use crate::commands::serialize_pretty;
use crate::json_types::{self, DiagnosticJson, TokenEntry, TokensOutput};

// ── Public entry point ────────────────────────────────────────────────────────

/// List all tokens in `src` with their resolved values.
///
/// Returns a formatted string (human or JSON) on success, or an error message
/// + exit code on parse failure.
pub fn list(src: &str, json: bool) -> Result<String, (String, u8)> {
    // Parse ─────────────────────────────────────────────────────────────────
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| (format!("error[parse.error]: {}", e.message), 2u8))?;

    // Resolve tokens ─────────────────────────────────────────────────────────
    let resolution = resolve_tokens(&doc.tokens);

    let entries: Vec<TokenEntry> = resolution
        .resolved
        .iter()
        .map(|(id, rt)| TokenEntry {
            id: id.clone(),
            token_type: token_type_str(&rt.token_type).to_owned(),
            resolved_value: resolved_value_str(&rt.value),
        })
        .collect();

    let output = if json {
        let out = TokensOutput {
            schema: "zenith-tokens-v1",
            tokens: entries,
            diagnostics: resolution
                .diagnostics
                .iter()
                .map(DiagnosticJson::from)
                .collect(),
        };
        serialize_pretty(&out)
    } else {
        format_human(&resolution.resolved, &resolution.diagnostics)
    };

    Ok(output)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn token_type_str(tt: &zenith_core::TokenType) -> &'static str {
    match tt {
        zenith_core::TokenType::Color => "color",
        zenith_core::TokenType::Dimension => "dimension",
        zenith_core::TokenType::Number => "number",
        zenith_core::TokenType::FontFamily => "fontFamily",
        zenith_core::TokenType::FontWeight => "fontWeight",
        zenith_core::TokenType::Gradient => "gradient",
        zenith_core::TokenType::Shadow => "shadow",
        zenith_core::TokenType::Unknown(_) => "unknown",
    }
}

fn resolved_value_str(rv: &ResolvedValue) -> String {
    match rv {
        ResolvedValue::Color(s) => s.clone(),
        ResolvedValue::CmykColor { c, m, y, k, hex } => {
            format!("cmyk({c},{m},{y},{k}) ({hex})")
        }
        ResolvedValue::Dimension(d) => {
            let unit = match d.unit {
                zenith_core::Unit::Px => "px",
                zenith_core::Unit::Pt => "pt",
                zenith_core::Unit::Pct => "pct",
                zenith_core::Unit::Deg => "deg",
                zenith_core::Unit::Unknown(ref u) => {
                    return format!("{}{}", d.value, u);
                }
            };
            format!("{}({})", d.value, unit)
        }
        ResolvedValue::Number(n) => n.to_string(),
        ResolvedValue::FontFamily(s) => s.clone(),
        ResolvedValue::FontWeight(w) => w.to_string(),
        ResolvedValue::Gradient(g) => {
            let stops: Vec<String> = g
                .stops
                .iter()
                .map(|(offset, color_token)| format!("{offset}:{color_token}"))
                .collect();
            format!("linear-gradient({}deg, {})", g.angle_deg, stops.join(", "))
        }
        ResolvedValue::Shadow(s) => {
            let layers: Vec<String> = s
                .layers
                .iter()
                .map(|layer| {
                    format!(
                        "{}px {}px {}px {}",
                        layer.dx, layer.dy, layer.blur, layer.color_token
                    )
                })
                .collect();
            format!("shadow({})", layers.join(", "))
        }
    }
}

fn format_human(
    resolved: &std::collections::BTreeMap<String, zenith_core::ResolvedToken>,
    diagnostics: &[zenith_core::Diagnostic],
) -> String {
    let mut out = String::new();

    if resolved.is_empty() {
        out.push_str("no tokens defined\n");
    } else {
        out.push_str("tokens:\n");
        for (id, rt) in resolved {
            out.push_str(&format!(
                "  {} ({}) = {}\n",
                id,
                token_type_str(&rt.token_type),
                resolved_value_str(&rt.value)
            ));
        }
    }

    if !diagnostics.is_empty() {
        out.push_str("\ndiagnostics:\n");
        for d in diagnostics {
            let sev = json_types::severity_str(&d.severity);
            out.push_str(&format!("  {}[{}]: {}\n", sev, d.code, d.message));
        }
    }

    out.trim_end().to_owned()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_WITH_TOKENS: &str = r##"zenith version=1 {
  project id="proj.t" name="Tokens Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.accent" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.t" title="Tokens Test" {
    page id="page.t" w=(px)320 h=(px)200 {
      rect id="rect.t" x=(px)0 y=(px)0 w=(px)320 h=(px)200 fill=(token)"color.bg"
    }
  }
}
"##;

    #[test]
    fn lists_expected_token() {
        let out = list(DOC_WITH_TOKENS, false).expect("must succeed");
        assert!(
            out.contains("color.bg"),
            "expected color.bg in output; got: {}",
            out
        );
        assert!(
            out.contains("#f8fafc"),
            "expected resolved color value; got: {}",
            out
        );
    }

    #[test]
    fn json_contains_schema() {
        let out = list(DOC_WITH_TOKENS, true).expect("must succeed");
        assert!(
            out.contains("zenith-tokens-v1"),
            "JSON must contain schema field; got: {}",
            out
        );
    }

    #[test]
    fn json_contains_token_entries() {
        let out = list(DOC_WITH_TOKENS, true).expect("must succeed");
        assert!(
            out.contains("color.bg"),
            "JSON must list token id; got: {}",
            out
        );
        assert!(
            out.contains("color.accent"),
            "JSON must list second token; got: {}",
            out
        );
    }

    #[test]
    fn parse_error_returns_err() {
        let result = list("not kdl {{{", false);
        assert!(result.is_err());
        let (_, code) = result.unwrap_err();
        assert_eq!(code, 2);
    }
}
