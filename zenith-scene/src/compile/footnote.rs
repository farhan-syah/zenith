//! Page-level footnote system for book interiors.
//!
//! Footnotes are NON-flowing page furniture. Every [`FootnoteNode`](zenith_core::FootnoteNode)
//! that is a DIRECT child of a [`Page`] is:
//!
//! 1. collected in SOURCE order and auto-numbered `1..N` — a footnote that
//!    declares an explicit `marker` uses that string instead of a number but
//!    still occupies a numbering slot;
//! 2. mapped `footnote_id → marker_string` (an ordered [`BTreeMap`], so the
//!    inline-marker lookup in [`super::text::compile_text`] is deterministic);
//! 3. rendered, stacked, in a RESERVED zone at the bottom of the page just ABOVE
//!    the page's bottom margin, beneath a thin horizontal SEPARATOR RULE.
//!
//! Each footnote renders as its marker (a superscript prefix) followed by its
//! content spans, left-aligned and wrapped to the live-area width, at the
//! footnote's resolved style/font. Rendering is done by SYNTHESIZING a
//! [`TextNode`] and compiling it through [`super::text::compile_text`] — the
//! exact same shaping / wrap / super-subscript / height-measurement path a
//! normal text node uses (no duplicated typesetting).
//!
//! Reflow / overlap (v0): the zone is reserved bottom-up from the bottom margin;
//! v0 does NOT auto-reflow an explicit body box to make room. If a body node's
//! `y + h` crosses the zone top an advisory `footnote.body_overlap` names the
//! node (the author should shorten it). Footnote-aware auto-reflow of a flow
//! frame / chain is a documented v0 follow-up.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, FootnoteNode, Node, Page, ResolvedToken, Style, TextNode, TextSpan,
    dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::{Color, Paint, SceneCommand};

use super::RenderCtx;
use super::anchor::AnchorMap;
use super::chain::ChainAssignments;
use super::field::FieldCtx;
use super::paint::resolve_property_color;
use super::style_prop;
use super::text::{TextCompileEnv, compile_text};
use super::util::{px, resolve_property_dimension_px};

/// The gap (px) between stacked footnotes in the zone.
const FOOTNOTE_GAP: f64 = 6.0;
/// The vertical gap (px) between the separator rule and the first footnote.
const SEPARATOR_GAP: f64 = 10.0;
/// The separator rule thickness in px.
const SEPARATOR_THICKNESS: f64 = 1.0;
/// The separator rule width as a fraction of the live-area width (~1/3).
const SEPARATOR_WIDTH_FRACTION: f64 = 1.0 / 3.0;
/// Default footnote font size in px when neither node nor style declares one.
const DEFAULT_FOOTNOTE_FONT_SIZE: f64 = 13.0;
/// Default muted color for the separator rule when the footnote declares no
/// fill (a mid grey).
const DEFAULT_RULE_COLOR: Color = Color::srgb(136, 136, 136, 255);

/// Collect this page's footnote markers `footnote_id → marker_string` in SOURCE
/// order (auto-numbered `1..N`; an explicit `marker` overrides the number but
/// still occupies a slot). Only DIRECT page children count.
///
/// Returns an ordered [`BTreeMap`] for deterministic inline-marker lookup.
pub(super) fn collect_footnote_markers(page: &Page) -> BTreeMap<String, String> {
    let mut markers: BTreeMap<String, String> = BTreeMap::new();
    let mut number: u32 = 0;
    for child in &page.children {
        if let Node::Footnote(fnote) = child {
            number += 1;
            let marker = match &fnote.marker {
                Some(explicit) => explicit.clone(),
                None => number.to_string(),
            };
            // First declaration of an id wins (a duplicate id is flagged by the
            // validator); keep the earliest slot deterministically.
            markers.entry(fnote.id.clone()).or_insert(marker);
        }
    }
    markers
}

