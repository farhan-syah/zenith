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
            // Extract the first diagnostic span if available.
            let span = e.diagnostics.first().map(|d| {
                let ss = d.span;
                crate::ast::Span {
                    start: ss.offset(),
                    end: ss.offset() + ss.len(),
                }
            });
            match span {
                Some(s) => ParseError::with_span(
                    ParseErrorCode::InvalidKdl,
                    s,
                    format!("KDL parse error: {e}"),
                ),
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
}
