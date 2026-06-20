//! Scene compilation: `Document` → `CompileResult`.
//!
//! Entry point: [`compile`].
//!
//! Rect, ellipse, line, text, code, and group nodes are compiled; the page
//! background is emitted first; unknown nodes produce an advisory diagnostic
//! and are skipped.
//!
//! [`compile`] renders page 0; [`compile_page`] renders a chosen page by index.
//!
//! The compiler is split across submodules: [`leaf`] (rect/ellipse/line/
//! polygon/polyline), [`text`] (text + code shaping), [`container`] (group +
//! frame), [`image`], [`paint`] (color/gradient/shadow resolvers), and
//! [`util`] (small geometry/diagnostic helpers). This module keeps the public
//! entry points, the per-subtree [`RenderCtx`], and the [`compile_node`]
//! dispatcher that routes each node kind to its submodule.

mod chain;
mod container;
mod crop;
mod field;
mod footnote;
mod image;
mod leaf;
mod paint;
mod text;
mod util;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use zenith_core::{
    ComponentDef, Diagnostic, Document, FontProvider, MasterDef, Node, PropertyValue,
    ResolvedToken, Style, dim_to_px, resolve_tokens,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::{Rect, Scene, SceneCommand};

use chain::{ChainAssignments, resolve_chains_document};
use container::{compile_frame, compile_group, compile_instance};
use field::{
    FieldCtx, build_node_boxes, build_page_index_map, compute_live_area, resolve_field_to_text,
};
use image::compile_image;
use leaf::{compile_ellipse, compile_line, compile_polygon, compile_polyline, compile_rect};
use paint::{resolve_property_color, resolve_property_gradient};
use text::{compile_code, compile_text};

/// Compile-time lookup of component definitions by id. Threaded through the
/// node-compilation dispatch so [`Node::Instance`] can expand its referenced
/// component subtree. Ordered (`BTreeMap`) for deterministic iteration.
pub(super) type ComponentMap<'a> = BTreeMap<&'a str, &'a ComponentDef>;

/// Compile-time lookup of master-page definitions by id. A page with a `master`
/// attribute projects the named master's nodes (with fields resolved against
/// that page) UNDER its own children. Ordered (`BTreeMap`) for determinism.
pub(super) type MasterMap<'a> = BTreeMap<&'a str, &'a MasterDef>;

// ── Render context ────────────────────────────────────────────────────────────

/// Per-subtree rendering context that cascades through the node tree.
///
/// Each field accumulates transformations as we descend:
/// - `opacity` — multiplied together at each group boundary; leaf nodes
///   apply it on top of their own node-level opacity.
/// - `dx`/`dy` — translation offset accumulated from all ancestor groups
///   with an `x`/`y` property; added to every leaf geometry position.
#[derive(Clone, Copy)]
pub(super) struct RenderCtx {
    /// Accumulated opacity multiplier (1.0 = fully opaque).
    pub(super) opacity: f64,
    /// Accumulated x-translation in pixels.
    pub(super) dx: f64,
    /// Accumulated y-translation in pixels.
    pub(super) dy: f64,
    /// Resolved page baseline-grid pitch in pixels, when active on this page.
    /// `Some(g)` with `g > 0.0` snaps text line baselines onto `{0, g, 2g, …}`
    /// measured in the post-`dy` coordinate space; `None` → no grid (the snap is
    /// skipped, byte-identical to before). Cascades unchanged to every child
    /// context so all text on the page shares one grid.
    pub(super) baseline_grid: Option<f64>,
}

impl RenderCtx {
    fn root() -> Self {
        RenderCtx {
            opacity: 1.0,
            dx: 0.0,
            dy: 0.0,
            baseline_grid: None,
        }
    }

    /// Identity context used by the footnote zone's scratch MEASURE pass: the
    /// synthesized footnote text is compiled into a throwaway buffer at the
    /// origin to read its laid-out height before the real (offset) emit. Same
    /// fields as [`RenderCtx::root`].
    pub(super) fn measure() -> Self {
        RenderCtx {
            opacity: 1.0,
            dx: 0.0,
            dy: 0.0,
            baseline_grid: None,
        }
    }