/// Synthesize the [`TextNode`] that renders one footnote at `(x, y)` with width
/// `w`: a superscript marker span followed by the footnote's content spans,
/// left-aligned and wrapped to `w`, carrying the footnote's resolved
/// style/fill/font. `y`/`h` are filled in by the caller per layout pass.
fn synth_footnote_text(fnote: &FootnoteNode, marker: &str, x: f64, y: f64, w: f64) -> TextNode {
    let mut spans: Vec<TextSpan> = Vec::with_capacity(fnote.spans.len() + 1);
    // Marker as a superscript prefix (reuses the vertical-align="super" path).
    spans.push(TextSpan {
        text: format!("{marker} "),
        fill: fnote.fill.clone(),
        font_weight: None,
        italic: None,
        underline: None,
        strikethrough: None,
        vertical_align: Some("super".to_owned()),
        footnote_ref: None,
        data_ref: None,
        data_format: None,
        highlight: None,
        code: None,
        link: None,
    });
    spans.extend(fnote.spans.iter().cloned());

    TextNode {
        id: fnote.id.clone(),
        name: fnote.name.clone(),
        role: fnote.role.clone(),
        x: Some(px(x)),
        y: Some(px(y)),
        w: Some(px(w)),
        h: None,
        align: Some("start".to_owned()),
        v_align: None,
        direction: None,
        // The zone is reserved bottom-up; the footnote text may exceed its (open)
        // box without a hard fail, so use "visible" to avoid an overflow warning.
        overflow: Some("visible".to_owned()),
        overflow_wrap: None,
        style: fnote.style.clone(),
        fill: fnote.fill.clone(),
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: fnote.font_family.clone(),
        font_size: fnote.font_size.clone(),
        font_size_min: None,
        font_weight: None,
        shadow: None,
        filter: None,
        mask: None,
        blend_mode: None,
        blur: None,
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        chain: None,
        drop_cap_lines: None,
        hyphenate: None,
        widow_orphan: None,
        tab_leader: None,
        text_exclusion: None,
        padding_left: None,
        text_indent: None,
        bullet: None,
        bullet_gap: None,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        spans,
        source_span: fnote.source_span,
        unknown_props: BTreeMap::new(),
    }
}

/// The shared, read-only typesetting environment for the footnote zone.
///
/// Bundles the (borrowed) inputs that do not vary across the measure/emit passes
/// — the token table, style map, font provider, shaping engine, chain/anchor
/// maps, the page's footnote markers, and the per-page field context — so the
/// zone entry point takes a small argument list. `Copy` because every field is a
/// shared reference (the borrows outlive the whole compile).
#[derive(Clone, Copy)]
pub(in crate::compile) struct FootnoteZoneEnv<'a> {
    pub(in crate::compile) markers: &'a BTreeMap<String, String>,
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    pub(in crate::compile) chains: &'a ChainAssignments,
    pub(in crate::compile) anchors: &'a AnchorMap,
    pub(in crate::compile) field_ctx: &'a FieldCtx<'a>,
}

