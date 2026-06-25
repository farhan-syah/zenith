//! Integration tests for `block role="…"` declarations.
//!
//! Covers:
//! 1. Parsing at all three scopes (document, page, text) with all optional
//!    fields set and verified.
//! 2. Round-trip: format → reparse → the block_styles vecs are equal.
//! 3. Additive byte-identity: a document with NO `block` decls formats
//!    identically via KdlAdapter parse → format (the existing format parity
//!    tests cover this path; here we add an explicit regression guard).
//! 4. Validation: token referenced only via a `block` decl is NOT flagged
//!    `token.unused`.
//! 5. Validation: a `block` decl referencing a missing token yields
//!    `token.unknown_reference`.

mod common;
use common::*;

use zenith_core::format::format_document;
use zenith_core::{KdlAdapter, KdlSource, validate};

// ── 1. Parse at all three scopes ─────────────────────────────────────────────

#[test]
fn block_decls_parsed_at_all_three_scopes() {
    // Uses r##"…"## because the KDL booleans (#true/#false) contain `#`.
    let src = r##"zenith version=1 {
  assets {}
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.main" {
    block role="h1" font-size=(token)"size.h1" font-weight=(token)"weight.bold" space-after=(px)16
    block role="p" space-after=(px)8
    page id="pg.cover" w=(px)1280 h=(px)720 {
      block role="h1" fill=(token)"color.accent"
      text id="body" format="markdown" x=(px)80 y=(px)80 w=(px)1120 h=(px)560 {
        block role="p" space-after=(px)4 italic=#true align="left"
      }
    }
  }
}
"##;

    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("should parse");

    // Document-scope block decls.
    let doc_blocks = &doc.body.block_styles;
    assert_eq!(doc_blocks.len(), 2, "expected 2 document-scope block decls");

    let h1 = &doc_blocks[0];
    assert_eq!(h1.role, "h1");
    assert_eq!(
        h1.font_size,
        Some(PropertyValue::TokenRef("size.h1".to_owned()))
    );
    assert_eq!(
        h1.font_weight,
        Some(PropertyValue::TokenRef("weight.bold".to_owned()))
    );
    assert_eq!(h1.space_after, Some(px(16.0)));
    assert_eq!(h1.fill, None);
    assert_eq!(h1.align, None);
    assert_eq!(h1.italic, None);

    let p = &doc_blocks[1];
    assert_eq!(p.role, "p");
    assert_eq!(p.space_after, Some(px(8.0)));
    assert_eq!(p.font_size, None);

    // Page-scope block decls.
    assert_eq!(doc.body.pages.len(), 1);
    let page = &doc.body.pages[0];
    assert_eq!(
        page.block_styles.len(),
        1,
        "expected 1 page-scope block decl"
    );

    let page_h1 = &page.block_styles[0];
    assert_eq!(page_h1.role, "h1");
    assert_eq!(
        page_h1.fill,
        Some(PropertyValue::TokenRef("color.accent".to_owned()))
    );
    assert_eq!(page_h1.font_size, None);

    // Text-node-scope block decls.
    assert_eq!(page.children.len(), 1);
    let Node::Text(text) = &page.children[0] else {
        panic!("expected a text node");
    };
    assert_eq!(
        text.block_styles.len(),
        1,
        "expected 1 text-scope block decl"
    );

    let text_p = &text.block_styles[0];
    assert_eq!(text_p.role, "p");
    assert_eq!(text_p.space_after, Some(px(4.0)));
    assert_eq!(text_p.italic, Some(true));
    assert_eq!(text_p.align, Some("left".to_owned()));
}

// ── 2. Round-trip: format → reparse → equal block_styles ─────────────────────

