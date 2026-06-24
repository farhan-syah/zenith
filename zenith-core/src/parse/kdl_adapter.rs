//! `KdlAdapter` — the concrete implementation of `KdlSource` backed by the
//! `kdl` 6.x crate.

use crate::ast::Document;
use crate::error::{FormatError, ParseError, ParseErrorCode};
use crate::format::format_document;
use crate::parse::transform;

use super::KdlSource;

/// Parses `.zen` source bytes into a `Document` AST using the KDL v2 parser.
///
/// This is the only struct in zenith-core that directly touches the `kdl` crate.
/// All other code works with the Zenith AST types.
#[derive(Debug, Clone, Default)]
pub struct KdlAdapter;

/// Converts a byte offset within `text` into a 1-based (line, column) pair.
///
/// Both line and column are counted in bytes, which matches the convention used
/// by the `kdl` crate's `SourceSpan`. The offset is clamped to `text.len()` so
/// no unchecked indexing can occur.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let safe_offset = offset.min(text.len());
    // Iterate over bytes up to safe_offset.  We only need to count '\n' bytes;
    // the source is valid UTF-8 (guaranteed by Step 1) so byte-by-byte is safe.
    let prefix = match text.get(..safe_offset) {
        Some(s) => s,
        // `safe_offset` is already clamped, so this branch is unreachable in
        // practice, but we handle it gracefully rather than panicking.
        None => text,
    };
    let mut line = 1usize;
    let mut last_newline_byte = 0usize;
    for (i, b) in prefix.bytes().enumerate() {
        if b == b'\n' {
            line += 1;
            last_newline_byte = i + 1;
        }
    }
    let col = safe_offset - last_newline_byte + 1;
    (line, col)
}

impl KdlSource for KdlAdapter {
    fn parse(&self, source: &[u8]) -> Result<Document, ParseError> {
        // Step 1: validate UTF-8.
        let text = std::str::from_utf8(source).map_err(|e| {
            ParseError::spanless(
                ParseErrorCode::NotUtf8,
                format!("source is not valid UTF-8: {e}"),
            )
        })?;

        // Step 2: parse KDL.
        let kdl_doc: kdl::KdlDocument = text.parse().map_err(|e: kdl::KdlError| {
            // Extract the first diagnostic span and rich message if available.
            match e.diagnostics.first() {
                Some(d) => {
                    let offset = d.span.offset();
                    let (line, col) = line_col(text, offset);
                    let mut msg = format!("KDL parse error at line {line}, column {col}");
                    match (&d.message, &d.help) {
                        (Some(m), Some(h)) => {
                            msg.push_str(": ");
                            msg.push_str(m);
                            msg.push_str(" (help: ");
                            msg.push_str(h);
                            msg.push(')');
                        }
                        (Some(m), None) => {
                            msg.push_str(": ");
                            msg.push_str(m);
                        }
                        (None, Some(h)) => {
                            msg.push_str(" (help: ");
                            msg.push_str(h);
                            msg.push(')');
                        }
                        (None, None) => {
                            // No per-diagnostic detail; fall back to the top-level
                            // error so no information is lost.
                            msg.push_str(": ");
                            msg.push_str(&e.to_string());
                        }
                    }
                    let span = crate::ast::Span {
                        start: offset,
                        end: offset + d.span.len(),
                    };
                    ParseError::with_span(ParseErrorCode::InvalidKdl, span, msg)
                }
                None => ParseError::spanless(
                    ParseErrorCode::InvalidKdl,
                    format!("KDL parse error: {e}"),
                ),
            }
        })?;

        // Step 3: transform the KDL tree into the Zenith AST.
        transform::transform(&kdl_doc)
    }