/// Render the page's footnote zone: the separator rule plus the stacked,
/// auto-numbered footnotes, in the reserved area above the page's bottom margin.
///
/// `live_area` is the page live area `(x, y, w, h)` in AUTHORED coordinates; the
/// zone spans the live-area width and stacks upward from the live-area bottom.
/// `ctx` carries the bleed offset (so authored coords land in the trim box). When
/// the page has no footnotes, or no resolvable live area, nothing is emitted.
///
/// Reuses [`compile_text`] for typesetting + height measurement (a scratch
/// measure pass, then the real emit), so a footnote shapes byte-identically to a
/// normal wrapped text node.
pub(in crate::compile) fn compile_footnote_zone(
    page: &Page,
    live_area: Option<(f64, f64, f64, f64)>,
    env: FootnoteZoneEnv,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    let FootnoteZoneEnv {
        markers,
        resolved,
        style_map,
        fonts,
        engine,
        chains,
        anchors,
        field_ctx,
    } = env;
    // Collect the footnote nodes in source order (direct page children only).
    let footnotes: Vec<&FootnoteNode> = page
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Footnote(f) => Some(f),
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Frame(_)
            | Node::Group(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Toc(_)
            | Node::Table(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Unknown(_) => None,
        })
        .collect();
    if footnotes.is_empty() {
        return;
    }

    // The zone needs a live area to know the width to wrap to and the bottom
    // edge to stack from. Without one we cannot honestly place the zone.
    let Some((live_x, live_y, live_w, live_h)) = live_area else {
        diagnostics.push(Diagnostic::advisory(
            "footnote.no_live_area",
            format!(
                "page '{}' declares footnotes but has no resolvable live area \
                 (all four margins required); the footnote zone is skipped",
                page.id
            ),
            page.source_span,
            Some(page.id.clone()),
        ));
        return;
    };

    // Bottom edge of the live area in authored coords (where the bottom margin
    // begins); the footnote block sits just above it.
    let live_bottom = live_y + live_h;

    // ── Measure pass ──────────────────────────────────────────────────────
    // Compile each footnote into a scratch buffer at y = 0 to get its laid-out
    // content height (line_count × line_height), reusing the exact wrap +
    // measurement path the real emit uses. Heights are summed (plus gaps) to
    // place the zone top, then footnotes are emitted top-down from there.
    // Footnotes never use text-runaround exclusion, so the node-box map is empty.
    let empty_node_boxes: BTreeMap<String, (f64, f64, f64, f64)> = BTreeMap::new();
    let text_env = TextCompileEnv {
        resolved,
        style_map,
        fonts,
        engine,
        chains,
        footnote_markers: markers,
        node_boxes: &empty_node_boxes,
        anchors,
    };
    let mut heights: Vec<f64> = Vec::with_capacity(footnotes.len());
    for fnote in &footnotes {
        let marker = markers.get(&fnote.id).map(String::as_str).unwrap_or("?");
        let text = synth_footnote_text(fnote, marker, live_x, 0.0, live_w);
        let mut scratch: Vec<SceneCommand> = Vec::new();
        let mut scratch_diags: Vec<Diagnostic> = Vec::new();
        let h = compile_text(
            &text,
            text_env,
            &mut scratch,
            &mut scratch_diags,
            RenderCtx::measure(),
        );
        // A footnote that measured to zero height (e.g. all-empty content) still
        // reserves at least one line so its marker shows; fall back to the
        // resolved font size as a minimal line height.
        let h = if h > 0.0 {
            h
        } else {
            footnote_font_size(fnote, resolved, style_map)
        };
        heights.push(h);
    }

    let total_height: f64 =
        heights.iter().sum::<f64>() + FOOTNOTE_GAP * (footnotes.len().saturating_sub(1)) as f64;

    // Zone top (authored coords): the footnote block bottom sits at the live
    // bottom; the block extends upward by total_height.
    let zone_block_top = live_bottom - total_height;
    // The separator rule sits SEPARATOR_GAP above the block top.
    let separator_y = zone_block_top - SEPARATOR_GAP;
    // The whole zone (including the rule) begins here; body content above this y
    // is safe, content crossing it overlaps.
    let zone_top = separator_y - SEPARATOR_THICKNESS;

    // ── Body-overlap advisory ─────────────────────────────────────────────
    // A body node whose bottom edge (y + h) crosses the zone top overlaps the
    // reserved zone. v0 does NOT auto-reflow an explicit body box — warn so the
    // author shortens it. Only direct page children with a resolvable bbox are
    // checked (footnotes themselves are skipped).
    for child in &page.children {
        if matches!(child, Node::Footnote(_)) {
            continue;
        }
        if let Some((_bx, by, _bw, bh, id)) = node_bottom_box(child)
            && by + bh > zone_top
        {
            diagnostics.push(Diagnostic::advisory(
                "footnote.body_overlap",
                format!(
                    "body node '{}' (bottom y={:.0}) overlaps the footnote zone \
                         (top y={:.0}) on page '{}'; v0 does not auto-reflow an explicit \
                         body box — shorten the node",
                    id,
                    by + bh,
                    zone_top,
                    page.id
                ),
                page.source_span,
                Some(id),
            ));
        }
    }

    // ── Separator rule ────────────────────────────────────────────────────
    // A thin filled rect spanning ~1/3 the live-area width, left-aligned at the
    // live-area left edge, just above the footnote block. Color = the FIRST
    // footnote's resolved fill, else a muted default.
    let rule_w = live_w * SEPARATOR_WIDTH_FRACTION;
    let rule_color = footnotes
        .first()
        .and_then(|f| f.fill.as_ref())
        .and_then(|fp| resolve_property_color(fp, resolved, diagnostics, &page.id))
        .unwrap_or(DEFAULT_RULE_COLOR);
    commands.push(SceneCommand::FillRect {
        x: live_x + ctx.dx,
        y: separator_y + ctx.dy,
        w: rule_w,
        h: SEPARATOR_THICKNESS,
        paint: Paint::solid(rule_color),
    });

    // ── Emit pass: stack footnotes top-down from the block top ────────────
    let mut cursor_y = zone_block_top;
    for (fnote, h) in footnotes.iter().zip(heights.iter()) {
        let marker = markers.get(&fnote.id).map(String::as_str).unwrap_or("?");
        let text = synth_footnote_text(fnote, marker, live_x, cursor_y, live_w);
        compile_text(&text, text_env, commands, diagnostics, ctx);
        cursor_y += h + FOOTNOTE_GAP;
    }

    // `field_ctx` is unused here (footnote content never resolves fields) but is
    // accepted for signature symmetry with the rest of the compile pipeline.
    let _ = field_ctx;
}

