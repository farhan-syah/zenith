mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::{FitMode, SceneCommand};

// ── Shared source for all recipe tests ───────────────────────────────────────
//
// A 1080×1080 page that exercises three S-recipe scenarios simultaneously:
//
//   S-recipe-01 — layered gradient background + translucent gradient ellipses
//   S-recipe-02 — general node blur producing a soft-blob effect
//                 (resolved G-06; verified against current engine 2026-06)
//   S-recipe-10 — image-over-native-background with fit="contain"
//
// The source must use r##"…"## raw strings because .zen sources contain
// `#` hex color literals.
const RECIPE_SRC: &str = r##"zenith version=1 {
  project id="proj.recipe" name="Recipe"
  assets {
    asset id="asset.hero" kind="image" src="hero.png"
  }
  tokens format="zenith-token-v1" {
    token id="color.navy"   type="color" value="#001f5b"
    token id="color.indigo" type="color" value="#4b0082"
    token id="color.cyan"   type="color" value="#00bcd4"
    token id="color.violet" type="color" value="#7c00ff"
    token id="gradient.bg" type="gradient" angle=(deg)135 {
      stop offset=0.0 color=(token)"color.navy"
      stop offset=1.0 color=(token)"color.indigo"
    }
    token id="gradient.blob" type="gradient" angle=(deg)45 {
      stop offset=0.0 color=(token)"color.cyan"
      stop offset=1.0 color=(token)"color.violet"
    }
  }
  styles {}
  document id="doc.recipe" title="Recipe" {
    page id="page.recipe" w=(px)1080 h=(px)1080 background=(token)"gradient.bg" {
      ellipse id="ell.translucent" x=(px)100 y=(px)100 w=(px)600 h=(px)400 fill=(token)"gradient.blob" opacity=0.25
      ellipse id="ell.blur" x=(px)300 y=(px)300 w=(px)480 h=(px)480 fill=(token)"gradient.blob" opacity=0.18 blur=(px)40
      image id="img.hero" asset="asset.hero" x=(px)200 y=(px)400 w=(px)680 h=(px)500 fit="contain"
    }
  }
}
"##;

// ── Test 1: S-recipe-01 / S-recipe-02 / S-recipe-10 ─────────────────────────

/// Locks S-recipe-01 (layered gradient background + translucent gradient
/// ellipses), S-recipe-02 (general node blur / soft-blob; resolved G-06;
/// verified against current engine 2026-06), and S-recipe-10 (image over
/// native background with fit="contain") against the current compile engine.
///
/// Asserts the scene command stream contains:
/// - a page-background `FillRectGradient` covering the full 1080×1080 canvas,
/// - at least one `FillEllipseGradient` (from the translucent gradient ellipses),
/// - exactly one `BeginBlur`/`EndBlur` bracket around the blurred soft-blob ellipse,
/// - a `DrawImage` whose `fit` field equals `FitMode::Contain`.
#[test]
fn procedural_background_emits_expected_scene_commands() {
    let doc = parse(RECIPE_SRC);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;

    // ── S-recipe-01: page background must be a full-page FillRectGradient ──
    let bg_grad = cmds.iter().find(|c| {
        matches!(
            c,
            SceneCommand::FillRectGradient { x, y, w, h, .. }
                if *x == 0.0 && *y == 0.0 && *w == 1080.0 && *h == 1080.0
        )
    });
    assert!(
        bg_grad.is_some(),
        "page background must emit FillRectGradient covering 1080×1080: {cmds:?}"
    );

    // Verify angle from gradient.bg token (135°).
    if let Some(SceneCommand::FillRectGradient { gradient, .. }) = bg_grad {
        assert_eq!(
            gradient.angle_deg, 135.0,
            "background gradient angle must be 135°: {gradient:?}"
        );
        assert_eq!(
            gradient.stops.len(),
            2,
            "background gradient must have 2 stops"
        );
    }

    // ── S-recipe-01: translucent ellipses must emit FillEllipseGradient ────
    let ellipse_grads = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::FillEllipseGradient { .. }))
        .count();
    assert!(
        ellipse_grads >= 1,
        "at least one FillEllipseGradient expected (translucent ellipses): {cmds:?}"
    );

    // ── S-recipe-02: blurred soft-blob ellipse must emit BeginBlur/EndBlur ──
    let begin_blur_count = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .count();
    let end_blur_count = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::EndBlur))
        .count();
    assert_eq!(
        begin_blur_count, 1,
        "exactly one BeginBlur expected (blurred soft-blob): {cmds:?}"
    );
    assert_eq!(
        end_blur_count, 1,
        "exactly one EndBlur expected (blurred soft-blob): {cmds:?}"
    );

    // Verify the blur radius matches blur=(px)40.
    let blur_radius = cmds.iter().find_map(|c| match c {
        SceneCommand::BeginBlur { radius } => Some(*radius),
        _ => None,
    });
    assert_eq!(
        blur_radius,
        Some(40.0),
        "BeginBlur radius must be 40.0 (blur=(px)40): {cmds:?}"
    );

    // BeginBlur must precede EndBlur.
    let begin_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .expect("BeginBlur must be present");
    let end_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndBlur))
        .expect("EndBlur must be present");
    assert!(
        begin_idx < end_idx,
        "BeginBlur must precede EndBlur: begin={begin_idx} end={end_idx}"
    );

    // ── S-recipe-10: image must emit DrawImage with FitMode::Contain ────────
    let draw_image = cmds.iter().find_map(|c| match c {
        SceneCommand::DrawImage { fit, asset_id, .. } => Some((*fit, asset_id.as_str())),
        _ => None,
    });
    let (fit, aid) = draw_image.expect("DrawImage must be emitted for the hero image");
    assert_eq!(
        fit,
        FitMode::Contain,
        "image fit must be FitMode::Contain: {fit:?}"
    );
    assert_eq!(aid, "asset.hero", "asset_id must be 'asset.hero'");
}