#[test]
fn block_decls_round_trip() {
    let src = r##"zenith version=1 {
  assets {}
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.main" {
    block role="h1" font-size=(token)"size.h1" font-weight=(token)"weight.bold" space-after=(px)16
    page id="pg.one" w=(px)1280 h=(px)720 {
      block role="blockquote" fill=(token)"color.muted" space-before=(px)8 space-after=(px)8
      text id="t1" format="markdown" x=(px)0 y=(px)0 w=(px)640 h=(px)480 {
        block role="p" space-after=(px)4
      }
    }
  }
}
"##;

    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");

    // Format to canonical form.
    let formatted = format_document(&doc1).expect("format");
    let formatted_str = String::from_utf8(formatted).expect("utf8");

    // Reparse the formatted output.
    let doc2 = adapter
        .parse(formatted_str.as_bytes())
        .expect("parse 2 (round-trip)");

    // Block decls at all three scopes must survive verbatim.
    assert_eq!(
        doc1.body.block_styles, doc2.body.block_styles,
        "document-scope block_styles must round-trip"
    );
    assert_eq!(
        doc1.body.pages[0].block_styles, doc2.body.pages[0].block_styles,
        "page-scope block_styles must round-trip"
    );
    let Node::Text(t1) = &doc1.body.pages[0].children[0] else {
        panic!("expected text node in doc1");
    };
    let Node::Text(t2) = &doc2.body.pages[0].children[0] else {
        panic!("expected text node in doc2");
    };
    assert_eq!(
        t1.block_styles, t2.block_styles,
        "text-scope block_styles must round-trip"
    );

    // Spot-check canonical output contains the expected lines.
    assert!(
        formatted_str.contains(r##"block role="h1" font-size=(token)"size.h1""##),
        "formatted output must contain document-scope block decl; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r##"block role="blockquote" fill=(token)"color.muted""##),
        "formatted output must contain page-scope block decl; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r##"block role="p" space-after=(px)4"##),
        "formatted output must contain text-scope block decl; got:\n{formatted_str}"
    );
}

// ── 3. Additive byte-identity: no block decls → identical output ──────────────

#[test]
fn no_block_decls_byte_identical() {
    // A document with ZERO block decls: format must be stable (parse → format
    // → parse → format produces the same bytes).
    let src = r##"zenith version=1 {
  assets {}
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.main" {
    page id="pg.one" w=(px)1280 h=(px)720 {
      text id="t1" x=(px)0 y=(px)0 w=(px)640 h=(px)480 {
      }
    }
  }
}
"##;

    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // No block decls anywhere.
    assert!(doc.body.block_styles.is_empty());
    assert!(doc.body.pages[0].block_styles.is_empty());
    let Node::Text(text) = &doc.body.pages[0].children[0] else {
        panic!("expected text node");
    };
    assert!(text.block_styles.is_empty());

    // Format and reparse — output must be stable.
    let formatted1 = format_document(&doc).expect("format 1");
    let doc2 = adapter.parse(&formatted1).expect("parse 2");
    let formatted2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted1, formatted2,
        "format of a no-block-decl document must be byte-identical across two passes"
    );

    // Confirm the output contains no `block` lines.
    let out = String::from_utf8(formatted1).expect("utf8");
    assert!(
        !out.contains("\n  block ") && !out.contains("    block "),
        "output must not contain any block lines when no block decls are declared; got:\n{out}"
    );
}

// ── 4. Token referenced ONLY via a `block` decl is NOT flagged `token.unused` ─

#[test]
fn block_decl_token_ref_not_flagged_unused() {
    // The token "color.accent" is declared but referenced ONLY via a document-scope
    // `block` decl. Without the validator traversing block_styles this would produce
    // a false `token.unused` advisory.
    let src = r##"zenith version=1 {
  assets {}
  tokens format="zenith-token-v1" {
    color id="color.accent" "#FF5500"
  }
  styles {}
  document id="doc.main" {
    block role="h1" fill=(token)"color.accent"
    page id="pg.one" w=(px)1280 h=(px)720 {
      text id="t1" x=(px)0 y=(px)0 w=(px)640 h=(px)480 {
      }
    }
  }
}
"##;

    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("should parse");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "token.unused"),
        "token referenced only via a block decl must not be flagged token.unused; \
         got diagnostics: {:?}",
        codes(&report)
    );
}

// ── 5. `block` decl referencing a missing token yields `token.unknown_reference` ─

#[test]
fn block_decl_missing_token_yields_unknown_reference() {
    // "color.missing" is referenced in a page-scope block decl but never declared.
    let src = r##"zenith version=1 {
  assets {}
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.main" {
    page id="pg.one" w=(px)1280 h=(px)720 {
      block role="p" fill=(token)"color.missing"
      text id="t1" x=(px)0 y=(px)0 w=(px)640 h=(px)480 {
      }
    }
  }
}
"##;

    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("should parse");
    let report = validate(&doc);
    assert!(
        has_code(&report, "token.unknown_reference"),
        "block decl referencing a missing token must yield token.unknown_reference; \
         got diagnostics: {:?}",
        codes(&report)
    );
}