/// Resolve a footnote's font size in px with style cascade (default
/// [`DEFAULT_FOOTNOTE_FONT_SIZE`]). Mirrors the text-node resolution.
fn footnote_font_size(
    fnote: &FootnoteNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
) -> f64 {
    let font_size_prop = fnote
        .font_size
        .clone()
        .or_else(|| style_prop(&fnote.style, style_map, "font-size").cloned());
    resolve_property_dimension_px(
        font_size_prop.as_ref(),
        resolved,
        DEFAULT_FOOTNOTE_FONT_SIZE,
    )
}

/// The authored `(x, y, w, h, id)` of a body node with a resolvable rectangular
/// box, or `None` for kinds without one. Used only for the overlap advisory, so
/// geometry-less kinds (line/polygon/polyline/group/instance/field/footnote/
/// unknown) yield `None` (no check).
fn node_bottom_box(node: &Node) -> Option<(f64, f64, f64, f64, String)> {
    let (x, y, w, h, id) = match node {
        Node::Rect(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Ellipse(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Text(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Code(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Image(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Frame(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Table(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Shape(n) => (&n.x, &n.y, &n.w, &n.h, &n.id),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Group(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Unknown(_) => return None,
    };
    let xd = x.as_ref()?;
    let yd = y.as_ref()?;
    let wd = w.as_ref()?;
    let hd = h.as_ref()?;
    Some((
        dim_to_px(xd.value, &xd.unit)?,
        dim_to_px(yd.value, &yd.unit)?,
        dim_to_px(wd.value, &wd.unit)?,
        dim_to_px(hd.value, &hd.unit)?,
        id.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::collect_footnote_markers;
    use zenith_core::{KdlAdapter, KdlSource};

    /// Two page-level footnotes auto-number `1` and `2` in source order, keyed by
    /// their ids in the ordered marker map. This is a UNIT test, not an
    /// integration test: `collect_footnote_markers` is `pub(super)`, so the marker
    /// NUMBERS can only be asserted with crate-internal access (the public scene
    /// exposes the rendered glyphs, not the marker strings).
    #[test]
    fn two_footnotes_auto_number_one_and_two() {
        let src = br##"zenith version=1 {
  project id="proj.fn2" name="FN2"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.fn2" title="FN2" {
    page id="page.fn2" w=(px)600 h=(px)900 {
      text id="body" x=(px)60 y=(px)80 w=(px)480 h=(px)200 {
        span "First mark" footnote-ref="fn.1"
        span " and second mark" footnote-ref="fn.2"
      }
      footnote id="fn.1" { span "First note." }
      footnote id="fn.2" { span "Second note." }
    }
  }
}
"##;
        let doc = KdlAdapter.parse(src).expect("test document must parse");
        let page = doc.body.pages.first().expect("one page");
        let markers = collect_footnote_markers(page);
        assert_eq!(markers.get("fn.1").map(String::as_str), Some("1"));
        assert_eq!(markers.get("fn.2").map(String::as_str), Some("2"));
        assert_eq!(markers.len(), 2);
    }
}
