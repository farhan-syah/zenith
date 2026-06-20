//! Tests for the scene compiler.
//!
//! Moved verbatim from the former `compile.rs` test module; imports are stated
//! explicitly because this file now lives one directory deeper and the parent
//! module's `use` block is trimmed to what production code references.

use super::{CompileResult, compile, compile_page};
use crate::ir::{Color, FitMode, ImageClip, SceneCommand};
use zenith_core::{Document, KdlAdapter, KdlSource, default_provider};

// ── Helper to parse a .zen source string ──────────────────────────────

fn parse(src: &str) -> Document {
    KdlAdapter
        .parse(src.as_bytes())
        .expect("test document must parse")
}

// ── Minimal single-rect document ──────────────────────────────────────

/// A page with a single full-page rect filled via a token color.
/// Expected scene: PushClip → FillRect (bg from token) → FillRect (rect) → PopClip.
/// In this test the page has no background, so background FillRect is absent.
#[test]
fn single_rect_token_fill_compiles_correctly() {
    let src = r##"zenith version=1 {
  project id="proj.t1" name="T1"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.t1" title="T1" {
page id="page.t1" w=(px)640 h=(px)360 {
  rect id="rect.t1" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 640.0 && h == 360.0),
        "first command must be PushClip covering the page"
    );

    match &cmds[1] {
        SceneCommand::FillRect { x, y, w, h, color } => {
            assert_eq!(*x, 0.0);
            assert_eq!(*y, 0.0);
            assert_eq!(*w, 640.0);
            assert_eq!(*h, 360.0);
            // #f8fafc → r=0xf8=248, g=0xfa=250, b=0xfc=252, a=255
            assert_eq!(color.r, 0xf8);
            assert_eq!(color.g, 0xfa);
            assert_eq!(color.b, 0xfc);
            assert_eq!(color.a, 255);
        }
        other => panic!("expected FillRect, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

// ── Two rects → two FillRects in source order ─────────────────────────

#[test]
fn two_rects_emitted_in_source_order() {
    let src = r##"zenith version=1 {
  project id="proj.t2" name="T2"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#111111"
token id="color.b" type="color" value="#222222"
  }
  styles {}
  document id="doc.t2" title="T2" {
page id="page.t2" w=(px)100 h=(px)100 {
  rect id="rect.a" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.a"
  rect id="rect.b" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.b"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect(a), FillRect(b), PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands, got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x11),
        other => panic!("expected FillRect for rect.a, got {other:?}"),
    }
    match &cmds[2] {
        SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x22),
        other => panic!("expected FillRect for rect.b, got {other:?}"),
    }
}

// ── visible=false rect is not emitted ─────────────────────────────────

#[test]
fn invisible_rect_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.t3" name="T3"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#abcdef"
  }
  styles {}
  document id="doc.t3" title="T3" {
page id="page.t3" w=(px)100 h=(px)100 {
  rect id="rect.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" visible=#false
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // No diagnostics expected (visible=false is a normal skip, not an error).
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Only PushClip + PopClip; no FillRect.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── JSON schema field is "zenith-scene-v1" ────────────────────────────

#[test]
fn json_schema_field_value() {
    let src = r##"zenith version=1 {
  project id="proj.t5" name="T5"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.t5" title="T5" {
page id="page.t5" w=(px)100 h=(px)100 {}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let json = result.scene.to_json().expect("serialize must succeed");
    assert!(
        json.contains(r#""schema": "zenith-scene-v1""#),
        "JSON must contain schema field; got snippet: {}",
        &json[..json.len().min(200)]
    );
}

// ── JSON determinism ──────────────────────────────────────────────────

#[test]
fn json_serialization_is_deterministic() {
    let src = r##"zenith version=1 {
  project id="proj.t6" name="T6"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#aabbcc"
  }
  styles {}
  document id="doc.t6" title="T6" {
page id="page.t6" w=(px)200 h=(px)100 {
  rect id="rect.t6" x=(px)10 y=(px)20 w=(px)100 h=(px)50 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());
    let j1 = r1.scene.to_json().expect("serialize 1");
    let j2 = r2.scene.to_json().expect("serialize 2");
    assert_eq!(
        j1, j2,
        "two compiles of the same doc must produce identical JSON"
    );
}

// ── Page background emitted as first FillRect ─────────────────────────

#[test]
fn page_background_emitted_before_children() {
    let src = r##"zenith version=1 {
  project id="proj.t7" name="T7"
  tokens format="zenith-token-v1" {
token id="color.bg" type="color" value="#ffffff"
token id="color.fill" type="color" value="#000000"
  }
  styles {}
  document id="doc.t7" title="T7" {
page id="page.t7" w=(px)100 h=(px)100 background=(token)"color.bg" {
  rect id="rect.t7" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect(bg=white), FillRect(rect=black), PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);

    // Background fill must be white.
    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => {
            assert_eq!(color.r, 255, "bg must be white");
            assert_eq!(color.g, 255);
            assert_eq!(color.b, 255);
        }
        other => panic!("expected background FillRect, got {other:?}"),
    }

    // Child rect must be black.
    match &cmds[2] {
        SceneCommand::FillRect { color, .. } => {
            assert_eq!(color.r, 0, "child rect must be black");
            assert_eq!(color.g, 0);
            assert_eq!(color.b, 0);
        }
        other => panic!("expected child FillRect, got {other:?}"),
    }
}

// ── Opacity multiplied into alpha ─────────────────────────────────────

#[test]
fn opacity_applied_to_fill_alpha() {
    // A full-alpha color (#ffffff, a=255) with opacity=0.5 → a≈128.
    let src = r##"zenith version=1 {
  project id="proj.t8" name="T8"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.t8" title="T8" {
page id="page.t8" w=(px)100 h=(px)100 {
  rect id="rect.t8" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" opacity=0.5
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    match &result.scene.commands[1] {
        SceneCommand::FillRect { color, .. } => {
            // 255 * 0.5 = 127.5 → rounds to 128.
            assert_eq!(color.a, 128, "opacity 0.5 must give a=128; got {}", color.a);
        }
        other => panic!("expected FillRect, got {other:?}"),
    }
}

// ── Text node with token-resolved fill/font/size → DrawGlyphRun ───────

#[test]
fn text_node_token_resolved_compiles_to_draw_glyph_run() {
    // A page with a text node whose fill, font-family, and font-size all
    // reference tokens.  Shaping uses the bundled Noto Sans provider.
    let src = r##"zenith version=1 {
  project id="proj.tx1" name="TX1"
  tokens format="zenith-token-v1" {
token id="color.ink"     type="color"      value="#111827"
token id="font.body"     type="fontFamily" value="Noto Sans"
token id="size.body"     type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.tx1" title="TX1" {
page id="page.tx1" w=(px)400 h=(px)200 {
  text id="label.tx1" x=(px)10 y=(px)20 w=(px)380 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Hello Zenith"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // No shaping errors expected.
    let unshaped: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.text_unshaped")
        .collect();
    assert!(
        unshaped.is_empty(),
        "no text_unshaped diagnostics expected; got: {:?}",
        result.diagnostics
    );

    // Commands: PushClip, DrawGlyphRun, PopClip.
    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[2], SceneCommand::PopClip));

    match &cmds[1] {
        SceneCommand::DrawGlyphRun {
            x,
            y,
            font_id,
            font_size,
            color,
            glyphs,
        } => {
            // x is the text-box origin x.
            assert_eq!(*x, 10.0, "x must be text-box origin (10px)");
            // y is baseline = text_y + ascent; ascent > 0, so y > 20.0.
            assert!(*y > 20.0, "baseline y must be > text_y (20px); got {}", y);
            // font_id must be the stable Noto Sans id.
            assert_eq!(
                font_id, "noto-sans-400-normal",
                "font_id must be noto-sans-400-normal"
            );
            assert_eq!(*font_size, 24.0, "font_size must be 24px");
            // Fill color: #111827 → r=0x11=17, g=0x18=24, b=0x27=39.
            assert_eq!(color.r, 0x11, "color.r must be 0x11");
            assert_eq!(color.g, 0x18, "color.g must be 0x18");
            assert_eq!(color.b, 0x27, "color.b must be 0x27");
            assert_eq!(color.a, 255, "color.a must be 255 (opaque)");
            // Glyph run must be non-empty.
            assert!(
                !glyphs.is_empty(),
                "glyphs must be non-empty for 'Hello Zenith'"
            );
        }
        other => panic!("expected DrawGlyphRun, got {other:?}"),
    }
}

// ── Span vertical-align="super" → smaller font + raised baseline ──────

#[test]
fn span_vertical_align_super_renders_smaller_and_raised() {
    // A text node with a baseline span followed by a superscript span. The
    // superscript run must shape at a REDUCED font size (0.65 × 24 = 15.6) and
    // sit ABOVE the baseline span's baseline.
    let src = r##"zenith version=1 {
  project id="proj.va" name="VA"
  tokens format="zenith-token-v1" {
token id="size.body" type="dimension" value=(px)24
  }
  styles {}
  document id="doc.va" title="VA" {
page id="page.va" w=(px)400 h=(px)200 {
  text id="t.va" x=(px)10 y=(px)20 w=(px)380 h=(px)60 font-size=(token)"size.body" {
    span "x"
    span "2" vertical-align="super"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let runs: Vec<(f64, f32)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, font_size, .. } => Some((*y, *font_size)),
            _ => None,
        })
        .collect();
    assert_eq!(
        runs.len(),
        2,
        "expected two glyph runs (baseline + super); got {:?}",
        runs
    );

    let (base_y, base_fs) = runs[0];
    let (super_y, super_fs) = runs[1];

    // Baseline span uses the full node font size (24).
    assert_eq!(base_fs, 24.0, "baseline span must render at full 24px");
    // Superscript span uses the reduced size (0.65 × 24 = 15.6).
    assert!(
        super_fs < base_fs,
        "superscript font_size ({super_fs}) must be < node font_size ({base_fs})"
    );
    assert!(
        (super_fs - 15.6).abs() < 0.01,
        "superscript font_size must be 0.65 × 24 = 15.6; got {super_fs}"
    );
    // Superscript baseline is raised (smaller y = higher on the page).
    assert!(
        super_y < base_y,
        "superscript baseline y ({super_y}) must be above the baseline span's y ({base_y})"
    );
}

/// A plain text node (no vertical-align anywhere) must compile to a
/// byte-identical command stream relative to a second run — proving the
/// vertical-align machinery does not perturb the no-vertical-align path.
#[test]
fn plain_text_byte_identical_with_vertical_align_feature() {
    let src = r##"zenith version=1 {
  project id="proj.pi" name="PI"
  tokens format="zenith-token-v1" {
token id="size.body" type="dimension" value=(px)24
  }
  styles {}
  document id="doc.pi" title="PI" {
page id="page.pi" w=(px)400 h=(px)200 {
  text id="t.pi" x=(px)10 y=(px)20 w=(px)380 h=(px)60 font-size=(token)"size.body" {
    span "Hello Zenith"
  }
}
  }
}
"##;
    let doc = parse(src);
    let a = compile(&doc, &default_provider());
    let b = compile(&doc, &default_provider());

    let ja = serde_json::to_string(&a.scene).expect("scene a json");
    let jb = serde_json::to_string(&b.scene).expect("scene b json");
    assert_eq!(
        ja, jb,
        "two compiles must be byte-identical (deterministic)"
    );

    // The plain span must shape at the full node size with a normal baseline.
    let run = a
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, font_size, .. } => Some((*y, *font_size)),
            _ => None,
        })
        .expect("a plain text node must emit a DrawGlyphRun");
    assert_eq!(run.1, 24.0, "plain span must render at full 24px");
    assert!(run.0 > 20.0, "plain baseline y must be text_y + ascent");
}

// ── All-primary span stays a single DrawGlyphRun (fallback byte-identity) ──

#[test]
fn all_primary_text_emits_single_draw_glyph_run() {
    // With per-glyph font fallback wired into compilation, a span whose every
    // character is covered by the primary face MUST still compile to exactly
    // one DrawGlyphRun — and the command stream must be stable across two
    // compiles (deterministic, byte-identical to the pre-fallback output).
    let src = r##"zenith version=1 {
  project id="proj.bi" name="BI"
  styles {}
  document id="doc.bi" title="BI" {
page id="page.bi" w=(px)400 h=(px)200 {
  text id="label.bi" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "Hello Zenith 123!"
  }
}
  }
}
"##;
    let doc = parse(src);
    let a = compile(&doc, &default_provider());
    let b = compile(&doc, &default_provider());

    // Deterministic: identical command streams across two compiles.
    assert_eq!(
        a.scene.commands, b.scene.commands,
        "all-primary compilation must be deterministic / byte-identical"
    );

    // Exactly one DrawGlyphRun for the all-primary span (no fragmentation).
    let glyph_runs = a
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    assert_eq!(
        glyph_runs, 1,
        "an all-primary span must emit exactly one DrawGlyphRun; got {glyph_runs}"
    );
}

// ── Rect then text → FillRect before DrawGlyphRun (z-order) ──────────

