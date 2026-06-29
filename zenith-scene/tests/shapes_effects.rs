mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::{LineCap, Paint, SceneCommand};

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

#[test]
fn light_emits_radial_gradient_ellipse() {
    let src = r##"zenith version=1 {
  project id="proj.light" name="Light"
  tokens format="zenith-token-v1" {
token id="color.glow" type="color" value="#7cc7ff"
  }
  styles {}
  document id="doc.light" title="Light" {
page id="page.light" w=(px)300 h=(px)200 {
  light id="bg.glow" kind="ambient" x=(px)150 y=(px)80 radius=(px)40 color=(token)"color.glow" opacity=0.5
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let fill = result.scene.commands.iter().find_map(|cmd| match cmd {
        SceneCommand::FillEllipse {
            x,
            y,
            w,
            h,
            rx,
            ry,
            paint,
        } => Some((*x, *y, *w, *h, *rx, *ry, paint)),
        _ => None,
    });
    let (x, y, w, h, rx, ry, paint) = fill.expect("light must emit a FillEllipse");
    assert_eq!(
        (x, y, w, h, rx, ry),
        (110.0, 40.0, 80.0, 80.0, Some(40.0), Some(40.0))
    );
    let Paint::Gradient(gradient) = paint else {
        panic!("light paint must be a radial gradient");
    };
    assert!(gradient.radial, "light gradient must be radial");
    assert_eq!(gradient.stops.len(), 2);
    assert_eq!(gradient.stops[0].color.r, 0x7c);
    assert_eq!(gradient.stops[0].color.g, 0xc7);
    assert_eq!(gradient.stops[0].color.b, 0xff);
    assert_eq!(gradient.stops[0].color.a, 128);
    assert_eq!(gradient.stops[1].color.a, 0);
}

#[test]
fn mesh_orthographic_emits_deterministic_lines() {
    let src = r##"zenith version=1 {
  project id="proj.mesh" name="Mesh"
  tokens format="zenith-token-v1" {
token id="color.grid" type="color" value="#203040"
  }
  styles {}
  document id="doc.mesh" title="Mesh" {
page id="page.mesh" w=(px)300 h=(px)200 {
  mesh id="grid" kind="orthographic" x=(px)10 y=(px)20 w=(px)100 h=(px)80 rows=2 columns=4 extend=(px)5 stroke=(token)"color.grid" stroke-width=(px)2 opacity=0.5
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());
    let lines: Vec<&SceneCommand> = result
        .scene
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, SceneCommand::StrokeLine { .. }))
        .collect();
    assert_eq!(lines.len(), 8, "rows+1 plus columns+1 lines");
    match lines[0] {
        SceneCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            color,
            stroke_width,
            ..
        } => {
            assert_eq!((*x1, *y1, *x2, *y2), (5.0, 20.0, 115.0, 20.0));
            assert_eq!(*stroke_width, 2.0);
            assert_eq!(
                (color.r, color.g, color.b, color.a),
                (0x20, 0x30, 0x40, 128)
            );
        }
        other => panic!("expected first mesh command to be StrokeLine, got {other:?}"),
    }
    match lines[3] {
        SceneCommand::StrokeLine { x1, y1, x2, y2, .. } => {
            assert_eq!((*x1, *y1, *x2, *y2), (10.0, 15.0, 10.0, 105.0));
        }
        other => panic!("expected vertical mesh line, got {other:?}"),
    }
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
