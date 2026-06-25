//! Integration tests for SINGLE-BOX block-level markdown layout.
//!
//! A non-chained `text format="markdown"` node renders its content as
//! VERTICALLY STACKED, individually styled blocks (headings, paragraphs,
//! blockquotes, list items, code blocks, horizontal rules) resolved through the
//! `block role="…"` cascade (node > page > document).
//!
//! Coverage:
//! 1. A multi-block node (h1 + two paragraphs) with block decls stacks at
//!    increasing y, the heading run uses the h1 font-size, paragraphs sit in
//!    distinct y bands.
//! 2. A fenced code block emits a background FillRect + mono glyphs.
//! 3. A horizontal rule emits a thin FillRect rule.
//! 4. BYTE-IDENTITY: a single-paragraph markdown node with NO block decls
//!    produces the same glyph-run stream (font-size, x, y, count) as a
//!    non-markdown equivalent.

mod common;
use common::*;
use zenith_core::default_provider;
use zenith_scene::ir::SceneCommand;

/// All `DrawGlyphRun` records as `(x, y, font_size)` in emission order.
fn glyph_run_rows(result: &CompileResult) -> Vec<(f64, f64, f32)> {
    result
        .scene
        .commands
        .iter()
        .filter_map(|c| match c {
            SceneCommand::DrawGlyphRun {
                x, y, font_size, ..
            } => Some((*x, *y, *font_size)),
            _ => None,
        })
        .collect()
}

// ── Test 1: multi-block stack with cascade styling ──────────────────────────

#[test]
fn multi_block_stacks_with_styled_heading_and_separated_paragraphs() {
    // h1 + two paragraphs. Block decls set the h1 font-size to 40 and the p
    // font-size to 20, so the heading run is clearly larger and the paragraphs
    // land in distinct y bands below it.
    let src = r##"zenith version=1 {
  project id="proj.mb" name="MB"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#111827"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension" value=(px)20
    token id="size.h1"   type="dimension" value=(px)40
  }
  styles {}
  document id="doc.mb" title="MB" {
    page id="page.mb" w=(px)600 h=(px)400 {
      text id="t.mb" x=(px)20 y=(px)20 w=(px)560 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" format="markdown" {
        block role="h1" font-size=(token)"size.h1"
        span "# Title\n\nFirst paragraph here.\n\nSecond paragraph here."
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != zenith_core::Severity::Error),
        "expected no errors; got: {:?}",
        result.diagnostics
    );

    let rows = glyph_run_rows(&result);
    assert!(
        rows.len() >= 3,
        "expected >= 3 glyph runs (heading + 2 paragraphs); got {}",
        rows.len()
    );

    // The first run is the heading and must use the h1 font-size (40px).
    let (_, heading_y, heading_fs) = rows[0];
    assert!(
        (heading_fs - 40.0).abs() < 0.5,
        "heading run must use the h1 font-size 40; got {heading_fs}"
    );

    // Paragraph runs use the smaller body size and sit BELOW the heading.
    let body_runs: Vec<_> = rows
        .iter()
        .filter(|(_, _, fs)| (*fs - 20.0).abs() < 0.5)
        .collect();
    assert!(
        body_runs.len() >= 2,
        "expected >= 2 body-size paragraph runs; got {}",
        body_runs.len()
    );

    // Distinct y bands: heading above first paragraph above second paragraph.
    let first_p_y = body_runs[0].1;
    let second_p_y = body_runs[body_runs.len() - 1].1;
    assert!(
        heading_y < first_p_y,
        "heading (y={heading_y}) must sit above first paragraph (y={first_p_y})"
    );
    assert!(
        first_p_y < second_p_y,
        "first paragraph (y={first_p_y}) must sit above second paragraph (y={second_p_y})"
    );
}

// ── Test 2: code block → background FillRect + mono glyphs ───────────────────

#[test]
fn code_block_emits_background_rect_and_glyphs() {
    let src = r##"zenith version=1 {
  project id="proj.cb" name="CB"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#111827"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension" value=(px)18
  }
  styles {}
  document id="doc.cb" title="CB" {
    page id="page.cb" w=(px)600 h=(px)400 {
      text id="t.cb" x=(px)20 y=(px)20 w=(px)560 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" format="markdown" {
        span "```\nlet x = 1;\n```"
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != zenith_core::Severity::Error),
        "expected no errors; got: {:?}",
        result.diagnostics
    );

    let rects = fill_rects(&result);
    assert!(
        !rects.is_empty(),
        "code block must emit at least one background FillRect"
    );
    let glyph_runs = glyph_run_rows(&result);
    assert!(
        !glyph_runs.is_empty(),
        "code block must emit mono glyph runs for its content"
    );
}

// ── Test 3: horizontal rule → thin FillRect ─────────────────────────────────

#[test]
fn horizontal_rule_emits_rule_fill_rect() {
    let src = r##"zenith version=1 {
  project id="proj.hr" name="HR"
  tokens format="zenith-token-v1" {
    token id="color.ink" type="color" value="#111827"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension" value=(px)18
  }
  styles {}
  document id="doc.hr" title="HR" {
    page id="page.hr" w=(px)600 h=(px)400 {
      text id="t.hr" x=(px)20 y=(px)20 w=(px)560 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body" format="markdown" {
        span "Above\n\n---\n\nBelow"
      }
    }
  }
}"##;
    let doc = parse(src);
    let result = compile(&doc, &default_provider());

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != zenith_core::Severity::Error),
        "expected no errors; got: {:?}",
        result.diagnostics
    );

    // The rule is a thin, full-width FillRect between the two paragraphs.
    let rects = fill_rects(&result);
    let thin_full_width = rects.iter().any(|(x, _, w, h)| {
        (*x - 20.0).abs() < 1.0 && (*w - 560.0).abs() < 1.0 && *h <= 4.0 && *h > 0.0
    });
    assert!(
        thin_full_width,
        "horizontal rule must emit a thin full-width FillRect; rects: {rects:?}"
    );
}

// ── Test 4: byte-identity for a lone paragraph with no block decls ──────────

#[test]
fn single_paragraph_no_block_decls_matches_plain_text() {
    // A markdown node whose content is one plain paragraph, with NO block decls,
    // must produce the SAME glyph-run stream (x / y / font-size / count) as a
    // non-markdown text node carrying the identical text.
    let body = "Just a single ordinary paragraph of text that wraps a little.";
    let make = |fmt: &str| {
        format!(
            r##"zenith version=1 {{
  project id="proj.bi" name="BI"
  tokens format="zenith-token-v1" {{
    token id="color.ink" type="color" value="#111827"
    token id="font.body" type="fontFamily" value="Noto Sans"
    token id="size.body" type="dimension" value=(px)22
  }}
  styles {{}}
  document id="doc.bi" title="BI" {{
    page id="page.bi" w=(px)400 h=(px)300 {{
      text id="t.bi" x=(px)15 y=(px)25 w=(px)360 fill=(token)"color.ink" font-family=(token)"font.body" font-size=(token)"size.body"{fmt} {{
        span "{body}"
      }}
    }}
  }}
}}"##
        )
    };

    let md = compile(&parse(&make(r#" format="markdown""#)), &default_provider());
    let plain = compile(&parse(&make("")), &default_provider());

    let md_rows = glyph_run_rows(&md);
    let plain_rows = glyph_run_rows(&plain);

    assert_eq!(
        md_rows, plain_rows,
        "single-paragraph markdown block layout must match the plain-text glyph stream"
    );
}
