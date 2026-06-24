//! Integration tests: assets validation.
//!
//! Test bodies moved verbatim from the former in-`src` `validate/check/tests/`
//! concern files; only import paths changed (`crate::`/`super::common` ->
//! `zenith_core::`/`common`).

use std::collections::BTreeMap;

mod common;

use common::*;

// ══════════════════════════════════════════════════════════════════════
// Asset validation tests
// ══════════════════════════════════════════════════════════════════════

/// Build a Document that has an AssetBlock but no content nodes.
fn doc_with_assets(assets: Vec<AssetDecl>) -> Document {
    Document {
        version: 1,
        colorspace: None,
        doc_id: None,
        mirror_margins: None,
        facing_pages: None,
        spread_gutter: None,
        page_progression: None,
        page_parity_start: None,
        margin_inner: None,
        margin_outer: None,
        margin_top: None,
        margin_bottom: None,
        project: None,
        assets: AssetBlock {
            assets,
            source_span: None,
        },
        libraries: Vec::new(),
        actions: Vec::new(),
        tokens: TokenBlock {
            format: "zenith-token-v1".to_owned(),
            tokens: vec![],
        },
        styles: StyleBlock::default(),
        components: Vec::new(),
        masters: Vec::new(),
        sections: Vec::new(),
        provenance: Vec::new(),
        variants: Vec::new(),
        recipes: Vec::new(),
        body: DocumentBody {
            id: "doc.asset-test".to_owned(),
            title: None,
            // A valid document needs ≥1 page; these asset tests don't care about
            // page content, so use a single minimal page.
            pages: vec![minimal_page("page.one", vec![])],
        },
    }
}

fn image_asset(id: &str, src: &str) -> AssetDecl {
    AssetDecl {
        id: id.to_owned(),
        kind: AssetKind::Image,
        src: src.to_owned(),
        sha256: None,
        ai_prompt: None,
        ai_model: None,
        ai_provider: None,
        ai_seed: None,
        ai_generation_date: None,
        ai_license: None,
        ai_source_rights: None,
        ai_safety_status: None,
        ai_reuse_policy: None,
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
            ai_prompt: None,
            ai_model: None,
            ai_provider: None,
            ai_seed: None,
            ai_generation_date: None,
            ai_license: None,
            ai_source_rights: None,
            ai_safety_status: None,
            ai_reuse_policy: None,
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
        ai_prompt: None,
        ai_model: None,
        ai_provider: None,
        ai_seed: None,
        ai_generation_date: None,
        ai_license: None,
        ai_source_rights: None,
        ai_safety_status: None,
        ai_reuse_policy: None,
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
        zenith_core::UnknownProperty {
            value: zenith_core::UnknownValue::Integer(96),
            ty: None,
        },
    );
    let doc = doc_with_assets(vec![AssetDecl {
        id: "asset.hi-res".to_owned(),
        kind: AssetKind::Image,
        src: "img/hi.png".to_owned(),
        sha256: None,
        ai_prompt: None,
        ai_model: None,
        ai_provider: None,
        ai_seed: None,
        ai_generation_date: None,
        ai_license: None,
        ai_source_rights: None,
        ai_safety_status: None,
        ai_reuse_policy: None,
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
        shadow: None,
        filter: None,
        mask: None,
        id: id.to_owned(),
        name: None,
        role: None,
        asset: asset.to_owned(),
        x: Some(px(40.0)),
        y: Some(px(40.0)),
        w: Some(px(160.0)),
        h: Some(px(120.0)),
        src_x: None,
        src_y: None,
        src_w: None,
        src_h: None,
        fit: fit.map(str::to_owned),
        clip: None,
        clip_radius: None,
        object_position_x: None,
        object_position_y: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
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

// ── AI-provenance fields: no asset.unknown_property diagnostic ────────

/// Assets with all 9 known `ai-` fields must not trigger `asset.unknown_property`.
#[test]
fn test_asset_ai_provenance_no_unknown_property_warning() {
    let doc = doc_with_assets(vec![AssetDecl {
        id: "asset.ai-full".to_owned(),
        kind: AssetKind::Image,
        src: "assets/ai.png".to_owned(),
        sha256: None,
        ai_prompt: Some("a red fox".to_owned()),
        ai_model: Some("dall-e-3".to_owned()),
        ai_provider: Some("openai".to_owned()),
        ai_seed: Some(42),
        ai_generation_date: Some("2024-01-15".to_owned()),
        ai_license: Some("CC0-1.0".to_owned()),
        ai_source_rights: Some("none".to_owned()),
        ai_safety_status: Some("approved".to_owned()),
        ai_reuse_policy: Some("free".to_owned()),
        source_span: None,
        unknown_props: BTreeMap::new(),
    }]);
    let report = validate(&doc);
    let unknown_prop_diags: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "asset.unknown_property")
        .collect();
    assert!(
        unknown_prop_diags.is_empty(),
        "ai-provenance fields must not trigger asset.unknown_property; got: {:?}",
        codes(&report)
    );
}
