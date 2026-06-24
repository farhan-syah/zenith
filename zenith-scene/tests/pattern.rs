//! Integration tests for `pattern` node scene-compile expansion.
//!
//! A `pattern` lays out copies of its `motif` template across its bounds:
//! `grid` tiles on a fixed `spacing` lattice (optionally jittered), `scatter`
//! places `count` seed-derived copies. Each instance is the motif compiled with
//! a translation offset; the bounds box is emitted as a clip around all
//! instances. Placement is fully deterministic.

mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::SceneCommand;

// ── Document wrapper ──────────────────────────────────────────────────────────

/// Wrap a single page child (raw KDL) in a minimal document on a 400×300 page.
fn doc_with_node(node_kdl: &str) -> String {
    format!(
        r##"zenith version=1 {{
  project id="proj.pat" name="Pattern"
  tokens format="zenith-token-v1" {{}}
  styles {{}}
  document id="doc.pat" title="Pattern" {{
page id="page.pat" w=(px)400 h=(px)300 {{
  {node_kdl}
}}
  }}
}}"##
    )
}

/// Collect every `FillEllipse` command's `(x, y, w, h)` in emission order.
fn fill_ellipses(result: &CompileResult) -> Vec<(f64, f64, f64, f64)> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::FillEllipse { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

/// Approximate-equality of two `(x, y, w, h)` tuples.
fn close(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6
        && (a.1 - b.1).abs() < 1e-6
        && (a.2 - b.2).abs() < 1e-6
        && (a.3 - b.3).abs() < 1e-6
}

// ── Grid: exact coordinates, no jitter ────────────────────────────────────────

#[test]
fn grid_exact_coordinates_no_jitter() {
    // Bounds x=0 y=0 w=100 h=100, spacing=50, jitter absent.
    // Cells where row*50 < 100 and col*50 < 100 → rows {0,1}, cols {0,1}.
    // Cell origins (row-major): (0,0),(50,0),(0,50),(50,50).
    // Motif ellipse authored at (0,0,10,10) → instance box is at the cell origin.
    let src = doc_with_node(
        r##"pattern id="pat.grid" kind="grid" x=(px)0 y=(px)0 w=(px)100 h=(px)100 spacing=(px)50 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill="#ff0000"
    }"##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());

    let ellipses = fill_ellipses(&result);
    assert_eq!(
        ellipses.len(),
        4,
        "expected 4 grid instances; got {ellipses:?}"
    );
    for expected in [
        (0.0, 0.0, 10.0, 10.0),
        (50.0, 0.0, 10.0, 10.0),
        (0.0, 50.0, 10.0, 10.0),
        (50.0, 50.0, 10.0, 10.0),
    ] {
        assert!(
            ellipses.iter().any(|&e| close(e, expected)),
            "expected an ellipse at {expected:?}; got {ellipses:?}"
        );
    }
}

// ── Grid: bounds-origin offset translates every instance ──────────────────────

#[test]
fn grid_bounds_origin_offset() {
    // Bounds x=20 y=30, same 100×100 / spacing 50 lattice → 4 cells, every
    // instance translated by (20, 30): (20,30),(70,30),(20,80),(70,80).
    let src = doc_with_node(
        r##"pattern id="pat.off" kind="grid" x=(px)20 y=(px)30 w=(px)100 h=(px)100 spacing=(px)50 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill="#00ff00"
    }"##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());

    let ellipses = fill_ellipses(&result);
    assert_eq!(ellipses.len(), 4, "expected 4 instances; got {ellipses:?}");
    for expected in [
        (20.0, 30.0, 10.0, 10.0),
        (70.0, 30.0, 10.0, 10.0),
        (20.0, 80.0, 10.0, 10.0),
        (70.0, 80.0, 10.0, 10.0),
    ] {
        assert!(
            ellipses.iter().any(|&e| close(e, expected)),
            "expected an ellipse at {expected:?}; got {ellipses:?}"
        );
    }
}

// ── Scatter: exact count, deterministic positions ─────────────────────────────

#[test]
fn scatter_count_and_determinism() {
    let src = doc_with_node(
        r##"pattern id="pat.sc" kind="scatter" x=(px)0 y=(px)0 w=(px)200 h=(px)200 count=5 seed=7 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)8 h=(px)8 fill="#0000ff"
    }"##,
    );
    let doc = parse(&src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());

    let e1 = fill_ellipses(&r1);
    assert_eq!(e1.len(), 5, "expected 5 scatter instances; got {e1:?}");
    // Every instance lands inside the bounds box.
    for (x, y, _, _) in &e1 {
        assert!(
            *x >= 0.0 && *x < 200.0 && *y >= 0.0 && *y < 200.0,
            "scatter instance ({x},{y}) escaped bounds"
        );
    }
    // Reproducible: two compiles of the same doc give identical command vectors.
    assert_eq!(
        r1.scene.commands, r2.scene.commands,
        "scatter must be deterministic across compiles"
    );
}

// ── Determinism: a grid doc compiles to identical command vectors ─────────────

#[test]
fn grid_deterministic() {
    let src = doc_with_node(
        r##"pattern id="pat.det" kind="grid" x=(px)5 y=(px)5 w=(px)120 h=(px)80 spacing=(px)25 jitter=0.4 seed=11 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)6 h=(px)6 fill="#222222"
    }"##,
    );
    let doc = parse(&src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());
    assert_eq!(
        r1.scene.commands, r2.scene.commands,
        "jittered grid must be byte-identical across compiles"
    );
    // Jitter is active but instances still exist.
    assert!(!fill_ellipses(&r1).is_empty());
}

// ── Clip: a clip of the bounds box brackets the instances ─────────────────────

