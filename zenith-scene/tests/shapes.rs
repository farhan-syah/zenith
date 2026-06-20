mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::ir::{LineCap, SceneCommand};
use zenith_scene::{CompileResult, compile};

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
        SceneCommand::FillEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            color,
        } => {
            assert_eq!(*x, 0.0);
            assert_eq!(*y, 0.0);
            assert_eq!(*w, 640.0);
            assert_eq!(*h, 360.0);
            assert!(rx.is_none(), "expected rx=None for inscribed ellipse");
            assert!(ry.is_none(), "expected ry=None for inscribed ellipse");
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
        SceneCommand::FillEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            color,
        } => {
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 10.0);
            assert_eq!(*w, 180.0);
            assert_eq!(*h, 180.0);
            assert!(rx.is_none(), "expected rx=None for inscribed ellipse");
            assert!(ry.is_none(), "expected ry=None for inscribed ellipse");
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
            ..
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
            ..
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
            ..
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

/// A rect with a LITERAL `radius=(px)16` (no token) must emit a
/// `FillRoundedRect` whose radius is 16.0.
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

/// A radial gradient token (`radial=#true`) referenced via `fill=(token)` on
/// a rect must compile to a `FillRectGradient` with `GradientPaint { radial: true, … }`.
#[test]
fn radial_gradient_fill_emits_radial_gradient_paint() {
    use zenith_scene::ir::GradientPaint;
    let src = r##"zenith version=1 {
  project id="proj.rg" name="RG"
  tokens format="zenith-token-v1" {
token id="color.inner" type="color" value="#ffffff"
token id="color.outer" type="color" value="#000000"
token id="grad.radial" type="gradient" radial=#true center-x=0.5 center-y=0.5 radius=0.8 {
  stop offset=0.0 color=(token)"color.inner"
  stop offset=1.0 color=(token)"color.outer"
}
  }
  styles {}
  document id="doc.rg" title="RG" {
page id="page.rg" w=(px)100 h=(px)100 {
  rect id="rect.rg" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"grad.radial"
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
    let found = cmds.iter().find_map(|c| {
        if let SceneCommand::FillRectGradient { gradient, .. } = c {
            Some(gradient)
        } else {
            None
        }
    });
    let grad: &GradientPaint = found.expect("expected FillRectGradient");
    assert!(grad.radial, "GradientPaint must have radial=true");
    assert_eq!(grad.center_x, Some(0.5));
    assert_eq!(grad.center_y, Some(0.5));
    assert_eq!(grad.radius_frac, Some(0.8));
    assert_eq!(grad.stops.len(), 2);
    assert_eq!(grad.stops[0].color.r, 0xff);
    assert_eq!(grad.stops[1].color.r, 0x00);
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

// ── dashed stroke: rect with stroke-dash/gap/linecap compiles correctly ──

/// A rect with `stroke-dash=(px)8 stroke-gap=(px)4 stroke-linecap="round"` must
/// compile to a `StrokeRect` with `stroke_dash=Some(8.0)`, `stroke_gap=Some(4.0)`,
/// and `stroke_linecap=Some(LineCap::Round)`.
#[test]
fn rect_dashed_stroke_compiles_to_stroke_rect_with_dash_fields() {
    let src = r##"zenith version=1 {
  project id="proj.ds" name="DS"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color" value="#112233"
token id="size.sw" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.ds" title="DS" {
page id="page.ds" w=(px)100 h=(px)100 {
  rect id="rect.ds" x=(px)10 y=(px)10 w=(px)40 h=(px)40 stroke=(token)"color.stroke" stroke-width=(token)"size.sw" stroke-dash=(px)8 stroke-gap=(px)4 stroke-linecap="round"
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
    let stroke_cmd = result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::StrokeRect { .. }));
    let cmd = stroke_cmd.expect("expected a StrokeRect in the scene");
    match cmd {
        SceneCommand::StrokeRect {
            stroke_dash,
            stroke_gap,
            stroke_linecap,
            ..
        } => {
            assert_eq!(*stroke_dash, Some(8.0), "stroke_dash must be Some(8.0)");
            assert_eq!(*stroke_gap, Some(4.0), "stroke_gap must be Some(4.0)");
            assert_eq!(
                *stroke_linecap,
                Some(LineCap::Round),
                "stroke_linecap must be Some(Round)"
            );
        }
        other => panic!("expected StrokeRect, got {other:?}"),
    }
}

/// A plain solid-stroke rect (no stroke-dash/gap/linecap) must produce a
/// `StrokeRect` with all three dash fields = `None` (byte-compatible with prior IR).
#[test]
fn rect_solid_stroke_has_no_dash_fields() {
    let src = r##"zenith version=1 {
  project id="proj.ss" name="SS"
  tokens format="zenith-token-v1" {
token id="color.stroke" type="color" value="#445566"
token id="size.sw" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.ss" title="SS" {
page id="page.ss" w=(px)100 h=(px)100 {
  rect id="rect.ss" x=(px)10 y=(px)10 w=(px)40 h=(px)40 stroke=(token)"color.stroke" stroke-width=(token)"size.sw"
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
    let stroke_cmd = result
        .scene
        .commands
        .iter()
        .find(|c| matches!(c, SceneCommand::StrokeRect { .. }));
    let cmd = stroke_cmd.expect("expected a StrokeRect in the scene");
    match cmd {
        SceneCommand::StrokeRect {
            stroke_dash,
            stroke_gap,
            stroke_linecap,
            ..
        } => {
            assert_eq!(
                *stroke_dash, None,
                "solid stroke must have stroke_dash=None"
            );
            assert_eq!(*stroke_gap, None, "solid stroke must have stroke_gap=None");
            assert_eq!(
                *stroke_linecap, None,
                "solid stroke must have stroke_linecap=None"
            );
        }
        other => panic!("expected StrokeRect, got {other:?}"),
    }
}

// ── Blend-mode layer bracket ──────────────────────────────────────────

/// A rect with `blend-mode="multiply"` must wrap its FillRect in a
/// `PushLayer { blend_mode: Some(Multiply) } … PopLayer` bracket.
#[test]
fn rect_blend_mode_wraps_fill_in_layer() {
    use zenith_scene::ir::BlendMode;

    let src = r##"zenith version=1 {
  project id="proj.bm" name="BM"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.bm" title="BM" {
    page id="page.bm" w=(px)100 h=(px)100 {
      rect id="rect.bm" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.fill" blend-mode="multiply"
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
    // PushClip, PushLayer, FillRect, PopLayer, PopClip
    assert_eq!(cmds.len(), 5, "expected 5 commands, got: {:?}", cmds);

    assert!(
        matches!(cmds[0], SceneCommand::PushClip { .. }),
        "first command must be PushClip"
    );
    match &cmds[1] {
        SceneCommand::PushLayer {
            blend_mode,
            opacity,
        } => {
            assert_eq!(*blend_mode, Some(BlendMode::Multiply));
            assert_eq!(*opacity, 1.0, "no node opacity → layer opacity 1.0");
        }
        other => panic!("expected PushLayer, got {other:?}"),
    }
    match &cmds[2] {
        SceneCommand::FillRect { color, .. } => {
            // Colors emit at full alpha when a blend layer is active.
            assert_eq!(color.a, 255, "blend-layer fill must use full alpha");
        }
        other => panic!("expected FillRect, got {other:?}"),
    }
    assert!(
        matches!(cmds[3], SceneCommand::PopLayer),
        "fourth command must be PopLayer"
    );
    assert!(
        matches!(cmds[4], SceneCommand::PopClip),
        "last command must be PopClip"
    );
}

/// A rect WITHOUT blend-mode must NOT emit any PushLayer/PopLayer — the
/// command stream is byte-identical to before blend-mode existed.
#[test]
fn rect_without_blend_mode_emits_no_layer() {
    let src = r##"zenith version=1 {
  project id="proj.nb" name="NB"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#112233"
  }
  styles {}
  document id="doc.nb" title="NB" {
    page id="page.nb" w=(px)100 h=(px)100 {
      rect id="rect.nb" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.fill"
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    assert!(result.diagnostics.is_empty());

    let cmds = &result.scene.commands;
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::PushLayer { .. } | SceneCommand::PopLayer)),
        "a node without blend-mode must emit no layer commands: {:?}",
        cmds
    );
}

// ── Element gaussian blur ─────────────────────────────────────────────

/// A rect with `blur=(px)8` must emit a `BeginBlur { radius: 8.0 }` … `EndBlur`
/// bracket around its fill/stroke draws. The radius must match exactly.
#[test]
fn rect_with_blur_emits_begin_end_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.bl1" name="Bl1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
  }
  styles {}
  document id="doc.bl1" title="Bl1" {
    page id="page.bl1" w=(px)200 h=(px)200 {
      rect id="rect.bl" x=(px)10 y=(px)10 w=(px)80 h=(px)60 fill=(token)"color.fill" blur=(px)8
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Locate the BeginBlur and check the radius.
    let radius = cmds.iter().find_map(|c| match c {
        SceneCommand::BeginBlur { radius } => Some(*radius),
        _ => None,
    });
    assert_eq!(radius, Some(8.0), "BeginBlur radius must be 8.0: {cmds:?}");

    // Exactly one Begin and one End.
    let begins = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .count();
    let ends = cmds
        .iter()
        .filter(|c| matches!(c, SceneCommand::EndBlur))
        .count();
    assert_eq!(begins, 1, "exactly one BeginBlur: {cmds:?}");
    assert_eq!(ends, 1, "exactly one EndBlur: {cmds:?}");

    // Begin must precede End and a fill must sit between them.
    let begin_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::BeginBlur { .. }))
        .expect("begin index");
    let end_idx = cmds
        .iter()
        .position(|c| matches!(c, SceneCommand::EndBlur))
        .expect("end index");
    assert!(begin_idx < end_idx, "Begin must precede End");
    let has_fill = cmds
        .get(begin_idx + 1..end_idx)
        .map(|w| w.iter().any(|c| matches!(c, SceneCommand::FillRect { .. })))
        .unwrap_or(false);
    assert!(
        has_fill,
        "a fill must sit inside the blur bracket: {cmds:?}"
    );
}

