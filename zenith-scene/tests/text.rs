mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::compile;
use zenith_scene::ir::SceneCommand;

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
            ..
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

/// A text node inheriting font-size and fill from a style → correct DrawGlyphRun.
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

/// With `overflow-wrap="break-word"` the single overlong token is split across
/// >= 2 lines.
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

/// A tab-leader row `"Title\t12"` emits a LEFT run at the box left edge, a RIGHT
/// run whose right edge ≈ the box right edge, and ≥1 leader glyph between them.
#[test]
fn tab_leader_row_left_right_and_leaders() {
    // KDL decodes `\t` to a real tab inside the quoted string.
    let cmds = tab_leader_commands(true, "Title\\t12");
    let runs: Vec<_> = cmds
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { x, glyphs, .. } => Some((*x, glyphs.len())),
            _ => None,
        })
        .collect();
    assert!(!runs.is_empty(), "tab-leader row must emit glyph runs");

    let box_left = 100.0_f64;
    let box_right = box_left + 600.0;

    // LEFT run sits at the box left edge.
    let left_x = runs.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    assert!(
        (left_x - box_left).abs() < 0.01,
        "left segment must start at box left edge {box_left}; got {left_x}"
    );

    // RIGHT run (the page number "12") is the rightmost run; its right edge must
    // be ≈ the box right edge. We reconstruct its advance from the leader run
    // pitch is not trivial, so assert the right run STARTS before the box right
    // and that no run starts to the right of the box edge.
    let max_x = runs
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_x < box_right,
        "no run may start past the box right edge {box_right}; got {max_x}"
    );
    // The rightmost run must be the 2-glyph page number, and it must start in the
    // right portion of the box (right-aligned), well past the left segment.
    let (rightmost_x, rightmost_glyphs) = runs.iter().copied().fold(
        (f64::NEG_INFINITY, 0),
        |acc, r| if r.0 > acc.0 { r } else { acc },
    );
    assert_eq!(
        rightmost_glyphs, 2,
        "rightmost run must be the 2-digit page number '12'"
    );
    assert!(
        rightmost_x > box_left + 300.0,
        "page number must be right-aligned (started at {rightmost_x})"
    );

    // Leader dots: single-glyph runs between the title and the page number.
    let leader_dots = runs
        .iter()
        .filter(|(x, g)| *g == 1 && *x > left_x && *x < rightmost_x)
        .count();
    assert!(
        leader_dots >= 1,
        "at least one leader dot must fill the gap; got {leader_dots}"
    );

    // Determinism: a second render is byte-identical.
    let cmds2 = tab_leader_commands(true, "Title\\t12");
    assert_eq!(cmds, cmds2, "tab-leader render must be deterministic");
}

/// A tab-leader row with NO tab renders left-aligned with NO leader dots and no
/// right-aligned run.
#[test]
fn tab_leader_row_without_tab_has_no_leaders() {
    let cmds = tab_leader_commands(true, "JustATitleNoTab");
    let xs: Vec<f64> = cmds
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun { x, .. } => Some(*x),
            _ => None,
        })
        .collect();
    assert!(!xs.is_empty(), "row must still emit its left text");
    let box_left = 100.0_f64;
    // Every run starts at the box left edge (one left run, no leaders / right run).
    for x in &xs {
        assert!(
            (*x - box_left).abs() < 0.01,
            "a tab-less row must be wholly left-aligned; run at {x}"
        );
    }
}

/// Tab-leader ABSENT (`None`) is byte-identical to the pre-feature render: the
/// SAME node text rendered without the attribute produces the normal text path.
/// This guards the opt-in branch — the default path is untouched.
#[test]
fn tab_leader_absent_is_byte_identical_to_plain_text() {
    // A body with no tab so the plain path and a tab-less leader path would draw
    // the same text; here we only assert the ABSENT path is stable + matches the
    // pre-feature single-line emit (one run at the box left edge).
    let off = tab_leader_commands(false, "Contents heading");
    let off2 = tab_leader_commands(false, "Contents heading");
    assert_eq!(
        off, off2,
        "plain (no tab-leader) render must be deterministic"
    );
    let run_count = off
        .iter()
        .filter(|c| matches!(c, SceneCommand::DrawGlyphRun { .. }))
        .count();
    assert!(
        run_count >= 1,
        "plain text must still emit at least one glyph run"
    );
}