// ── Test 2: determinism ───────────────────────────────────────────────────────

/// Locks S-recipe-01 / S-recipe-02 / S-recipe-10: compiling the same source
/// twice must produce a byte-identical scene, proving the compile pipeline is
/// deterministic and cannot silently diverge due to HashMap ordering or other
/// non-determinism. Verified against current engine 2026-06; S-recipe-02
/// general blur resolved G-06.
#[test]
fn procedural_background_is_deterministic() {
    let doc = parse(RECIPE_SRC);
    let provider = default_provider();

    let result_a = compile(&doc, &provider);
    let result_b = compile(&doc, &provider);

    // Scene implements PartialEq (derived in ir.rs) and to_json() is also
    // deterministic (no HashMap in Scene). Both are verified here.
    assert_eq!(
        result_a.scene, result_b.scene,
        "two compiles of the same source must produce identical scenes (PartialEq)"
    );

    let json_a = result_a
        .scene
        .to_json()
        .expect("first JSON serialization must succeed");
    let json_b = result_b
        .scene
        .to_json()
        .expect("second JSON serialization must succeed");
    assert_eq!(
        json_a, json_b,
        "two compiles of the same source must serialize to identical JSON"
    );
}

// ── Test 3: opacity bakes into gradient stop alpha ────────────────────────────

/// Locks S-recipe-01: the translucent ellipse (opacity=0.25) with a gradient
/// fill must have its opacity baked into the gradient stop alpha values.
///
/// `FillEllipseGradient.gradient.stops[*].color.a` reflects
/// `round(255 * 0.25) == 64` for fully-opaque stop colors.
/// This is observable in the compile-layer scene IR because
/// `apply_gradient_opacity` multiplies each stop's `.a` by `node_opacity *
/// ctx_opacity` (see `zenith-scene/src/compile/paint.rs`).
///
/// Verified against current engine 2026-06; S-recipe-02 general blur
/// resolved G-06.
#[test]
fn translucent_ellipse_opacity_bakes_into_fill() {
    let doc = parse(RECIPE_SRC);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // Collect all FillEllipseGradient commands that are NOT inside a
    // BeginBlur/EndBlur bracket (those belong to the blurred ellipse which
    // has opacity=0.18, a different multiplier). We want the first one
    // (ell.translucent, opacity=0.25).
    //
    // Strategy: find the BeginBlur/EndBlur window to exclude it, then look
    // for a FillEllipseGradient outside that window.
    let cmds = &result.scene.commands;

    let begin_blur_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginBlur { .. }));
    let end_blur_idx = cmds.iter().position(|c| matches!(c, SceneCommand::EndBlur));

    // Find a FillEllipseGradient outside the blur bracket.
    let translucent_grad = cmds.iter().enumerate().find_map(|(i, c)| {
        // Skip commands inside the blur bracket.
        let inside_blur = match (begin_blur_idx, end_blur_idx) {
            (Some(b), Some(e)) => i > b && i < e,
            _ => false,
        };
        if inside_blur {
            return None;
        }
        match c {
            SceneCommand::FillEllipseGradient { gradient, .. } => Some(gradient),
            _ => None,
        }
    });

    let gradient =
        translucent_grad.expect("a FillEllipseGradient outside the blur bracket must be emitted");

    // opacity=0.25 applied to fully-opaque stops (a=255):
    // 255 * 0.25 = 63.75 → rounds to 64.
    for (i, stop) in gradient.stops.iter().enumerate() {
        assert_eq!(
            stop.color.a, 64,
            "stop[{i}] alpha must be 64 (255 * 0.25 = 63.75 → 64) for opacity=0.25; \
             got {} — opacity is not baking into gradient stop alpha",
            stop.color.a
        );
    }
}