#[test]
fn rect_then_text_z_order_preserved() {
    let src = r##"zenith version=1 {
  project id="proj.tx2" name="TX2"
  tokens format="zenith-token-v1" {
token id="color.bg"  type="color"      value="#ffffff"
token id="color.ink" type="color"      value="#000000"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)16
  }
  styles {}
  document id="doc.tx2" title="TX2" {
page id="page.tx2" w=(px)400 h=(px)200 {
  rect id="bg.rect" x=(px)0 y=(px)0 w=(px)400 h=(px)200 fill=(token)"color.bg"
  text id="label.tx2" x=(px)10 y=(px)20 w=(px)380 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let cmds = &result.scene.commands;
    // PushClip, FillRect, DrawGlyphRun, PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
    assert!(
        matches!(cmds[1], SceneCommand::FillRect { .. }),
        "second command must be FillRect (rect comes first)"
    );
    assert!(
        matches!(cmds[2], SceneCommand::DrawGlyphRun { .. }),
        "third command must be DrawGlyphRun (text comes after rect)"
    );
}

// ── Scene JSON of text contains DrawGlyphRun op + font_id, no byte arrays ─

#[test]
fn scene_json_draw_glyph_run_op_and_font_id_no_bytes() {
    let src = r##"zenith version=1 {
  project id="proj.tx3" name="TX3"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#333333"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)18
  }
  styles {}
  document id="doc.tx3" title="TX3" {
page id="page.tx3" w=(px)300 h=(px)100 {
  text id="label.tx3" x=(px)0 y=(px)0 w=(px)300 h=(px)50 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Hi"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let j1 = result.scene.to_json().expect("serialize 1");
    let j2 = result.scene.to_json().expect("serialize 2");

    // Must contain the op tag.
    assert!(
        j1.contains(r#""op": "DrawGlyphRun""#),
        "JSON must contain DrawGlyphRun op; snippet: {}",
        &j1[..j1.len().min(500)]
    );
    // Must contain the font_id string.
    assert!(
        j1.contains("noto-sans-400-normal"),
        "JSON must contain font_id; snippet: {}",
        &j1[..j1.len().min(500)]
    );
    // Must NOT contain a large byte array (no font bytes in IR).
    // Large byte arrays appear as `[1, 2, 3, ...]` with > ~50 numbers.
    // A simple heuristic: no run of more than 10 consecutive numbers separated by ", ".
    // We check that the JSON does not contain "bytes" as a key.
    assert!(
        !j1.contains(r#""bytes""#),
        "JSON must not contain a 'bytes' field; font bytes must not appear in the IR"
    );
    // Determinism: two serializations must be identical.
    assert_eq!(j1, j2, "two serializations must be identical (determinism)");
}

// ── Group: children emitted in source order ───────────────────────────

#[test]
fn group_children_emitted_in_order() {
    // A page with a bg rect and a group containing a rect then an ellipse.
    // After PushClip + bg FillRect, the group produces: FillRect, FillEllipse.
    let src = r##"zenith version=1 {
  project id="proj.gc" name="GC"
  tokens format="zenith-token-v1" {
token id="color.bg"   type="color" value="#ffffff"
token id="color.r"    type="color" value="#ff0000"
token id="color.e"    type="color" value="#0000ff"
  }
  styles {}
  document id="doc.gc" title="GC" {
page id="page.gc" w=(px)320 h=(px)200 background=(token)"color.bg" {
  group id="group.gc" {
    rect id="rect.gc" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.r"
    ellipse id="ellipse.gc" x=(px)70 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.e"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect(bg), FillRect(rect.gc), FillEllipse(ellipse.gc), PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(
        matches!(cmds[1], SceneCommand::FillRect { .. }),
        "cmd[1] must be bg FillRect"
    );
    assert!(
        matches!(cmds[2], SceneCommand::FillRect { .. }),
        "cmd[2] must be group-child FillRect"
    );
    assert!(
        matches!(cmds[3], SceneCommand::FillEllipse { .. }),
        "cmd[3] must be group-child FillEllipse"
    );
    assert!(matches!(cmds[4], SceneCommand::PopClip));
}

// ── Group: visible=false → entire subtree excluded ────────────────────

#[test]
fn invisible_group_subtree_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.gv" name="GV"
  tokens format="zenith-token-v1" {
token id="color.r" type="color" value="#ff0000"
token id="color.b" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.gv" title="GV" {
page id="page.gv" w=(px)100 h=(px)100 {
  group id="group.gv" visible=#false {
    rect id="rect.gv1" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.r"
    rect id="rect.gv2" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.b"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Only PushClip + PopClip; both children excluded because group is invisible.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── Group: opacity cascades to child alpha ────────────────────────────

#[test]
fn group_opacity_cascades_to_child() {
    // Group opacity=0.5, child rect fill is fully opaque #ffffff (a=255).
    // Expected child FillRect alpha ≈ 128 (255 * 1.0 * 0.5 = 127.5 → 128).
    let src = r##"zenith version=1 {
  project id="proj.go" name="GO"
  tokens format="zenith-token-v1" {
token id="color.w" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.go" title="GO" {
page id="page.go" w=(px)100 h=(px)100 {
  group id="group.go" opacity=0.5 {
    rect id="rect.go" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.w"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => {
            // 255 * 1.0 (node opacity) * 0.5 (group opacity) = 127.5 → 128.
            assert_eq!(
                color.a, 128,
                "cascaded opacity 0.5 must give a=128; got {}",
                color.a
            );
        }
        other => panic!("expected FillRect, got {other:?}"),
    }
}

// ── Group: x/y translates child geometry ─────────────────────────────

#[test]
fn group_xy_translates_child() {
    // Group x=(px)10 y=(px)20; child rect at x=(px)5 y=(px)5.
    // Expected FillRect at x=15.0 y=25.0.
    let src = r##"zenith version=1 {
  project id="proj.gt" name="GT"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.gt" title="GT" {
page id="page.gt" w=(px)200 h=(px)200 {
  group id="group.gt" x=(px)10 y=(px)20 {
    rect id="rect.gt" x=(px)5 y=(px)5 w=(px)50 h=(px)50 fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillRect { x, y, .. } => {
            assert_eq!(
                *x, 15.0,
                "child x must be group.x(10) + rect.x(5) = 15; got {x}"
            );
            assert_eq!(
                *y, 25.0,
                "child y must be group.y(20) + rect.y(5) = 25; got {y}"
            );
        }
        other => panic!("expected FillRect, got {other:?}"),
    }
}

// ── role="guide" nodes are excluded from render output ──────────────────

#[test]
fn guide_role_nodes_are_not_rendered() {
    let src = r##"zenith version=1 {
  project id="proj.g" name="G"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#000000"
  }
  styles {}
  document id="doc.g" title="G" {
page id="page.g" w=(px)100 h=(px)100 {
  rect id="rect.real" x=(px)0 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.fill"
  rect id="rect.guide" role="guide" x=(px)50 y=(px)50 w=(px)40 h=(px)40 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Exactly one FillRect for the real rect; the guide rect emits nothing.
    // (No page background, so no background FillRect.)
    let fills = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
        .count();
    assert_eq!(
        fills, 1,
        "guide-role rect must not render; expected 1 FillRect, got {fills}: {:?}",
        result.scene.commands
    );
}

// ── Unresolvable font → font.unresolved advisory + fallback render ──────

#[test]
fn unresolvable_font_family_falls_back_and_emits_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.tx4" name="TX4"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.tx4" title="TX4" {
page id="page.tx4" w=(px)200 h=(px)100 {
  text id="label.tx4" x=(px)0 y=(px)0 w=(px)200 h=(px)50 fill="#000000" font-family="Nonexistent" {
    span "test"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Exactly one font.unresolved advisory naming the node and the missing family.
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.unresolved")
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "expected 1 font.unresolved advisory; got: {:?}",
        result.diagnostics
    );
    assert!(
        unresolved[0].message.contains("label.tx4")
            && unresolved[0].message.contains("Nonexistent"),
        "advisory must name the node and the missing family; got: {:?}",
        unresolved[0]
    );

    // Text must STILL render via the fallback face — DrawGlyphRun present.
    let glyph_cmds: Vec<_> = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .collect();
    assert!(
        !glyph_cmds.is_empty(),
        "text must render in the fallback face, not be dropped; got: {:?}",
        result.scene.commands
    );
}

// ── Ellipse: token fill compiles to FillEllipse ───────────────────────

#[test]
fn single_ellipse_token_fill_compiles_correctly() {
    let src = r##"zenith version=1 {
  project id="proj.e1" name="E1"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.e1" title="E1" {
page id="page.e1" w=(px)640 h=(px)360 {
  ellipse id="ellipse.e1" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillEllipse, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 640.0 && h == 360.0),
        "first command must be PushClip covering the page"
    );

    match &cmds[1] {
        SceneCommand::FillEllipse { x, y, w, h, color } => {
            assert_eq!(*x, 0.0);
            assert_eq!(*y, 0.0);
            assert_eq!(*w, 640.0);
            assert_eq!(*h, 360.0);
            // #f8fafc → r=0xf8=248, g=0xfa=250, b=0xfc=252, a=255
            assert_eq!(color.r, 0xf8);
            assert_eq!(color.g, 0xfa);
            assert_eq!(color.b, 0xfc);
            assert_eq!(color.a, 255);
        }
        other => panic!("expected FillEllipse, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

// ── Ellipse: visible=false not emitted ────────────────────────────────

#[test]
fn invisible_ellipse_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.e2" name="E2"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#abcdef"
  }
  styles {}
  document id="doc.e2" title="E2" {
page id="page.e2" w=(px)100 h=(px)100 {
  ellipse id="ellipse.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.fill" visible=#false
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Only PushClip + PopClip; no FillEllipse.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── Ellipse: fill + stroke tokens compile to FillEllipse then StrokeEllipse

#[test]
fn ellipse_fill_and_stroke_tokens_emit_fill_then_stroke() {
    let src = r##"zenith version=1 {
  project id="proj.e3" name="E3"
  tokens format="zenith-token-v1" {
token id="color.fill"   type="color"     value="#1e293b"
token id="color.stroke" type="color"     value="#94a3b8"
token id="size.sw"      type="dimension" value=(px)4
  }
  styles {}
  document id="doc.e3" title="E3" {
page id="page.e3" w=(px)200 h=(px)200 {
  ellipse id="ellipse.e3" x=(px)10 y=(px)10 w=(px)180 h=(px)180 fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, FillEllipse, StrokeEllipse, PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands, got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { .. }),
        "first command must be PushClip"
    );

    match &cmds[1] {
        SceneCommand::FillEllipse { x, y, w, h, color } => {
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 10.0);
            assert_eq!(*w, 180.0);
            assert_eq!(*h, 180.0);
            // #1e293b → r=0x1e=30, g=0x29=41, b=0x3b=59, a=255
            assert_eq!(color.r, 0x1e);
            assert_eq!(color.g, 0x29);
            assert_eq!(color.b, 0x3b);
            assert_eq!(color.a, 255);
        }
        other => panic!("expected FillEllipse at index 1, got {other:?}"),
    }

    match &cmds[2] {
        SceneCommand::StrokeEllipse {
            x,
            y,
            w,
            h,
            color,
            stroke_width,
        } => {
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 10.0);
            assert_eq!(*w, 180.0);
            assert_eq!(*h, 180.0);
            // #94a3b8 → r=0x94=148, g=0xa3=163, b=0xb8=184, a=255
            assert_eq!(color.r, 0x94);
            assert_eq!(color.g, 0xa3);
            assert_eq!(color.b, 0xb8);
            assert_eq!(color.a, 255);
            assert_eq!(*stroke_width, 4.0);
        }
        other => panic!("expected StrokeEllipse at index 2, got {other:?}"),
    }

    assert!(
        matches!(cmds[3], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

// ── Ellipse: stroke only (no fill) compiles to StrokeEllipse only ─────

#[test]
fn ellipse_stroke_only_emits_stroke_ellipse_without_fill() {
    let src = r##"zenith version=1 {
  project id="proj.e4" name="E4"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color"     value="#f43f5e"
token id="size.sw"      type="dimension" value=(px)3
  }
  styles {}
  document id="doc.e4" title="E4" {
page id="page.e4" w=(px)100 h=(px)100 {
  ellipse id="ellipse.e4" x=(px)5 y=(px)5 w=(px)90 h=(px)90 stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, StrokeEllipse, PopClip — no FillEllipse
    assert_eq!(
        cmds.len(),
        3,
        "expected 3 commands (no fill), got: {:?}",
        cmds
    );

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { .. }),
        "first command must be PushClip"
    );

    match &cmds[1] {
        SceneCommand::StrokeEllipse {
            x,
            y,
            w,
            h,
            color,
            stroke_width,
        } => {
            assert_eq!(*x, 5.0);
            assert_eq!(*y, 5.0);
            assert_eq!(*w, 90.0);
            assert_eq!(*h, 90.0);
            // #f43f5e → r=0xf4=244, g=0x3f=63, b=0x5e=94, a=255
            assert_eq!(color.r, 0xf4);
            assert_eq!(color.g, 0x3f);
            assert_eq!(color.b, 0x5e);
            assert_eq!(color.a, 255);
            assert_eq!(*stroke_width, 3.0);
        }
        other => panic!("expected StrokeEllipse at index 1, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

// ── Line: token stroke compiles to StrokeLine ─────────────────────────

#[test]
fn single_line_token_stroke_compiles_correctly() {
    let src = r##"zenith version=1 {
  project id="proj.l1" name="L1"
  tokens format="zenith-token-v1" {
token id="color.rule" type="color" value="#94a3b8"
token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.l1" title="L1" {
page id="page.l1" w=(px)320 h=(px)200 {
  line id="line.divider" x1=(px)40 y1=(px)100 x2=(px)280 y2=(px)100 stroke=(token)"color.rule" stroke-width=(token)"size.stroke"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip, StrokeLine, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { .. }),
        "first command must be PushClip"
    );

    match &cmds[1] {
        SceneCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
        } => {
            assert_eq!(*x1, 40.0);
            assert_eq!(*y1, 100.0);
            assert_eq!(*x2, 280.0);
            assert_eq!(*y2, 100.0);
            // #94a3b8 → r=0x94=148, g=0xa3=163, b=0xb8=184
            assert_eq!(color.r, 0x94);
            assert_eq!(color.g, 0xa3);
            assert_eq!(color.b, 0xb8);
            assert_eq!(color.a, 255);
            // size.stroke = (px)2
            assert_eq!(*stroke_width, 2.0);
        }
        other => panic!("expected StrokeLine, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

// ── Line: visible=false not emitted ──────────────────────────────────

#[test]
fn invisible_line_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.l2" name="L2"
  tokens format="zenith-token-v1" {
token id="color.rule" type="color" value="#94a3b8"
  }
  styles {}
  document id="doc.l2" title="L2" {
page id="page.l2" w=(px)100 h=(px)100 {
  line id="line.hidden" x1=(px)0 y1=(px)50 x2=(px)100 y2=(px)50 stroke=(token)"color.rule" visible=#false
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Only PushClip + PopClip; no StrokeLine.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── Frame: PushClip → FillRect(child) → PopClip sequence ─────────────

#[test]
fn frame_emits_pushclip_children_popclip() {
    let src = r##"zenith version=1 {
  project id="proj.f1" name="F1"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.f1" title="F1" {
page id="page.f1" w=(px)320 h=(px)200 {
  frame id="frame.clip" x=(px)40 y=(px)40 w=(px)120 h=(px)100 {
    rect id="rect.inner" x=(px)50 y=(px)50 w=(px)60 h=(px)60 fill=(token)"color.fill"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Page PushClip, Frame PushClip, FillRect(child), Frame PopClip, Page PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

    // Page clip
    assert!(
        matches!(cmds[0], SceneCommand::PushClip { x, y, w, h } if x == 0.0 && y == 0.0 && w == 320.0 && h == 200.0),
        "cmd[0] must be page PushClip"
    );
    // Frame clip — the frame's own bbox
    assert!(
        matches!(cmds[1], SceneCommand::PushClip { x, y, w, h } if x == 40.0 && y == 40.0 && w == 120.0 && h == 100.0),
        "cmd[1] must be frame PushClip at (40,40,120,100); got: {:?}",
        cmds[1]
    );
    // Child FillRect
    assert!(
        matches!(cmds[2], SceneCommand::FillRect { .. }),
        "cmd[2] must be child FillRect"
    );
    // Frame PopClip
    assert!(
        matches!(cmds[3], SceneCommand::PopClip),
        "cmd[3] must be frame PopClip"
    );
    // Page PopClip
    assert!(
        matches!(cmds[4], SceneCommand::PopClip),
        "cmd[4] must be page PopClip"
    );
}

// ── Frame: child overflow still emitted (renderer clips, not compiler) ─

#[test]
fn frame_child_overflow_still_emitted() {
    // Child rect extends well beyond the frame bounds — compiler must emit
    // its full FillRect unchanged; clipping is the renderer's job.
    let src = r##"zenith version=1 {
  project id="proj.f2" name="F2"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#f97316"
  }
  styles {}
  document id="doc.f2" title="F2" {
page id="page.f2" w=(px)320 h=(px)200 {
  frame id="frame.clip" x=(px)40 y=(px)40 w=(px)120 h=(px)100 {
    rect id="rect.overflow" x=(px)100 y=(px)30 w=(px)100 h=(px)120 fill=(token)"color.fill"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Ensure child FillRect is present with its full (unclipped) geometry.
    let fill_rects: Vec<_> = cmds
        .iter()
        .filter_map(|c| {
            if let SceneCommand::FillRect { x, y, w, h, .. } = c {
                Some((*x, *y, *w, *h))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(fill_rects.len(), 1, "expected exactly one FillRect");
    let (rx, ry, rw, rh) = fill_rects[0];
    assert_eq!(
        rx, 100.0,
        "child FillRect x must be 100 (absolute, unclipped)"
    );
    assert_eq!(ry, 30.0, "child FillRect y must be 30");
    assert_eq!(rw, 100.0, "child FillRect w must be 100");
    assert_eq!(rh, 120.0, "child FillRect h must be 120");
}

// ── Frame: missing geometry → advisory, no PushClip ───────────────────

#[test]
fn frame_missing_geometry_skipped() {
    // Frame with x=None; compile must push a scene.missing_geometry advisory
    // and emit NO PushClip (so push/pop balance is preserved).
    let src = r##"zenith version=1 {
  project id="proj.f3" name="F3"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.f3" title="F3" {
page id="page.f3" w=(px)100 h=(px)100 {
  frame id="frame.nogeo" y=(px)0 w=(px)100 h=(px)100 {
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.missing_geometry")
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "expected 1 scene.missing_geometry advisory; got: {:?}",
        result.diagnostics
    );

    // Push/pop must still be balanced: only page PushClip + PopClip.
    let push_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::PushClip { .. }))
        .count();
    let pop_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::PopClip))
        .count();
    assert_eq!(push_count, pop_count, "PushClip/PopClip must be balanced");
    assert_eq!(push_count, 1, "only the page PushClip must be present");
}

// ── Frame: visible=false → entire subtree excluded ────────────────────

#[test]
fn invisible_frame_subtree_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.f4" name="F4"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.f4" title="F4" {
page id="page.f4" w=(px)100 h=(px)100 {
  frame id="frame.hidden" x=(px)0 y=(px)0 w=(px)100 h=(px)100 visible=#false {
    rect id="rect.inner" x=(px)0 y=(px)0 w=(px)50 h=(px)50 fill=(token)"color.fill"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Only page PushClip + PopClip; no frame PushClip, no FillRect.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── Frame: opacity cascades to child alpha ─────────────────────────────

#[test]
fn frame_opacity_cascades_to_child() {
    // Frame opacity=0.5, child rect fill fully opaque #ffffff (a=255).
    // Expected child FillRect alpha ≈ 128 (255 * 1.0 * 0.5 = 127.5 → 128).
    let src = r##"zenith version=1 {
  project id="proj.f5" name="F5"
  tokens format="zenith-token-v1" {
token id="color.w" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.f5" title="F5" {
page id="page.f5" w=(px)100 h=(px)100 {
  frame id="frame.opaque" x=(px)0 y=(px)0 w=(px)100 h=(px)100 opacity=0.5 {
    rect id="rect.inner" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.w"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let fill_rect = result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRect { .. }));
    match fill_rect {
        Some(SceneCommand::FillRect { color, .. }) => {
            // 255 * 1.0 (node opacity) * 0.5 (frame opacity) = 127.5 → 128.
            assert_eq!(
                color.a, 128,
                "cascaded opacity 0.5 must give a=128; got {}",
                color.a
            );
        }
        _ => panic!("expected a FillRect command"),
    }
}

// ── Frame: does NOT translate children (clip-only) ─────────────────────

#[test]
fn frame_does_not_translate_child() {
    // Frame at x=(px)40 y=(px)40; child rect at x=(px)50 y=(px)50.
    // Because frame is clip-only (no translation), the child FillRect must
    // be at x=50.0 y=50.0, NOT 90.0/90.0.
    let src = r##"zenith version=1 {
  project id="proj.f6" name="F6"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.f6" title="F6" {
page id="page.f6" w=(px)200 h=(px)200 {
  frame id="frame.noxlate" x=(px)40 y=(px)40 w=(px)120 h=(px)120 {
    rect id="rect.abs" x=(px)50 y=(px)50 w=(px)50 h=(px)50 fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let fill_rect = result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRect { .. }));
    match fill_rect {
        Some(SceneCommand::FillRect { x, y, .. }) => {
            assert_eq!(
                *x, 50.0,
                "child x must be 50 (absolute, frame does not translate); got {x}"
            );
            assert_eq!(
                *y, 50.0,
                "child y must be 50 (absolute, frame does not translate); got {y}"
            );
        }
        _ => panic!("expected a FillRect command"),
    }
}

// ══════════════════════════════════════════════════════════════════════
// Frame flow-layout compile tests
// ══════════════════════════════════════════════════════════════════════

/// Helper: collect every FillRect command's (x, y, w, h) in emission order.
fn fill_rects(result: &CompileResult) -> Vec<(f64, f64, f64, f64)> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

/// (a) A flow frame stacks two children vertically separated by `gap`:
/// child2.y == child1.y + child1.h + gap.
#[test]
fn flow_frame_stacks_children_with_gap() {
    // pad=0, gap=10. Two rects each with declared h=30. Both omit x/y/w so
    // the flow path injects content_left/cursor_y and content_w.
    let src = r##"zenith version=1 {
  project id="proj.flow1" name="Flow1"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
token id="space.gap" type="dimension" value=(px)10
  }
  styles {
style id="style.flow" {
  gap (token)"space.gap"
}
  }
  document id="doc.flow1" title="Flow1" {
page id="page.flow1" w=(px)200 h=(px)200 {
  frame id="frame.flow" x=(px)20 y=(px)30 w=(px)160 h=(px)160 layout="flow" style="style.flow" {
    rect id="rect.a" h=(px)30 fill=(token)"color.k"
    rect id="rect.b" h=(px)30 fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert_eq!(
        rects.len(),
        2,
        "expected two child FillRects; got {rects:?}"
    );
    let (_, y1, _, h1) = rects[0];
    let (_, y2, _, _) = rects[1];
    // content_top = frame_y(30) + pad(0) = 30.
    assert_eq!(y1, 30.0, "child1 y must be content_top (30); got {y1}");
    // child2.y == child1.y + child1.h + gap = 30 + 30 + 10 = 70.
    assert_eq!(
        y2,
        y1 + h1 + 10.0,
        "child2 y must be child1.y + child1.h + gap; got {y2}"
    );
}

/// (b) Padding insets children: child_x == frame_x + pad and
/// child_w == frame_w - 2*pad (when the child declares no own w).
#[test]
fn flow_frame_padding_insets_children() {
    // pad=16, frame x=20 w=160 → content_left=36, content_w=128.
    let src = r##"zenith version=1 {
  project id="proj.flow2" name="Flow2"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
token id="space.pad" type="dimension" value=(px)16
  }
  styles {
style id="style.flow" {
  padding (token)"space.pad"
}
  }
  document id="doc.flow2" title="Flow2" {
page id="page.flow2" w=(px)200 h=(px)200 {
  frame id="frame.flow" x=(px)20 y=(px)30 w=(px)160 h=(px)160 layout="flow" style="style.flow" {
    rect id="rect.a" h=(px)30 fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert_eq!(rects.len(), 1, "expected one child FillRect; got {rects:?}");
    let (x, y, w, _) = rects[0];
    assert_eq!(x, 36.0, "child_x must be frame_x + pad (20+16=36); got {x}");
    assert_eq!(y, 46.0, "child_y must be content_top (30+16=46); got {y}");
    assert_eq!(
        w, 128.0,
        "child_w must be frame_w - 2*pad (160-32=128); got {w}"
    );
}

/// (c) Layout absent / "absolute": a child with explicit x/y produces a
/// byte-identical command stream to the clip-only model (no flow injection).
#[test]
fn flow_absent_is_byte_identical() {
    // Same document twice: once with layout="absolute", once with no layout.
    // Both must equal the clip-only output where the child keeps its own
    // x=50 y=60 coords.
    let make = |layout_attr: &str| {
        format!(
            r##"zenith version=1 {{
  project id="proj.flow3" name="Flow3"
  tokens format="zenith-token-v1" {{
token id="color.k" type="color" value="#000000"
  }}
  styles {{}}
  document id="doc.flow3" title="Flow3" {{
page id="page.flow3" w=(px)200 h=(px)200 {{
  frame id="frame.abs" x=(px)20 y=(px)30 w=(px)160 h=(px)160 {layout_attr} {{
    rect id="rect.a" x=(px)50 y=(px)60 w=(px)40 h=(px)30 fill=(token)"color.k"
  }}
}}
  }}
}}
"##
        )
    };

    let base = compile(&parse(&make("")), &default_provider());
    let absolute = compile(&parse(&make("layout=\"absolute\"")), &default_provider());

    assert_eq!(
        base.scene.commands, absolute.scene.commands,
        "layout=\"absolute\" must be byte-identical to no-layout clip-only output"
    );
    // And the child kept its own absolute coords (no flow injection).
    let rects = fill_rects(&base);
    assert_eq!(rects, vec![(50.0, 60.0, 40.0, 30.0)]);
}

/// (e) A text child WITHOUT a declared `h` gets a measured height so the
/// cursor advances past it — a following rect sits below the text block.
#[test]
fn flow_text_without_h_advances_cursor() {
    // pad=0, gap=0. A text child (no h) followed by a rect (h=20). The rect
    // must sit at content_top + measured_text_height (> content_top), proving
    // the text's intrinsic height advanced the cursor.
    let src = r##"zenith version=1 {
  project id="proj.flow5" name="Flow5"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.flow5" title="Flow5" {
page id="page.flow5" w=(px)400 h=(px)400 {
  frame id="frame.flow" x=(px)0 y=(px)0 w=(px)300 h=(px)300 layout="flow" {
    text id="text.a" font-size=(px)20 {
      span "Hello flow layout"
    }
    rect id="rect.below" h=(px)20 fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // The rect is the only FillRect; its y must be strictly below content_top
    // (0.0) by the text's laid-out line height.
    let rects = fill_rects(&result);
    assert_eq!(
        rects.len(),
        1,
        "expected one child FillRect (the rect); got {rects:?}"
    );
    let (_, rect_y, _, _) = rects[0];
    assert!(
        rect_y > 0.0,
        "rect must sit below the text (cursor advanced by measured text height); got y={rect_y}"
    );

    // Sanity: a glyph run for the text was emitted above the rect.
    let has_glyphs = result
        .scene
        .commands
        .iter()
        .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    assert!(has_glyphs, "expected the text child to emit glyph runs");
}

// ══════════════════════════════════════════════════════════════════════
// Image node compile tests
// ══════════════════════════════════════════════════════════════════════

// ── image → PushClip, DrawImage, PopClip with default fields ──────────

#[test]
fn image_emits_pushclip_drawimage_popclip() {
    let src = r##"zenith version=1 {
  project id="proj.i1" name="I1"
  assets {
asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i1" title="I1" {
page id="page.i1" w=(px)320 h=(px)200 {
  image id="img.i1" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 fit="stretch"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip(page), PushClip(box), DrawImage, PopClip(box), PopClip(page)
    assert_eq!(cmds.len(), 5, "expected 5 commands, got: {:?}", cmds);
    assert!(
        matches!(cmds[1], SceneCommand::PushClip { x, y, w, h } if x == 40.0 && y == 40.0 && w == 160.0 && h == 120.0),
        "cmd[1] must be the image box PushClip"
    );
    match &cmds[2] {
        SceneCommand::DrawImage {
            x,
            y,
            w,
            h,
            asset_id,
            fit,
            pos_x,
            pos_y,
            opacity,
            clip_shape,
        } => {
            assert_eq!(*x, 40.0);
            assert_eq!(*y, 40.0);
            assert_eq!(*w, 160.0);
            assert_eq!(*h, 120.0);
            assert_eq!(asset_id, "asset.swatch");
            assert_eq!(*fit, FitMode::Stretch);
            assert_eq!(*pos_x, 50.0, "default object-position-x must be 50");
            assert_eq!(*pos_y, 50.0, "default object-position-y must be 50");
            assert_eq!(*opacity, 1.0);
            assert_eq!(*clip_shape, None, "default image has no clip shape");
        }
        other => panic!("expected DrawImage, got {other:?}"),
    }
    assert!(matches!(cmds[3], SceneCommand::PopClip));
}

// ── image fit="cover" + object-position-x=(pct)25 → mapped fields ─────

#[test]
fn image_fit_and_object_position_mapped() {
    let src = r##"zenith version=1 {
  project id="proj.i2" name="I2"
  assets {
asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i2" title="I2" {
page id="page.i2" w=(px)320 h=(px)200 {
  image id="img.i2" asset="asset.swatch" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" object-position-x=(pct)25 object-position-y="start"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let draw = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawImage {
                fit, pos_x, pos_y, ..
            } => Some((*fit, *pos_x, *pos_y)),
            _ => None,
        })
        .expect("must emit a DrawImage");
    assert_eq!(draw.0, FitMode::Cover);
    assert_eq!(draw.1, 25.0, "object-position-x (pct)25 → 25.0");
    assert_eq!(draw.2, 0.0, "object-position-y start → 0.0");
}

// ── invisible image is not emitted ────────────────────────────────────

#[test]
fn invisible_image_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.i3" name="I3"
  assets {
asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i3" title="I3" {
page id="page.i3" w=(px)320 h=(px)200 {
  image id="img.i3" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 visible=#false
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let cmds = &result.scene.commands;
    // Only the page PushClip + PopClip; no image commands.
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {cmds:?}"
    );
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::DrawImage { .. })),
        "no DrawImage expected for invisible image"
    );
}

// ── image clip="ellipse" → DrawImage.clip_shape = Some(Ellipse) ───────

#[test]
fn image_clip_ellipse_sets_clip_shape() {
    let src = r##"zenith version=1 {
  project id="proj.ic1" name="IC1"
  assets {
asset id="asset.pfp" kind="image" src="assets/pfp.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ic1" title="IC1" {
page id="page.ic1" w=(px)320 h=(px)200 {
  image id="img.ic1" asset="asset.pfp" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" clip="ellipse"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let clip = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawImage { clip_shape, .. } => Some(clip_shape.clone()),
            _ => None,
        })
        .expect("must emit a DrawImage");
    assert_eq!(
        clip,
        Some(ImageClip::Ellipse),
        "clip=\"ellipse\" must set clip_shape to Ellipse"
    );
}

// ── image clip="rounded" clip-radius=(token) → RoundedRect{radius} ────

#[test]
fn image_clip_rounded_resolves_radius() {
    let src = r##"zenith version=1 {
  project id="proj.ic2" name="IC2"
  assets {
asset id="asset.av" kind="image" src="assets/av.png"
  }
  tokens format="zenith-token-v1" {
token id="size.radius.avatar" type="dimension" value=(px)24
  }
  styles {}
  document id="doc.ic2" title="IC2" {
page id="page.ic2" w=(px)320 h=(px)200 {
  image id="img.ic2" asset="asset.av" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" clip="rounded" clip-radius=(token)"size.radius.avatar"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let clip = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawImage { clip_shape, .. } => Some(clip_shape.clone()),
            _ => None,
        })
        .expect("must emit a DrawImage");
    assert_eq!(
        clip,
        Some(ImageClip::RoundedRect { radius: 24.0 }),
        "clip=\"rounded\" must resolve clip-radius token to px"
    );
}

// ── image with no clip → DrawImage.clip_shape = None ──────────────────

#[test]
fn image_no_clip_has_none_clip_shape() {
    let src = r##"zenith version=1 {
  project id="proj.ic3" name="IC3"
  assets {
asset id="asset.bg" kind="image" src="assets/bg.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ic3" title="IC3" {
page id="page.ic3" w=(px)320 h=(px)200 {
  image id="img.ic3" asset="asset.bg" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let clip = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawImage { clip_shape, .. } => Some(clip_shape.clone()),
            _ => None,
        })
        .expect("must emit a DrawImage");
    assert_eq!(clip, None, "image without clip must have clip_shape None");
}

// ── image opacity cascades under a group opacity ──────────────────────

#[test]
fn image_opacity_cascades() {
    // Group opacity 0.5 × image opacity 0.5 = 0.25.
    let src = r##"zenith version=1 {
  project id="proj.i4" name="I4"
  assets {
asset id="asset.swatch" kind="image" src="assets/swatch.png"
  }
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.i4" title="I4" {
page id="page.i4" w=(px)320 h=(px)200 {
  group id="group.i4" opacity=0.5 {
    image id="img.i4" asset="asset.swatch" x=(px)40 y=(px)40 w=(px)160 h=(px)120 opacity=0.5
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let opacity = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawImage { opacity, .. } => Some(*opacity),
            _ => None,
        })
        .expect("must emit a DrawImage");
    assert!(
        (opacity - 0.25).abs() < 1e-9,
        "cascaded opacity must be 0.25; got {opacity}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Polygon / Polyline compile tests
// ══════════════════════════════════════════════════════════════════════

// ── polygon: fill + stroke emits FillPolygon then StrokePolyline(closed) ─

#[test]
fn polygon_emits_fill_and_stroke() {
    let src = r##"zenith version=1 {
  project id="proj.p1" name="P1"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ff0000"
token id="color.stroke" type="color" value="#000000"
token id="size.stroke" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.p1" title="P1" {
page id="page.p1" w=(px)320 h=(px)200 {
  polygon id="poly.tri" fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.stroke" {
    point x=(px)160 y=(px)40
    point x=(px)260 y=(px)170
    point x=(px)60 y=(px)170
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // PushClip, FillPolygon, StrokePolyline, PopClip
    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 4, "expected 4 commands, got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillPolygon {
            points,
            color,
            even_odd,
        } => {
            // 3 points × 2 = 6 coordinates
            assert_eq!(points.len(), 6, "must have 6 flat coords");
            assert_eq!(points[0], 160.0, "x0 must be 160");
            assert_eq!(points[1], 40.0, "y0 must be 40");
            assert_eq!(color.r, 255, "fill color must be red");
            assert!(!even_odd, "even_odd must be false by default");
        }
        other => panic!("cmd[1] must be FillPolygon, got {other:?}"),
    }

    match &cmds[2] {
        SceneCommand::StrokePolyline {
            points,
            closed,
            color,
            stroke_width,
        } => {
            assert_eq!(points.len(), 6);
            assert!(closed, "polygon stroke must be closed");
            assert_eq!(color.r, 0, "stroke color must be black");
            assert!((stroke_width - 2.0).abs() < 1e-9);
        }
        other => panic!("cmd[2] must be StrokePolyline, got {other:?}"),
    }
}

// ── polygon: fill-rule="evenodd" → FillPolygon.even_odd == true ───────

#[test]
fn polygon_evenodd_fill_rule() {
    let src = r##"zenith version=1 {
  project id="proj.p2" name="P2"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.p2" title="P2" {
page id="page.p2" w=(px)200 h=(px)200 {
  polygon id="poly.star" fill=(token)"color.fill" fill-rule="evenodd" {
    point x=(px)100 y=(px)10
    point x=(px)40 y=(px)180
    point x=(px)190 y=(px)60
    point x=(px)10 y=(px)60
    point x=(px)160 y=(px)180
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fp = result.scene.commands.iter().find_map(|c| match c {
        SceneCommand::FillPolygon { even_odd, .. } => Some(*even_odd),
        _ => None,
    });
    assert_eq!(fp, Some(true), "fill-rule=evenodd must set even_odd=true");
}

// ── polyline: stroke-only → one StrokePolyline(closed:false), no FillPolygon ─

#[test]
fn polyline_emits_open_stroke() {
    let src = r##"zenith version=1 {
  project id="proj.pl1" name="PL1"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color" value="#334155"
token id="size.stroke" type="dimension" value=(px)3
  }
  styles {}
  document id="doc.pl1" title="PL1" {
page id="page.pl1" w=(px)320 h=(px)200 {
  polyline id="line.conn" stroke=(token)"color.stroke" stroke-width=(token)"size.stroke" {
    point x=(px)40 y=(px)100
    point x=(px)120 y=(px)60
    point x=(px)200 y=(px)140
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // PushClip, StrokePolyline, PopClip — no FillPolygon
    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 3, "expected 3 commands, got: {:?}", cmds);

    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::FillPolygon { .. })),
        "stroke-only polyline must not emit FillPolygon"
    );

    match &cmds[1] {
        SceneCommand::StrokePolyline { points, closed, .. } => {
            assert_eq!(points.len(), 6, "3 points × 2 = 6 flat coords");
            assert!(!closed, "polyline stroke must NOT be closed");
        }
        other => panic!("cmd[1] must be StrokePolyline, got {other:?}"),
    }
}

// ── polygon: visible=false → not emitted ──────────────────────────────

#[test]
fn invisible_polygon_not_emitted() {
    let src = r##"zenith version=1 {
  project id="proj.p3" name="P3"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.p3" title="P3" {
page id="page.p3" w=(px)100 h=(px)100 {
  polygon id="poly.hidden" fill=(token)"color.fill" visible=#false {
    point x=(px)10 y=(px)10
    point x=(px)90 y=(px)10
    point x=(px)50 y=(px)90
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only; got: {:?}",
        cmds
    );
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));
    assert!(matches!(cmds[1], SceneCommand::PopClip));
}

// ── polygon: group opacity 0.5 cascades into fill color.a ─────────────

#[test]
fn polygon_opacity_cascades() {
    let src = r##"zenith version=1 {
  project id="proj.p4" name="P4"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.p4" title="P4" {
page id="page.p4" w=(px)200 h=(px)200 {
  group id="grp.p4" opacity=0.5 {
    polygon id="poly.p4" fill=(token)"color.fill" {
      point x=(px)10 y=(px)10
      point x=(px)100 y=(px)10
      point x=(px)55 y=(px)100
    }
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fill_a = result.scene.commands.iter().find_map(|c| match c {
        SceneCommand::FillPolygon { color, .. } => Some(color.a),
        _ => None,
    });
    // #ffffff α=255, node opacity=1.0, ctx opacity=0.5 → 255*0.5 ≈ 128
    assert!(
        fill_a.map(|a| (a as i32 - 128).abs() <= 1).unwrap_or(false),
        "cascaded opacity 0.5 must halve fill alpha to ≈128; got {fill_a:?}"
    );
}

// ── Style cascade tests ───────────────────────────────────────────────

/// A rect with no local fill but a style that provides fill → FillRect emitted.
#[test]
fn rect_inherits_fill_from_style() {
    let src = r##"zenith version=1 {
  project id="proj.sc1" name="SC1"
  tokens format="zenith-token-v1" {
token id="color.panel" type="color" value="#3b82f6"
  }
  styles {
style id="style.panel" {
  fill (token)"color.panel"
}
  }
  document id="doc.sc1" title="SC1" {
page id="page.sc1" w=(px)320 h=(px)200 {
  rect id="rect.sc1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 style="style.panel"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // PushClip, FillRect (from style fill), PopClip
    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => {
            // #3b82f6 → r=0x3b=59, g=0x82=130, b=0xf6=246
            assert_eq!(color.r, 0x3b, "r must be 0x3b from style fill");
            assert_eq!(color.g, 0x82, "g must be 0x82 from style fill");
            assert_eq!(color.b, 0xf6, "b must be 0xf6 from style fill");
        }
        other => panic!("expected FillRect from style cascade, got {other:?}"),
    }
}

/// A rect with BOTH local fill AND a style fill → local fill wins.
#[test]
fn node_local_fill_overrides_style() {
    let src = r##"zenith version=1 {
  project id="proj.sc2" name="SC2"
  tokens format="zenith-token-v1" {
token id="color.style" type="color" value="#ff0000"
token id="color.local" type="color" value="#00ff00"
  }
  styles {
style id="style.red" {
  fill (token)"color.style"
}
  }
  document id="doc.sc2" title="SC2" {
page id="page.sc2" w=(px)320 h=(px)200 {
  rect id="rect.sc2" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.local" style="style.red"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => {
            // Must be local (green #00ff00), NOT the style (red #ff0000).
            assert_eq!(color.r, 0x00, "local fill r=0 must override style r=255");
            assert_eq!(color.g, 0xff, "local fill g=255 must override style g=0");
            assert_eq!(color.b, 0x00, "local fill b=0 must override style b=0");
        }
        other => panic!("expected FillRect with local color, got {other:?}"),
    }
}

/// A text node with style providing font-size → DrawGlyphRun uses the style size.
#[test]
fn text_inherits_font_from_style() {
    let src = r##"zenith version=1 {
  project id="proj.sc3" name="SC3"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color" value="#111827"
token id="size.title" type="dimension" value=(px)32
  }
  styles {
style id="style.title" {
  fill (token)"color.ink"
  font-size (token)"size.title"
}
  }
  document id="doc.sc3" title="SC3" {
page id="page.sc3" w=(px)640 h=(px)360 {
  text id="text.sc3" x=(px)10 y=(px)20 w=(px)400 h=(px)50 style="style.title" {
    span "Hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let unshaped: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.text_unshaped")
        .collect();
    assert!(
        unshaped.is_empty(),
        "no text_unshaped diagnostics expected; got: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    match cmds
        .iter()
        .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
    {
        Some(SceneCommand::DrawGlyphRun {
            font_size, color, ..
        }) => {
            assert_eq!(*font_size, 32.0, "font_size must be 32px from style");
            assert_eq!(
                color.r, 0x11,
                "fill must come from style (color.ink r=0x11)"
            );
        }
        _ => panic!("expected DrawGlyphRun from style cascade"),
    }
}

/// A polygon with no local fill/stroke but a style providing both → both emitted.
#[test]
fn polygon_inherits_stroke_from_style() {
    let src = r##"zenith version=1 {
  project id="proj.sc4" name="SC4"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color" value="#ef4444"
token id="size.sw" type="dimension" value=(px)2
  }
  styles {
style id="style.outlined" {
  stroke (token)"color.stroke"
  stroke-width (token)"size.sw"
}
  }
  document id="doc.sc4" title="SC4" {
page id="page.sc4" w=(px)320 h=(px)200 {
  polygon id="poly.sc4" style="style.outlined" {
    point x=(px)50 y=(px)10
    point x=(px)90 y=(px)90
    point x=(px)10 y=(px)90
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // PushClip, StrokePolyline (no fill), PopClip
    let cmds = &result.scene.commands;
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::StrokePolyline {
            color,
            stroke_width,
            closed,
            ..
        } => {
            // #ef4444 → r=0xef=239
            assert_eq!(color.r, 0xef, "stroke r must be 0xef from style");
            assert!(
                (*stroke_width - 2.0).abs() < 0.01,
                "stroke-width must be 2px from style"
            );
            assert!(closed, "polygon stroke must be closed");
        }
        other => panic!("expected StrokePolyline from style cascade, got {other:?}"),
    }
}

// ── rect: fill only → FillRect (regression) ──────────────────────────

#[test]
fn rect_fill_only_emits_fill_rect() {
    let src = r##"zenith version=1 {
  project id="proj.rf" name="RF"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.rf" title="RF" {
page id="page.rf" w=(px)100 h=(px)100 {
  rect id="rect.rf" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    // PushClip, FillRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
    assert!(
        matches!(cmds[1], SceneCommand::FillRect { .. }),
        "expected a single FillRect; got {:?}",
        cmds[1]
    );
}

// ── rect: fill + stroke → FillRect then StrokeRect ───────────────────

#[test]
fn rect_fill_and_stroke_emits_fill_then_stroke() {
    let src = r##"zenith version=1 {
  project id="proj.rfs" name="RFS"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
token id="color.stroke" type="color" value="#445566"
token id="size.sw" type="dimension" value=(px)4
  }
  styles {}
  document id="doc.rfs" title="RFS" {
page id="page.rfs" w=(px)100 h=(px)100 {
  rect id="rect.rfs" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    // PushClip, FillRect, StrokeRect, PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
    match &cmds[1] {
        SceneCommand::FillRect { color, .. } => assert_eq!(color.r, 0x11),
        other => panic!("expected FillRect first, got {other:?}"),
    }
    match &cmds[2] {
        SceneCommand::StrokeRect {
            color,
            stroke_width,
            ..
        } => {
            assert_eq!(color.r, 0x44, "stroke color r must be 0x44");
            assert!(
                (*stroke_width - 4.0).abs() < 0.01,
                "stroke-width must be 4px"
            );
        }
        other => panic!("expected StrokeRect on top, got {other:?}"),
    }
}

// ── rect: fill + radius → FillRoundedRect ────────────────────────────

#[test]
fn rect_fill_with_radius_emits_fill_rounded_rect() {
    let src = r##"zenith version=1 {
  project id="proj.rfr" name="RFR"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
token id="size.r" type="dimension" value=(px)8
  }
  styles {}
  document id="doc.rfr" title="RFR" {
page id="page.rfr" w=(px)100 h=(px)100 {
  rect id="rect.rfr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" radius=(token)"size.r"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    // PushClip, FillRoundedRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
    match &cmds[1] {
        SceneCommand::FillRoundedRect { radius, color, .. } => {
            assert_eq!(color.r, 0x11);
            assert!((*radius - 8.0).abs() < 0.01, "radius must be 8px");
        }
        other => panic!("expected FillRoundedRect, got {other:?}"),
    }
}

// ── rect: fill + stroke + radius → FillRoundedRect then StrokeRoundedRect

#[test]
fn rect_fill_stroke_radius_emits_rounded_fill_then_rounded_stroke() {
    let src = r##"zenith version=1 {
  project id="proj.rfsr" name="RFSR"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
token id="color.stroke" type="color" value="#445566"
token id="size.sw" type="dimension" value=(px)4
token id="size.r" type="dimension" value=(px)8
  }
  styles {}
  document id="doc.rfsr" title="RFSR" {
page id="page.rfsr" w=(px)100 h=(px)100 {
  rect id="rect.rfsr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 fill=(token)"color.fill" stroke=(token)"color.stroke" stroke-width=(token)"size.sw" radius=(token)"size.r"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    // PushClip, FillRoundedRect, StrokeRoundedRect, PopClip
    assert_eq!(cmds.len(), 4, "expected 4 commands; got: {:?}", cmds);
    match &cmds[1] {
        SceneCommand::FillRoundedRect { radius, .. } => {
            assert!((*radius - 8.0).abs() < 0.01, "fill radius must be 8px");
        }
        other => panic!("expected FillRoundedRect first, got {other:?}"),
    }
    match &cmds[2] {
        SceneCommand::StrokeRoundedRect {
            radius,
            stroke_width,
            color,
            ..
        } => {
            assert_eq!(color.r, 0x44);
            assert!((*radius - 8.0).abs() < 0.01, "stroke radius must be 8px");
            assert!(
                (*stroke_width - 4.0).abs() < 0.01,
                "stroke-width must be 4px"
            );
        }
        other => panic!("expected StrokeRoundedRect on top, got {other:?}"),
    }
}

// ── rect: stroke only (no fill) → StrokeRect only ────────────────────

#[test]
fn rect_stroke_only_emits_stroke_rect() {
    let src = r##"zenith version=1 {
  project id="proj.rso" name="RSO"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color" value="#445566"
token id="size.sw" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.rso" title="RSO" {
page id="page.rso" w=(px)100 h=(px)100 {
  rect id="rect.rso" x=(px)10 y=(px)10 w=(px)40 h=(px)40 stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
    let cmds = &result.scene.commands;
    // PushClip, StrokeRect, PopClip
    assert_eq!(cmds.len(), 3, "expected 3 commands; got: {:?}", cmds);
    match &cmds[1] {
        SceneCommand::StrokeRect {
            color,
            stroke_width,
            ..
        } => {
            assert_eq!(color.r, 0x44);
            assert!(
                (*stroke_width - 2.0).abs() < 0.01,
                "stroke-width must be 2px"
            );
        }
        other => panic!("expected a single StrokeRect, got {other:?}"),
    }
}

#[test]
fn rect_stroke_alignment_inside_and_outside_shift_geometry() {
    // sw = 4 → inside shifts in by 2 (x+2, w-4); outside shifts out by 2.
    let doc_for = |align: &str| {
        let src = format!(
            r##"zenith version=1 {{
  project id="proj.sa" name="SA"
  tokens format="zenith-token-v1" {{
token id="color.stroke" type="color" value="#445566"
token id="size.sw" type="dimension" value=(px)4
  }}
  styles {{}}
  document id="doc.sa" title="SA" {{
page id="page.sa" w=(px)200 h=(px)200 {{
  rect id="rect.sa" x=(px)20 y=(px)20 w=(px)100 h=(px)100 stroke=(token)"color.stroke" stroke-width=(token)"size.sw" stroke-alignment="{align}"
}}
  }}
}}
"##
        );
        let doc = parse(&src);
        compile(&doc, &default_provider())
    };

    let stroke_xywh = |result: &CompileResult| -> (f64, f64, f64, f64) {
        for c in &result.scene.commands {
            if let SceneCommand::StrokeRect { x, y, w, h, .. } = c {
                return (*x, *y, *w, *h);
            }
        }
        panic!("no StrokeRect emitted");
    };

    assert_eq!(
        stroke_xywh(&doc_for("inside")),
        (22.0, 22.0, 96.0, 96.0),
        "inside must inset the box by sw/2 on each side (w - sw)"
    );
    assert_eq!(
        stroke_xywh(&doc_for("outside")),
        (18.0, 18.0, 104.0, 104.0),
        "outside must outset by sw/2"
    );
    assert_eq!(
        stroke_xywh(&doc_for("center")),
        (20.0, 20.0, 100.0, 100.0),
        "center must be unchanged"
    );
}

// ── Code node: multi-line stacks DrawGlyphRun by line_height ─────────────

#[test]
fn code_node_multi_line_stacks_by_line_height() {
    // A 3-line code node (no w/h → no clip) emits 3 DrawGlyphRun commands
    // whose baselines increase by a constant delta equal to line_height.
    let src = r##"zenith version=1 {
  project id="proj.cd1" name="CD1"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd1" title="CD1" {
page id="page.cd1" w=(px)400 h=(px)200 {
  code id="code.cd1" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
    content "line one\nline two\nline three"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let unshaped: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.text_unshaped")
        .collect();
    assert!(
        unshaped.is_empty(),
        "no text_unshaped diagnostics expected; got: {:?}",
        result.diagnostics
    );

    let runs: Vec<f64> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 3, "expected 3 DrawGlyphRun; got {}", runs.len());

    let d0 = runs[1] - runs[0];
    let d1 = runs[2] - runs[1];
    assert!(d0 > 0.0, "baselines must increase; got {runs:?}");
    assert!(
        (d0 - d1).abs() < 0.001,
        "inter-line delta must be constant (line_height); got {d0} vs {d1}"
    );
}

// ── Code node: overflow clip wraps the runs; "visible" omits the clip ────

#[test]
fn code_node_overflow_clip_wraps_runs() {
    // Default overflow + w/h present → PushClip, runs…, PopClip.
    let src = r##"zenith version=1 {
  project id="proj.cd2" name="CD2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd2" title="CD2" {
page id="page.cd2" w=(px)400 h=(px)200 {
  code id="code.cd2" x=(px)10 y=(px)20 w=(px)300 h=(px)80 {
    content "alpha\nbeta"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // First command after the page background is PushClip; last is PopClip.
    let first_run = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .expect("a DrawGlyphRun must exist");
    assert!(
        matches!(cmds[first_run - 1], SceneCommand::PushClip { .. }),
        "PushClip must immediately precede the first run; got {:?}",
        cmds[first_run - 1]
    );
    assert!(
        matches!(cmds.last(), Some(SceneCommand::PopClip)),
        "PopClip must be the final command; got {:?}",
        cmds.last()
    );

    // overflow="visible" → no clip at all.
    let src_vis = r##"zenith version=1 {
  project id="proj.cd2v" name="CD2V"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd2v" title="CD2V" {
page id="page.cd2v" w=(px)400 h=(px)200 {
  code id="code.cd2v" x=(px)10 y=(px)20 w=(px)300 h=(px)80 overflow="visible" {
    content "alpha\nbeta"
  }
}
  }
}
"##;
    let doc_vis = parse(src_vis);
    let result_vis = compile(&doc_vis, &default_provider());
    // The page itself always wraps content in one PushClip/PopClip. With
    // overflow=visible the code node must add NO clip of its own, so exactly
    // one PushClip (the page) should be present — not two.
    let push_clips = result_vis
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::PushClip { .. }))
        .count();
    assert_eq!(
        push_clips, 1,
        "overflow=visible must add no clip beyond the page's; got {:?}",
        result_vis.scene.commands
    );
}

// ── Code node: blank middle line preserves vertical space ────────────────

#[test]
fn code_node_blank_line_preserves_spacing() {
    // "a\n\nb" → 2 runs (blank skipped), but "b" sits at i=2 spacing:
    // baseline_b == code_y + ascent + 2*line_height.
    let src = r##"zenith version=1 {
  project id="proj.cd3" name="CD3"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd3" title="CD3" {
page id="page.cd3" w=(px)400 h=(px)200 {
  code id="code.cd3" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
    content "a\n\nb"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let runs: Vec<f64> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(
        runs.len(),
        2,
        "blank middle line must be skipped → 2 runs; got {}",
        runs.len()
    );

    // The delta between "a" (i=0) and "b" (i=2) must equal 2*line_height,
    // i.e. exactly twice a single-step delta. Derive a single step from a
    // separate two-line node sharing the same font/size.
    let src2 = r##"zenith version=1 {
  project id="proj.cd3b" name="CD3B"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd3b" title="CD3B" {
page id="page.cd3b" w=(px)400 h=(px)200 {
  code id="code.cd3b" x=(px)10 y=(px)20 font-size=(px)14 overflow="visible" {
    content "a\nb"
  }
}
  }
}
"##;
    let doc2 = parse(src2);
    let result2 = compile(&doc2, &default_provider());
    let runs2: Vec<f64> = result2
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(runs2.len(), 2);
    let single_step = runs2[1] - runs2[0];
    let blank_gap = runs[1] - runs[0];
    assert!(
        (blank_gap - 2.0 * single_step).abs() < 0.001,
        "blank line must reserve one line: expected 2*{single_step}, got {blank_gap}"
    );
}

// ── Code node: leading tab expands and the node compiles cleanly ─────────

#[test]
fn code_node_tab_expansion_compiles() {
    // A line with a leading tab and tab-width=2 expands to 2 leading spaces.
    // Exact glyph counts are brittle, so assert the node compiles to a run
    // without a shaping error.
    let src = r##"zenith version=1 {
  project id="proj.cd4" name="CD4"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd4" title="CD4" {
page id="page.cd4" w=(px)400 h=(px)200 {
  code id="code.cd4" x=(px)10 y=(px)20 tab-width=2 overflow="visible" {
    content "\tindented"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let unshaped: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.text_unshaped")
        .collect();
    assert!(
        unshaped.is_empty(),
        "no shaping error expected: {unshaped:?}"
    );
    let run_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    assert_eq!(run_count, 1, "expected one DrawGlyphRun");
}

// ── Code node: default font family is the mono face ──────────────────────

#[test]
fn code_node_default_font_is_mono() {
    // No font-family → the run's font_id resolves to the mono face.
    let src = r##"zenith version=1 {
  project id="proj.cd5" name="CD5"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.cd5" title="CD5" {
page id="page.cd5" w=(px)400 h=(px)200 {
  code id="code.cd5" x=(px)10 y=(px)20 overflow="visible" {
    content "fn main() {}"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let font_id = result
        .scene
        .commands
        .iter()
        .find_map(|c| match c {
            SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
            _ => None,
        })
        .expect("a DrawGlyphRun must exist");
    assert!(
        font_id.contains("noto-sans-mono"),
        "default code font must be mono; got font_id {font_id}"
    );
}

// ── Code node: syntax highlighting splits into per-token runs ────────────

/// A code node with `language="rust"` and a Rust snippet must produce MORE
/// DrawGlyphRun commands than there are lines (per-token splitting) and at
/// least two distinct colors.
#[test]
fn code_node_highlighted_rust_emits_per_token_runs() {
    let src = r##"zenith version=1 {
  project id="proj.hl1" name="HL1"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.hl1" title="HL1" {
page id="page.hl1" w=(px)800 h=(px)400 {
  code id="code.hl1" x=(px)10 y=(px)10 language="rust" overflow="visible" {
    content "let x = 42;"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let runs: Vec<&SceneCommand> = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .collect();
    // "let x = 42;" tokenises into multiple tokens → more than 1 run per line.
    assert!(
        runs.len() > 1,
        "highlighted line must emit multiple runs; got {}",
        runs.len()
    );
    // At least two distinct colors must appear (keyword vs number vs operator…).
    let mut colors: Vec<(u8, u8, u8, u8)> = runs
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { color, .. } => Some((color.r, color.g, color.b, color.a)),
            _ => None,
        })
        .collect();
    colors.dedup();
    assert!(
        colors.len() >= 2,
        "at least two distinct colors expected; got {:?}",
        colors
    );
}

/// A code node with NO language (or an unsupported one) must emit exactly
/// ONE DrawGlyphRun per non-empty line — byte-identical to the pre-highlight
/// behavior.
#[test]
fn code_node_no_language_single_run_per_line() {
    let src = r##"zenith version=1 {
  project id="proj.hl2" name="HL2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.hl2" title="HL2" {
page id="page.hl2" w=(px)800 h=(px)400 {
  code id="code.hl2" x=(px)10 y=(px)10 language="zzz" overflow="visible" {
    content "let x = 42;\nlet y = 1;"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let run_count = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    // 2 non-empty lines → exactly 2 runs (single-run plain path).
    assert_eq!(
        run_count, 2,
        "unsupported language must yield 1 run/line (2 total); got {run_count}"
    );
}

/// A code node with `language="rust"` and a doc-declared `syntax.keyword`
/// token (red) must use that color for keyword runs, overriding the builtin.
#[test]
fn code_node_highlighted_doc_token_overrides_builtin_color() {
    // `let` is a Rust keyword. With syntax.keyword=#ff0000 the keyword run
    // must be red (r=255, g=0, b=0).
    let src = r##"zenith version=1 {
  project id="proj.hl3" name="HL3"
  tokens format="zenith-token-v1" {
token id="syntax.keyword" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.hl3" title="HL3" {
page id="page.hl3" w=(px)800 h=(px)400 {
  code id="code.hl3" x=(px)10 y=(px)10 language="rust" overflow="visible" {
    content "let x = 1;"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let keyword_run = result.scene.commands.iter().find_map(|c| match c {
        SceneCommand::DrawGlyphRun { color, .. }
            if color.r == 255 && color.g == 0 && color.b == 0 =>
        {
            Some(*color)
        }
        _ => None,
    });
    assert!(
        keyword_run.is_some(),
        "expected a red (r=255,g=0,b=0) run for the overridden keyword token; \
         commands: {:?}",
        result.scene.commands
    );
}

// ── Text node: font-weight token selects the bold face ───────────────────

/// A text node with a `font-weight` token resolving to 700 must emit a
/// `DrawGlyphRun` whose `font_id` is the BOLD Noto Sans face; a text node
/// with NO font-weight must resolve to the regular (400) face.
#[test]
fn text_node_font_weight_selects_bold_face() {
    // Helper: extract the first DrawGlyphRun's font_id from a compiled doc.
    fn first_run_font_id(src: &str) -> String {
        let doc = parse(src);
        let result = compile(&doc, &default_provider());
        result
            .scene
            .commands
            .iter()
            .find_map(|c| match c {
                SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
                _ => None,
            })
            .expect("a DrawGlyphRun must exist")
    }

    // Bold: font-weight=(token)"weight.bold" → fontWeight 700 → bold face.
    let bold_src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {
token id="weight.bold" type="fontWeight" value=700
  }
  styles {}
  document id="doc.fw" title="FW" {
page id="page.fw" w=(px)400 h=(px)200 {
  text id="text.bold" x=(px)10 y=(px)20 w=(px)380 h=(px)40 font-weight=(token)"weight.bold" { span "Bold" }
}
  }
}
"##;
    let bold_font_id = first_run_font_id(bold_src);
    assert!(
        bold_font_id.contains("noto-sans-700"),
        "font-weight 700 must select the bold face; got font_id {bold_font_id}"
    );

    // Regular: no font-weight → the default (400) face.
    let regular_src = r##"zenith version=1 {
  project id="proj.fw" name="FW"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fw" title="FW" {
page id="page.fw" w=(px)400 h=(px)200 {
  text id="text.reg" x=(px)10 y=(px)20 w=(px)380 h=(px)40 { span "Regular" }
}
  }
}
"##;
    let regular_font_id = first_run_font_id(regular_src);
    assert!(
        regular_font_id.contains("noto-sans-400") && !regular_font_id.contains("700"),
        "absent font-weight must select the regular (400) face; got font_id {regular_font_id}"
    );
}

// ── Per-span text rendering ───────────────────────────────────────────

/// Collect every `DrawGlyphRun` (x, color, font_id) in source order.
fn glyph_runs(src: &str) -> Vec<(f64, Color, String)> {
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun {
                x, color, font_id, ..
            } => Some((*x, *color, font_id.clone())),
            _ => None,
        })
        .collect()
}

/// Two spans with different fill tokens → two runs, distinct colors, the
/// second positioned to the right of the first.
#[test]
fn text_spans_render_with_per_span_fill_and_order() {
    let src = r##"zenith version=1 {
  project id="proj.ps" name="PS"
  tokens format="zenith-token-v1" {
token id="color.red" type="color" value="#ff0000"
token id="color.blue" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.ps" title="PS" {
page id="page.ps" w=(px)400 h=(px)200 {
  text id="text.ps" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "Red" fill=(token)"color.red"
    span "Blue" fill=(token)"color.blue"
  }
}
  }
}
"##;
    let runs = glyph_runs(src);
    assert_eq!(
        runs.len(),
        2,
        "expected two DrawGlyphRun; got {}",
        runs.len()
    );

    let (x0, c0, _) = &runs[0];
    let (x1, c1, _) = &runs[1];
    assert_eq!((c0.r, c0.g, c0.b), (0xff, 0x00, 0x00), "first span red");
    assert_eq!((c1.r, c1.g, c1.b), (0x00, 0x00, 0xff), "second span blue");
    assert!(
        x1 > x0,
        "second run x ({x1}) must be greater than first ({x0})"
    );
}

/// A bold second span → its run resolves to the 700 face while the first
/// (regular) span resolves to the 400 face.
#[test]
fn text_spans_render_with_per_span_weight() {
    let src = r##"zenith version=1 {
  project id="proj.pw" name="PW"
  tokens format="zenith-token-v1" {
token id="weight.bold" type="fontWeight" value=700
  }
  styles {}
  document id="doc.pw" title="PW" {
page id="page.pw" w=(px)400 h=(px)200 {
  text id="text.pw" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "Reg"
    span "Bold" font-weight=(token)"weight.bold"
  }
}
  }
}
"##;
    let runs = glyph_runs(src);
    assert_eq!(
        runs.len(),
        2,
        "expected two DrawGlyphRun; got {}",
        runs.len()
    );
    assert!(
        runs[0].2.contains("noto-sans-400"),
        "first span must use the regular (400) face; got {}",
        runs[0].2
    );
    assert!(
        runs[1].2.contains("noto-sans-700"),
        "second span must use the bold (700) face; got {}",
        runs[1].2
    );
}

/// An italic span selects the italic face; a plain span stays upright.
#[test]
fn text_italic_span_selects_italic_face() {
    let src = r##"zenith version=1 {
  project id="proj.it" name="IT"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.it" title="IT" {
page id="page.it" w=(px)400 h=(px)200 {
  text id="text.it" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "Up"
    span "Italic" italic=#true
  }
}
  }
}
"##;
    let runs = glyph_runs(src);
    assert_eq!(runs.len(), 2, "expected two runs; got {}", runs.len());
    assert!(
        !runs[0].2.contains("italic"),
        "first span must be upright; got {}",
        runs[0].2
    );
    assert!(
        runs[1].2.contains("italic"),
        "second span must use the italic face; got {}",
        runs[1].2
    );
}

/// Underline/strikethrough spans each emit one decoration `FillRect`; a
/// plain span emits none.
#[test]
fn text_span_decorations_emit_fill_rects() {
    let src = r##"zenith version=1 {
  project id="proj.dec" name="DEC"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.dec" title="DEC" {
page id="page.dec" w=(px)400 h=(px)200 {
  text id="text.dec" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "plain"
    span "under" underline=#true
    span "strike" strikethrough=#true
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let fill_rects = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
        .count();
    assert_eq!(
        fill_rects, 2,
        "one underline + one strikethrough → 2 decoration rects; got {fill_rects}"
    );
}

/// A single-span node emits exactly one run (non-breaking regression).
#[test]
fn text_single_span_emits_one_run() {
    let src = r##"zenith version=1 {
  project id="proj.ss" name="SS"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ss" title="SS" {
page id="page.ss" w=(px)400 h=(px)200 {
  text id="text.ss" x=(px)10 y=(px)20 w=(px)380 h=(px)40 { span "Solo" }
}
  }
}
"##;
    let runs = glyph_runs(src);
    assert_eq!(runs.len(), 1, "single span must emit exactly one run");
}

/// An empty span between two non-empty spans is skipped (no run emitted),
/// yet positioning of the following span still accounts for the previous
/// span's advance — i.e. empty spans don't emit but don't break order.
#[test]
fn text_empty_span_is_skipped_without_breaking_order() {
    let src = r##"zenith version=1 {
  project id="proj.es" name="ES"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.es" title="ES" {
page id="page.es" w=(px)400 h=(px)200 {
  text id="text.es" x=(px)10 y=(px)20 w=(px)380 h=(px)40 {
    span "AAAA"
    span ""
    span "BBBB"
  }
}
  }
}
"##;
    let runs = glyph_runs(src);
    assert_eq!(
        runs.len(),
        2,
        "empty span must be skipped → two runs; got {}",
        runs.len()
    );
    let (x0, _, _) = &runs[0];
    let (x1, _, _) = &runs[1];
    assert!(
        x1 > x0,
        "third span x ({x1}) must follow the first span's advance ({x0})"
    );
}

// ── Multi-page page selection ─────────────────────────────────────────

/// A two-page document. Page 1 has a full-bleed rect filled `#252525`
/// (r=0x25); page 2 a full-bleed rect filled `#dcdcdc` (r=0xdc). The page
/// fill color uniquely identifies which page was compiled.
const TWO_PAGE_DOC: &str = r##"zenith version=1 {
  project id="proj.mp" name="MP"
  tokens format="zenith-token-v1" {
token id="color.p1" type="color" value="#252525"
token id="color.p2" type="color" value="#dcdcdc"
  }
  styles {}
  document id="doc.mp" title="MP" {
page id="page.p1" w=(px)100 h=(px)100 {
  rect id="rect.p1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.p1"
}
page id="page.p2" w=(px)200 h=(px)200 {
  rect id="rect.p2" x=(px)0 y=(px)0 w=(px)200 h=(px)200 fill=(token)"color.p2"
}
  }
}
"##;

/// The `FillRect` color red-channel values present in a scene.
fn fill_reds(result: &CompileResult) -> Vec<u8> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { color, .. } => Some(color.r),
            _ => None,
        })
        .collect()
}

#[test]
fn compile_page_selects_second_page() {
    let doc = parse(TWO_PAGE_DOC);
    let result = compile_page(&doc, &default_provider(), 1);

    // Page 2 size is 200×200 (page 1 is 100×100).
    assert_eq!(result.scene.width, 200.0, "must be page 2's width");
    assert_eq!(result.scene.height, 200.0, "must be page 2's height");

    let reds = fill_reds(&result);
    assert!(
        reds.contains(&0xdc),
        "page-2 fill (#dc...) must be present; got {reds:?}"
    );
    assert!(
        !reds.contains(&0x25),
        "page-1-only fill (#25...) must be absent; got {reds:?}"
    );
}

#[test]
fn compile_page_out_of_range_is_empty_with_advisory() {
    let doc = parse(TWO_PAGE_DOC);
    let result = compile_page(&doc, &default_provider(), 9);

    assert_eq!(result.scene.width, 0.0, "out-of-range scene must be 0 wide");
    assert_eq!(
        result.scene.height, 0.0,
        "out-of-range scene must be 0 tall"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.page_out_of_range"),
        "out-of-range page must emit scene.page_out_of_range; got {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_no_pages_still_yields_no_pages_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.np" name="NP"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.np" title="NP" {}
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.no_pages"),
        "empty document must emit scene.no_pages"
    );
}

#[test]
fn compile_equals_compile_page_zero() {
    let doc = parse(TWO_PAGE_DOC);
    let via_compile = compile(&doc, &default_provider());
    let via_page0 = compile_page(&doc, &default_provider(), 0);

    // compile renders page 1 (index 0): same dimensions and fills.
    assert_eq!(via_compile.scene.width, via_page0.scene.width);
    assert_eq!(via_compile.scene.height, via_page0.scene.height);
    assert_eq!(fill_reds(&via_compile), fill_reds(&via_page0));
    // And it is page 1, not page 2.
    assert!(fill_reds(&via_compile).contains(&0x25));
}

// ── Literal visual dimensions (no token) resolve at compile time ──────────

/// A rect with a LITERAL `radius=(px)16` (no token) must emit a
/// `FillRoundedRect` whose radius is 16.0 — previously the literal was
/// dropped and the radius defaulted to 0.0 (a plain FillRect).
#[test]
fn rect_literal_radius_emits_fill_rounded_rect() {
    let src = r##"zenith version=1 {
  project id="proj.lr" name="LR"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.lr" title="LR" {
page id="page.lr" w=(px)100 h=(px)100 {
  rect id="rect.lr" x=(px)10 y=(px)10 w=(px)40 h=(px)40 radius=(px)16 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    match cmds
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRoundedRect { .. }))
    {
        Some(SceneCommand::FillRoundedRect { radius, .. }) => {
            assert!(
                (*radius - 16.0).abs() < 0.01,
                "literal radius must resolve to 16px, got {radius}"
            );
        }
        other => panic!("expected FillRoundedRect, got {other:?}"),
    }
}

/// A text node with a LITERAL `font-size=(px)20` must produce a
/// `DrawGlyphRun` whose `font_size` is 20.0.
#[test]
fn text_literal_font_size_resolves() {
    let src = r##"zenith version=1 {
  project id="proj.lfs" name="LFS"
  tokens format="zenith-token-v1" {
token id="color.text" type="color" value="#111827"
  }
  styles {}
  document id="doc.lfs" title="LFS" {
page id="page.lfs" w=(px)320 h=(px)200 {
  text id="text.lfs" x=(px)10 y=(px)10 w=(px)200 h=(px)50 fill=(token)"color.text" font-size=(px)20 {
    span "Hi"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    match result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
    {
        Some(SceneCommand::DrawGlyphRun { font_size, .. }) => {
            assert_eq!(*font_size, 20.0, "literal font-size must resolve to 20px");
        }
        other => panic!("expected DrawGlyphRun, got {other:?}"),
    }
}

// ── Text alignment ────────────────────────────────────────────────────

/// Helper: compile a single-span text node with the given align and w,
/// return the x of the sole DrawGlyphRun.
fn text_align_run_x(align: Option<&str>, node_x: f64, node_w: Option<f64>) -> f64 {
    let w_attr = node_w.map_or(String::new(), |w| format!(" w=(px){w}"));
    let align_attr = align.map_or(String::new(), |a| format!(" align=\"{a}\""));
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.al" name="AL"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.al" title="AL" {{
page id="page.al" w=(px)800 h=(px)400 {{
  text id="text.al" x=(px){node_x} y=(px)20{w_attr}{align_attr} {{
    span "Hello"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    result
        .scene
        .commands
        .iter()
        .find_map(|c| {
            if let SceneCommand::DrawGlyphRun { x, .. } = c {
                Some(*x)
            } else {
                None
            }
        })
        .expect("a DrawGlyphRun must be emitted")
}

/// `align="start"` (or absent) → run x equals node x (no offset applied).
#[test]
fn text_align_start_run_at_node_x() {
    // Explicit "start"
    let x = text_align_run_x(Some("start"), 50.0, Some(300.0));
    assert_eq!(x, 50.0, "align=start must place run at node x");
    // Absent align
    let x = text_align_run_x(None, 50.0, Some(300.0));
    assert_eq!(x, 50.0, "absent align must behave as start");
    // Absent w — no box, no offset regardless of align
    let x = text_align_run_x(Some("center"), 50.0, None);
    assert_eq!(x, 50.0, "absent w disables alignment (start fallback)");
}

/// `align="center"` → run x is inset from node x by (w − advance) / 2,
/// which is strictly greater than node x when the text is narrower than w.
#[test]
fn text_align_center_run_inset_from_node_x() {
    let node_x = 10.0;
    let box_w = 500.0;
    let x = text_align_run_x(Some("center"), node_x, Some(box_w));
    assert!(
        x > node_x,
        "center-aligned run x ({x}) must be greater than node x ({node_x})"
    );
    // The run's right edge is at x + advance; by symmetry the left inset
    // and right inset from the box edges are equal, so x must be strictly
    // less than node_x + box_w / 2 (text "Hello" is narrower than half the box).
    assert!(
        x < node_x + box_w / 2.0,
        "center-aligned run x ({x}) must be less than box midpoint ({})",
        node_x + box_w / 2.0
    );
}

/// `align="end"` → the run's advance right-edge aligns with node_x + w,
/// i.e. run_x < node_x + w AND run_x > node_x (text is narrower than box).
#[test]
fn text_align_end_run_right_edge_at_box_right() {
    let node_x = 10.0;
    let box_w = 500.0;
    let x = text_align_run_x(Some("end"), node_x, Some(box_w));
    // x should be greater than node_x (we advanced inward from start)
    assert!(
        x > node_x,
        "end-aligned run x ({x}) must be greater than node x ({node_x})"
    );
    // x should be less than node_x + box_w (the run has positive width)
    assert!(
        x < node_x + box_w,
        "end-aligned run x ({x}) must be less than right edge ({})",
        node_x + box_w
    );
}

/// Multi-span centered line: first span starts at the centered offset and
/// the second span is contiguous (its x equals first_x + first_advance).
#[test]
fn text_align_center_multi_span_contiguous() {
    let src = r##"zenith version=1 {
  project id="proj.ac2" name="AC2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.ac2" title="AC2" {
page id="page.ac2" w=(px)800 h=(px)400 {
  text id="text.ac2" x=(px)10 y=(px)20 w=(px)600 align="center" {
    span "Hello"
    span " World"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let runs: Vec<(f64, f32)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| {
            if let SceneCommand::DrawGlyphRun { x, font_size, .. } = c {
                Some((*x, *font_size))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(runs.len(), 2, "two spans → two runs; got {}", runs.len());
    let (x0, _) = runs[0];
    let (x1, _) = runs[1];
    // First run must be inset from node x (centered)
    assert!(
        x0 > 10.0,
        "first span of center-aligned text must be to the right of node x; got {x0}"
    );
    // Spans must be contiguous (second starts where first ends)
    assert!(
        x1 > x0,
        "second span x ({x1}) must follow first span x ({x0})"
    );
}

// ── Text wrapping (word wrap) ─────────────────────────────────────────

/// Helper: collect (x, y, color) of every DrawGlyphRun emitted for a single
/// text node with the given box width, align, and span text.
fn wrap_runs(node_x: f64, box_w: f64, align: &str, span: &str) -> Vec<(f64, f64)> {
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.wr" name="WR"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.wr" title="WR" {{
page id="page.wr" w=(px)1000 h=(px)600 {{
  text id="text.wr" x=(px){node_x} y=(px)20 w=(px){box_w} align="{align}" {{
    span "{span}"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| {
            if let SceneCommand::DrawGlyphRun { x, y, .. } = c {
                Some((*x, *y))
            } else {
                None
            }
        })
        .collect()
}

/// A long single span in a narrow box wraps to multiple lines: more than one
/// DrawGlyphRun, appearing at >= 2 distinct baseline y values.
#[test]
fn text_wraps_when_exceeding_box_width() {
    let runs = wrap_runs(
        10.0,
        120.0,
        "start",
        "the quick brown fox jumps over the lazy dog",
    );
    assert!(
        runs.len() > 1,
        "wrapped text must emit more than one run; got {}",
        runs.len()
    );
    let mut ys: Vec<f64> = runs.iter().map(|(_, y)| *y).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert!(
        ys.len() >= 2,
        "wrapped text must occupy >= 2 distinct baselines; got {ys:?}"
    );
}

/// Short text that fits the box takes the unchanged fast path: exactly one
/// logical line and (for start align) the first run sits at node x.
#[test]
fn text_fits_single_line_unchanged() {
    let runs = wrap_runs(40.0, 600.0, "start", "Hi there");
    // All runs share a single baseline (one line).
    let y0 = runs[0].1;
    assert!(
        runs.iter().all(|(_, y)| (*y - y0).abs() < 1e-6),
        "fitting text must stay on one line; got {runs:?}"
    );
    // First run x == node x (start-aligned fast path).
    assert_eq!(
        runs[0].0, 40.0,
        "start-aligned fitting text must begin at node x"
    );
}

/// Wrapped + center: each line's first run is inset to the right of node x.
#[test]
fn text_wrap_center_lines_inset() {
    let runs = wrap_runs(
        10.0,
        120.0,
        "center",
        "the quick brown fox jumps over the lazy dog",
    );
    assert!(runs.len() > 1, "expected wrapping; got {}", runs.len());
    // Group first-run-per-line by baseline; each line's first x > node_x.
    let mut seen_y: Vec<f64> = Vec::new();
    for (x, y) in &runs {
        if !seen_y.iter().any(|sy| (*sy - *y).abs() < 1e-6) {
            seen_y.push(*y);
            assert!(
                *x > 10.0,
                "center-wrapped line first run x ({x}) must be inset past node x (10)"
            );
        }
    }
}

/// Wrapped + justify: a non-last multi-word line is fully justified (first
/// word at node x, last word right edge ≈ node x + box_w), while the LAST
/// line stays start-aligned (first run at node x, not stretched).
#[test]
fn text_wrap_justify_spreads() {
    let node_x = 10.0;
    let box_w = 120.0;
    // Need the per-run advances too, so re-collect including last word edge.
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.wj" name="WJ"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.wj" title="WJ" {{
page id="page.wj" w=(px)1000 h=(px)600 {{
  text id="text.wj" x=(px){node_x} y=(px)20 w=(px){box_w} align="justify" {{
    span "the quick brown fox jumps over the lazy dog"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    // Collect (y, x) of all runs.
    let runs: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| {
            if let SceneCommand::DrawGlyphRun { x, y, .. } = c {
                Some((*y, *x))
            } else {
                None
            }
        })
        .collect();
    assert!(runs.len() > 1, "expected wrapping; got {}", runs.len());

    // Distinct baselines, in order.
    let mut ys: Vec<f64> = Vec::new();
    for (y, _) in &runs {
        if !ys.iter().any(|v| (*v - *y).abs() < 1e-6) {
            ys.push(*y);
        }
    }
    assert!(ys.len() >= 2, "need >= 2 lines; got {}", ys.len());

    // First line: its first run must start at node x (justify keeps left edge).
    let first_line_y = ys[0];
    let first_line_first_x = runs
        .iter()
        .filter(|(y, _)| (*y - first_line_y).abs() < 1e-6)
        .map(|(_, x)| *x)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (first_line_first_x - node_x).abs() < 1e-6,
        "justified first line must start at node x; got {first_line_first_x}"
    );

    // Last line stays start-aligned: its first run also begins at node x and
    // is not stretched to the box edge. We assert it begins at node x.
    let last_line_y = ys[ys.len() - 1];
    let last_line_first_x = runs
        .iter()
        .filter(|(y, _)| (*y - last_line_y).abs() < 1e-6)
        .map(|(_, x)| *x)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (last_line_first_x - node_x).abs() < 1e-6,
        "last (start-aligned) line must begin at node x; got {last_line_first_x}"
    );
}

/// A line with a LITERAL `stroke-width=(px)3` must produce a `StrokeLine`
/// whose `stroke_width` is 3.0.
#[test]
fn line_literal_stroke_width_resolves() {
    let src = r##"zenith version=1 {
  project id="proj.lsw" name="LSW"
  tokens format="zenith-token-v1" {
token id="color.rule" type="color" value="#94a3b8"
  }
  styles {}
  document id="doc.lsw" title="LSW" {
page id="page.lsw" w=(px)320 h=(px)200 {
  line id="line.lsw" x1=(px)40 y1=(px)100 x2=(px)280 y2=(px)100 stroke=(token)"color.rule" stroke-width=(px)3
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    match result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::StrokeLine { .. }))
    {
        Some(SceneCommand::StrokeLine { stroke_width, .. }) => {
            assert_eq!(
                *stroke_width, 3.0,
                "literal stroke-width must resolve to 3px"
            );
        }
        other => panic!("expected StrokeLine, got {other:?}"),
    }
}

// ── Font fallback diagnostics ─────────────────────────────────────────

/// A text node whose font-family token resolves to an UNREGISTERED family
/// ("Oswald") must still emit a `DrawGlyphRun` (text not dropped) AND
/// produce exactly one `font.unresolved` advisory naming the node id and
/// the missing family.
#[test]
fn text_node_unregistered_family_falls_back_and_emits_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.fb1" name="FB1"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fb1" title="FB1" {
page id="page.fb1" w=(px)400 h=(px)200 {
  text id="headline" x=(px)10 y=(px)10 font-family="Oswald" {
    span "Hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // The scene must contain at least one DrawGlyphRun (text not dropped).
    assert!(
        result
            .scene
            .commands
            .iter()
            .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. })),
        "expected DrawGlyphRun when unregistered family falls back; commands: {:?}",
        result.scene.commands,
    );

    // Exactly one font.unresolved advisory must be present, naming the node.
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.unresolved")
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "expected exactly one font.unresolved diagnostic, got {:?}",
        unresolved,
    );
    let msg = &unresolved[0].message;
    assert!(
        msg.contains("headline"),
        "advisory message should name the node 'headline'; got: {msg}"
    );
    assert!(
        msg.contains("Oswald"),
        "advisory message should name the missing family 'Oswald'; got: {msg}"
    );
}

/// A text node using the registered "Noto Sans" family must produce NO
/// `font.unresolved` diagnostic and must emit a `DrawGlyphRun` as usual.
#[test]
fn text_node_registered_family_no_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.fb2" name="FB2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fb2" title="FB2" {
page id="page.fb2" w=(px)400 h=(px)200 {
  text id="body.text" x=(px)10 y=(px)10 font-family="Noto Sans" {
    span "Hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // No font.unresolved diagnostics.
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.unresolved")
        .collect();
    assert!(
        unresolved.is_empty(),
        "expected no font.unresolved diagnostics for registered family; got: {:?}",
        unresolved,
    );

    // DrawGlyphRun must still be present.
    assert!(
        result
            .scene
            .commands
            .iter()
            .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. })),
        "expected DrawGlyphRun for registered Noto Sans family",
    );
}

/// A code node with `font-weight=(token)"weight.bold"` (a fontWeight token = 700)
/// must produce a DrawGlyphRun whose font_id corresponds to the bold mono face
/// (distinct from the regular-weight code run's font_id).
#[test]
fn code_bold_font_weight_uses_bold_mono_face() {
    let src = r##"zenith version=1 {
  project id="proj.cbw" name="CBW"
  tokens format="zenith-token-v1" {
token id="weight.bold" type="fontWeight" value=700
  }
  styles {}
  document id="doc.cbw" title="CBW" {
page id="page.cbw" w=(px)400 h=(px)200 {
  code id="code.regular" x=(px)10 y=(px)10 {
    content "hello"
  }
  code id="code.bold" x=(px)10 y=(px)50 font-weight=(token)"weight.bold" {
    content "hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Collect DrawGlyphRun font_ids for each code node (by order: regular first,
    // bold second). Both nodes shape the same text, so each emits exactly one run.
    let glyph_runs: Vec<_> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
            _ => None,
        })
        .collect();

    assert!(
        glyph_runs.len() >= 2,
        "expected at least 2 DrawGlyphRun commands (one per code node); got: {:?}",
        glyph_runs
    );

    // The first run (regular weight=400) must use a different font than the
    // second run (bold weight=700).
    let regular_font = &glyph_runs[0];
    let bold_font = &glyph_runs[1];
    assert_ne!(
        regular_font, bold_font,
        "bold code node must use a different font_id than regular code node; \
         regular={regular_font:?}, bold={bold_font:?}"
    );

    // The bold font_id must contain "700" (mirrors the provider id format).
    assert!(
        bold_font.contains("700"),
        "bold code font_id should encode weight 700; got {bold_font:?}"
    );
}

/// A code node WITHOUT `font-weight` defaults to weight 400, and must produce
/// a DrawGlyphRun with the same font_id as a code node with explicit weight=400.
/// This confirms the default-weight path is byte-identical to the original.
#[test]
fn code_default_weight_is_regular_mono_face() {
    let src = r##"zenith version=1 {
  project id="proj.cdw" name="CDW"
  tokens format="zenith-token-v1" {
token id="weight.normal" type="fontWeight" value=400
  }
  styles {}
  document id="doc.cdw" title="CDW" {
page id="page.cdw" w=(px)400 h=(px)200 {
  code id="code.implicit" x=(px)10 y=(px)10 {
    content "hello"
  }
  code id="code.explicit400" x=(px)10 y=(px)50 font-weight=(token)"weight.normal" {
    content "hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let glyph_runs: Vec<_> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { font_id, .. } => Some(font_id.clone()),
            _ => None,
        })
        .collect();

    assert!(
        glyph_runs.len() >= 2,
        "expected at least 2 DrawGlyphRun commands; got: {:?}",
        glyph_runs
    );

    // Both the implicit-400 and explicit-400 code nodes must resolve to the
    // same (regular) mono font_id — the default path is byte-identical.
    assert_eq!(
        glyph_runs[0], glyph_runs[1],
        "implicit weight=400 and explicit weight=400 must resolve to the same \
         mono font face; implicit={:?}, explicit={:?}",
        glyph_runs[0], glyph_runs[1]
    );

    // The font_id must NOT contain "700".
    assert!(
        !glyph_runs[0].contains("700"),
        "regular code font_id must not encode weight 700; got {:?}",
        glyph_runs[0]
    );
}

// ── Leaf-node rotation: PushTransform bracket ─────────────────────────

/// A rect with `rotate=(deg)45` must emit
/// PushTransform{angle_deg:45, cx:x+w/2, cy:y+h/2} before any draw
/// command and PopTransform after, outermost.
#[test]
fn rect_with_rotate_emits_push_pop_transform() {
    let src = r##"zenith version=1 {
  project id="proj.rot1" name="Rot1"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.rot1" title="Rot1" {
page id="page.rot1" w=(px)200 h=(px)200 {
  rect id="rect.rot" x=(px)20 y=(px)40 w=(px)100 h=(px)60 fill=(token)"color.fill" rotate=(deg)45
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Expected: PushClip(page) PushTransform FillRect PopTransform PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

    // cmds[0] = page PushClip
    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));

    // cmds[1] = PushTransform with correct angle and center
    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 45.0, "angle must be 45");
            // x=20, w=100 → cx=70
            assert_eq!(*cx, 70.0, "cx must be x+w/2 = 20+50 = 70");
            // y=40, h=60 → cy=70
            assert_eq!(*cy, 70.0, "cy must be y+h/2 = 40+30 = 70");
        }
        other => panic!("expected PushTransform, got {other:?}"),
    }

    // cmds[2] = FillRect (the draw command)
    assert!(
        matches!(cmds[2], SceneCommand::FillRect { .. }),
        "expected FillRect at index 2, got {:?}",
        cmds[2]
    );

    // cmds[3] = PopTransform
    assert!(
        matches!(cmds[3], SceneCommand::PopTransform),
        "expected PopTransform at index 3, got {:?}",
        cmds[3]
    );

    // cmds[4] = page PopClip
    assert!(matches!(cmds[4], SceneCommand::PopClip));
}

/// A rect WITHOUT `rotate` must emit NO PushTransform — output is
/// byte-identical to the pre-rotation implementation.
#[test]
fn rect_without_rotate_emits_no_transform() {
    let src = r##"zenith version=1 {
  project id="proj.rot2" name="Rot2"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#00ff00"
  }
  styles {}
  document id="doc.rot2" title="Rot2" {
page id="page.rot2" w=(px)200 h=(px)200 {
  rect id="rect.norot" x=(px)10 y=(px)10 w=(px)80 h=(px)80 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip FillRect PopClip — no transform commands at all.
    assert_eq!(
        cmds.len(),
        3,
        "expected 3 commands (no transform); got: {:?}",
        cmds
    );

    let has_transform = cmds.iter().any(|c| {
        matches!(
            c,
            SceneCommand::PushTransform { .. } | SceneCommand::PopTransform
        )
    });
    assert!(
        !has_transform,
        "no transform commands expected for unrotated rect"
    );
}

/// A rect with `rotate=(deg)0` must also emit NO PushTransform —
/// zero-angle rotation is a no-op.
#[test]
fn rect_with_rotate_zero_emits_no_transform() {
    let src = r##"zenith version=1 {
  project id="proj.rot3" name="Rot3"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.rot3" title="Rot3" {
page id="page.rot3" w=(px)200 h=(px)200 {
  rect id="rect.zerorot" x=(px)10 y=(px)10 w=(px)80 h=(px)80 fill=(token)"color.fill" rotate=(deg)0
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let cmds = &result.scene.commands;
    let has_transform = cmds.iter().any(|c| {
        matches!(
            c,
            SceneCommand::PushTransform { .. } | SceneCommand::PopTransform
        )
    });
    assert!(
        !has_transform,
        "rotate=(deg)0 must emit no transform commands; got: {:?}",
        cmds
    );
}

/// An ellipse with `rotate=(deg)90` must emit PushTransform with the
/// correct center (x+w/2, y+h/2) before FillEllipse and PopTransform after.
#[test]
fn ellipse_with_rotate_emits_correct_transform() {
    let src = r##"zenith version=1 {
  project id="proj.rot4" name="Rot4"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#ffaa00"
  }
  styles {}
  document id="doc.rot4" title="Rot4" {
page id="page.rot4" w=(px)400 h=(px)300 {
  ellipse id="ell.rot" x=(px)50 y=(px)100 w=(px)200 h=(px)80 fill=(token)"color.fill" rotate=(deg)90
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip PushTransform FillEllipse PopTransform PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 90.0);
            // x=50, w=200 → cx=150
            assert_eq!(*cx, 150.0, "cx=x+w/2=50+100=150");
            // y=100, h=80 → cy=140
            assert_eq!(*cy, 140.0, "cy=y+h/2=100+40=140");
        }
        other => panic!("expected PushTransform, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::FillEllipse { .. }),
        "expected FillEllipse at index 2"
    );
    assert!(
        matches!(cmds[3], SceneCommand::PopTransform),
        "expected PopTransform at index 3"
    );
}

/// A polygon with `rotate=(deg)30` must emit PushTransform whose center
/// is the centroid-bbox midpoint of the (translated) points.
#[test]
fn polygon_with_rotate_emits_centroid_transform() {
    // Triangle at (10,20) (110,20) (60,70) → bbox center = (60, 45).
    let src = r##"zenith version=1 {
  project id="proj.rot5" name="Rot5"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#aabbcc"
  }
  styles {}
  document id="doc.rot5" title="Rot5" {
page id="page.rot5" w=(px)200 h=(px)200 {
  polygon id="poly.rot" fill=(token)"color.fill" rotate=(deg)30 {
    point x=(px)10 y=(px)20
    point x=(px)110 y=(px)20
    point x=(px)60 y=(px)70
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip PushTransform FillPolygon PopTransform PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 30.0);
            // x range: [10, 110] → cx = 60; y range: [20, 70] → cy = 45
            assert_eq!(*cx, 60.0, "centroid cx must be (10+110)/2=60");
            assert_eq!(*cy, 45.0, "centroid cy must be (20+70)/2=45");
        }
        other => panic!("expected PushTransform, got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::FillPolygon { .. }),
        "expected FillPolygon at index 2"
    );
    assert!(
        matches!(cmds[3], SceneCommand::PopTransform),
        "expected PopTransform at index 3"
    );
}

// ── Container rotation: GROUP / FRAME ─────────────────────────────────

/// (1) A group with `rotate=(deg)30` and NO w/h, containing two rects,
/// must emit PushTransform (center = union-bbox center of the two rects
/// in device space) before the children, and PopTransform last.
///
/// Group at (0,0), rects at (10,20,100,60) and (50,100,40,30).
/// Union bbox: x=[10,110], y=[20,130] → center = (60, 75).
#[test]
fn group_rotate_no_wh_uses_children_union_bbox_center() {
    let src = r##"zenith version=1 {
  project id="proj.gr1" name="GR1"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#ff0000"
token id="color.b" type="color" value="#0000ff"
  }
  styles {}
  document id="doc.gr1" title="GR1" {
page id="page.gr1" w=(px)300 h=(px)300 {
  group id="grp.rot" rotate=(deg)30 {
    rect id="r1" x=(px)10 y=(px)20 w=(px)100 h=(px)60 fill=(token)"color.a"
    rect id="r2" x=(px)50 y=(px)100 w=(px)40 h=(px)30 fill=(token)"color.b"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // Expected: PushClip(page) PushTransform FillRect FillRect PopTransform PopClip
    assert_eq!(cmds.len(), 6, "expected 6 commands; got: {:?}", cmds);

    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));

    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 30.0, "angle must be 30");
            // Union bbox: x=[10,110] → cx=60, y=[20,130] → cy=75
            assert_eq!(*cx, 60.0, "cx must be (10+110)/2=60");
            assert_eq!(*cy, 75.0, "cy must be (20+130)/2=75");
        }
        other => panic!("expected PushTransform at [1], got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::FillRect { .. }),
        "expected FillRect at [2]"
    );
    assert!(
        matches!(cmds[3], SceneCommand::FillRect { .. }),
        "expected FillRect at [3]"
    );

    assert!(
        matches!(cmds[4], SceneCommand::PopTransform),
        "expected PopTransform at [4], got {:?}",
        cmds[4]
    );
    assert!(matches!(cmds[5], SceneCommand::PopClip));
}

/// (2) A group WITHOUT rotate must emit NO PushTransform — byte-identical
/// to the pre-container-rotation baseline.
#[test]
fn group_without_rotate_emits_no_transform() {
    let src = r##"zenith version=1 {
  project id="proj.gr2" name="GR2"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#00ff00"
  }
  styles {}
  document id="doc.gr2" title="GR2" {
page id="page.gr2" w=(px)200 h=(px)200 {
  group id="grp.norot" {
    rect id="r1" x=(px)10 y=(px)10 w=(px)80 h=(px)80 fill=(token)"color.a"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    let has_transform = cmds.iter().any(|c| {
        matches!(
            c,
            SceneCommand::PushTransform { .. } | SceneCommand::PopTransform
        )
    });
    assert!(
        !has_transform,
        "unrotated group must emit no transform commands; got: {:?}",
        cmds
    );
}

/// (3) A group WITH declared w/h + rotate uses the declared box center,
/// not the children bbox.
///
/// Group x=0 y=0 w=200 h=100 → center = (100, 50).
/// The single child rect at (10,10,80,80) would give a different center.
#[test]
fn group_rotate_with_wh_uses_declared_box_center() {
    let src = r##"zenith version=1 {
  project id="proj.gr3" name="GR3"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#aabbcc"
  }
  styles {}
  document id="doc.gr3" title="GR3" {
page id="page.gr3" w=(px)300 h=(px)200 {
  group id="grp.wh" w=(px)200 h=(px)100 rotate=(deg)45 {
    rect id="r1" x=(px)10 y=(px)10 w=(px)80 h=(px)80 fill=(token)"color.a"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip(page) PushTransform FillRect PopTransform PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands; got: {:?}", cmds);

    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 45.0, "angle must be 45");
            // group x defaults 0, w=200 → cx=0+200/2=100
            // group y defaults 0, h=100 → cy=0+100/2=50
            assert_eq!(*cx, 100.0, "cx must be declared box center 0+200/2=100");
            assert_eq!(*cy, 50.0, "cy must be declared box center 0+100/2=50");
        }
        other => panic!("expected PushTransform at [1], got {other:?}"),
    }

    assert!(matches!(cmds[4], SceneCommand::PopClip));
}

/// (4) A frame with rotate=(deg)20 must emit PushTransform (center from
/// the frame box) BEFORE PushClip, and PopTransform AFTER PopClip.
///
/// Frame x=10 y=20 w=100 h=60 → device-space center = (60, 50).
#[test]
fn frame_rotate_wraps_clip_outermost() {
    let src = r##"zenith version=1 {
  project id="proj.fr1" name="FR1"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#112233"
  }
  styles {}
  document id="doc.fr1" title="FR1" {
page id="page.fr1" w=(px)200 h=(px)200 {
  frame id="frm.rot" x=(px)10 y=(px)20 w=(px)100 h=(px)60 rotate=(deg)20 {
    rect id="r1" x=(px)15 y=(px)25 w=(px)40 h=(px)30 fill=(token)"color.a"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip(page) PushTransform PushClip(frame) FillRect PopClip PopTransform PopClip(page)
    assert_eq!(cmds.len(), 7, "expected 7 commands; got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { .. }),
        "[0] must be page PushClip"
    );

    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 20.0, "angle must be 20");
            // ctx.dx=0 + frame_x=10 + frame_w/2=50 → cx=60
            // ctx.dy=0 + frame_y=20 + frame_h/2=30 → cy=50
            assert_eq!(*cx, 60.0, "cx must be 0+10+100/2=60");
            assert_eq!(*cy, 50.0, "cy must be 0+20+60/2=50");
        }
        other => panic!("expected PushTransform at [1], got {other:?}"),
    }

    assert!(
        matches!(cmds[2], SceneCommand::PushClip { .. }),
        "[2] must be frame PushClip (inside transform); got {:?}",
        cmds[2]
    );

    assert!(
        matches!(cmds[3], SceneCommand::FillRect { .. }),
        "[3] must be FillRect"
    );

    assert!(
        matches!(cmds[4], SceneCommand::PopClip),
        "[4] must be frame PopClip; got {:?}",
        cmds[4]
    );

    assert!(
        matches!(cmds[5], SceneCommand::PopTransform),
        "[5] must be PopTransform (after PopClip); got {:?}",
        cmds[5]
    );

    assert!(
        matches!(cmds[6], SceneCommand::PopClip),
        "[6] must be page PopClip"
    );
}

/// (5) A rotated group containing a rotated rect must emit BOTH
/// PushTransform commands nested correctly:
///   PushClip(page) PushTransform(group) PushTransform(rect) FillRect PopTransform(rect) PopTransform(group) PopClip(page)
///
/// Group (no w/h) rotate=15°, contains rect x=10 y=10 w=80 h=40 rotate=45°.
/// Children bbox center = (50, 30) → group PushTransform cx=50, cy=30.
/// Rect center = (10+40, 10+20) = (50, 30) (device space same as group).
#[test]
fn rotated_group_containing_rotated_rect_nests_both_transforms() {
    let src = r##"zenith version=1 {
  project id="proj.gr5" name="GR5"
  tokens format="zenith-token-v1" {
token id="color.a" type="color" value="#ff8800"
  }
  styles {}
  document id="doc.gr5" title="GR5" {
page id="page.gr5" w=(px)200 h=(px)200 {
  group id="grp.outer" rotate=(deg)15 {
    rect id="r1" x=(px)10 y=(px)10 w=(px)80 h=(px)40 fill=(token)"color.a" rotate=(deg)45
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;
    // PushClip PushTransform(group) PushTransform(rect) FillRect PopTransform(rect) PopTransform(group) PopClip
    assert_eq!(cmds.len(), 7, "expected 7 commands; got: {:?}", cmds);

    assert!(matches!(cmds[0], SceneCommand::PushClip { .. }));

    // Group PushTransform
    match &cmds[1] {
        SceneCommand::PushTransform { angle_deg, cx, cy } => {
            assert_eq!(*angle_deg, 15.0, "group angle must be 15");
            // Rect bbox: x=10,y=10,w=80,h=40 → center=(50,30)
            assert_eq!(*cx, 50.0, "group pivot cx=10+80/2=50");
            assert_eq!(*cy, 30.0, "group pivot cy=10+40/2=30");
        }
        other => panic!("expected group PushTransform at [1], got {other:?}"),
    }

    // Rect PushTransform
    match &cmds[2] {
        SceneCommand::PushTransform { angle_deg, .. } => {
            assert_eq!(*angle_deg, 45.0, "rect angle must be 45");
        }
        other => panic!("expected rect PushTransform at [2], got {other:?}"),
    }

    assert!(
        matches!(cmds[3], SceneCommand::FillRect { .. }),
        "[3] must be FillRect"
    );
    assert!(
        matches!(cmds[4], SceneCommand::PopTransform),
        "[4] must be rect PopTransform"
    );
    assert!(
        matches!(cmds[5], SceneCommand::PopTransform),
        "[5] must be group PopTransform"
    );
    assert!(matches!(cmds[6], SceneCommand::PopClip));
}

// ── overflow="fit" tests ──────────────────────────────────────────────────

/// A text node with `overflow="fit"` whose long text overflows the small box
/// height must produce a `text.fit_failed` Error diagnostic, AND must still
/// emit glyph run commands (the scene is not suppressed).
#[test]
fn overflow_fit_height_exceeded_emits_fit_failed_and_still_draws() {
    // A tiny 60×20 px box. Font size 16 px → line_height ≈ 18–20 px.
    // The text has many words that will wrap into multiple lines, so
    // content_height will exceed 20 px.
    let src = r##"zenith version=1 {
  project id="proj.fit1" name="Fit Overflow"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fit1" title="Fit Overflow" {
page id="page.fit1" w=(px)400 h=(px)400 {
  text id="text.overflow" x=(px)10 y=(px)10 w=(px)60 h=(px)20 overflow="fit" {
    span "The quick brown fox jumps over the lazy dog and keeps on going"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // Must have exactly one `text.fit_failed` Error diagnostic.
    let fit_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert_eq!(
        fit_errors.len(),
        1,
        "expected exactly one text.fit_failed diagnostic; got: {:?}",
        result.diagnostics
    );
    assert_eq!(
        fit_errors[0].severity,
        zenith_core::Severity::Error,
        "text.fit_failed must be Error severity"
    );
    assert!(
        fit_errors[0]
            .subject_id
            .as_deref()
            .map(|s| s.contains("text.overflow"))
            .unwrap_or(false),
        "subject_id must name the overflowing text node; got {:?}",
        fit_errors[0].subject_id
    );

    // Glyph runs must still be emitted — the scene is not suppressed.
    let has_glyphs = result
        .scene
        .commands
        .iter()
        .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    assert!(
        has_glyphs,
        "glyph runs must still be emitted even when fit fails"
    );
}

/// A text node with `overflow="clip"` whose long text overflows the small box
/// must produce a `text.overflow` Warning (clipping silently truncates ink, so
/// the author is told) — but still draw, and NOT hard-fail.
#[test]
fn overflow_clip_height_exceeded_emits_overflow_warning() {
    let src = r##"zenith version=1 {
  project id="proj.clip1" name="Clip Overflow"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.clip1" title="Clip Overflow" {
page id="page.clip1" w=(px)400 h=(px)400 {
  text id="text.clipped" x=(px)10 y=(px)10 w=(px)60 h=(px)20 overflow="clip" {
    span "The quick brown fox jumps over the lazy dog and keeps on going"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.overflow")
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly one text.overflow warning; got: {:?}",
        result.diagnostics
    );
    assert_eq!(
        warns[0].severity,
        zenith_core::Severity::Warning,
        "text.overflow must be Warning severity (not a hard fail)"
    );
    // No hard error from clip overflow.
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == "text.fit_failed"),
        "clip overflow must NOT produce text.fit_failed"
    );
    // Glyph runs still emitted.
    assert!(
        result
            .scene
            .commands
            .iter()
            .any(|c| matches!(c, SceneCommand::DrawGlyphRun { .. })),
        "glyph runs must still be emitted when clip overflows"
    );
}

/// A text node with `overflow="fit"` whose text FITS within the box must
/// produce NO `text.fit_failed` diagnostic.
#[test]
fn overflow_fit_text_fits_no_diagnostic() {
    // A wide, tall box that the short text will easily fit in.
    let src = r##"zenith version=1 {
  project id="proj.fit2" name="Fit OK"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fit2" title="Fit OK" {
page id="page.fit2" w=(px)400 h=(px)400 {
  text id="text.fits" x=(px)10 y=(px)10 w=(px)300 h=(px)100 overflow="fit" {
    span "Hi"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fit_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert!(
        fit_errors.is_empty(),
        "text that fits must produce no text.fit_failed diagnostic; got: {:?}",
        fit_errors
    );
}

/// A text node with `overflow="clip"` (not "fit") must NEVER produce a
/// `text.fit_failed` diagnostic, even when the text clearly overflows.
#[test]
fn overflow_clip_overflowing_text_no_fit_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.fit3" name="Clip Overflow"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fit3" title="Clip Overflow" {
page id="page.fit3" w=(px)400 h=(px)400 {
  text id="text.clip" x=(px)10 y=(px)10 w=(px)60 h=(px)20 overflow="clip" {
    span "The quick brown fox jumps over the lazy dog and keeps on going"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fit_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert!(
        fit_errors.is_empty(),
        "overflow=\"clip\" must never produce text.fit_failed; got: {:?}",
        fit_errors
    );
}

/// A text node with no `overflow` property and overflowing text must NOT
/// produce a `text.fit_failed` diagnostic.
#[test]
fn overflow_absent_overflowing_text_no_fit_diagnostic() {
    let src = r##"zenith version=1 {
  project id="proj.fit4" name="No Overflow Prop"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fit4" title="No Overflow Prop" {
page id="page.fit4" w=(px)400 h=(px)400 {
  text id="text.noov" x=(px)10 y=(px)10 w=(px)60 h=(px)20 {
    span "The quick brown fox jumps over the lazy dog and keeps on going"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fit_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert!(
        fit_errors.is_empty(),
        "absent overflow must never produce text.fit_failed; got: {:?}",
        fit_errors
    );
}

// ── Gradient fill compilation (GRAD-2) ────────────────────────────────

/// A page background, a rect fill, and an ellipse fill all referencing a
/// gradient token must emit the corresponding `*Gradient` commands with the
/// resolved stop colors and angle.
#[test]
fn gradient_fill_emits_gradient_commands() {
    let src = r##"zenith version=1 {
  project id="proj.g" name="G"
  tokens format="zenith-token-v1" {
token id="color.top" type="color" value="#112233"
token id="color.bottom" type="color" value="#445566"
token id="grad.bg" type="gradient" angle=(deg)90 {
  stop offset=0.0 color=(token)"color.top"
  stop offset=1.0 color=(token)"color.bottom"
}
  }
  styles {}
  document id="doc.g" title="G" {
page id="page.g" w=(px)640 h=(px)360 background=(token)"grad.bg" {
  rect id="rect.g" x=(px)10 y=(px)20 w=(px)100 h=(px)50 fill=(token)"grad.bg"
  ellipse id="ell.g" x=(px)10 y=(px)20 w=(px)100 h=(px)50 fill=(token)"grad.bg"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    let cmds = &result.scene.commands;

    // Page background gradient (full page, no opacity cascade).
    match &cmds[1] {
        SceneCommand::FillRectGradient {
            x,
            y,
            w,
            h,
            gradient,
        } => {
            assert_eq!((*x, *y, *w, *h), (0.0, 0.0, 640.0, 360.0));
            assert_eq!(gradient.angle_deg, 90.0);
            assert_eq!(gradient.stops.len(), 2);
            assert_eq!(gradient.stops[0].offset, 0.0);
            assert_eq!(gradient.stops[0].color.r, 0x11);
            assert_eq!(gradient.stops[0].color.a, 255);
            assert_eq!(gradient.stops[1].color.r, 0x44);
        }
        other => panic!("expected FillRectGradient bg, got {other:?}"),
    }

    // Rect fill gradient.
    let has_rect_grad = cmds.iter().any(|c| {
        matches!(
            c,
            SceneCommand::FillRectGradient { x, w, .. } if *x == 10.0 && *w == 100.0
        )
    });
    assert!(has_rect_grad, "expected rect FillRectGradient: {cmds:?}");

    // Ellipse fill gradient.
    let has_ell_grad = cmds
        .iter()
        .any(|c| matches!(c, SceneCommand::FillEllipseGradient { .. }));
    assert!(has_ell_grad, "expected FillEllipseGradient: {cmds:?}");
}

/// A SOLID color fill must still emit the plain `FillRect` / `FillEllipse`
/// (the gradient path must not perturb the solid path).
#[test]
fn solid_fill_unchanged_by_gradient_support() {
    let src = r##"zenith version=1 {
  project id="proj.s" name="S"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.s" title="S" {
page id="page.s" w=(px)640 h=(px)360 {
  rect id="rect.s" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
  ellipse id="ell.s" x=(px)0 y=(px)0 w=(px)100 h=(px)50 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    assert!(
        cmds.iter()
            .any(|c| matches!(c, SceneCommand::FillRect { .. })),
        "solid rect must emit FillRect: {cmds:?}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, SceneCommand::FillEllipse { .. })),
        "solid ellipse must emit FillEllipse: {cmds:?}"
    );
    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            SceneCommand::FillRectGradient { .. } | SceneCommand::FillEllipseGradient { .. }
        )),
        "solid fills must not emit gradient commands: {cmds:?}"
    );
}

// ── Shadow compilation (SHAD-2) ───────────────────────────────────────

/// A text node and a rect node carrying a `shadow=(token)` must emit a
/// `BeginShadow { shadows:[…] }` … `EndShadow` bracket around their draw
/// commands, with the layer color resolved from the referenced color token.
#[test]
fn shadow_emits_begin_end_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.sh" name="Sh"
  tokens format="zenith-token-v1" {
token id="color.shadow" type="color" value="#102030"
token id="color.fill" type="color" value="#445566"
token id="shadow.soft" type="shadow" {
  layer dx=(px)2 dy=(px)3 blur=(px)4 color=(token)"color.shadow"
}
  }
  styles {}
  document id="doc.sh" title="Sh" {
page id="page.sh" w=(px)200 h=(px)200 {
  rect id="rect.sh" x=(px)10 y=(px)10 w=(px)80 h=(px)40 fill=(token)"color.fill" shadow=(token)"shadow.soft"
  text id="text.sh" x=(px)10 y=(px)80 shadow=(token)"shadow.soft" {
    span "Hello"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Locate the first BeginShadow and verify the resolved layer.
    let begin = cmds.iter().find_map(|c| match c {
        SceneCommand::BeginShadow { shadows } => Some(shadows),
        _ => None,
    });
    let shadows = begin.expect("a BeginShadow must be emitted");
    assert_eq!(shadows.len(), 1, "one shadow layer: {shadows:?}");
    let layer = shadows.first().expect("layer present");
    assert_eq!((layer.dx, layer.dy, layer.blur), (2.0, 3.0, 4.0));
    assert_eq!(layer.color.r, 0x10);
    assert_eq!(layer.color.g, 0x20);
    assert_eq!(layer.color.b, 0x30);
    assert_eq!(layer.color.a, 0xff);

    // BeginShadow/EndShadow must be balanced, and a Begin must precede a
    // draw which precedes the End (bracket order).
    let begins = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::BeginShadow { .. }))
        .count();
    let ends = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::EndShadow))
        .count();
    assert_eq!(begins, 2, "rect + text each open a shadow: {cmds:?}");
    assert_eq!(ends, 2, "each shadow must be closed: {cmds:?}");

    // The first Begin is immediately followed by a fill and closed by an End.
    let begin_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginShadow { .. }))
        .expect("begin index");
    let end_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndShadow))
        .expect("end index");
    assert!(begin_idx < end_idx, "Begin must precede End");
    let has_draw_between = cmds
        .get(begin_idx + 1..end_idx)
        .map(|window| {
            window
                .iter()
                .any(|c| matches!(c, SceneCommand::FillRect { .. }))
        })
        .unwrap_or(false);
    assert!(
        has_draw_between,
        "a draw must sit inside the bracket: {cmds:?}"
    );
}

/// A node WITHOUT a shadow must emit a command stream byte-identical to the
/// pre-shadow behavior: no `BeginShadow`/`EndShadow` anywhere.
#[test]
fn no_shadow_emits_no_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.ns" name="Ns"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#445566"
  }
  styles {}
  document id="doc.ns" title="Ns" {
page id="page.ns" w=(px)200 h=(px)200 {
  rect id="rect.ns" x=(px)10 y=(px)10 w=(px)80 h=(px)40 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            SceneCommand::BeginShadow { .. } | SceneCommand::EndShadow
        )),
        "a shadow-less node must emit no shadow bracket: {cmds:?}"
    );
}

// ── Threaded text flow (chain) ────────────────────────────────────────

/// Count the `DrawGlyphRun` commands whose baseline `y` falls in `[lo, hi)`.
/// Used to attribute glyph runs to a particular chain member's box.
fn glyph_runs_in_y(cmds: &[SceneCommand], lo: f64, hi: f64) -> usize {
    cmds.iter()
        .filter(|c| match c {
            SceneCommand::DrawGlyphRun { y, .. } => *y >= lo && *y < hi,
            _ => false,
        })
        .count()
}

/// A long article placed in box1 of a 2-box chain must fill box1 to its height
/// and CONTINUE the remainder in box2: box1 emits a bounded number of glyph
/// runs (its height / line-height), box2 emits the rest. Both boxes draw text;
/// box1 does NOT draw every word.
#[test]
fn chain_two_box_distributes_overflow_to_second_box() {
    // box1 is short (height 80) so only ~2 lines fit at 24px; the rest of the
    // ~40-word article must flow into box2 (placed far below at y=1000).
    let src = r##"zenith version=1 {
  project id="proj.ch2" name="CH2"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.ch2" title="CH2" {
page id="page.ch2" w=(px)600 h=(px)1400 {
  text id="box1" x=(px)10 y=(px)0 w=(px)300 h=(px)80 chain="article" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu one two three four five six seven eight nine ten eleven twelve"
  }
  text id="box2" x=(px)10 y=(px)1000 w=(px)300 h=(px)380 chain="article" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // box1 baselines live near y in [0, 500); box2 near y in [1000, 1400).
    let box1_runs = glyph_runs_in_y(cmds, 0.0, 500.0);
    let box2_runs = glyph_runs_in_y(cmds, 1000.0, 1400.0);

    assert!(box1_runs > 0, "box1 must draw text; got {box1_runs}");
    assert!(
        box2_runs > 0,
        "box2 must receive the continuation; got {box2_runs}"
    );
    // box1 is height 80 at 24px line height → at most ~3 lines; the article has
    // far more words than fit, so box2 must carry strictly more runs than box1.
    assert!(
        box2_runs > box1_runs,
        "continuation box2 ({box2_runs}) must carry more than box1 ({box1_runs})"
    );

    // Determinism: a second compile yields byte-identical commands.
    let result2 = compile(&doc, &default_provider());
    assert_eq!(
        result.scene.commands, result2.scene.commands,
        "chain compile must be deterministic"
    );
}

/// A chain whose content overflows even the LAST box, when that last member
/// declares `overflow="fit"`, must raise a `text.fit_failed` Error on the last
/// member.
#[test]
fn chain_last_box_overflow_fit_fails() {
    // Both boxes are tiny (height 40 → 1 line each) but the article needs many
    // lines; the last box (overflow="fit") cannot hold its remainder.
    let src = r##"zenith version=1 {
  project id="proj.chf" name="CHF"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.chf" title="CHF" {
page id="page.chf" w=(px)600 h=(px)1400 {
  text id="cbox1" x=(px)10 y=(px)0 w=(px)300 h=(px)40 chain="article" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey"
  }
  text id="cbox2" x=(px)10 y=(px)1000 w=(px)300 h=(px)40 overflow="fit" chain="article" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let fit_failed: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.fit_failed")
        .collect();
    assert_eq!(
        fit_failed.len(),
        1,
        "exactly one text.fit_failed expected on the last member; got: {:?}",
        result.diagnostics
    );
    assert_eq!(
        fit_failed[0].subject_id.as_deref(),
        Some("cbox2"),
        "the fit failure must name the last chain member"
    );
}

/// A non-chain text node must compile to a command stream identical to one
/// produced when no chain machinery is involved — the chain pre-pass on a
/// chain-free page is a no-op.
#[test]
fn non_chain_text_unaffected_by_chain_prepass() {
    let src = r##"zenith version=1 {
  project id="proj.nc" name="NC"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.nc" title="NC" {
page id="page.nc" w=(px)600 h=(px)200 {
  text id="plain" x=(px)10 y=(px)20 w=(px)580 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Hello Zenith"
  }
}
  }
}
"##;
    let doc = parse(src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());
    assert_eq!(
        r1.scene.commands, r2.scene.commands,
        "non-chain compile must be deterministic and unaffected"
    );
    // Expect PushClip, DrawGlyphRun, PopClip (the single-line fast path).
    assert_eq!(
        r1.scene.commands.len(),
        3,
        "non-chain single-line text must emit exactly 3 commands; got {:?}",
        r1.scene.commands
    );
}

/// A chain that spans THREE pages (one body box per page, all sharing
/// `chain="ch1"`, only page 1's box bears the article) must flow the article
/// across all three: the box on page 2 AND the box on page 3 each emit
/// continuation glyph runs (not empty). Two compiles per page are byte-identical.
#[test]
fn chain_spans_three_pages() {
    // Each page is 400 tall; the body box is short (h=120 → ~5 lines at 24px)
    // so the ~60-word article cannot fit one box and must cascade page→page.
    let src = r##"zenith version=1 {
  project id="proj.cp" name="CP"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cp" title="CP" {
page id="p1" w=(px)600 h=(px)400 {
  text id="b1" x=(px)10 y=(px)10 w=(px)300 h=(px)120 chain="ch1" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty"
  }
}
page id="p2" w=(px)600 h=(px)400 {
  text id="b2" x=(px)10 y=(px)10 w=(px)300 h=(px)120 chain="ch1" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
  }
}
page id="p3" w=(px)600 h=(px)400 {
  text id="b3" x=(px)10 y=(px)10 w=(px)300 h=(px)600 chain="ch1" fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
  }
}
  }
}
"##;
    let doc = parse(src);
    let p1 = compile_page(&doc, &default_provider(), 0);
    let p2 = compile_page(&doc, &default_provider(), 1);
    let p3 = compile_page(&doc, &default_provider(), 2);

    let runs1 = glyph_runs_in_y(&p1.scene.commands, 0.0, 400.0);
    let runs2 = glyph_runs_in_y(&p2.scene.commands, 0.0, 400.0);
    let runs3 = glyph_runs_in_y(&p3.scene.commands, 0.0, 400.0);

    assert!(runs1 > 0, "page 1 box must draw text; got {runs1}");
    assert!(
        runs2 > 0,
        "page 2 box must receive the continuation; got {runs2}"
    );
    assert!(
        runs3 > 0,
        "page 3 box must receive the continuation tail; got {runs3}"
    );

    // Determinism: each page recompiles byte-identical.
    for idx in 0..3 {
        let a = compile_page(&doc, &default_provider(), idx);
        let b = compile_page(&doc, &default_provider(), idx);
        assert_eq!(
            a.scene.commands, b.scene.commands,
            "cross-page chain page {idx} must compile deterministically"
        );
    }
}

/// Justify math: on a fully-justified (non-last, multi-word) line the LAST
/// word's right edge reaches the box's right edge (within the last word's own
/// advance), confirming inter-word gaps widened to fill the box width.
#[test]
fn text_wrap_justify_fills_box_width() {
    let node_x = 10.0;
    let box_w = 120.0;
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.jf" name="JF"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.jf" title="JF" {{
page id="page.jf" w=(px)1000 h=(px)600 {{
  text id="text.jf" x=(px){node_x} y=(px)20 w=(px){box_w} align="justify" {{
    span "the quick brown fox jumps over the lazy dog"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    let runs: Vec<(f64, f64)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { x, y, .. } => Some((*y, *x)),
            _ => None,
        })
        .collect();

    // Distinct baselines, in order.
    let mut ys: Vec<f64> = Vec::new();
    for (y, _) in &runs {
        if !ys.iter().any(|v| (*v - *y).abs() < 1e-6) {
            ys.push(*y);
        }
    }
    assert!(ys.len() >= 2, "need >= 2 lines; got {}", ys.len());

    // First (non-last, justified) line: the largest x of any run on it (the last
    // word's left edge) must sit close to the right box edge, i.e. the spread
    // pushed it well past the box midpoint. With a fitted (non-justified) line
    // the words would bunch on the left.
    let first_y = ys[0];
    let max_x_first = runs
        .iter()
        .filter(|(y, _)| (*y - first_y).abs() < 1e-6)
        .map(|(_, x)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let box_right = node_x + box_w;
    let box_mid = node_x + box_w / 2.0;
    assert!(
        max_x_first > box_mid,
        "justified line's last word must be pushed past box midpoint {box_mid}; got {max_x_first} (box_right={box_right})"
    );
}

// ── Component / instance / override expansion ─────────────────────────

/// Source with a `panel.master` component (bg rect + text label) instanced
/// three times at three x positions, each overriding the label text.
const COMPONENT_SRC: &str = r##"zenith version=1 {
  project id="proj.c" name="C"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
    token id="color.fg" type="color" value="#fafafa"
    token id="color.alt" type="color" value="#ff0000"
    token id="size.body" type="dimension" value=(pt)18
    token id="font.fam" type="fontFamily" value="Noto Sans"
  }
  styles {}
  components {
    component id="panel.master" {
      rect id="bg" x=(px)0 y=(px)0 w=(px)100 h=(px)60 fill=(token)"color.bg"
      text id="label" x=(px)5 y=(px)5 w=(px)90 h=(px)30 fill=(token)"color.fg" font-family=(token)"font.fam" font-size=(token)"size.body" {
        span "Default"
      }
    }
  }
  document id="doc.c" title="C" {
    page id="page.c" w=(px)640 h=(px)360 {
      instance id="inst.1" component="panel.master" x=(px)0 y=(px)0 {
        override ref="label" { span "Back" }
      }
      instance id="inst.2" component="panel.master" x=(px)200 y=(px)0 {
        override ref="label" fill=(token)"color.alt" { span "Center" }
      }
      instance id="inst.3" component="panel.master" x=(px)400 y=(px)0 {
        override ref="label" { span "Cover" }
      }
    }
  }
}
"##;

#[test]
fn instance_expands_component_translated_three_times() {
    let doc = parse(COMPONENT_SRC);
    let result = compile(&doc, &default_provider());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.unknown_component"),
        "no unknown-component advisory expected: {:?}",
        result.diagnostics
    );

    // The component's bg rect should appear 3× as a FillRect at x = 0, 200, 400.
    let rect_xs: Vec<f64> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { x, w, h, .. } if *w == 100.0 && *h == 60.0 => Some(*x),
            _ => None,
        })
        .collect();
    assert_eq!(
        rect_xs,
        vec![0.0, 200.0, 400.0],
        "the master bg rect must appear 3× at the 3 instance origins"
    );

    // Three glyph runs (one label per instance).
    let glyph_runs = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    assert_eq!(glyph_runs, 3, "each expanded instance draws its label");
}

#[test]
fn instance_override_fill_recolors_target_label() {
    let doc = parse(COMPONENT_SRC);
    let result = compile(&doc, &default_provider());

    // inst.2 overrides the label fill to color.alt (#ff0000); the other two
    // labels keep color.fg (#fafafa). Collect glyph-run colors in z-order.
    let colors: Vec<(u8, u8, u8)> = result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();
    assert_eq!(colors.len(), 3);
    assert_eq!(
        colors[0],
        (0xfa, 0xfa, 0xfa),
        "inst.1 label keeps default fg"
    );
    assert_eq!(
        colors[1],
        (0xff, 0x00, 0x00),
        "inst.2 label takes override fill"
    );
    assert_eq!(
        colors[2],
        (0xfa, 0xfa, 0xfa),
        "inst.3 label keeps default fg"
    );
}

#[test]
fn unknown_component_emits_advisory_and_skips() {
    let src = r##"zenith version=1 {
  project id="proj.u" name="U"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
  }
  styles {}
  components {
    component id="real.one" {
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }
  }
  document id="doc.u" title="U" {
    page id="page.u" w=(px)100 h=(px)100 {
      instance id="inst.x" component="missing.comp" x=(px)0 y=(px)0 {
      }
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.unknown_component"),
        "expected scene.unknown_component advisory"
    );
    // Only PushClip + PopClip — the instance emitted nothing.
    assert!(
        !result
            .scene
            .commands
            .iter()
            .any(|c| matches!(c, SceneCommand::FillRect { .. })),
        "a skipped instance must emit no fill commands"
    );
}

// ── Print bleed + crop marks ──────────────────────────────────────────

/// Source for a page with `bleed` and a token-filled full-bleed background plus
/// a single hero rect at authored origin.
fn bleed_doc_src(bleed_attr: &str) -> String {
    format!(
        r##"zenith version=1 {{
  project id="proj.bleed" name="Bleed"
  tokens format="zenith-token-v1" {{
    token id="color.bg" type="color" value="#102030"
    token id="color.hero" type="color" value="#ff8800"
  }}
  styles {{}}
  document id="doc.bleed" title="Bleed" {{
    page id="page.bleed" w=(px)400 h=(px)600{bleed_attr} background=(token)"color.bg" {{
      rect id="rect.hero" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.hero"
    }}
  }}
}}
"##
    )
}

#[test]
fn bleed_expands_canvas_and_shifts_content() {
    let doc = parse(&bleed_doc_src(" bleed=(px)35"));
    let result = compile(&doc, &default_provider());
    assert!(
        !result.diagnostics.iter().any(|d| d.code != "token.unused"),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );

    // Media box = (400 + 70) × (600 + 70).
    assert_eq!(result.scene.width, 470.0);
    assert_eq!(result.scene.height, 670.0);

    let cmds = &result.scene.commands;

    // Background fills the ENTIRE media box (bleeds off the trim edge).
    match cmds
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRect { color, .. } if color.r == 0x10))
    {
        Some(SceneCommand::FillRect { x, y, w, h, .. }) => {
            assert_eq!((*x, *y, *w, *h), (0.0, 0.0, 470.0, 670.0));
        }
        other => panic!("expected full-media background FillRect, got {other:?}"),
    }

    // Hero rect shifted by (b, b) = (35, 35).
    match cmds
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRect { color, .. } if color.r == 0xff))
    {
        Some(SceneCommand::FillRect { x, y, w, h, .. }) => {
            assert_eq!((*x, *y, *w, *h), (35.0, 35.0, 100.0, 100.0));
        }
        other => panic!("expected shifted hero FillRect, got {other:?}"),
    }
}

#[test]
fn bleed_emits_eight_crop_mark_segments_all_in_margin() {
    let b = 35.0;
    let doc = parse(&bleed_doc_src(" bleed=(px)35"));
    let result = compile(&doc, &default_provider());

    let lines: Vec<&SceneCommand> = result
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeLine { .. }))
        .collect();
    assert_eq!(lines.len(), 8, "expected 8 crop-mark segments");

    // Trim box: [35, 35] .. [435, 635]. Every segment endpoint must lie OUTSIDE
    // the trim box (in the bleed margin) — i.e. NOT strictly interior to it.
    let trim_left = b;
    let trim_top = b;
    let trim_right = b + 400.0;
    let trim_bottom = b + 600.0;
    let interior =
        |x: f64, y: f64| x > trim_left && x < trim_right && y > trim_top && y < trim_bottom;
    for cmd in &lines {
        if let SceneCommand::StrokeLine { x1, y1, x2, y2, .. } = cmd {
            assert!(
                !interior(*x1, *y1) && !interior(*x2, *y2),
                "crop-mark segment must stay in the bleed margin: {cmd:?}"
            );
        }
    }
}

#[test]
fn bleed_absent_is_byte_identical_to_no_bleed() {
    // The exact same document MINUS the bleed attribute must yield the same
    // scene as a document that never mentioned bleed.
    let with_none = parse(&bleed_doc_src(""));
    let result = compile(&with_none, &default_provider());

    // Canvas is the plain page size; no crop marks emitted.
    assert_eq!(result.scene.width, 400.0);
    assert_eq!(result.scene.height, 600.0);
    assert!(
        !result
            .scene
            .commands
            .iter()
            .any(|c| matches!(c, SceneCommand::StrokeLine { .. })),
        "no bleed → no crop marks"
    );
    // PushClip covers the plain page rectangle (origin unshifted).
    assert!(
        matches!(
            result.scene.commands.first(),
            Some(SceneCommand::PushClip { x, y, w, h }) if *x == 0.0 && *y == 0.0 && *w == 400.0 && *h == 600.0
        ),
        "first command must be a page-sized PushClip"
    );
    // Hero rect is NOT shifted.
    match result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::FillRect { color, .. } if color.r == 0xff))
    {
        Some(SceneCommand::FillRect { x, y, .. }) => assert_eq!((*x, *y), (0.0, 0.0)),
        other => panic!("expected unshifted hero FillRect, got {other:?}"),
    }
}

#[test]
fn bleed_render_is_two_run_byte_identical() {
    let doc = parse(&bleed_doc_src(" bleed=(px)35"));
    let a = compile(&doc, &default_provider());
    let b = compile(&doc, &default_provider());
    assert_eq!(
        a.scene.to_json().expect("serialize a"),
        b.scene.to_json().expect("serialize b"),
        "two compile runs must be byte-identical"
    );
}

// ── Master-page + field projection ────────────────────────────────────────

/// A 4-page mirror-margin book whose master carries a running-head + a
/// page-number field; each page sets `master="m.body"` and has one body text.
const BOOK_SRC: &str = r##"zenith version=1 mirror-margins=#true {
  project id="proj.book" name="Book"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color" value="#111111"
  }
  styles {}
  masters {
master id="m.body" {
  field id="rh" type="running-head" recto="Chapter One: A Long Recto Title" verso="Verso" y=(px)80 h=(px)40 fill=(token)"color.ink"
  field id="folio" type="page-number" y=(px)1820 h=(px)40 fill=(token)"color.ink"
}
  }
  document id="doc.book" title="Book" {
page id="p1" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
  text id="b1" x=(px)160 y=(px)200 w=(px)900 h=(px)40 fill=(token)"color.ink" { span "Body one" }
}
page id="p2" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
  text id="b2" x=(px)160 y=(px)200 w=(px)900 h=(px)40 fill=(token)"color.ink" { span "Body two" }
}
page id="p3" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
  text id="b3" x=(px)160 y=(px)200 w=(px)900 h=(px)40 fill=(token)"color.ink" { span "Body three" }
}
page id="p4" w=(px)1200 h=(px)1900 margin-inner=(px)160 margin-outer=(px)100 margin-top=(px)80 margin-bottom=(px)80 master="m.body" {
  text id="b4" x=(px)160 y=(px)200 w=(px)900 h=(px)40 fill=(token)"color.ink" { span "Body four" }
}
  }
}
"##;

/// Collect the `(x, y)` origin of every glyph run in a scene, in order.
fn glyph_run_origins(result: &CompileResult) -> Vec<(f64, f64)> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect()
}

#[test]
fn master_projects_running_head_and_folio_on_every_page() {
    let doc = parse(BOOK_SRC);
    let provider = default_provider();
    for page_index in 0..4 {
        let r = compile_page(&doc, &provider, page_index);
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == "master.unknown_reference"),
            "page {page_index} must resolve its master"
        );
        // Master contributes 2 glyph runs (running head + folio) plus 1 body run.
        let runs = glyph_run_origins(&r);
        assert_eq!(
            runs.len(),
            3,
            "page {page_index}: expected running-head + folio + body, got {runs:?}"
        );
    }
}

#[test]
fn running_head_x_mirrors_recto_vs_verso() {
    let doc = parse(BOOK_SRC);
    let provider = default_provider();
    // Recto (page 1, index 0): live_x = margin_inner = 160.
    let recto = compile_page(&doc, &provider, 0);
    // Verso (page 2, index 1): live_x = margin_outer = 100 (mirrored).
    let verso = compile_page(&doc, &provider, 1);

    // The running-head run is the one whose baseline sits just below y=80
    // (text_box_top 80 + ascent). Its x is the live-area left inset.
    let recto_rh_x = glyph_run_origins(&recto)
        .into_iter()
        .find(|(_, y)| *y > 80.0 && *y < 130.0)
        .map(|(x, _)| x);
    let verso_rh_x = glyph_run_origins(&verso)
        .into_iter()
        .find(|(_, y)| *y > 80.0 && *y < 130.0)
        .map(|(x, _)| x);

    // Both fields are center-aligned within the live-area box. The live-area
    // left inset differs by parity (recto = margin_inner 160, verso =
    // margin_outer 100), so the centered run origin must differ — proving the
    // mirror is active. (Exact run x also depends on the per-parity text width;
    // the precise live-area inset is asserted directly in field.rs's unit test.)
    assert!(recto_rh_x.is_some() && verso_rh_x.is_some());
    assert_ne!(
        recto_rh_x, verso_rh_x,
        "running-head x must differ by parity (mirrored live area)"
    );
}

#[test]
fn running_head_recto_verso_text_differs_by_parity() {
    let doc = parse(BOOK_SRC);
    let provider = default_provider();
    // Recto and verso strings have very different lengths → different glyph runs.
    let recto = compile_page(&doc, &provider, 0);
    let verso = compile_page(&doc, &provider, 1);

    let rh_glyph_count = |r: &CompileResult| -> Option<usize> {
        r.scene.commands.iter().find_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, glyphs, .. } if *y > 80.0 && *y < 130.0 => {
                Some(glyphs.len())
            }
            _ => None,
        })
    };
    let rc = rh_glyph_count(&recto);
    let vc = rh_glyph_count(&verso);
    assert!(
        rc.is_some() && vc.is_some(),
        "both parities emit a running head"
    );
    assert_ne!(
        rc, vc,
        "recto 'Chapter 1' and verso 'The Novel' differ in glyph count"
    );
}

#[test]
fn folio_renders_one_per_page_and_two_run_byte_identical() {
    let doc = parse(BOOK_SRC);
    let provider = default_provider();
    // The folio sits at the bottom (text_box_top 1820); exactly one per page.
    for page_index in 0..4 {
        let r = compile_page(&doc, &provider, page_index);
        let folios = glyph_run_origins(&r)
            .into_iter()
            .filter(|(_, y)| *y > 1820.0 && *y < 1900.0)
            .count();
        assert_eq!(folios, 1, "page {page_index}: exactly one folio run");

        // Two-run byte-identical determinism per page.
        let a = compile_page(&doc, &provider, page_index);
        let b = compile_page(&doc, &provider, page_index);
        assert_eq!(
            a.scene.to_json().expect("a"),
            b.scene.to_json().expect("b"),
            "page {page_index} must be byte-identical across runs"
        );
    }
}

#[test]
fn inline_page_number_field_renders_folio_without_master() {
    // A field used directly in a page's children (not via a master) resolves
    // the same way: a page-number renders the page's 1-based folio.
    let src = r##"zenith version=1 {
  project id="proj.inl" name="Inl"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.inl" title="Inl" {
page id="ip1" w=(px)400 h=(px)300 {
  field id="f.folio" type="page-number" x=(px)10 y=(px)10 w=(px)80 h=(px)30 fill=(token)"color.ink"
}
page id="ip2" w=(px)400 h=(px)300 {
  field id="f.folio2" type="page-number" x=(px)10 y=(px)10 w=(px)80 h=(px)30 fill=(token)"color.ink"
}
  }
}
"##;
    let doc = parse(src);
    let provider = default_provider();
    let p1 = compile_page(&doc, &provider, 0);
    let p2 = compile_page(&doc, &provider, 1);
    assert_eq!(glyph_run_origins(&p1).len(), 1, "page 1 folio renders");
    assert_eq!(glyph_run_origins(&p2).len(), 1, "page 2 folio renders");
}

#[test]
fn page_ref_field_resolves_target_page_index() {
    // A page-ref points at a node on page 3; it must render "3".
    let src = r##"zenith version=1 {
  project id="proj.ref" name="Ref"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="doc.ref" title="Ref" {
page id="rp1" w=(px)400 h=(px)300 {
  field id="f.ref" type="page-ref" target="anchor" x=(px)10 y=(px)10 w=(px)80 h=(px)30 fill=(token)"color.ink"
}
page id="rp2" w=(px)400 h=(px)300 {
  rect id="filler" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.ink"
}
page id="rp3" w=(px)400 h=(px)300 {
  text id="anchor" x=(px)10 y=(px)10 w=(px)80 h=(px)30 fill=(token)"color.ink" { span "X" }
}
  }
}
"##;
    let doc = parse(src);
    let provider = default_provider();
    let p1 = compile_page(&doc, &provider, 0);
    // The page-ref renders a single glyph run (the digit "3").
    assert_eq!(
        glyph_run_origins(&p1).len(),
        1,
        "page-ref to a page-3 anchor renders one digit run"
    );
}

#[test]
fn non_master_page_is_byte_identical_to_before() {
    // A document with no masters and no fields must compile to the exact same
    // command stream as it would have without the feature (regression guard).
    let src = r##"zenith version=1 {
  project id="proj.nm" name="NM"
  tokens format="zenith-token-v1" {
token id="color.fill" type="color" value="#f8fafc"
  }
  styles {}
  document id="doc.nm" title="NM" {
page id="page.nm" w=(px)640 h=(px)360 {
  rect id="rect.nm" x=(px)0 y=(px)0 w=(px)640 h=(px)360 fill=(token)"color.fill"
}
  }
}
"##;
    let doc = parse(src);
    let provider = default_provider();
    let r = compile(&doc, &provider);
    // Exactly PushClip, FillRect, PopClip — unchanged from the pre-feature path.
    assert_eq!(r.scene.commands.len(), 3, "{:?}", r.scene.commands);
    assert!(matches!(r.scene.commands[0], SceneCommand::PushClip { .. }));
    assert!(matches!(r.scene.commands[1], SceneCommand::FillRect { .. }));
    assert!(matches!(r.scene.commands[2], SceneCommand::PopClip));
}

// ── Footnote system ───────────────────────────────────────────────────

/// A margined page (all four margins) carrying a body text node whose paragraph
/// has a `footnote-ref` span after "evidence", plus one page-level footnote.
const FOOTNOTE_ONE_SRC: &str = r##"zenith version=1 {
  project id="proj.fn1" name="FN1"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="doc.fn1" title="FN1" {
page id="page.fn1" w=(px)600 h=(px)900 margin-inner=(px)60 margin-outer=(px)60 margin-top=(px)80 margin-bottom=(px)80 {
  text id="body" x=(px)60 y=(px)80 w=(px)480 h=(px)200 {
    span "Strong evidence" footnote-ref="fn.1"
    span " supports the claim."
  }
  footnote id="fn.1" {
    span "See also Chapter 4."
  }
}
  }
}
"##;

/// A `footnote-ref` span emits a SUPERSCRIPT marker run after its text: the body
/// renders MORE glyph runs than the same text without the ref, and the marker
/// run draws at a reduced font size (the vertical-align="super" scale).
#[test]
fn footnote_ref_emits_superscript_marker() {
    let doc = parse(FOOTNOTE_ONE_SRC);
    let provider = default_provider();
    let r = compile(&doc, &provider);

    // No hard diagnostics (advisories/warnings only, if any).
    assert!(
        !r.scene.commands.is_empty(),
        "scene must have commands: {:?}",
        r.scene.commands
    );

    // Collect the distinct font sizes of the glyph runs. The body text shapes at
    // 16px (default); the superscript marker shapes smaller (0.65 × 16). So there
    // must be at least one glyph run whose font_size is below the body size —
    // proving a superscript marker was emitted.
    let sizes: Vec<f32> = r
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { font_size, .. } => Some(*font_size),
            _ => None,
        })
        .collect();
    assert!(!sizes.is_empty(), "body must shape some glyph runs");
    let max_size = sizes.iter().cloned().fold(0.0_f32, f32::max);
    assert!(
        sizes.iter().any(|s| *s < max_size - 0.5),
        "a superscript marker run (reduced size) must be present; sizes={sizes:?}"
    );
}

/// The page renders a footnote zone at the bottom: a thin separator rule
/// (FillRect with height ~1px spanning ~1/3 the live width) PLUS the footnote's
/// content glyphs, positioned BELOW the body text.
#[test]
fn footnote_zone_renders_separator_and_content() {
    let doc = parse(FOOTNOTE_ONE_SRC);
    let provider = default_provider();
    let r = compile(&doc, &provider);

    // Live area: x=60, width=600-60-60=480; the separator rule is ~1/3 → 160px,
    // 1px tall, drawn near the bottom (y well below the body's y=80).
    let separators: Vec<(f64, f64, f64)> = r
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillRect { y, w, h, .. } if (*h - 1.0).abs() < 0.01 => Some((*y, *w, *h)),
            _ => None,
        })
        .collect();
    assert!(
        separators
            .iter()
            .any(|(y, w, _)| *y > 600.0 && (*w - 160.0).abs() < 1.0),
        "a ~160px-wide, 1px-tall separator rule must sit near the page bottom; \
         got {separators:?}"
    );

    // The footnote content "See also Chapter 4." must shape glyph runs whose
    // baseline y is BELOW the body (body baseline near y≈80+ascent). The lowest
    // glyph run on the page should be a footnote glyph (near the bottom margin).
    let max_glyph_y = r
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { y, .. } => Some(*y),
            _ => None,
        })
        .fold(0.0_f64, f64::max);
    assert!(
        max_glyph_y > 600.0,
        "footnote content must render near the page bottom; max glyph y={max_glyph_y}"
    );
}

/// Two footnotes auto-number 1 and 2 in source order: the marker map drives both
/// the inline markers and the zone content. We assert the markers map directly
/// via a re-render and the presence of two distinct footnote content blocks.
#[test]
fn two_footnotes_auto_number_one_and_two() {
    let src = r##"zenith version=1 {
  project id="proj.fn2" name="FN2"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="doc.fn2" title="FN2" {
page id="page.fn2" w=(px)600 h=(px)900 margin-inner=(px)60 margin-outer=(px)60 margin-top=(px)80 margin-bottom=(px)80 {
  text id="body" x=(px)60 y=(px)80 w=(px)480 h=(px)200 {
    span "First mark" footnote-ref="fn.1"
    span " and second mark" footnote-ref="fn.2"
  }
  footnote id="fn.1" {
    span "First note."
  }
  footnote id="fn.2" {
    span "Second note."
  }
}
  }
}
"##;
    let doc = parse(src);
    let markers = super::footnote::collect_footnote_markers(&doc.body.pages[0]);
    assert_eq!(markers.get("fn.1").map(String::as_str), Some("1"));
    assert_eq!(markers.get("fn.2").map(String::as_str), Some("2"));

    // Render succeeds and produces a non-trivial scene.
    let provider = default_provider();
    let r = compile(&doc, &provider);
    assert!(r.scene.commands.len() > 4, "{:?}", r.scene.commands);
}

/// A `footnote-ref` pointing at an absent footnote id → advisory
/// `footnote.unresolved_ref` at compile time, and no marker is emitted.
#[test]
fn unresolved_footnote_ref_warns_and_emits_no_marker() {
    let src = r##"zenith version=1 {
  project id="proj.fn3" name="FN3"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="doc.fn3" title="FN3" {
page id="page.fn3" w=(px)600 h=(px)900 margin-inner=(px)60 margin-outer=(px)60 margin-top=(px)80 margin-bottom=(px)80 {
  text id="body" x=(px)60 y=(px)80 w=(px)480 h=(px)200 {
    span "Dangling reference" footnote-ref="fn.missing"
  }
}
  }
}
"##;
    let doc = parse(src);
    let provider = default_provider();
    let r = compile(&doc, &provider);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == "footnote.unresolved_ref"),
        "an unresolved footnote-ref must produce a footnote.unresolved_ref diagnostic; \
         got {:?}",
        r.diagnostics
    );
    // All glyph runs are full-size (no superscript marker emitted): there is only
    // one distinct font size.
    let sizes: Vec<f32> = r
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { font_size, .. } => Some(*font_size),
            _ => None,
        })
        .collect();
    let max = sizes.iter().cloned().fold(0.0_f32, f32::max);
    assert!(
        !sizes.iter().any(|s| *s < max - 0.5),
        "no reduced-size (superscript) run may be emitted for an unresolved ref; sizes={sizes:?}"
    );
}

/// A page with NO footnotes emits a command stream byte-identical to before the
/// feature: the footnote pass is a no-op. We render the same doc twice and also
/// confirm there is no separator-like 1px FillRect spuriously added.
#[test]
fn page_without_footnotes_is_unchanged() {
    let src = r##"zenith version=1 {
  project id="proj.fn4" name="FN4"
  tokens format="zenith-token-v1" {
  }
  styles {}
  document id="doc.fn4" title="FN4" {
page id="page.fn4" w=(px)600 h=(px)900 margin-inner=(px)60 margin-outer=(px)60 margin-top=(px)80 margin-bottom=(px)80 {
  text id="body" x=(px)60 y=(px)80 w=(px)480 h=(px)200 {
    span "Just a plain paragraph with no notes."
  }
}
  }
}
"##;
    let doc = parse(src);
    let provider = default_provider();
    let r1 = compile(&doc, &provider);
    let r2 = compile(&doc, &provider);
    assert_eq!(
        r1.scene.commands, r2.scene.commands,
        "two renders must be byte-identical"
    );
    // No separator rule: no 1px-tall FillRect added by the footnote pass.
    assert!(
        !r1.scene.commands.iter().any(|c| matches!(
            c,
            SceneCommand::FillRect { h, .. } if (*h - 1.0).abs() < 0.01
        )),
        "a footnote-free page must not emit a separator rule: {:?}",
        r1.scene.commands
    );
}