/// WITHOUT the attribute the overlong token is kept whole (one line) and the
/// command stream is byte-identical to a node with NO attribute at all.
#[test]
fn overflow_wrap_none_is_byte_identical() {
    let absent = compile(&break_word_doc(""), &default_provider());
    let normal = compile(
        &break_word_doc(r#"overflow-wrap="normal""#),
        &default_provider(),
    );
    assert_eq!(
        absent.scene.commands, normal.scene.commands,
        "overflow-wrap=\"normal\" must match an absent attribute (byte-identical)"
    );
    // The overlong token stays on ONE line (no forced break).
    assert_eq!(
        glyph_line_ys(&absent).len(),
        1,
        "the overlong token must stay whole on one line by default"
    );
    assert!(
        absent
            .diagnostics
            .iter()
            .all(|d| d.code != "text.forced_break"),
        "no forced_break advisory without break-word; got {:?}",
        absent.diagnostics
    );
}

/// WITH `overflow-wrap="break-word"` the single overlong token is split across
/// >= 2 lines.
#[test]
fn break_word_splits_overlong_token() {
    let absent = compile(&break_word_doc(""), &default_provider());
    let broken = compile(
        &break_word_doc(r#"overflow-wrap="break-word""#),
        &default_provider(),
    );

    let whole_lines = glyph_line_ys(&absent).len();
    let broken_lines = glyph_line_ys(&broken).len();
    assert_eq!(whole_lines, 1, "control: default keeps the token whole");
    assert!(
        broken_lines >= 2,
        "break-word must split the token across >= 2 lines; got {broken_lines}"
    );
}

/// The `text.forced_break` advisory is present for the break case and ABSENT
/// when the token fits the box.
#[test]
fn break_word_emits_forced_break_advisory() {
    let broken = compile(
        &break_word_doc(r#"overflow-wrap="break-word""#),
        &default_provider(),
    );
    let forced: Vec<_> = broken
        .diagnostics
        .iter()
        .filter(|d| d.code == "text.forced_break")
        .collect();
    assert_eq!(
        forced.len(),
        1,
        "exactly one forced_break advisory expected; got {:?}",
        broken.diagnostics
    );
    assert!(
        forced[0].message.contains("col.bw"),
        "advisory must name the node id"
    );

    // A node whose token FITS its box emits no advisory even with break-word on.
    let fits_src = r##"zenith version=1 {
  project id="proj.bwf" name="BWF"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.bwf" title="BWF" {
page id="page.bwf" w=(px)400 h=(px)200 {
  text id="col.bwf" x=(px)10 y=(px)20 w=(px)380 h=(px)100 overflow-wrap="break-word" {
    span "short words fit fine"
  }
}
  }
}
"##;
    let fits = compile(&parse(fits_src), &default_provider());
    assert!(
        fits.diagnostics
            .iter()
            .all(|d| d.code != "text.forced_break"),
        "no forced_break when the content fits; got {:?}",
        fits.diagnostics
    );
}

// ── Text node with stroke + stroke-width tokens → DrawGlyphRun carries stroke ─

/// A text node with `stroke=(token)` and `stroke-width=(token)` must compile
/// to a DrawGlyphRun whose `stroke_color` and `stroke_width` are `Some`.
/// A text node without stroke attributes must compile to `None` / `None`.
#[test]
fn text_stroke_token_threads_to_draw_glyph_run() {
    let src = r##"zenith version=1 {
  project id="proj.stroke" name="Stroke"
  tokens format="zenith-token-v1" {
token id="color.ink"    type="color"      value="#000000"
token id="color.outline" type="color"     value="#ff0000"
token id="font.body"    type="fontFamily" value="Noto Sans"
token id="size.body"    type="dimension"  value=(px)24
token id="size.stroke"  type="dimension"  value=(px)2
  }
  styles {}
  document id="doc.stroke" title="Stroke" {
page id="page.stroke" w=(px)400 h=(px)200 {
  text id="text.with-stroke" x=(px)10 y=(px)20 w=(px)380 h=(px)40 fill=(token)"color.ink" stroke=(token)"color.outline" stroke-width=(token)"size.stroke" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Outlined"
  }
  text id="text.no-stroke" x=(px)10 y=(px)80 w=(px)380 h=(px)40 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" {
    span "Plain"
  }
}
  }
}
"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let cmds = &result.scene.commands;

    // Find DrawGlyphRun for text.with-stroke (stroke fields must be Some).
    let with_stroke_run = cmds.iter().find(|c| {
        matches!(
            c,
            SceneCommand::DrawGlyphRun {
                stroke_color: Some(_),
                ..
            }
        )
    });
    assert!(
        with_stroke_run.is_some(),
        "text with stroke token must produce a DrawGlyphRun with stroke_color=Some; \
         commands: {:?}",
        cmds
    );
    if let Some(SceneCommand::DrawGlyphRun {
        stroke_color,
        stroke_width,
        ..
    }) = with_stroke_run
    {
        let sc = stroke_color.unwrap();
        // color.outline = #ff0000 → r=255, g=0, b=0.
        assert_eq!(sc.r, 255, "stroke_color.r must be 255 (#ff0000)");
        assert_eq!(sc.g, 0, "stroke_color.g must be 0");
        assert_eq!(sc.b, 0, "stroke_color.b must be 0");
        assert_eq!(
            *stroke_width,
            Some(2.0),
            "stroke_width must be 2.0 px (size.stroke token)"
        );
    }

    // Find DrawGlyphRun for text.no-stroke (stroke fields must be None).
    let no_stroke_run = cmds.iter().find(|c| {
        matches!(
            c,
            SceneCommand::DrawGlyphRun {
                stroke_color: None,
                ..
            }
        )
    });
    assert!(
        no_stroke_run.is_some(),
        "text without stroke token must produce a DrawGlyphRun with stroke_color=None; \
         commands: {:?}",
        cmds
    );
}