    /// Root context translated by a fixed pixel offset on both axes. Used to
    /// shift all page content into the trim box when a print bleed is active:
    /// authored coordinate `(0, 0)` then lands at the trim corner `(b, b)`.
    fn root_offset(dx: f64, dy: f64) -> Self {
        RenderCtx {
            opacity: 1.0,
            dx,
            dy,
            baseline_grid: None,
        }
    }
}

// ── Public result type ────────────────────────────────────────────────────────

/// The result of compiling a [`Document`] into a [`Scene`].
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// The compiled display list.
    pub scene: Scene,
    /// All diagnostics collected during compilation (may include token-resolution
    /// diagnostics, unit advisories, and unsupported-node advisories).
    pub diagnostics: Vec<Diagnostic>,
}

// ── Style cascade helper ──────────────────────────────────────────────────────

/// Look up a style property value by (style_ref, style_map, key).
///
/// Returns `None` when there is no style reference, the style id is not in the
/// map, or the style does not carry the requested key.
pub(super) fn style_prop<'a>(
    style_ref: &Option<String>,
    style_map: &'a BTreeMap<&str, &Style>,
    key: &str,
) -> Option<&'a PropertyValue> {
    let sid = style_ref.as_deref()?;
    style_map.get(sid)?.properties.get(key)
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Compile `doc` into a [`CompileResult`], using `fonts` to shape text nodes.
///
/// [`compile_page`] renders a chosen page; this wrapper renders page 0.  If the
/// document has no pages an empty scene is returned with an advisory diagnostic.
///
/// Pass `&zenith_core::default_provider()` to use the bundled Noto Sans
/// font, which is sufficient for basic text rendering.
///
/// # No-panic guarantee
///
/// This function never calls `unwrap`, `expect`, `panic!`, `todo!`,
/// `unimplemented!`, or performs unchecked indexing.  All failure paths push a
/// diagnostic and continue.
pub fn compile(doc: &Document, fonts: &dyn FontProvider) -> CompileResult {
    compile_page(doc, fonts, 0)
}