#[test]
fn clip_brackets_instances() {
    let src = doc_with_node(
        r##"pattern id="pat.clip" kind="grid" x=(px)10 y=(px)20 w=(px)100 h=(px)50 spacing=(px)50 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill="#ff00ff"
    }"##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Find the pattern's bounds clip: PushClip at (10, 20, 100, 50).
    let push_idx = cmds.iter().position(|c| {
        matches!(
            c,
            SceneCommand::PushClip { x, y, w, h }
                if (*x - 10.0).abs() < 1e-6
                    && (*y - 20.0).abs() < 1e-6
                    && (*w - 100.0).abs() < 1e-6
                    && (*h - 50.0).abs() < 1e-6
        )
    });
    let push_idx = push_idx.expect("expected a PushClip of the pattern bounds box");

    // A matching PopClip follows, with at least one instance draw between them.
    let pop_idx = cmds[push_idx + 1..]
        .iter()
        .position(|c| matches!(c, SceneCommand::PopClip))
        .map(|p| push_idx + 1 + p)
        .expect("expected a PopClip after the bounds PushClip");

    let inner = &cmds[push_idx + 1..pop_idx];
    assert!(
        inner
            .iter()
            .any(|c| matches!(c, SceneCommand::FillEllipse { .. })),
        "expected motif instances inside the bounds clip"
    );
}

// ── Background panel: a fill paints behind the clipped motif tiling ───────────

#[test]
fn background_fill_paints_before_clip() {
    // The pattern carries its OWN `fill` → a FillRect of the bounds box is
    // emitted BEFORE the bounds PushClip (so the panel sits behind the tiling
    // and any stroke outline is not clipped).
    let src = doc_with_node(
        r##"pattern id="pat.bg" kind="grid" x=(px)10 y=(px)20 w=(px)100 h=(px)50 spacing=(px)50 fill="#102030" {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill="#ffffff"
    }"##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    // Index of the bounds PushClip.
    let push_idx = cmds
        .iter()
        .position(|c| {
            matches!(
                c,
                SceneCommand::PushClip { x, y, w, h }
                    if (*x - 10.0).abs() < 1e-6
                        && (*y - 20.0).abs() < 1e-6
                        && (*w - 100.0).abs() < 1e-6
                        && (*h - 50.0).abs() < 1e-6
            )
        })
        .expect("expected a PushClip of the pattern bounds box");

    // A FillRect of the bounds box must precede the clip.
    let fill_idx = cmds
        .iter()
        .position(|c| {
            matches!(
                c,
                SceneCommand::FillRect { x, y, w, h, .. }
                    if (*x - 10.0).abs() < 1e-6
                        && (*y - 20.0).abs() < 1e-6
                        && (*w - 100.0).abs() < 1e-6
                        && (*h - 50.0).abs() < 1e-6
            )
        })
        .expect("expected a FillRect of the pattern bounds box (background panel)");
    assert!(
        fill_idx < push_idx,
        "background fill must be emitted before the bounds clip"
    );
}

#[test]
fn no_background_without_fill_or_stroke() {
    // A pattern WITHOUT fill/stroke emits no background panel: the first command
    // for the pattern is the bounds PushClip, exactly as before (byte-identical).
    let src = doc_with_node(
        r##"pattern id="pat.plain" kind="grid" x=(px)10 y=(px)20 w=(px)100 h=(px)50 spacing=(px)50 {
      ellipse id="dot" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill="#ffffff"
    }"##,
    );
    let doc = parse(&src);
    let result = compile(&doc, &default_provider());
    let cmds = &result.scene.commands;

    let push_idx = cmds
        .iter()
        .position(|c| {
            matches!(
                c,
                SceneCommand::PushClip { x, y, w, h }
                    if (*x - 10.0).abs() < 1e-6
                        && (*y - 20.0).abs() < 1e-6
                        && (*w - 100.0).abs() < 1e-6
                        && (*h - 50.0).abs() < 1e-6
            )
        })
        .expect("expected a PushClip of the pattern bounds box");

    // No bounds-box FillRect / FillRoundedRect / StrokeRect / StrokeRoundedRect
    // exists anywhere before the clip.
    let has_panel = cmds[..push_idx].iter().any(|c| {
        matches!(
            c,
            SceneCommand::FillRect { x, y, w, h, .. }
            | SceneCommand::FillRoundedRect { x, y, w, h, .. }
            | SceneCommand::StrokeRect { x, y, w, h, .. }
            | SceneCommand::StrokeRoundedRect { x, y, w, h, .. }
                if (*x - 10.0).abs() < 1e-6
                    && (*y - 20.0).abs() < 1e-6
                    && (*w - 100.0).abs() < 1e-6
                    && (*h - 50.0).abs() < 1e-6
        )
    });
    assert!(
        !has_panel,
        "a pattern without fill/stroke must emit no background panel"
    );
}

// ── Absent pattern: byte-identical compile (sanity) ───────────────────────────

#[test]
fn absent_pattern_unaffected() {
    // A doc with no pattern compiles the same with the new code path present.
    let src =
        doc_with_node(r##"ellipse id="plain" x=(px)10 y=(px)10 w=(px)20 h=(px)20 fill="#abcdef""##);
    let doc = parse(&src);
    let r1 = compile(&doc, &default_provider());
    let r2 = compile(&doc, &default_provider());
    assert_eq!(r1.scene.commands, r2.scene.commands);
    // Exactly one ellipse, at its authored position.
    let ellipses = fill_ellipses(&r1);
    assert_eq!(ellipses.len(), 1);
    assert!(close(ellipses[0], (10.0, 10.0, 20.0, 20.0)));
}
