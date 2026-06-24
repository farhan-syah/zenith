//! Integration tests for the canonical writer: asset declarations.
//!
//! Covers the `assets` block — parse, format, round-trip, format idempotency,
//! canonical property order, and AI-provenance fields — exercising both the
//! basic `AssetDecl` fields and all nine `ai-*` provenance properties.

mod common;

use common::*;
use zenith_core::format::format_document;

/// A `.zen` document with an assets block containing two declarations.
const WITH_ASSETS: &str = r##"zenith version=1 {
  project id="proj.assets" name="Assets Test"
  assets {
    asset id="asset.logo" kind="svg" src="assets/logo.svg" sha256="deadbeef"
    asset id="asset.hero" kind="image" src="assets/hero.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.assets" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;

/// Assets block parses correctly: 2 assets, fields correct.
#[test]
fn test_assets_parse_fields() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");

    let assets = &doc.assets.assets;
    assert_eq!(assets.len(), 2, "expected 2 asset declarations");

    let logo = &assets[0];
    assert_eq!(logo.id, "asset.logo");
    assert_eq!(logo.kind, zenith_core::AssetKind::Svg);
    assert_eq!(logo.src, "assets/logo.svg");
    assert_eq!(logo.sha256.as_deref(), Some("deadbeef"));

    let hero = &assets[1];
    assert_eq!(hero.id, "asset.hero");
    assert_eq!(hero.kind, zenith_core::AssetKind::Image);
    assert_eq!(hero.src, "assets/hero.png");
    assert!(hero.sha256.is_none(), "sha256 should be None when absent");
}

/// Assets block round-trip: parse → format → parse yields same fields.
#[test]
fn test_assets_round_trip_ast_equality() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");
    let doc2 = adapter.parse(&formatted).expect("re-parse after format");

    // Compare assets (spans may differ; compare fields directly).
    let a1 = &doc_orig.assets.assets;
    let a2 = &doc2.assets.assets;
    assert_eq!(a1.len(), a2.len(), "asset count must survive round-trip");
    for (orig, reparsed) in a1.iter().zip(a2.iter()) {
        assert_eq!(orig.id, reparsed.id);
        assert_eq!(orig.kind, reparsed.kind);
        assert_eq!(orig.src, reparsed.src);
        assert_eq!(orig.sha256, reparsed.sha256);
    }
}

/// Format idempotency: format twice → identical bytes.
#[test]
fn test_assets_format_idempotency() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");
    let s1 = format_document(&doc).expect("format 1");
    let doc2 = adapter.parse(&s1).expect("parse after first format");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "assets format must be idempotent"
    );
}

/// Canonical property order: id, kind, src, sha256 in that order.
#[test]
fn test_assets_canonical_property_order() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(WITH_ASSETS.as_bytes())
        .expect("parse must succeed");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    let logo_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("asset") && l.contains("asset.logo"))
        .expect("must find logo asset line");

    let pos_id = logo_line.find("id=").expect("id= must be present");
    let pos_kind = logo_line.find("kind=").expect("kind= must be present");
    let pos_src = logo_line.find("src=").expect("src= must be present");
    let pos_sha256 = logo_line.find("sha256=").expect("sha256= must be present");

    assert!(pos_id < pos_kind, "id must come before kind");
    assert!(pos_kind < pos_src, "kind must come before src");
    assert!(pos_src < pos_sha256, "src must come before sha256");
}

// ── Asset AI-provenance tests ─────────────────────────────────────────

