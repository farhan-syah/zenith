mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::SceneCommand;

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

// ── Frame: PushClip → children → PopClip ─────────────────────────────

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
    let fill_rects_vec: Vec<_> = cmds
        .iter()
        .filter_map(|c| {
            if let SceneCommand::FillRect { x, y, w, h, .. } = c {
                Some((*x, *y, *w, *h))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(fill_rects_vec.len(), 1, "expected exactly one FillRect");
    let (rx, ry, rw, rh) = fill_rects_vec[0];
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

/// 2×3 grid, 6 children, gap=20, pad=0. Children tile row-major into a fixed
/// `cols × rows` grid; horizontal and vertical gutters equal `gap`.
#[test]
fn grid_two_by_three_positions_children_with_gutters() {
    // frame w=320 h=300, cols=2, rows=3, gap=20, pad=0.
    //   col_w = (320 - (2-1)*20) / 2 = 150
    //   row_h = (300 - (3-1)*20) / 3 = 260/3
    let src = r##"zenith version=1 {
  project id="proj.grid1" name="Grid1"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
token id="space.gap" type="dimension" value=(px)20
  }
  styles {
style id="style.grid" {
  gap (token)"space.gap"
}
  }
  document id="doc.grid1" title="Grid1" {
page id="page.grid1" w=(px)400 h=(px)400 {
  frame id="frame.grid" x=(px)0 y=(px)0 w=(px)320 h=(px)300 layout="grid" columns=2 rows=3 style="style.grid" {
    rect id="r0" fill=(token)"color.k"
    rect id="r1" fill=(token)"color.k"
    rect id="r2" fill=(token)"color.k"
    rect id="r3" fill=(token)"color.k"
    rect id="r4" fill=(token)"color.k"
    rect id="r5" fill=(token)"color.k"
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
        6,
        "expected six child FillRects; got {rects:?}"
    );

    let gap = 20.0;
    let col_w = (320.0 - gap) / 2.0; // 150
    let row_h = (300.0 - 2.0 * gap) / 3.0; // 260/3

    // Expected origins, row-major.
    for (i, (x, y, w, h)) in rects.iter().enumerate() {
        let col = (i % 2) as f64;
        let row = (i / 2) as f64;
        let exp_x = col * (col_w + gap);
        let exp_y = row * (row_h + gap);
        assert!(
            (*x - exp_x).abs() < 1e-9,
            "cell {i}: x expected {exp_x}; got {x}"
        );
        assert!(
            (*y - exp_y).abs() < 1e-9,
            "cell {i}: y expected {exp_y}; got {y}"
        );
        assert!(
            (*w - col_w).abs() < 1e-9,
            "cell {i}: w expected {col_w}; got {w}"
        );
        assert!(
            (*h - row_h).abs() < 1e-9,
            "cell {i}: h expected {row_h}; got {h}"
        );
    }

    // Horizontal gutter between col0 and col1 equals gap.
    let (x0, _, w0, _) = rects[0];
    let (x1, _, _, _) = rects[1];
    assert!(
        (x1 - (x0 + w0) - gap).abs() < 1e-9,
        "horizontal gutter must equal gap ({gap})"
    );
    // Vertical gutter between row0 and row1 equals gap.
    let (_, y0, _, h0) = rects[0];
    let (_, y2, _, _) = rects[2];
    assert!(
        (y2 - (y0 + h0) - gap).abs() < 1e-9,
        "vertical gutter must equal gap ({gap})"
    );
}

/// `layout="grid"` with no `columns` → single column stack; the scene defaults
/// to 1 column and validation emits a `grid.missing_columns` advisory.
#[test]
fn grid_default_columns_is_one() {
    let src = r##"zenith version=1 {
  project id="proj.grid2" name="Grid2"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.grid2" title="Grid2" {
page id="page.grid2" w=(px)400 h=(px)400 {
  frame id="frame.grid" x=(px)0 y=(px)0 w=(px)300 h=(px)300 layout="grid" {
    rect id="r0" fill=(token)"color.k"
    rect id="r1" fill=(token)"color.k"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let rects = fill_rects(&result);
    assert_eq!(
        rects.len(),
        2,
        "expected two child FillRects; got {rects:?}"
    );
    // Single column: both children share x and span the full content width.
    let (x0, _, w0, _) = rects[0];
    let (x1, _, w1, _) = rects[1];
    assert_eq!(x0, 0.0, "single-column child0 x must be content_left (0)");
    assert_eq!(x1, 0.0, "single-column child1 x must be content_left (0)");
    assert_eq!(w0, 300.0, "single column width must be full content width");
    assert_eq!(w1, 300.0, "single column width must be full content width");
    // Stacked vertically (row1 below row0).
    let (_, y0, _, _) = rects[0];
    let (_, y1, _, _) = rects[1];
    assert!(y1 > y0, "child1 must sit below child0 in a single column");

    // The validator emits a grid.missing_columns advisory.
    let report = zenith_core::validate(&doc);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "grid.missing_columns"),
        "expected grid.missing_columns advisory; codes: {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
}

/// `rows` omitted → derived as `ceil(n / cols)`; the last row is positioned
/// correctly (3 children, 2 cols → 2 rows; child index 2 starts row 1).
#[test]
fn grid_derived_rows_from_child_count() {
    let src = r##"zenith version=1 {
  project id="proj.grid3" name="Grid3"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.grid3" title="Grid3" {
page id="page.grid3" w=(px)400 h=(px)400 {
  frame id="frame.grid" x=(px)0 y=(px)0 w=(px)300 h=(px)300 layout="grid" columns=2 {
    rect id="r0" fill=(token)"color.k"
    rect id="r1" fill=(token)"color.k"
    rect id="r2" fill=(token)"color.k"
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
        3,
        "expected three child FillRects; got {rects:?}"
    );

    // n=3, cols=2 → effective_rows = ceil(3/2) = 2. gap=0, pad=0.
    //   col_w = 300/2 = 150, row_h = 300/2 = 150.
    let (x0, y0, w0, h0) = rects[0];
    let (x1, y1, _, _) = rects[1];
    let (x2, y2, _, _) = rects[2];
    assert_eq!((x0, y0, w0, h0), (0.0, 0.0, 150.0, 150.0));
    // r1 is col1 of row0.
    assert_eq!((x1, y1), (150.0, 0.0));
    // r2 wraps to the last row (row1, col0).
    assert_eq!((x2, y2), (0.0, 150.0));
}

/// A non-grid frame (absolute and flow) emits the identical command stream
/// regardless of the grid fields existing on the AST — default-off identity.
#[test]
fn non_grid_frame_byte_identical() {
    // Absolute frame: child keeps its own coords.
    let abs_src = r##"zenith version=1 {
  project id="proj.grid4" name="Grid4"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.grid4" title="Grid4" {
page id="page.grid4" w=(px)200 h=(px)200 {
  frame id="frame.abs" x=(px)20 y=(px)30 w=(px)160 h=(px)160 {
    rect id="rect.a" x=(px)50 y=(px)60 w=(px)40 h=(px)30 fill=(token)"color.k"
  }
}
  }
}
"##;
    let abs = compile(&parse(abs_src), &default_provider());
    // The child kept its own absolute coords (no grid injection).
    assert_eq!(fill_rects(&abs), vec![(50.0, 60.0, 40.0, 30.0)]);

    // Flow frame: still stacks vertically, unaffected by grid code.
    let flow_src = r##"zenith version=1 {
  project id="proj.grid5" name="Grid5"
  tokens format="zenith-token-v1" {
token id="color.k" type="color" value="#000000"
  }
  styles {}
  document id="doc.grid5" title="Grid5" {
page id="page.grid5" w=(px)200 h=(px)200 {
  frame id="frame.flow" x=(px)0 y=(px)0 w=(px)160 h=(px)160 layout="flow" {
    rect id="rect.a" h=(px)30 fill=(token)"color.k"
    rect id="rect.b" h=(px)30 fill=(token)"color.k"
  }
}
  }
}
"##;
    let flow = compile(&parse(flow_src), &default_provider());
    let rects = fill_rects(&flow);
    assert_eq!(rects.len(), 2);
    // Stacked (flow), NOT side-by-side (grid would tile horizontally).
    assert_eq!(rects[0].0, rects[1].0, "flow children share x (stacked)");
    assert!(rects[1].1 > rects[0].1, "flow child2 below child1");
}

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
        "inst.2 label overridden to color.alt (red)"
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
  project id="proj.uc" name="UC"
  tokens format="zenith-token-v1" {}
  styles {}
  components {}
  document id="doc.uc" title="UC" {
page id="page.uc" w=(px)200 h=(px)200 {
  instance id="inst.bad" component="nonexistent.panel" x=(px)0 y=(px)0
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let unknown: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "scene.unknown_component")
        .collect();
    assert_eq!(
        unknown.len(),
        1,
        "expected 1 scene.unknown_component advisory; got: {:?}",
        result.diagnostics
    );

    // The instance emits NO commands (just the page PushClip/PopClip).
    let cmds = &result.scene.commands;
    assert_eq!(
        cmds.len(),
        2,
        "expected PushClip + PopClip only (instance skipped); got: {:?}",
        cmds
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

// ── Unknown node: subtree skipped, no commands, advisory emitted ───────

#[test]
fn unknown_node_with_children_emits_no_commands() {
    // An unrecognized `sparkle` node carries a real rect child. Because the
    // unknown parent's layout semantics are unknown, the WHOLE subtree is
    // skipped at compile time: NO scene commands are emitted for it or its
    // children, and the existing `scene.unsupported_node` advisory fires.
    let src = r##"zenith version=1 {
  project id="proj.uk" name="UK"
  tokens format="zenith-token-v1" {
token id="color.r" type="color" value="#ff0000"
  }
  styles {}
  document id="doc.uk" title="UK" {
page id="page.uk" w=(px)320 h=(px)200 {
  sparkle id="fx" {
    rect id="inner" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.r"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    // The unknown subtree is skipped: no fill commands for the node or its
    // rect child.
    let fills = fill_rects(&result);
    assert!(
        fills.is_empty(),
        "unknown node subtree must emit no FillRect commands; got: {fills:?}"
    );
    assert!(
        !result.scene.commands.iter().any(|c| matches!(
            c,
            SceneCommand::FillRect { .. } | SceneCommand::FillEllipse { .. }
        )),
        "unknown node subtree must emit no fill commands; got: {:?}",
        result.scene.commands
    );

    // The existing unsupported-node advisory must still fire.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "scene.unsupported_node"),
        "unknown node must emit the scene.unsupported_node advisory; got: {:?}",
        result.diagnostics
    );
}