/// A rect WITHOUT `blur` must emit no `BeginBlur`/`EndBlur` — the command
/// stream is byte-identical to the pre-blur behavior.
#[test]
fn rect_without_blur_emits_no_blur_bracket() {
    let src = r##"zenith version=1 {
  project id="proj.bl2" name="Bl2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
  }
  styles {}
  document id="doc.bl2" title="Bl2" {
    page id="page.bl2" w=(px)200 h=(px)200 {
      rect id="rect.nb" x=(px)10 y=(px)10 w=(px)80 h=(px)60 fill=(token)"color.fill"
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, SceneCommand::BeginBlur { .. } | SceneCommand::EndBlur)),
        "no blur attribute → no blur bracket: {cmds:?}"
    );
}

/// When both `blur` and `shadow` are set on the same rect, blur wins:
/// only a `BeginBlur`/`EndBlur` bracket is emitted and NO
/// `BeginShadow`/`EndShadow` appears in the stream.
#[test]
fn blur_wins_over_shadow_when_both_set() {
    let src = r##"zenith version=1 {
  project id="proj.bl3" name="Bl3"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#445566"
    token id="color.sh" type="color" value="#000000"
    token id="shadow.s" type="shadow" {
      layer dx=(px)2 dy=(px)2 blur=(px)4 color=(token)"color.sh"
    }
  }
  styles {}
  document id="doc.bl3" title="Bl3" {
    page id="page.bl3" w=(px)200 h=(px)200 {
      rect id="rect.bs" x=(px)10 y=(px)10 w=(px)80 h=(px)60 fill=(token)"color.fill" shadow=(token)"shadow.s" blur=(px)6
    }
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Blur bracket present.
    assert!(
        cmds.iter()
            .any(|c| matches!(c, SceneCommand::BeginBlur { .. })),
        "BeginBlur must be emitted when blur wins: {cmds:?}"
    );
    // Shadow bracket absent.
    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            SceneCommand::BeginShadow { .. } | SceneCommand::EndShadow
        )),
        "BeginShadow must NOT be emitted when blur wins: {cmds:?}"
    );
}