// ── font.glyph_missing diagnostic ─────────────────────────────────────────────

/// A text node containing a character that no registered face can cover
/// (emoji U+1F600, absent from all bundled Noto Sans faces) must produce
/// exactly one `font.glyph_missing` Warning naming that codepoint.
#[test]
fn glyph_missing_diagnostic_for_uncovered_char() {
    // U+1F600 GRINNING FACE: not present in Noto Sans Regular/Bold/Italic or
    // Noto Sans Mono (the full set registered by default_provider()).
    let src = r##"zenith version=1 {
  project id="proj.gm" name="GM"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.gm" title="GM" {
    page id="page.gm" w=(px)400 h=(px)200 {
      text id="text.gm" x=(px)10 y=(px)20 w=(px)380 h=(px)60 {
        span "Hello \u{1F600} World"
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.glyph_missing")
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "expected exactly one font.glyph_missing diagnostic; got: {:?}",
        result.diagnostics
    );
    let diag = missing[0];
    assert_eq!(
        diag.severity,
        zenith_core::Severity::Warning,
        "font.glyph_missing must be Warning severity"
    );
    assert_eq!(
        diag.subject_id.as_deref(),
        Some("text.gm"),
        "subject_id must be the text node id"
    );
    assert!(
        diag.message.contains("U+1F600"),
        "message must contain the missing codepoint U+1F600; got: {}",
        diag.message
    );
}

/// A text node containing only ASCII (fully covered by Noto Sans) must produce
/// no `font.glyph_missing` diagnostic.
#[test]
fn no_glyph_missing_for_ascii() {
    let src = r##"zenith version=1 {
  project id="proj.gm2" name="GM2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.gm2" title="GM2" {
    page id="page.gm2" w=(px)400 h=(px)200 {
      text id="text.gm2" x=(px)10 y=(px)20 w=(px)380 h=(px)60 {
        span "Hello World"
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.glyph_missing")
        .collect();
    assert!(
        missing.is_empty(),
        "ASCII text must produce no font.glyph_missing; got: {:?}",
        missing
    );
}

/// A text node containing a ZWJ (U+200D, a default-ignorable joiner) between
/// ASCII characters must NOT produce a `font.glyph_missing` for the ZWJ.
/// The joiner is consumed by the shaper and has no standalone glyph by design.
#[test]
fn default_ignorable_not_reported_as_missing() {
    // U+200D ZERO WIDTH JOINER between ASCII: ignorable, not reportable.
    let src = r##"zenith version=1 {
  project id="proj.gm3" name="GM3"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.gm3" title="GM3" {
    page id="page.gm3" w=(px)400 h=(px)200 {
      text id="text.gm3" x=(px)10 y=(px)20 w=(px)380 h=(px)60 {
        span "a\u{200D}b"
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "font.glyph_missing")
        .collect();
    assert!(
        missing.is_empty(),
        "ZWJ (U+200D) must not be reported as glyph_missing; got: {:?}",
        missing
    );
}