    fn format(&self, doc: &Document) -> Result<Vec<u8>, FormatError> {
        format_document(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Node, PropertyValue, TokenLiteral, TokenType, TokenValue, Unit};

    /// A minimal but realistic `.zen` document exercising the full v0 parse
    /// surface: project, tokens (color + fontFamily + dimension + second color),
    /// empty styles, document → page → rect + text.
    const MINIMAL_DOC: &str = r##"zenith version=1 {
  project id="proj.test" name="Test Project"

  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="font.family.body" type="fontFamily" value="Inter"
    token id="size.title" type="dimension" value=(pt)48
    token id="color.text" type="color" value="#111827"
  }

  styles {
  }

  document id="doc.test" title="Test Doc" {
    page id="page.one" name="One" w=(px)640 h=(px)360 background=(token)"color.bg" {
      rect id="bg.rect" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.bg"
      text id="label" x=(px)10 y=(px)10 w=(px)200 h=(px)50 align="center" fill=(token)"color.text" {
        span "Hello Zenith"
      }
    }
  }
}
"##;

    #[test]
    fn test_minimal_doc_parses() {
        let adapter = KdlAdapter;
        let doc = adapter
            .parse(MINIMAL_DOC.as_bytes())
            .expect("parse must succeed");

        // Root version.
        assert_eq!(doc.version, 1);

        // Token count.
        assert_eq!(doc.tokens.tokens.len(), 4);
        assert_eq!(doc.tokens.format, "zenith-token-v1");

        // First token: color literal.
        let t0 = &doc.tokens.tokens[0];
        assert_eq!(t0.id, "color.bg");
        assert_eq!(t0.token_type, TokenType::Color);
        match &t0.value {
            TokenValue::Literal(TokenLiteral::String(s)) => assert_eq!(s, "#f8fafc"),
            other => panic!("expected string literal, got {other:?}"),
        }

        // Second token: fontFamily.
        let t1 = &doc.tokens.tokens[1];
        assert_eq!(t1.id, "font.family.body");
        assert_eq!(t1.token_type, TokenType::FontFamily);

        // Third token: dimension.
        let t2 = &doc.tokens.tokens[2];
        assert_eq!(t2.id, "size.title");
        assert_eq!(t2.token_type, TokenType::Dimension);
        match &t2.value {
            TokenValue::Literal(TokenLiteral::Dimension(d)) => {
                assert_eq!(d.value, 48.0);
                assert_eq!(d.unit, Unit::Pt);
            }
            other => panic!("expected dimension literal, got {other:?}"),
        }

        // Page dimensions.
        assert_eq!(doc.body.pages.len(), 1);
        let page = &doc.body.pages[0];
        assert_eq!(page.width.value, 640.0);
        assert_eq!(page.width.unit, Unit::Px);
        assert_eq!(page.height.value, 360.0);
        assert_eq!(page.height.unit, Unit::Px);

        // Page has exactly 2 children.
        assert_eq!(page.children.len(), 2);

        // First child: rect with token fill.
        match &page.children[0] {
            Node::Rect(r) => {
                assert_eq!(r.id, "bg.rect");
                assert_eq!(r.x.as_ref().map(|d| d.value), Some(0.0));
                assert_eq!(r.w.as_ref().map(|d| d.value), Some(640.0));
                match &r.fill {
                    Some(PropertyValue::TokenRef(tok)) => assert_eq!(tok, "color.bg"),
                    other => panic!("expected token ref fill, got {other:?}"),
                }
            }
            other => panic!("expected Rect, got {other:?}"),
        }

        // Second child: text with a span.
        match &page.children[1] {
            Node::Text(t) => {
                assert_eq!(t.id, "label");
                assert_eq!(t.align.as_deref(), Some("center"));
                assert_eq!(t.spans.len(), 1);
                assert_eq!(t.spans[0].text, "Hello Zenith");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// A literal (non-token) visual dimension must parse into
    /// `PropertyValue::Dimension`, preserving its numeric value and unit, rather
    /// than being silently dropped to a `Literal` string.
    #[test]
    fn test_literal_visual_dimension_parses() {
        use crate::ast::Dimension;
        let src = r##"zenith version=1 {
  project id="proj.dim" name="Dim"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.dim" title="Dim" {
    page id="page.one" w=(px)640 h=(px)360 {
      text id="t" x=(px)0 y=(px)0 w=(px)200 h=(px)50 font-size=(px)24 {
        span "Hi"
      }
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 stroke-width=(pt)13
    }
  }
}
"##;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let page = &doc.body.pages[0];

        match &page.children[0] {
            Node::Text(t) => assert_eq!(
                t.font_size,
                Some(PropertyValue::Dimension(Dimension {
                    value: 24.0,
                    unit: Unit::Px,
                })),
                "literal font-size=(px)24 must parse as a Dimension"
            ),
            other => panic!("expected Text, got {other:?}"),
        }

        match &page.children[1] {
            Node::Rect(r) => assert_eq!(
                r.stroke_width,
                Some(PropertyValue::Dimension(Dimension {
                    value: 13.0,
                    unit: Unit::Pt,
                })),
                "literal stroke-width=(pt)13 must parse as a Dimension with Pt unit"
            ),
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    /// A text node with `font-weight=(token)"weight.bold"` must parse the
    /// property into `font_weight = Some(TokenRef("weight.bold"))`.
    #[test]
    fn test_text_font_weight_token_parses() {
        let src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {
    token id="weight.bold" type="fontWeight" value=700
  }
  styles {
  }
  document id="doc.fw" title="FW" {
    page id="page.one" w=(px)640 h=(px)360 {
      text id="t" x=(px)0 y=(px)0 w=(px)200 h=(px)50 font-weight=(token)"weight.bold" {
        span "Bold"
      }
    }
  }
}
"##;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        match &doc.body.pages[0].children[0] {
            Node::Text(t) => assert_eq!(
                t.font_weight,
                Some(PropertyValue::TokenRef("weight.bold".to_owned())),
                "font-weight token ref must parse into font_weight"
            ),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// An unknown node kind must parse into `Node::Unknown`, never error.
    #[test]
    fn test_unknown_node_kind_forward_compat() {
        let src = r#"zenith version=1 {
  project id="proj.fc" name="FC"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.fc" title="FC" {
    page id="page.fc" w=(px)100 h=(px)100 {
      sparkle id="spark.one" magic=#true {}
    }
  }
}
"#;
        let adapter = KdlAdapter;
        let doc = adapter
            .parse(src.as_bytes())
            .expect("forward-compat unknown node must not error");
        let page = &doc.body.pages[0];
        assert_eq!(page.children.len(), 1);
        match &page.children[0] {
            Node::Unknown(u) => assert_eq!(u.kind, "sparkle"),
            other => panic!("expected Unknown node, got {other:?}"),
        }
    }

    /// An unknown property on a rect must land in `unknown_props`, not panic/error.
    #[test]
    fn test_unknown_property_preserved() {
        let src = r#"zenith version=1 {
  project id="proj.up" name="UP"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.up" title="UP" {
    page id="page.up" w=(px)100 h=(px)100 {
      rect id="r.one" x=(px)0 y=(px)0 w=(px)10 h=(px)10 future-prop="hello"
    }
  }
}
"#;
        let adapter = KdlAdapter;
        let doc = adapter
            .parse(src.as_bytes())
            .expect("unknown property must not error");
        match &doc.body.pages[0].children[0] {
            Node::Rect(r) => {
                assert!(
                    r.unknown_props.contains_key("future-prop"),
                    "unknown_props should contain future-prop; got: {:?}",
                    r.unknown_props
                );
                // The value must be typed as a String, not flattened to some
                // other variant — this is the forward-compat round-trip guarantee.
                assert_eq!(
                    r.unknown_props["future-prop"].value,
                    crate::ast::UnknownValue::String("hello".to_owned()),
                    "unknown string property must parse as UnknownValue::String"
                );
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    /// A `code` node carries its verbatim source as a `content` child whose
    /// escapes (`\n`, `\t`, `\"`, `\\`) decode into the stored content blob.
    #[test]
    fn test_code_node_content_decoded() {
        let src = r#"zenith version=1 {
  project id="proj.code" name="C"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.code" title="C" {
    page id="page.code" w=(px)100 h=(px)100 {
      code id="snippet" x=(px)8 y=(px)8 w=(px)80 h=(px)40 overflow="clip" language="rust" line-numbers=#false tab-width=4 {
        content "fn main() {\n\tlet s = \"a\\\\b\";\n}"
      }
    }
  }
}
"#;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("code node must parse");
        match &doc.body.pages[0].children[0] {
            Node::Code(c) => {
                assert_eq!(c.id, "snippet");
                assert_eq!(c.overflow.as_deref(), Some("clip"));
                assert_eq!(c.language.as_deref(), Some("rust"));
                assert_eq!(c.line_numbers, Some(false));
                assert_eq!(c.tab_width, Some(4));
                // Decoded content: literal newline, tab, quote, and backslash.
                assert_eq!(c.content, "fn main() {\n\tlet s = \"a\\\\b\";\n}");
            }
            other => panic!("expected Code node, got {other:?}"),
        }
    }

    /// Invalid UTF-8 bytes must yield `ParseErrorCode::NotUtf8`.
    #[test]
    fn test_invalid_utf8_error() {
        let adapter = KdlAdapter;
        let bad_bytes: &[u8] = &[0xff, 0xfe, 0x00];
        let err = adapter
            .parse(bad_bytes)
            .expect_err("must fail on invalid UTF-8");
        assert_eq!(
            err.code,
            crate::error::ParseErrorCode::NotUtf8,
            "expected NotUtf8, got {:?}",
            err.code
        );
    }

    /// Malformed KDL must yield `ParseErrorCode::InvalidKdl`.
    #[test]
    fn test_malformed_kdl_error() {
        let adapter = KdlAdapter;
        let bad_kdl = b"this is {{{ not valid kdl at all!!!";
        let err = adapter
            .parse(bad_kdl)
            .expect_err("must fail on malformed KDL");
        assert_eq!(
            err.code,
            crate::error::ParseErrorCode::InvalidKdl,
            "expected InvalidKdl, got {:?}",
            err.code
        );
    }

    /// A KDL parse error on a known line must produce a message that starts
    /// with `"KDL parse error at line N, column M"`.
    #[test]
    fn test_malformed_kdl_error_message_contains_location() {
        let adapter = KdlAdapter;
        // Three lines of valid KDL then a syntax error on line 4.
        let bad_kdl = b"foo\nbar\nbaz\n{{{ invalid";
        let err = adapter
            .parse(bad_kdl)
            .expect_err("must fail on malformed KDL");
        assert!(
            err.message.starts_with("KDL parse error at line "),
            "error message must start with location prefix; got: {:?}",
            err.message
        );
    }

    // ── line_col helper ──────────────────────────────────────────────────────

    #[test]
    fn line_col_first_line() {
        assert_eq!(line_col("hello world", 0), (1, 1));
        assert_eq!(line_col("hello world", 5), (1, 6));
    }

    #[test]
    fn line_col_second_line() {
        // "foo\nbar" — offset 4 is 'b', line 2 col 1.
        assert_eq!(line_col("foo\nbar", 4), (2, 1));
        assert_eq!(line_col("foo\nbar", 6), (2, 3));
    }

    #[test]
    fn line_col_clamps_past_end() {
        let text = "ab";
        // offset beyond length must not panic.
        let (l, c) = line_col(text, 999);
        assert_eq!(l, 1);
        assert_eq!(c, 3); // clamped to text.len() = 2, col = 2 - 0 + 1 = 3
    }

    #[test]
    fn line_col_empty_string() {
        assert_eq!(line_col("", 0), (1, 1));
        assert_eq!(line_col("", 5), (1, 1));
    }

    /// A gradient token (angle + 2 stops) parses into the expected AST shape:
    /// `TokenType::Gradient` + `TokenLiteral::Gradient` with both stops in order.
    #[test]
    fn test_gradient_token_parses() {
        let src = r##"zenith version=1 {
  project id="proj.grad" name="Grad"
  tokens format="zenith-token-v1" {
    token id="color.navy.top" type="color" value="#001133"
    token id="color.black.bottom" type="color" value="#000000"
    token id="gradient.bg.hero" type="gradient" angle=(deg)90 {
      stop offset=0.0 color=(token)"color.navy.top"
      stop offset=1.0 color=(token)"color.black.bottom"
    }
  }
  styles {
  }
  document id="doc.grad" title="Grad" {
    page id="p" w=(px)100 h=(px)100 {
    }
  }
}
"##;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

        let grad = doc
            .tokens
            .tokens
            .iter()
            .find(|t| t.id == "gradient.bg.hero")
            .expect("gradient token present");
        assert_eq!(grad.token_type, TokenType::Gradient);
        match &grad.value {
            TokenValue::Literal(TokenLiteral::Gradient(g)) => {
                assert_eq!(g.angle_deg, 90.0);
                assert_eq!(g.stops.len(), 2);
                assert_eq!(g.stops[0].offset, 0.0);
                assert_eq!(g.stops[0].color_token, "color.navy.top");
                assert_eq!(g.stops[1].offset, 1.0);
                assert_eq!(g.stops[1].color_token, "color.black.bottom");
            }
            other => panic!("expected gradient literal, got {other:?}"),
        }
    }

    /// When `angle=` is absent the gradient defaults to 90 degrees.
    #[test]
    fn test_gradient_token_default_angle() {
        let src = r##"zenith version=1 {
  project id="proj.grad" name="Grad"
  tokens format="zenith-token-v1" {
    token id="color.a" type="color" value="#001133"
    token id="color.b" type="color" value="#000000"
    token id="gradient.bg" type="gradient" {
      stop offset=0.0 color=(token)"color.a"
      stop offset=1.0 color=(token)"color.b"
    }
  }
  styles {
  }
  document id="doc.grad" title="Grad" {
    page id="p" w=(px)100 h=(px)100 {
    }
  }
}
"##;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
        let grad = doc
            .tokens
            .tokens
            .iter()
            .find(|t| t.id == "gradient.bg")
            .expect("gradient token present");
        match &grad.value {
            TokenValue::Literal(TokenLiteral::Gradient(g)) => assert_eq!(g.angle_deg, 90.0),
            other => panic!("expected gradient literal, got {other:?}"),
        }
    }

    /// A shadow token (2 layers: drop shadow + outer glow) parses into the
    /// expected AST shape, and a text node's `shadow=(token)"..."` prop parses
    /// into a `TokenRef`.
    #[test]
    fn test_shadow_token_and_node_prop_parse() {
        let src = r##"zenith version=1 {
  project id="proj.shadow" name="Shadow"
  tokens format="zenith-token-v1" {
    token id="color.shadow.black" type="color" value="#000000"
    token id="color.glow.cyan" type="color" value="#00ffff"
    token id="shadow.headline" type="shadow" {
      layer dx=(px)8 dy=(px)8 blur=(px)24 color=(token)"color.shadow.black"
      layer dx=(px)0 dy=(px)0 blur=(px)20 color=(token)"color.glow.cyan"
    }
  }
  styles {
  }
  document id="doc.shadow" title="Shadow" {
    page id="p" w=(px)100 h=(px)100 {
      text id="headline" x=(px)0 y=(px)0 w=(px)100 h=(px)40 shadow=(token)"shadow.headline" {
        span "Hi"
      }
    }
  }
}
"##;
        let adapter = KdlAdapter;
        let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

        let shadow = doc
            .tokens
            .tokens
            .iter()
            .find(|t| t.id == "shadow.headline")
            .expect("shadow token present");
        assert_eq!(shadow.token_type, TokenType::Shadow);
        match &shadow.value {
            TokenValue::Literal(TokenLiteral::Shadow(s)) => {
                assert_eq!(s.layers.len(), 2);
                assert_eq!(s.layers[0].dx, 8.0);
                assert_eq!(s.layers[0].dy, 8.0);
                assert_eq!(s.layers[0].blur, 24.0);
                assert_eq!(s.layers[0].color_token, "color.shadow.black");
                assert_eq!(s.layers[1].dx, 0.0);
                assert_eq!(s.layers[1].dy, 0.0);
                assert_eq!(s.layers[1].blur, 20.0);
                assert_eq!(s.layers[1].color_token, "color.glow.cyan");
            }
            other => panic!("expected shadow literal, got {other:?}"),
        }

        // The text node carries the shadow token ref.
        let page = &doc.body.pages[0];
        let text = page
            .children
            .iter()
            .find_map(|n| match n {
                Node::Text(t) if t.id == "headline" => Some(t),
                _ => None,
            })
            .expect("headline text node present");
        assert_eq!(
            text.shadow,
            Some(PropertyValue::TokenRef("shadow.headline".to_owned()))
        );
    }

    // ── Toc node parse / round-trip ───────────────────────────────────────────

    #[test]
    fn toc_node_parses_fields_correctly() {
        let src = r##"zenith version=1 {
  project id="proj.toc" name="Toc"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="d" {
    page id="p1" w=(px)595 h=(px)842 {
      toc id="contents" match-role="heading" leader="." folio-style="decimal" \
        x=(px)50 y=(px)100 w=(px)400 h=(px)300 style="body"
    }
  }
}"##;
        let doc = KdlAdapter
            .parse(src.as_bytes())
            .expect("parse must succeed");
        let page = &doc.body.pages[0];
        assert_eq!(page.children.len(), 1);
        match &page.children[0] {
            crate::ast::Node::Toc(t) => {
                assert_eq!(t.id, "contents");
                assert_eq!(t.match_role.as_deref(), Some("heading"));
                assert_eq!(t.match_style, None);
                assert_eq!(t.leader.as_deref(), Some("."));
                assert_eq!(t.folio_style.as_deref(), Some("decimal"));
                assert_eq!(t.x.as_ref().map(|d| d.value), Some(50.0));
                assert_eq!(t.y.as_ref().map(|d| d.value), Some(100.0));
                assert_eq!(t.w.as_ref().map(|d| d.value), Some(400.0));
                assert_eq!(t.h.as_ref().map(|d| d.value), Some(300.0));
                assert_eq!(t.style.as_deref(), Some("body"));
            }
            other => panic!("expected Toc, got {other:?}"),
        }
    }

    #[test]
    fn toc_node_round_trips_through_writer() {
        let src = "zenith version=1 {\n  project id=\"proj.t\" name=\"T\"\n  tokens format=\"zenith-token-v1\" {\n  }\n  styles {\n  }\n  document id=\"d\" {\n    page id=\"p1\" w=(px)595 h=(px)842 {\n      toc id=\"toc.1\" match-role=\"heading\"\n    }\n  }\n}";
        let doc = KdlAdapter.parse(src.as_bytes()).expect("first parse");
        let formatted = format_document(&doc).expect("format");
        let doc2 = KdlAdapter.parse(&formatted).expect("second parse");
        // After round-trip, the toc node must still exist and have the same id.
        match &doc2.body.pages[0].children[0] {
            crate::ast::Node::Toc(t) => {
                assert_eq!(t.id, "toc.1");
                assert_eq!(t.match_role.as_deref(), Some("heading"));
            }
            other => panic!("expected Toc after round-trip, got {other:?}"),
        }
    }
}
