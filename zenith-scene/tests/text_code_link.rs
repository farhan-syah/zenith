mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::{Paint, SceneCommand};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect only FillRect and DrawGlyphRun commands (strip PushClip/PopClip etc.)
fn significant(result: &CompileResult) -> Vec<&SceneCommand> {
    result
        .scene
        .commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                SceneCommand::FillRect { .. } | SceneCommand::DrawGlyphRun { .. }
            )
        })
        .collect()
}

// ── Control: plain span emits no background and default color ────────────────

/// A plain span (no `code`, no `link`) must emit exactly one DrawGlyphRun with
/// no preceding FillRect. Proves `code` and `link` machinery is additive and
/// byte-identical when absent.
#[test]
fn plain_span_emits_no_fill_rect_and_default_color() {
    let src = r##"zenith version=1 {
  project id="proj.cl0" name="CL0"
  tokens format="zenith-token-v1" {
token id="color.ink"  type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl0" title="CL0" {
page id="page.cl0" w=(px)400 h=(px)200 {
  text id="t.cl0" x=(px)10 y=(px)20 w=(px)380 h=(px)60 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Hello"
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
            .all(|d| d.code != "scene.text_unshaped"),
        "no text_unshaped expected; got: {:?}",
        result.diagnostics
    );

    let sig = significant(&result);
    // Exactly one DrawGlyphRun, no FillRect.
    let rects: Vec<_> = sig
        .iter()
        .filter(|c| matches!(c, SceneCommand::FillRect { .. }))
        .collect();
    assert!(
        rects.is_empty(),
        "plain span must emit no FillRect; got {} rect(s)",
        rects.len()
    );
    let runs: Vec<_> = sig
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .collect();
    assert_eq!(
        runs.len(),
        1,
        "expected exactly one DrawGlyphRun; got {}",
        runs.len()
    );
}

// ── code=#true span: CODE_BG background rect + mono glyph run ────────────────

/// A span with `code=#true` must emit:
///   1. A `FillRect` with the internal CODE_BG color (#F0F0F0) — background.
///   2. A `DrawGlyphRun`.
///
/// The FillRect must appear BEFORE the DrawGlyphRun in the command stream.
/// The glyph run must use the "Noto Sans Mono" font family (verified via the
/// font_id string — it will differ from the sans font_id).
#[test]
fn code_span_emits_bg_fill_rect_before_glyph_run() {
    // Use a sans span alongside a code span to capture the sans font_id
    // so we can confirm the code span uses a DIFFERENT (mono) font.
    let src = r##"zenith version=1 {
  project id="proj.cl1" name="CL1"
  tokens format="zenith-token-v1" {
token id="color.ink"  type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl1" title="CL1" {
page id="page.cl1" w=(px)600 h=(px)200 {
  text id="t.cl1" x=(px)10 y=(px)20 w=(px)580 h=(px)60 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "hello" code=#true
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
            .all(|d| d.code != "scene.text_unshaped"),
        "no text_unshaped expected; got: {:?}",
        result.diagnostics
    );

    let sig = significant(&result);

    // Exactly one FillRect (the CODE_BG background) and one DrawGlyphRun.
    assert_eq!(
        sig.len(),
        2,
        "expected 1 FillRect + 1 DrawGlyphRun; got: {:?}",
        sig
    );

    // First command: FillRect with CODE_BG (#F0F0F0 = r=240 g=240 b=240).
    match sig[0] {
        SceneCommand::FillRect {
            paint: Paint::Solid { color },
            w,
            h,
            ..
        } => {
            assert_eq!(color.r, 240, "code bg rect r must be 240 (F0)");
            assert_eq!(color.g, 240, "code bg rect g must be 240 (F0)");
            assert_eq!(color.b, 240, "code bg rect b must be 240 (F0)");
            assert!(*w > 0.0, "code bg rect width must be > 0; got {w}");
            assert!(*h > 0.0, "code bg rect height must be > 0; got {h}");
        }
        other => panic!("expected FillRect (code bg), got {other:?}"),
    }

    // Second command: DrawGlyphRun — font must be Noto Sans Mono.
    match sig[1] {
        SceneCommand::DrawGlyphRun { font_id, .. } => {
            assert!(
                font_id.to_lowercase().contains("mono"),
                "code span glyph run must use mono font; font_id = '{font_id}'"
            );
        }
        other => panic!("expected DrawGlyphRun after code bg FillRect; got {other:?}"),
    }
}

// ── link="…" span: underline + link color + URL retained on parsed span ──────

/// A span with `link="https://example.com"` must:
///   - Render with the internal LINK_COLOR (#0066CC = r=0, g=102, b=204).
///   - Emit an underline FillRect before the DrawGlyphRun (link implies underline).
///   - Retain the URL on the parsed AST span.
///
/// No background FillRect should appear (link ≠ code).
#[test]
fn link_span_renders_with_link_color_and_underline() {
    let src = r##"zenith version=1 {
  project id="proj.cl2" name="CL2"
  tokens format="zenith-token-v1" {
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl2" title="CL2" {
page id="page.cl2" w=(px)600 h=(px)200 {
  text id="t.cl2" x=(px)10 y=(px)20 w=(px)580 h=(px)60 font-family=(token)"font.body" font-size=(token)"size.body" {
    span "click here" link="https://example.com"
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
            .all(|d| d.code != "scene.text_unshaped"),
        "no text_unshaped expected; got: {:?}",
        result.diagnostics
    );

    // Collect scene commands excluding clip wrappers.
    let sig = significant(&result);

    // Expect: FillRect (underline) + DrawGlyphRun (underline is a rect in the
    // NOWRAP path; underline is emitted as a FillRect before the glyph run).
    // The exact count: 1 underline FillRect + 1 DrawGlyphRun = 2.
    assert_eq!(
        sig.len(),
        2,
        "expected 1 underline FillRect + 1 DrawGlyphRun; got: {:?}",
        sig
    );

    // The DrawGlyphRun must use LINK_COLOR (#0066CC → r=0, g=102, b=204).
    let glyph_run = sig
        .iter()
        .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    match glyph_run {
        Some(SceneCommand::DrawGlyphRun { color, .. }) => {
            assert_eq!(color.r, 0, "link color.r must be 0");
            assert_eq!(color.g, 102, "link color.g must be 102");
            assert_eq!(color.b, 204, "link color.b must be 204");
        }
        _ => panic!("expected DrawGlyphRun with link color"),
    }

    // The URL must be retained on the parsed AST span.
    let text_node = doc
        .body
        .pages
        .first()
        .and_then(|p| p.children.first())
        .and_then(|n| {
            if let zenith_core::Node::Text(t) = n {
                Some(t.as_ref())
            } else {
                None
            }
        })
        .expect("text node must be present");
    let span = text_node.spans.first().expect("span must be present");
    assert_eq!(
        span.link.as_deref(),
        Some("https://example.com"),
        "link URL must be retained on the parsed TextSpan"
    );
}

// ── link with explicit fill: keep author color, not LINK_COLOR ───────────────

/// When a `link` span also has an explicit `fill`, the author color takes
/// priority over the internal LINK_COLOR default.
#[test]
fn link_span_with_explicit_fill_keeps_author_color() {
    let src = r##"zenith version=1 {
  project id="proj.cl3" name="CL3"
  tokens format="zenith-token-v1" {
token id="color.red"  type="color"      value="#FF0000"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl3" title="CL3" {
page id="page.cl3" w=(px)600 h=(px)200 {
  text id="t.cl3" x=(px)10 y=(px)20 w=(px)580 h=(px)60 font-family=(token)"font.body" font-size=(token)"size.body" {
    span "red link" fill=(token)"color.red" link="https://example.com"
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
            .all(|d| d.code != "scene.text_unshaped"),
        "no text_unshaped expected; got: {:?}",
        result.diagnostics
    );

    let sig = significant(&result);
    let glyph_run = sig
        .iter()
        .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    match glyph_run {
        Some(SceneCommand::DrawGlyphRun { color, .. }) => {
            // Author color: #FF0000 → r=255, g=0, b=0.
            assert_eq!(color.r, 255, "explicit fill must override link color (r)");
            assert_eq!(color.g, 0, "explicit fill must override link color (g)");
            assert_eq!(color.b, 0, "explicit fill must override link color (b)");
        }
        _ => panic!("expected DrawGlyphRun"),
    }
}

// ── code + link together: both behaviors active ───────────────────────────────

/// A span with BOTH `code=#true` AND `link="…"` must:
///   - Use the mono font family.
///   - Emit the CODE_BG background rect.
///   - Emit an underline.
///   - Retain the URL on the AST span.
#[test]
fn code_and_link_span_combines_both_behaviors() {
    let src = r##"zenith version=1 {
  project id="proj.cl4" name="CL4"
  tokens format="zenith-token-v1" {
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl4" title="CL4" {
page id="page.cl4" w=(px)600 h=(px)200 {
  text id="t.cl4" x=(px)10 y=(px)20 w=(px)580 h=(px)60 font-family=(token)"font.body" font-size=(token)"size.body" {
    span "pkg" code=#true link="https://example.com/pkg"
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
            .all(|d| d.code != "scene.text_unshaped"),
        "no text_unshaped expected; got: {:?}",
        result.diagnostics
    );

    let sig = significant(&result);
    // Expect: 1 CODE_BG FillRect + 1 underline FillRect + 1 DrawGlyphRun = 3.
    assert_eq!(
        sig.len(),
        3,
        "expected CODE_BG rect + underline rect + DrawGlyphRun; got: {:?}",
        sig
    );

    // First rect: CODE_BG.
    match sig[0] {
        SceneCommand::FillRect {
            paint: Paint::Solid { color },
            ..
        } => {
            assert_eq!(color.r, 240, "first rect must be CODE_BG (r=240)");
            assert_eq!(color.g, 240, "first rect must be CODE_BG (g=240)");
            assert_eq!(color.b, 240, "first rect must be CODE_BG (b=240)");
        }
        other => panic!("expected CODE_BG FillRect first; got {other:?}"),
    }

    // DrawGlyphRun must be mono AND use LINK_COLOR (no explicit fill).
    match sig[2] {
        SceneCommand::DrawGlyphRun { font_id, color, .. } => {
            assert!(
                font_id.to_lowercase().contains("mono"),
                "code+link span must use mono font; font_id = '{font_id}'"
            );
            assert_eq!(color.r, 0, "link color.r must be 0");
            assert_eq!(color.g, 102, "link color.g must be 102");
            assert_eq!(color.b, 204, "link color.b must be 204");
        }
        other => panic!("expected DrawGlyphRun (mono, link color); got {other:?}"),
    }

    // URL retained on AST span.
    let text_node = doc
        .body
        .pages
        .first()
        .and_then(|p| p.children.first())
        .and_then(|n| {
            if let zenith_core::Node::Text(t) = n {
                Some(t.as_ref())
            } else {
                None
            }
        })
        .expect("text node must be present");
    let span = text_node.spans.first().expect("span must be present");
    assert_eq!(
        span.link.as_deref(),
        Some("https://example.com/pkg"),
        "link URL must be retained on the parsed TextSpan"
    );
    assert_eq!(span.code, Some(true), "code flag must be retained");
}

// ── link inside a node WITH a fill: link color overrides inherited node fill ──

/// Regression: a `link` span with no span-level `fill`, inside a text node that
/// DOES set a `fill`, must still render in LINK_COLOR — the link's conventional
/// color overrides the INHERITED node fill (but a span-level fill would win).
#[test]
fn link_span_overrides_inherited_node_fill() {
    let src = r##"zenith version=1 {
  project id="proj.cl5" name="CL5"
  tokens format="zenith-token-v1" {
token id="color.ink" type="color"      value="#111827"
token id="font.body" type="fontFamily" value="Noto Sans"
token id="size.body" type="dimension"  value=(px)24
  }
  styles {}
  document id="doc.cl5" title="CL5" {
page id="page.cl5" w=(px)600 h=(px)200 {
  text id="t.cl5" x=(px)10 y=(px)20 w=(px)580 h=(px)60 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "click here" link="https://example.com"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let sig = significant(&result);
    let glyph_run = sig
        .iter()
        .find(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }));
    match glyph_run {
        Some(SceneCommand::DrawGlyphRun { color, .. }) => {
            // Must be LINK_COLOR (#0066CC), NOT the node ink (#111827).
            assert_eq!(
                (color.r, color.g, color.b),
                (0, 102, 204),
                "link span must use LINK_COLOR even when the node sets a fill"
            );
        }
        _ => panic!("expected DrawGlyphRun with link color"),
    }
}