// ── U7: Per-side borders ───────────────────────────────────────────────

/// A rect with `border-bottom=(token)` produces a `StrokeLine` along the
/// bottom edge (y1 == y2 == y + h) with the resolved color/width.
/// A rect WITHOUT any border-* props produces NO extra `StrokeLine`.
#[test]
fn border_bottom_emits_stroke_line_along_bottom_edge() {
    // ── With border-bottom ──────────────────────────────────────────
    let src_with = r##"zenith version=1 {
  project id="proj.bb1" name="BB1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
    token id="color.border" type="color" value="#ff0000"
    token id="size.bw" type="dimension" value=(px)4
  }
  styles {}
  document id="doc.bb1" title="BB1" {
    page id="page.bb1" w=(px)200 h=(px)200 {
      rect id="rect.bb" x=(px)10 y=(px)20 w=(px)80 h=(px)60 fill=(token)"color.fill" border-bottom=(token)"color.border" border-width=(token)"size.bw"
    }
  }
}
"##;
    let doc_with = parse(src_with);
    let result_with = compile(&doc_with, &default_provider());
    assert!(
        result_with.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result_with.diagnostics
    );

    let cmds = &result_with.scene.commands;
    // Find the StrokeLine for the bottom border.
    let bottom_line = cmds.iter().find(|c| {
        matches!(c, SceneCommand::StrokeLine { y1, y2, .. } if (*y1 - 80.0).abs() < 1e-9 && (*y2 - 80.0).abs() < 1e-9)
    });
    assert!(
        bottom_line.is_some(),
        "expected a StrokeLine at y=80 (y+h=20+60); commands: {cmds:?}"
    );
    match bottom_line.unwrap() {
        SceneCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
            stroke_dash,
            stroke_gap,
            stroke_linecap,
        } => {
            // x span: rect x=10 to x+w=90.
            assert!((*x1 - 10.0).abs() < 1e-9, "x1 must be rect x=10; got {x1}");
            assert!(
                (*x2 - 90.0).abs() < 1e-9,
                "x2 must be rect x+w=90; got {x2}"
            );
            assert!(
                (*y1 - 80.0).abs() < 1e-9 && (*y2 - 80.0).abs() < 1e-9,
                "y1 and y2 must be y+h=80; got y1={y1} y2={y2}"
            );
            // Color: #ff0000 → r=255, g=0, b=0.
            assert_eq!(color.r, 255, "border color must be red");
            assert_eq!(color.g, 0);
            assert_eq!(color.b, 0);
            // Width: token resolves to 4px.
            assert!(
                (*stroke_width - 4.0).abs() < 1e-9,
                "stroke_width must be 4px; got {stroke_width}"
            );
            // Per-side borders never use dashes.
            assert!(stroke_dash.is_none(), "border StrokeLine must have no dash");
            assert!(stroke_gap.is_none());
            assert!(stroke_linecap.is_none());
        }
        _ => panic!("impossible — already matched"),
    }

    // ── Without border-* → no extra StrokeLine ─────────────────────
    let src_without = r##"zenith version=1 {
  project id="proj.bb2" name="BB2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.bb2" title="BB2" {
    page id="page.bb2" w=(px)200 h=(px)200 {
      rect id="rect.nob" x=(px)10 y=(px)20 w=(px)80 h=(px)60 fill=(token)"color.fill"
    }
  }
}
"##;
    let doc_without = parse(src_without);
    let result_without = compile(&doc_without, &default_provider());
    assert!(
        result_without.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result_without.diagnostics
    );
    let has_stroke_line = result_without
        .scene
        .commands
        .iter()
        .any(|c| matches!(c, SceneCommand::StrokeLine { .. }));
    assert!(
        !has_stroke_line,
        "a rect without border-* must emit no StrokeLine; commands: {:?}",
        result_without.scene.commands
    );
}

