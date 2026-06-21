//! Token-block writing: the `tokens { … }` block plus per-token emission,
//! including the gradient/shadow brace-block forms and token-literal values.

use crate::ast::{GradientKind, Token, TokenBlock, TokenLiteral, TokenType, TokenValue};

use super::{fmt_dimension, fmt_f64, indent};

pub(super) fn write_token_block(block: &TokenBlock, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("tokens format=\"");
    out.push_str(&block.format);
    out.push_str("\" {\n");

    for token in &block.tokens {
        write_token(token, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

fn write_token(token: &Token, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("token");
    // Canonical order: id, type, value
    out.push_str(" id=\"");
    out.push_str(&token.id);
    out.push('"');

    // type
    let type_str = match &token.token_type {
        TokenType::Color => "color",
        TokenType::Dimension => "dimension",
        TokenType::Number => "number",
        TokenType::FontFamily => "fontFamily",
        TokenType::FontWeight => "fontWeight",
        TokenType::Gradient => "gradient",
        TokenType::Shadow => "shadow",
        TokenType::Filter => "filter",
        TokenType::Unknown(s) => s.as_str(),
    };
    out.push_str(" type=\"");
    out.push_str(type_str);
    out.push('"');

    // Gradient tokens have no scalar `value=`; linear gradients emit an `angle`
    // prop; radial gradients emit `radial=#true` plus optional geometry params.
    // Both are followed by a brace block of `stop` children.
    if let TokenValue::Literal(TokenLiteral::Gradient(g)) = &token.value {
        match g.kind {
            GradientKind::Linear => {
                out.push_str(" angle=(deg)");
                out.push_str(&fmt_f64(g.angle_deg));
            }
            GradientKind::Radial => {
                out.push_str(" radial=#true");
                if let Some(cx) = g.center_x {
                    out.push_str(" center-x=");
                    out.push_str(&fmt_f64(cx));
                }
                if let Some(cy) = g.center_y {
                    out.push_str(" center-y=");
                    out.push_str(&fmt_f64(cy));
                }
                if let Some(r) = g.radius {
                    out.push_str(" radius=");
                    out.push_str(&fmt_f64(r));
                }
            }
        }
        out.push_str(" {\n");
        for stop in &g.stops {
            indent(out, depth + 1);
            out.push_str("stop offset=");
            out.push_str(&fmt_f64(stop.offset));
            out.push_str(" color=(token)\"");
            out.push_str(&stop.color_token);
            out.push_str("\"\n");
        }
        indent(out, depth);
        out.push_str("}\n");
        return;
    }

    // Shadow tokens have no scalar `value=`; they emit a brace block of `layer`
    // children. Handle and return before the value path.
    if let TokenValue::Literal(TokenLiteral::Shadow(s)) = &token.value {
        out.push_str(" {\n");
        for layer in &s.layers {
            indent(out, depth + 1);
            out.push_str("layer dx=(px)");
            out.push_str(&fmt_f64(layer.dx));
            out.push_str(" dy=(px)");
            out.push_str(&fmt_f64(layer.dy));
            out.push_str(" blur=(px)");
            out.push_str(&fmt_f64(layer.blur));
            out.push_str(" color=(token)\"");
            out.push_str(&layer.color_token);
            out.push_str("\"\n");
        }
        indent(out, depth);
        out.push_str("}\n");
        return;
    }

    // Filter tokens have no scalar `value=`; they emit a brace block of op
    // children. Handle and return before the value path.
    if let TokenValue::Literal(TokenLiteral::Filter(f)) = &token.value {
        out.push_str(" {\n");
        for op in &f.ops {
            indent(out, depth + 1);
            out.push_str(op.kind.as_op_name());
            // Order: name, shadow, highlight, amount. `shadow`/`highlight` are
            // emitted only for duotone ops (mirrors the shadow-layer color
            // emission); the round-trip parser reads them back by name.
            if let Some(shadow) = &op.shadow {
                out.push_str(" shadow=(token)\"");
                out.push_str(shadow);
                out.push('"');
            }
            if let Some(highlight) = &op.highlight {
                out.push_str(" highlight=(token)\"");
                out.push_str(highlight);
                out.push('"');
            }
            if let Some(amount) = op.amount {
                out.push_str(" amount=");
                out.push_str(&fmt_f64(amount));
            }
            out.push('\n');
        }
        indent(out, depth);
        out.push_str("}\n");
        return;
    }

    // value
    out.push_str(" value=");
    match &token.value {
        TokenValue::Literal(lit) => match lit {
            TokenLiteral::String(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            TokenLiteral::Dimension(d) => {
                out.push_str(&fmt_dimension(d));
            }
            TokenLiteral::Number(n) => {
                out.push_str(&fmt_f64(*n));
            }
            // Gradient and shadow literals are emitted by the early-return
            // blocks above; these arms are unreachable but keep the match
            // exhaustive.
            TokenLiteral::Gradient(_) => {}
            TokenLiteral::Shadow(_) => {}
            TokenLiteral::Filter(_) => {}
        },
        TokenValue::Reference { token_id } => {
            out.push_str("(token)\"");
            out.push_str(token_id);
            out.push('"');
        }
    }

    out.push('\n');
}