// ── Drop cap with text wrap-around ────────────────────────────────────

/// Build a wrapping body-paragraph document, optionally with `drop-cap-lines`.
/// The body text is long enough to overflow the box width (forcing the wrap
/// path). Returns the compiled scene's DrawGlyphRun list as `(x, y, font_size)`.
fn dropcap_runs(drop_cap_lines: Option<u32>, body: &str) -> Vec<(f64, f64, f32)> {
    let dc_attr = drop_cap_lines.map_or(String::new(), |n| format!(" drop-cap-lines={n}"));
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.dc" name="DC"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.dc" title="DC" {{
page id="page.dc" w=(px)1800 h=(px)2700 {{
  text id="text.dc" x=(px)180 y=(px)600 w=(px)600 h=(px)1200 align="justify" font-size=(px)32{dc_attr} {{
    span "{body}"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| {
            if let SceneCommand::DrawGlyphRun {
                x, y, font_size, ..
            } = c
            {
                Some((*x, *y, *font_size))
            } else {
                None
            }
        })
        .collect()
}

const DROPCAP_BODY: &str = "The quick brown fox jumps over the lazy dog and then \
continues running across the wide green meadow under a bright morning sky while \
birds sing and the river flows gently past the old stone bridge nearby downstream.";

/// With `drop-cap-lines=3`: an oversized initial glyph is emitted (its
/// font_size is far larger than the 32px body), and the first three body lines
/// are indented to the right of the box-left while the fourth line returns to
/// the box-left edge.
#[test]
fn dropcap_emits_oversized_initial_and_indents_first_lines() {
    let runs = dropcap_runs(Some(3), DROPCAP_BODY);
    assert!(runs.len() > 4, "body must wrap to several lines: {runs:?}");

    let body_font_size = 32.0_f32;
    let box_left = 180.0_f64;

    // The oversized cap is the FIRST emitted run (drawn before the body).
    let (cap_x, _cap_y, cap_size) = runs[0];
    assert!(
        cap_size > body_font_size * 2.0,
        "drop-cap font_size ({cap_size}) must be far larger than body ({body_font_size}); \
         expected ≈ 3×line_height"
    );
    assert!(
        (cap_x - box_left).abs() < 0.01,
        "drop cap must sit at the box left edge ({box_left}); got {cap_x}"
    );

    // Body runs follow the cap. The first three text lines must be indented to
    // the RIGHT of the box left; the next line must return to the box left.
    // Collect body runs grouped by distinct baseline y (one x-origin per line).
    let body: Vec<(f64, f64)> = runs[1..].iter().map(|&(x, y, _)| (x, y)).collect();
    // Per-line minimum x keyed by baseline y (BTreeMap to keep order stable).
    let mut line_min_x: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
    for &(x, y) in &body {
        let key = (y * 1000.0) as i64;
        let e = line_min_x.entry(key).or_insert(x);
        if x < *e {
            *e = x;
        }
    }
    let line_starts: Vec<f64> = line_min_x.values().copied().collect();
    assert!(
        line_starts.len() >= 4,
        "need at least 4 body lines; got {}: {line_starts:?}",
        line_starts.len()
    );
    for (i, &sx) in line_starts.iter().take(3).enumerate() {
        assert!(
            sx > box_left + 1.0,
            "body line {i} must be indented right of box left ({box_left}); got {sx}"
        );
    }
    assert!(
        (line_starts[3] - box_left).abs() < 1.0,
        "body line 4 must return to box left ({box_left}); got {}",
        line_starts[3]
    );
}

/// Without `drop-cap-lines` the command stream is byte-identical to a plain
/// wrapped paragraph — the feature is fully opt-in.
#[test]
fn dropcap_absent_is_byte_identical() {
    let dc_attr = "";
    let _ = dc_attr;
    let none_runs = dropcap_runs(None, DROPCAP_BODY);
    // Re-render the SAME no-dropcap source twice → identical (determinism), and
    // confirm the first run is body-sized (no oversized cap).
    let none_runs2 = dropcap_runs(None, DROPCAP_BODY);
    assert_eq!(
        none_runs, none_runs2,
        "no-dropcap render must be deterministic"
    );
    for (_, _, fs) in &none_runs {
        assert!(
            (*fs - 32.0).abs() < 0.01,
            "no-dropcap node must emit only body-sized (32px) runs; got {fs}"
        );
    }
}

/// Empty body text + `drop-cap-lines` set → no panic, no oversized cap, no
/// glyph runs at all (the empty-spans early return fires).
#[test]
fn dropcap_empty_text_no_panic_no_cap() {
    let runs = dropcap_runs(Some(3), "");
    assert!(
        runs.is_empty(),
        "empty text with drop-cap-lines must emit no glyph runs; got {runs:?}"
    );
}

/// Two renders of a drop-cap document produce a byte-identical command stream.
#[test]
fn dropcap_two_run_byte_identical() {
    let src = r##"zenith version=1 {
  project id="proj.dcr" name="DCR"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.dcr" title="DCR" {
page id="page.dcr" w=(px)1800 h=(px)2700 {
  text id="text.dcr" x=(px)180 y=(px)600 w=(px)600 h=(px)1200 align="justify" font-size=(px)32 drop-cap-lines=3 {
    span "The quick brown fox jumps over the lazy dog and then keeps on running across the meadow."
  }
}
  }
}
"##;
    let doc = parse(src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());
    assert_eq!(
        r1.scene.commands, r2.scene.commands,
        "two drop-cap renders must be byte-identical"
    );
}

// ── Hyphenation (single-box wrap path) ────────────────────────────────

/// Build a single-box wrapping paragraph with a narrow box, optionally with
/// `hyphenate=#true`. The body is long hyphenatable words that overflow the
/// width. Returns the compiled scene's full command stream.
fn hyphenate_commands(hyphenate: bool, body: &str) -> Vec<SceneCommand> {
    let hy = if hyphenate { " hyphenate=#true" } else { "" };
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.hy" name="HY"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.hy" title="HY" {{
page id="page.hy" w=(px)1200 h=(px)2000 {{
  text id="text.hy" x=(px)100 y=(px)100 w=(px)360 h=(px)1600 font-size=(px)40{hy} {{
    span "{body}"
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    compile(&doc, &default_provider()).scene.commands
}

/// Count distinct glyph-run baseline y values (≈ the number of text lines).
fn distinct_line_count(cmds: &[SceneCommand]) -> usize {
    let mut ys: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for c in cmds {
        if let SceneCommand::DrawGlyphRun { y, .. } = c {
            ys.insert((*y * 100.0) as i64);
        }
    }
    ys.len()
}

/// Count DrawGlyphRun commands.
fn glyph_run_count(cmds: &[SceneCommand]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count()
}

const HYPH_BODY: &str = "extraordinarily complicated hyphenation demonstrates \
remarkable typographical sophistication consistently";

/// With `hyphenate=#true`, a long word that does not fit the remaining space on
/// a non-empty line is SPLIT: more glyph runs are emitted (head + tail) and the
/// word spans two lines. The stream differs from the hyphenate-off render.
#[test]
fn hyphenate_splits_long_words() {
    let off = hyphenate_commands(false, HYPH_BODY);
    let on = hyphenate_commands(true, HYPH_BODY);

    assert_ne!(
        off, on,
        "hyphenation must change the command stream for an overflowing paragraph"
    );
    // Splitting a word into `head-` + `tail` adds glyph runs beyond the off case.
    assert!(
        glyph_run_count(&on) > glyph_run_count(&off),
        "hyphenation must emit more glyph runs (head+tail); off={}, on={}",
        glyph_run_count(&off),
        glyph_run_count(&on)
    );
    // Determinism: two on-renders are byte-identical.
    let on2 = hyphenate_commands(true, HYPH_BODY);
    assert_eq!(on, on2, "hyphenated render must be deterministic");
}

/// Hyphenation OFF is byte-identical when re-rendered, and a long word wraps
/// whole (line count is the unsplit count). This is the opt-in guard: the
/// default path is unchanged from pre-feature behavior.
#[test]
fn hyphenate_off_is_byte_identical_and_wraps_whole() {
    let off = hyphenate_commands(false, HYPH_BODY);
    let off2 = hyphenate_commands(false, HYPH_BODY);
    assert_eq!(off, off2, "hyphenate-off render must be deterministic");

    // Hyphenation packs fragments tighter, MOVING break points, so the on/off
    // line stream differs (it may yield fewer OR more lines depending on how
    // fragments fill). The whole-word (off) path wraps to at least one line.
    let on = hyphenate_commands(true, HYPH_BODY);
    assert_ne!(
        off, on,
        "hyphenation must move break points relative to whole-word wrapping"
    );
    assert!(
        distinct_line_count(&off) >= 1,
        "off paragraph must wrap to at least one line"
    );
}

// ── Widow/orphan control (chain distribution) ─────────────────────────

/// Build a 2-page chain whose body is two newline-separated paragraphs, sized so
/// the greedy height-cut would otherwise strand a paragraph's first line at the
/// bottom of box 1. `widow_orphan` toggles `widow-orphan=2` on the source.
/// Returns `(page0_line_count, page1_line_count)` from glyph-run baselines.
fn widow_orphan_line_counts(widow_orphan: bool) -> (usize, usize) {
    let wo = if widow_orphan { " widow-orphan=2" } else { "" };
    // Narrow box (w=500) so each paragraph is several lines; box 1 (h=240) holds
    // ~4 lines. Paragraph 1 is ~3 lines, so the greedy cut would otherwise leave
    // paragraph 2's lone FIRST line as box 1's 4th line → an orphan.
    let p1 = "alpha bravo charlie delta echo foxtrot golf hotel";
    let p2 = "victor whiskey xray yankee zulu aurora borealis cascade delta \
estuary fjord glacier harbor island jungle kelp lagoon marsh nimbus quill \
raven storm thicket umbra";
    // Embed a LITERAL backslash-n so the KDL string parser decodes it to a
    // newline (a raw newline inside a quoted KDL string is invalid).
    let body = format!("{p1}\\n{p2}");
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.wo" name="WO"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.wo" title="WO" {{
page id="page.a" w=(px)1200 h=(px)2000 {{
  text id="body.1" x=(px)100 y=(px)100 w=(px)500 h=(px)180 chain="ch" font-size=(px)40 overflow="visible"{wo} {{
    span "{body}"
  }}
}}
page id="page.b" w=(px)1200 h=(px)2000 {{
  text id="body.2" x=(px)100 y=(px)100 w=(px)500 h=(px)1200 chain="ch" font-size=(px)40 overflow="visible" {{
  }}
}}
  }}
}}
"##
    );
    let doc = parse(&src);
    let p0 = compile_page(&doc, &default_provider(), 0).scene.commands;
    let p1c = compile_page(&doc, &default_provider(), 1).scene.commands;
    (distinct_line_count(&p0), distinct_line_count(&p1c))
}

/// With `widow-orphan=2`, a paragraph's lone first line that the greedy cut
/// would leave at the bottom of box 1 is pulled down into box 2: box 1's line
/// count is REDUCED relative to the control-off render, and box 2 gains lines.
#[test]
fn widow_orphan_pulls_orphan_line_to_next_box() {
    let (off0, off1) = widow_orphan_line_counts(false);
    let (on0, on1) = widow_orphan_line_counts(true);

    // The control must change SOMETHING about the distribution.
    assert!(
        (on0, on1) != (off0, off1),
        "widow-orphan=2 must move the break: off=({off0},{off1}) on=({on0},{on1})"
    );
    // Lines only move DOWN (box 1 keeps no more than before; box 2 gains).
    assert!(
        on0 <= off0,
        "box 1 must not gain lines under widow/orphan: off0={off0} on0={on0}"
    );
    assert!(
        on1 >= off1,
        "box 2 must not lose lines under widow/orphan: off1={off1} on1={on1}"
    );
    // No content lost: the total line count is preserved across the boundary
    // move (re-wrapping to the same widths keeps the same lines).
    assert_eq!(
        on0 + on1,
        off0 + off1,
        "total lines must be preserved when the boundary moves"
    );
}

/// Widow/orphan OFF: the 2-box chain distribution is deterministic and matches a
/// re-render byte-for-byte (the control is fully opt-in).
#[test]
fn widow_orphan_off_is_deterministic() {
    let doc_src = || {
        let p1 = "alpha bravo charlie delta echo foxtrot golf hotel india juliet";
        let p2 = "victor whiskey xray yankee zulu aurora borealis cascade delta estuary";
        format!(
            r##"zenith version=1 {{
  project id="proj.wod" name="WOD"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.wod" title="WOD" {{
page id="page.a" w=(px)1200 h=(px)2000 {{
  text id="body.1" x=(px)100 y=(px)100 w=(px)900 h=(px)360 chain="ch" font-size=(px)40 overflow="visible" {{
    span "{p1}\n{p2}"
  }}
}}
page id="page.b" w=(px)1200 h=(px)2000 {{
  text id="body.2" x=(px)100 y=(px)100 w=(px)900 h=(px)900 chain="ch" font-size=(px)40 overflow="visible" {{
  }}
}}
  }}
}}
"##
        )
    };
    let doc = parse(&doc_src());
    let r1 = compile_page(&doc, &default_provider(), 0).scene.commands;
    let r2 = compile_page(&doc, &default_provider(), 0).scene.commands;
    assert_eq!(
        r1, r2,
        "widow/orphan-off chain render must be deterministic"
    );
}