/// Parse an asset with all 9 AI-provenance fields and assert each lands on `AssetDecl`.
#[test]
fn test_asset_ai_provenance_parse_fields() {
    let src = r##"zenith version=1 {
  assets {
    asset id="asset.gen" kind="image" src="assets/gen.png" ai-prompt="a red fox" ai-model="dall-e-3" ai-provider="openai" ai-seed=42 ai-generation-date="2024-01-15" ai-license="CC0-1.0" ai-source-rights="none" ai-safety-status="approved" ai-reuse-policy="free"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ai" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let decl = &doc.assets.assets[0];
    assert_eq!(decl.ai_prompt.as_deref(), Some("a red fox"));
    assert_eq!(decl.ai_model.as_deref(), Some("dall-e-3"));
    assert_eq!(decl.ai_provider.as_deref(), Some("openai"));
    assert_eq!(decl.ai_seed, Some(42_i64));
    assert_eq!(decl.ai_generation_date.as_deref(), Some("2024-01-15"));
    assert_eq!(decl.ai_license.as_deref(), Some("CC0-1.0"));
    assert_eq!(decl.ai_source_rights.as_deref(), Some("none"));
    assert_eq!(decl.ai_safety_status.as_deref(), Some("approved"));
    assert_eq!(decl.ai_reuse_policy.as_deref(), Some("free"));
}

/// Parse → format → parse: all 9 AI-provenance fields survive.
#[test]
fn test_asset_ai_provenance_round_trip_ast_equality() {
    let src = r##"zenith version=1 {
  assets {
    asset id="asset.gen" kind="image" src="assets/gen.png" ai-prompt="a red fox" ai-model="dall-e-3" ai-provider="openai" ai-seed=42 ai-generation-date="2024-01-15" ai-license="CC0-1.0" ai-source-rights="none" ai-safety-status="approved" ai-reuse-policy="free"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ai" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");
    let formatted = format_document(&doc1).expect("format");
    let doc2 = adapter.parse(&formatted).expect("parse 2");
    let d1 = &doc1.assets.assets[0];
    let d2 = &doc2.assets.assets[0];
    assert_eq!(d1.ai_prompt, d2.ai_prompt);
    assert_eq!(d1.ai_model, d2.ai_model);
    assert_eq!(d1.ai_provider, d2.ai_provider);
    assert_eq!(d1.ai_seed, d2.ai_seed);
    assert_eq!(d1.ai_generation_date, d2.ai_generation_date);
    assert_eq!(d1.ai_license, d2.ai_license);
    assert_eq!(d1.ai_source_rights, d2.ai_source_rights);
    assert_eq!(d1.ai_safety_status, d2.ai_safety_status);
    assert_eq!(d1.ai_reuse_policy, d2.ai_reuse_policy);
}

/// `format(format(doc)) == format(doc)` for assets with AI-provenance fields.
#[test]
fn test_asset_ai_provenance_format_idempotency() {
    let src = r##"zenith version=1 {
  assets {
    asset id="asset.gen" kind="image" src="assets/gen.png" ai-prompt="a red fox" ai-model="dall-e-3" ai-provider="openai" ai-seed=42 ai-generation-date="2024-01-15" ai-license="CC0-1.0" ai-source-rights="none" ai-safety-status="approved" ai-reuse-policy="free"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ai" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let s1 = format_document(&doc).expect("format 1");
    let doc2 = adapter.parse(&s1).expect("parse after format 1");
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "AI-provenance asset format must be idempotent"
    );
}

/// An asset with NO `ai-` props: formatted output contains no `"ai-"` substring.
#[test]
fn test_asset_ai_provenance_absent_byte_identity() {
    let src = r##"zenith version=1 {
  assets {
    asset id="asset.plain" kind="image" src="assets/plain.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.plain" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        !text.contains("ai-"),
        "formatted output must not contain \"ai-\" when no provenance fields are set; got:\n{text}"
    );
}

/// `ai-prompt` containing both a double-quote and a newline survives parse → format → parse.
#[test]
fn test_asset_ai_provenance_string_escaping_round_trip() {
    // Construct the prompt value with embedded quote and newline, then build a
    // document programmatically so the raw string is set on the AST directly
    // (avoiding the need to embed the escapes in a raw-string literal here).
    let prompt_value = "a \"quoted\" word\nand a newline".to_owned();

    let src = r##"zenith version=1 {
  assets {
    asset id="asset.esc" kind="image" src="assets/esc.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.esc" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let mut doc = adapter.parse(src.as_bytes()).expect("parse");
    doc.assets.assets[0].ai_prompt = Some(prompt_value.clone());

    let formatted = format_document(&doc).expect("format");
    let doc2 = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        doc2.assets.assets[0].ai_prompt.as_deref(),
        Some(prompt_value.as_str()),
        "ai-prompt with embedded quote and newline must survive format → parse"
    );
}

/// `sha256` is emitted before `ai-prompt`, and `ai-reuse-policy` before any unknown prop.
#[test]
fn test_asset_ai_provenance_canonical_order() {
    let src = r##"zenith version=1 {
  assets {
    asset id="asset.ord" kind="image" src="assets/ord.png" sha256="abc123" ai-prompt="fox" ai-reuse-policy="free" zzz-unknown="yes"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ord" {
    page id="page.one" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();

    let asset_line = text
        .lines()
        .find(|l| l.trim_start().starts_with("asset") && l.contains("asset.ord"))
        .expect("must find the asset.ord line");

    let pos_sha256 = asset_line.find("sha256=").expect("sha256= must be present");
    let pos_ai_prompt = asset_line
        .find("ai-prompt=")
        .expect("ai-prompt= must be present");
    let pos_ai_reuse = asset_line
        .find("ai-reuse-policy=")
        .expect("ai-reuse-policy= must be present");
    let pos_unknown = asset_line
        .find("zzz-unknown=")
        .expect("zzz-unknown= must be present");

    assert!(
        pos_sha256 < pos_ai_prompt,
        "sha256 must come before ai-prompt"
    );
    assert!(
        pos_ai_reuse < pos_unknown,
        "ai-reuse-policy must come before unknown props"
    );
}