// ── U8: Double border (stroke-outer) ──────────────────────────────────

/// A rect with `stroke-outer=(token)` + `stroke-outer-width=(token)` emits a
/// second `StrokeRect` at outset geometry (x-half, y-half, w+ow, h+ow) with the
/// outer color. A rect WITHOUT `stroke-outer` emits only the primary stroke.
#[test]
fn stroke_outer_emits_second_stroke_rect_at_outset() {
    // ── With stroke-outer ───────────────────────────────────────────
    let src_with = r##"zenith version=1 {
  project id="proj.so1" name="SO1"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
    token id="color.inner" type="color" value="#0000ff"
    token id="color.outer" type="color" value="#00ff00"
    token id="size.sw" type="dimension" value=(px)2
    token id="size.ow" type="dimension" value=(px)4
  }
  styles {}
  document id="doc.so1" title="SO1" {
    page id="page.so1" w=(px)300 h=(px)300 {
      rect id="rect.so" x=(px)50 y=(px)60 w=(px)100 h=(px)80 fill=(token)"color.fill" stroke=(token)"color.inner" stroke-width=(token)"size.sw" stroke-outer=(token)"color.outer" stroke-outer-width=(token)"size.ow"
    }
  }
}
"##;
    let doc_with = parse(src_with);
    let result_with = compile(&doc_with, &default_provider());
    assert!(
        result_with.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result_with.diagnostics
    );

    let cmds = &result_with.scene.commands;
    // Collect all StrokeRect commands.
    let stroke_rects: Vec<_> = cmds
        .iter()
        .filter_map(|c| match c {
            SceneCommand::StrokeRect {
                x,
                y,
                w,
                h,
                color,
                stroke_width,
                ..
            } => Some((*x, *y, *w, *h, *color, *stroke_width)),
            _ => None,
        })
        .collect();

    assert_eq!(
        stroke_rects.len(),
        2,
        "expected 2 StrokeRect commands (primary + outer); got: {cmds:?}"
    );

    // Primary stroke: x=50, y=60, w=100, h=80, color=blue (#0000ff), sw=2.
    let (px0, py0, pw0, ph0, pc0, psw0) = stroke_rects[0];
    assert!(
        (px0 - 50.0).abs() < 1e-9 && (py0 - 60.0).abs() < 1e-9,
        "primary stroke origin must be (50, 60); got ({px0}, {py0})"
    );
    assert!(
        (pw0 - 100.0).abs() < 1e-9 && (ph0 - 80.0).abs() < 1e-9,
        "primary stroke size must be 100×80; got {pw0}×{ph0}"
    );
    assert_eq!(pc0.b, 255, "primary stroke must be blue");
    assert_eq!(pc0.r, 0);
    assert!((psw0 - 2.0).abs() < 1e-9, "primary stroke_width must be 2");

    // Outer stroke: ow=4, half=2 → x=50-2=48, y=60-2=58, w=100+4=104, h=80+4=84.
    let (ox, oy, ow, oh, oc, osw) = stroke_rects[1];
    assert!(
        (ox - 48.0).abs() < 1e-9,
        "outer stroke x must be 48 (50 - 4/2); got {ox}"
    );
    assert!(
        (oy - 58.0).abs() < 1e-9,
        "outer stroke y must be 58 (60 - 4/2); got {oy}"
    );
    assert!(
        (ow - 104.0).abs() < 1e-9,
        "outer stroke w must be 104 (100 + 4); got {ow}"
    );
    assert!(
        (oh - 84.0).abs() < 1e-9,
        "outer stroke h must be 84 (80 + 4); got {oh}"
    );
    assert_eq!(oc.g, 255, "outer stroke must be green");
    assert_eq!(oc.r, 0);
    assert!((osw - 4.0).abs() < 1e-9, "outer stroke_width must be 4");

    // ── Without stroke-outer → only one StrokeRect (the primary) ───
    let src_without = r##"zenith version=1 {
  project id="proj.so2" name="SO2"
  tokens format="zenith-token-v1" {
    token id="color.fill" type="color" value="#ffffff"
    token id="color.inner" type="color" value="#0000ff"
    token id="size.sw" type="dimension" value=(px)2
  }
  styles {}
  document id="doc.so2" title="SO2" {
    page id="page.so2" w=(px)300 h=(px)300 {
      rect id="rect.nos" x=(px)50 y=(px)60 w=(px)100 h=(px)80 fill=(token)"color.fill" stroke=(token)"color.inner" stroke-width=(token)"size.sw"
    }
  }
}
"##;
    let doc_without = parse(src_without);
    let result_without = compile(&doc_without, &default_provider());
    assert!(
        result_without.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result_without.diagnostics
    );
    let stroke_rect_count = result_without
        .scene
        .commands
        .iter()
        .filter(|c| matches!(c, SceneCommand::StrokeRect { .. }))
        .count();
    assert_eq!(
        stroke_rect_count, 1,
        "a rect without stroke-outer must emit exactly 1 StrokeRect (the primary); \
         commands: {:?}",
        result_without.scene.commands
    );
}