/// Compile the page at `page_index` (0-based) of `doc` into a [`CompileResult`],
/// using `fonts` to shape text nodes.
///
/// If the document has no pages an empty scene is returned with a
/// `scene.no_pages` advisory; if `page_index` is out of range (but pages exist)
/// an empty scene is returned with a `scene.page_out_of_range` advisory.
///
/// Pass `&zenith_core::default_provider()` to use the bundled Noto Sans
/// font, which is sufficient for basic text rendering.
///
/// # No-panic guarantee
///
/// This function never calls `unwrap`, `expect`, `panic!`, `todo!`,
/// `unimplemented!`, or performs unchecked indexing (page lookup uses `.get()`).
/// All failure paths push a diagnostic and continue.
pub fn compile_page(doc: &Document, fonts: &dyn FontProvider, page_index: usize) -> CompileResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // ── Step 1: resolve tokens ────────────────────────────────────────────
    let token_resolution = resolve_tokens(&doc.tokens);
    diagnostics.extend(token_resolution.diagnostics);
    let resolved = &token_resolution.resolved;

    // ── Step 1b: build style lookup map ──────────────────────────────────
    let style_map: BTreeMap<&str, &Style> = doc
        .styles
        .styles
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    // ── Step 1c: build component lookup map ──────────────────────────────
    // Instances expand their referenced component at compile time. First
    // declaration wins on a duplicate id (the validator flags id.duplicate).
    let mut component_map: ComponentMap = BTreeMap::new();
    for comp in &doc.components {
        component_map.entry(comp.id.as_str()).or_insert(comp);
    }

    // ── Step 1d: build master lookup map + page-ref index ────────────────
    // A page's `master` attribute projects the named master's nodes (fields
    // resolved against that page) under the page's own children. The page-ref
    // index maps every node id to the 1-based page that contains it, for
    // `page-ref` field resolution. Both are document-wide and order-stable.
    let mut master_map: MasterMap = BTreeMap::new();
    for master in &doc.masters {
        master_map.entry(master.id.as_str()).or_insert(master);
    }
    let page_index_by_node_id = build_page_index_map(doc);

    // ── Step 2: select the requested page ────────────────────────────────
    let Some(page) = doc.body.pages.get(page_index) else {
        if doc.body.pages.is_empty() {
            diagnostics.push(Diagnostic::advisory(
                "scene.no_pages",
                "document has no pages; an empty scene is returned",
                None,
                Some(doc.body.id.clone()),
            ));
        } else {
            diagnostics.push(Diagnostic::advisory(
                "scene.page_out_of_range",
                format!(
                    "page index {} is out of range; document has {} page(s)",
                    page_index,
                    doc.body.pages.len()
                ),
                None,
                Some(doc.body.id.clone()),
            ));
        }
        return CompileResult {
            scene: Scene::new(0.0, 0.0),
            diagnostics,
        };
    };

    // ── Step 3: page dimensions → pixels ─────────────────────────────────
    let page_w = match dim_to_px(page.width.value, &page.width.unit) {
        Some(v) => v,
        None => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_unit",
                format!(
                    "page '{}' width uses an unsupported unit; cannot compile scene",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
            return CompileResult {
                scene: Scene::new(0.0, 0.0),
                diagnostics,
            };
        }
    };
    let page_h = match dim_to_px(page.height.value, &page.height.unit) {
        Some(v) => v,
        None => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_unit",
                format!(
                    "page '{}' height uses an unsupported unit; cannot compile scene",
                    page.id
                ),
                page.source_span,
                Some(page.id.clone()),
            ));
            return CompileResult {
                scene: Scene::new(0.0, 0.0),
                diagnostics,
            };
        }
    };

    // ── Step 3b: resolve print bleed ─────────────────────────────────────
    // A page may declare a uniform `bleed` margin. When it resolves to a
    // positive pixel value `b`, the media (canvas) box expands to
    // `(page_w + 2b) × (page_h + 2b)`, the trim box is the inner
    // `[b, b, page_w, page_h]`, all content shifts by `(b, b)`, the background
    // fills the whole media box, and crop marks are drawn at the trim corners.
    // An absent / unresolvable / non-positive bleed yields `b = 0`, which is
    // byte-identical to the no-bleed path. The validator surfaces a warning for
    // an unresolvable unit or a negative value; the compiler just ignores it.
    let bleed = page
        .bleed
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&px| px > 0.0)
        .unwrap_or(0.0);

    // Media box (full canvas including bleed on all four sides).
    let media_w = page_w + 2.0 * bleed;
    let media_h = page_h + 2.0 * bleed;

    let mut scene = Scene::new(media_w, media_h);

    // ── Step 4: outermost media-edge clip (doc 09 normative rule) ────────
    // The clip covers the entire media box so content and background may bleed
    // into the margin. With bleed = 0 this is exactly the page rectangle.
    scene.commands.push(SceneCommand::PushClip {
        x: 0.0,
        y: 0.0,
        w: media_w,
        h: media_h,
    });

    // ── Step 5: optional page background (fills the entire media box) ────
    if let Some(bg_prop) = &page.background {
        if let Some(gradient) = resolve_property_gradient(bg_prop, resolved, &page.id) {
            // Page background applies no opacity cascade (mirrors the solid path).
            scene.commands.push(SceneCommand::FillRectGradient {
                x: 0.0,
                y: 0.0,
                w: media_w,
                h: media_h,
                gradient,
            });
        } else if let Some(color) =
            resolve_property_color(bg_prop, resolved, &mut diagnostics, &page.id)
        {
            scene.commands.push(SceneCommand::FillRect {
                x: 0.0,
                y: 0.0,
                w: media_w,
                h: media_h,
                color,
            });
        }
    }

    // ── Step 6: threaded-text chain pre-pass (DOCUMENT-WIDE) ─────────────
    // Resolve every text chain ONCE across ALL pages (deterministic
    // page-then-source-order walk into frames + groups), distributing each
    // chain's source article across every member's box — flowing across page
    // boundaries. The map is keyed by global node id; this page's nodes look up
    // the slice assigned to them. Chains' diagnostics (e.g. a source font
    // fallback) are document-wide and would otherwise be emitted once per page;
    // they are collected into a throwaway buffer here and only the diagnostics
    // attributable to THIS page's chain members would be surfaced — but since
    // distribution is global, we keep the page-local behaviour deterministic by
    // discarding the pre-pass's own advisories on non-zero pages (they were
    // already surfaced on page 0). Page 0 keeps them.
    let engine = RustybuzzEngine::new();
    let mut chain_diags: Vec<Diagnostic> = Vec::new();
    let chains =
        resolve_chains_document(doc, resolved, &style_map, fonts, &engine, &mut chain_diags);
    if page_index == 0 {
        diagnostics.extend(chain_diags);
    }

    // ── Step 7: build the per-page field context ─────────────────────────
    // The 1-based page index drives the folio + parity (recto = odd, verso =
    // even). The live area mirrors the validator's margin formula so an omitted
    // field x/w auto-mirrors recto/verso via the page margins.
    let page_index_1based = page_index + 1;
    // Single source of truth for parity (explicit page.parity > document
    // page-parity-start > default index%2==1). Mirrors the validator.
    let is_recto = doc.page_is_recto(page, page_index_1based);
    let mirror_margins = doc.mirror_margins.unwrap_or(false);
    // RTL book: the binding margin is mirrored to the opposite side (recto →
    // inner-on-right). Matches the validator's `margin.rs` parity.
    let rtl_book = doc.page_progression.as_deref() == Some("rtl");
    let live_area = compute_live_area(
        doc,
        page,
        page_w,
        page_h,
        is_recto,
        mirror_margins,
        rtl_book,
    );

    // ── Step 7b: collect this page's footnote markers ────────────────────
    // Every `footnote` DIRECT child of the page is auto-numbered 1..N in source
    // order (an explicit `marker` overrides the number but keeps its slot). The
    // ordered map drives both the inline superscript markers (a text span's
    // `footnote_ref` keys in) and the bottom-zone rendering below.
    let footnote_markers = footnote::collect_footnote_markers(page);

    // ── Step 7c: build this page's node bounding-box map ─────────────────
    // Maps every id-bearing page node with a resolvable x/y/w/h rect to its
    // ABSOLUTE page-coordinate box, accumulating group/instance translation
    // (frames are clip-only). Drives text-runaround exclusion lookup. Empty when
    // no node carries a complete rect (byte-identical to before for any text node
    // without `text-exclusion`).
    let node_boxes = build_node_boxes(page);

    let field_ctx = FieldCtx {
        page_index_1based,
        is_recto,
        live_area,
        page_index_by_node_id: &page_index_by_node_id,
        footnote_markers: &footnote_markers,
        node_boxes: &node_boxes,
        total_pages: doc.body.pages.len(),
    };

    // ── Resolve the page baseline grid ───────────────────────────────────
    // A page may declare `baseline-grid=(px)14`. When it resolves to a positive
    // pixel value `g`, every text node on this page snaps its line baselines
    // onto the grid `{0, g, 2g, …}` (see [`RenderCtx::baseline_grid`]). An
    // absent / unresolvable / non-positive value yields `None`, byte-identical
    // to a page with no grid.
    let baseline_grid: Option<f64> = page
        .baseline_grid
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|g| g.is_finite() && *g > 0.0);

    let mut root_ctx = if bleed > 0.0 {
        // Shift authored coordinates into the trim box. With bleed = 0 this is
        // the identity root context (byte-identical to before).
        RenderCtx::root_offset(bleed, bleed)
    } else {
        RenderCtx::root()
    };
    // Thread the grid into BOTH the bleed and no-bleed root contexts. The grid
    // is measured in the post-`dy` (shifted) coordinate space, the same space
    // the emitted baselines live in, so a bleed-shifted page snaps consistently.
    root_ctx.baseline_grid = baseline_grid;

    // ── Step 7a: project the page's master (UNDER its own children) ──────
    // When `page.master` names a declared master, clone the master's children,
    // prefix every projected id with the page id (avoid cross-page collisions),
    // and compile them BEFORE the page's own children so running heads / folios
    // sit behind body text. Fields inside the master resolve against THIS page.
    // An unknown master reference is a hard validation error; here it is simply
    // skipped (the compiler never panics on bad references).
    if let Some(master_id) = &page.master
        && let Some(master) = master_map.get(master_id.as_str())
    {
        let mut projected = master.children.clone();
        let prefix = format!("{}/", page.id);
        container::prefix_ids_in_children(&mut projected, &prefix);
        for node in &projected {
            compile_node(
                node,
                resolved,
                &style_map,
                &component_map,
                fonts,
                &engine,
                &mut scene.commands,
                &mut diagnostics,
                &chains,
                &field_ctx,
                root_ctx,
            );
        }
    }

    // ── Step 7b: page children in source order (z-order: first = bottom) ─
    for node in &page.children {
        compile_node(
            node,
            resolved,
            &style_map,
            &component_map,
            fonts,
            &engine,
            &mut scene.commands,
            &mut diagnostics,
            &chains,
            &field_ctx,
            root_ctx,
        );
    }

    // ── Step 7c: footnote zone (page furniture, above the bottom margin) ─
    // Rendered AFTER the page's own children (so it paints on top of body
    // content) but inside the media clip. Draws the separator rule plus the
    // stacked, auto-numbered footnotes; warns on body/zone overlap. A page with
    // no footnotes emits nothing here (byte-identical to before).
    footnote::compile_footnote_zone(
        page,
        &footnote_markers,
        live_area,
        resolved,
        &style_map,
        fonts,
        &engine,
        &chains,
        &field_ctx,
        &mut scene.commands,
        &mut diagnostics,
        root_ctx,
    );

    // ── Step 8: close the outermost clip ─────────────────────────────────
    scene.commands.push(SceneCommand::PopClip);

    // ── Step 9: crop / trim marks (only when a bleed is active) ──────────
    // Emitted AFTER content and OUTSIDE the clip so the marks sit on top and
    // live entirely in the bleed margin at the four trim corners.
    if bleed > 0.0 {
        crop::emit_crop_marks(&mut scene.commands, bleed, page_w, page_h);
    }

    // ── Step 10: print trim box ──────────────────────────────────────────
    // When a bleed is active the media box (`scene.width`/`height`) includes
    // the bleed on all four sides; the trim box is the inner page rectangle
    // `[b, b, page_w, page_h]` that the finished piece is cut to. Backends that
    // care about print boxes (PDF) read this; raster backends ignore it. With
    // bleed = 0 the trim box equals the media box, so we leave `trim` as `None`.
    if bleed > 0.0 {
        scene.trim = Some(Rect {
            x: bleed,
            y: bleed,
            w: page_w,
            h: page_h,
        });
    }

    CompileResult { scene, diagnostics }
}

// ── Node dispatch ─────────────────────────────────────────────────────────────

/// The `role` of any node, if set. Used to exclude non-printing nodes
/// (`role="guide"`) from render output.
pub(super) fn node_role(node: &Node) -> Option<&str> {
    match node {
        Node::Rect(n) => n.role.as_deref(),
        Node::Ellipse(n) => n.role.as_deref(),
        Node::Line(n) => n.role.as_deref(),
        Node::Text(n) => n.role.as_deref(),
        Node::Code(n) => n.role.as_deref(),
        Node::Frame(n) => n.role.as_deref(),
        Node::Group(n) => n.role.as_deref(),
        Node::Image(n) => n.role.as_deref(),
        Node::Polygon(n) => n.role.as_deref(),
        Node::Polyline(n) => n.role.as_deref(),
        Node::Instance(n) => n.role.as_deref(),
        Node::Field(n) => n.role.as_deref(),
        Node::Footnote(n) => n.role.as_deref(),
        Node::Unknown(_) => None,
    }
}

/// Route a single node to the submodule that compiles its kind.
///
/// Each arm forwards the full cascade context to a `compile_*` function; the
/// emitted `SceneCommand` stream is identical to the previous inline match.
///
/// Returns the child's laid-out content height in pixels for the kinds whose
/// intrinsic height is meaningful to flow layout (`text`/`code`); every other
/// kind returns `0.0`. The absolute-positioning callers ignore this value, so
/// command output is unchanged; only the flow-layout path in [`container`]
/// consumes it to advance its vertical cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_node(
    node: &Node,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) -> f64 {
    // Non-printing guide nodes (`role="guide"`) are excluded from render output
    // entirely — including their subtree when the guide is a group/frame.
    if node_role(node) == Some("guide") {
        return 0.0;
    }

    match node {
        Node::Rect(rect) => {
            compile_rect(rect, resolved, style_map, commands, diagnostics, ctx);
            0.0
        }
        Node::Ellipse(ellipse) => {
            compile_ellipse(ellipse, resolved, style_map, commands, diagnostics, ctx);
            0.0
        }
        Node::Text(text) => compile_text(
            text,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            field_ctx.footnote_markers,
            field_ctx.node_boxes,
            ctx,
        ),
        Node::Line(line) => {
            compile_line(line, resolved, style_map, commands, diagnostics, ctx);
            0.0
        }
        Node::Frame(frame) => {
            compile_frame(
                frame,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                field_ctx,
                ctx,
            );
            0.0
        }
        Node::Group(group) => {
            compile_group(
                group,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                field_ctx,
                ctx,
            );
            0.0
        }
        Node::Instance(instance) => {
            compile_instance(
                instance,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                field_ctx,
                ctx,
            );
            0.0
        }
        Node::Field(field) => {
            // Resolve the field against this page into a concrete single-line
            // text node and compile it via the normal text path. An unresolved
            // field (absent running-head side, unknown type, unresolved
            // page-ref) yields nothing.
            if let Some(text_node) = resolve_field_to_text(field, field_ctx) {
                compile_text(
                    &text_node,
                    resolved,
                    style_map,
                    fonts,
                    engine,
                    commands,
                    diagnostics,
                    chains,
                    field_ctx.footnote_markers,
                    field_ctx.node_boxes,
                    ctx,
                );
            }
            0.0
        }
        Node::Image(image) => {
            compile_image(image, resolved, commands, diagnostics, ctx);
            0.0
        }
        Node::Polygon(poly) => {
            compile_polygon(poly, resolved, style_map, commands, diagnostics, ctx);
            0.0
        }
        Node::Polyline(poly) => {
            compile_polyline(poly, resolved, style_map, commands, diagnostics, ctx);
            0.0
        }
        Node::Code(code) => compile_code(
            code,
            resolved,
            style_map,
            fonts,
            engine,
            commands,
            diagnostics,
            ctx,
        ),
        Node::Footnote(_) => {
            // Footnotes are NON-flowing page furniture: they carry no x/y/w/h
            // and are NOT rendered in the normal z-order dispatch. The page-level
            // footnote pass (`footnote::compile_footnote_zone`, run by
            // `compile_page`) collects every page-level footnote in source order,
            // auto-numbers them, and renders the bottom zone + separator. A
            // footnote reached here (e.g. nested in a container) renders nothing.
            0.0
        }
        Node::Unknown(unknown) => {
            diagnostics.push(Diagnostic::advisory(
                "scene.unsupported_node",
                format!(
                    "unknown node kind '{}' cannot be compiled; the node is skipped \
                     (forward-compatibility: this kind may be supported in a later version)",
                    unknown.kind
                ),
                unknown.source_span,
                None,
            ));
            0.0
        }
    }
}
