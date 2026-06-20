//! Field-node resolution: turn an auto-resolved [`FieldNode`] into a concrete
//! single-line [`TextNode`] against the page it is projected onto.
//!
//! A `field` is the building block of the master-page / running-head / folio
//! system. At compile time each field is resolved against the page's 1-based
//! index (folio), its parity (recto = odd, verso = even), the page's live area
//! (so an omitted `x`/`w` auto-mirrors recto/verso via the page margins), and —
//! for a `page-ref` field — a document-wide page-index lookup keyed by node id.
//!
//! Resolution synthesizes a [`TextNode`] (a single span carrying the resolved
//! string) and the caller compiles it through the normal text path — this reuses
//! the existing single-line shaping/emit verbatim rather than duplicating it.

use std::collections::BTreeMap;

use zenith_core::{FieldNode, Node, TextNode, TextSpan};

use super::util::px;

/// Per-page context threaded into field resolution.
///
/// `live_area` is the page's live area `(x, y, w, h)` in AUTHORED coordinates
/// (pre-bleed-offset), mirroring the validator's `margin.rs` formula: recto
/// `live_x = margin_inner`, verso `live_x = margin_outer` (when mirrored),
/// `live_y = margin_top`, `live_w = page_w - inner - outer`, `live_h = page_h -
/// top - bottom`. `None` when the page declares no (complete) margin set.
///
/// `page_index_by_node_id` maps every node id in the document to the 1-based
/// index of the page that contains it, for `page-ref` resolution. Built once,
/// deterministically (ordered map, page-then-source order).
#[derive(Clone, Copy)]
pub(crate) struct FieldCtx<'a> {
    pub(super) page_index_1based: usize,
    pub(super) is_recto: bool,
    pub(super) live_area: Option<(f64, f64, f64, f64)>,
    pub(super) page_index_by_node_id: &'a BTreeMap<String, usize>,
    /// This page's footnote markers: `footnote_id → marker_string` (auto-number
    /// or explicit override), in id order. A text span whose `footnote_ref` keys
    /// into this map emits that marker as an inline superscript run. Empty when
    /// the page declares no footnotes.
    pub(super) footnote_markers: &'a BTreeMap<String, String>,
    /// This page's node id → ABSOLUTE page-coordinate bounding box `(x, y, w, h)`
    /// in pixels, accumulating group/instance translation (frames do not
    /// translate). Drives text-runaround exclusion lookup in `compile_text`. Only
    /// nodes with a fully-resolvable x/y/w/h rect are included. Empty when no node
    /// on the page has a resolvable box.
    pub(super) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    /// Total page count in `doc.body.pages`, for `page-count` field resolution.
    /// A `page-count` field resolves to this value as a decimal string (the "M"
    /// in a "Slide N of M" footer, where `page-number` supplies N).
    pub(super) total_pages: usize,
}

/// Resolve a [`FieldNode`] against the page context into a concrete single-line
/// [`TextNode`], or `None` when the field resolves to nothing (an absent
/// running-head side, an unknown field type, or an unresolved page-ref).
///
/// Geometry: `x`/`w` default to the page live area when omitted (so a running
/// head auto-mirrors recto/verso x via the margins); `y`/`h` default to the live
/// area's top/height when omitted. When the field declares neither geometry nor
/// a live area, the synthesized text node carries whatever geometry the field
/// did declare (a missing `x`/`y` then makes the text path emit its own
/// `scene.missing_geometry` advisory — surfaced honestly, never silently
/// dropped).
pub(super) fn resolve_field_to_text(field: &FieldNode, ctx: &FieldCtx) -> Option<TextNode> {
    // Skip invisible fields entirely (mirror the text/leaf visible=false path).
    if field.visible == Some(false) {
        return None;
    }

    let (text, default_align) = match field.field_type.as_str() {
        "running-head" => {
            let side = if ctx.is_recto {
                field.recto.as_deref()
            } else {
                field.verso.as_deref()
            };
            // An absent side renders nothing (no empty text node emitted).
            let s = side?;
            if s.is_empty() {
                return None;
            }
            (s.to_owned(), "center")
        }
        "page-number" => (ctx.page_index_1based.to_string(), "center"),
        "page-count" => (ctx.total_pages.to_string(), "center"),
        "page-ref" => {
            // Resolve the 1-based index of the page that contains `target`.
            let target = field.target.as_deref()?;
            let idx = ctx.page_index_by_node_id.get(target)?;
            (idx.to_string(), "start")
        }
        // Unknown field type → render nothing (the validator warns separately).
        _ => return None,
    };

    // Geometry: prefer the field's own x/w, falling back to the live area.
    let live = ctx.live_area;
    let x = field.x.clone().or_else(|| live.map(|(lx, _, _, _)| px(lx)));
    let y = field.y.clone().or_else(|| live.map(|(_, ly, _, _)| px(ly)));
    let w = field.w.clone().or_else(|| live.map(|(_, _, lw, _)| px(lw)));
    let h = field.h.clone().or_else(|| live.map(|(_, _, _, lh)| px(lh)));

    Some(TextNode {
        id: field.id.clone(),
        name: field.name.clone(),
        role: field.role.clone(),
        x,
        y,
        w,
        h,
        // A field is always a single line; alignment defaults by field type but
        // an explicit field-level note: fields do not expose `align` in v0, so
        // the per-type default is authoritative.
        align: Some(default_align.to_owned()),
        direction: None,
        overflow: Some("clip".to_owned()),
        overflow_wrap: None,
        style: field.style.clone(),
        fill: field.fill.clone(),
        contrast_bg: None,
        font_family: field.font_family.clone(),
        font_size: field.font_size.clone(),
        font_size_min: None,
        font_weight: None,
        shadow: None,
        opacity: field.opacity,
        visible: field.visible,
        locked: field.locked,
        rotate: None,
        chain: None,
        drop_cap_lines: None,
        hyphenate: None,
        widow_orphan: None,
        tab_leader: None,
        text_exclusion: None,
        padding_left: None,
        text_indent: None,
        spans: vec![TextSpan {
            text,
            fill: None,
            font_weight: None,
            italic: None,
            underline: None,
            strikethrough: None,
            vertical_align: None,
            footnote_ref: None,
        }],
        source_span: field.source_span,
        unknown_props: BTreeMap::new(),
    })
}

/// Build the document-wide `node id → 1-based page index` map for `page-ref`
/// resolution. Deterministic: walks pages in order, descending into
/// `group`/`frame` containers in source order. The FIRST occurrence of an id
/// wins (ids are globally unique in a valid document; a duplicate keeps the
/// earliest page, deterministically).
pub(super) fn build_page_index_map(doc: &zenith_core::Document) -> BTreeMap<String, usize> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for (page_idx0, page) in doc.body.pages.iter().enumerate() {
        let page_index_1based = page_idx0 + 1;
        index_nodes(&page.children, page_index_1based, &mut map);
    }
    map
}

/// Recursively record each node's id → `page_index_1based`, descending into
/// `group`/`frame` children. First write wins (does not overwrite).
fn index_nodes(children: &[Node], page_index_1based: usize, map: &mut BTreeMap<String, usize>) {
    for child in children {
        if let Some(id) = node_id(child) {
            map.entry(id.to_owned()).or_insert(page_index_1based);
        }
        match child {
            Node::Frame(f) => index_nodes(&f.children, page_index_1based, map),
            Node::Group(g) => index_nodes(&g.children, page_index_1based, map),
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Build a single page's `node id → ABSOLUTE bounding box (x, y, w, h)` map in
/// pixels for text-runaround exclusion lookup.
///
/// Walks the page's children recursively, accumulating the translation offset of
/// each ancestor container: a `group` (and an `instance`, which compiles as a
/// translated synthetic group) shifts its children by its own `x`/`y`; a `frame`
/// is clip-only and does NOT translate (matching the render-offset semantics in
/// [`super::container`]). A node's absolute box is `(dx + node_x, dy + node_y,
/// node_w, node_h)`. Only nodes whose x/y/w/h ALL resolve to pixels are recorded
/// (a node without a complete rect — `line`/`polygon`/`polyline`, or any node
/// missing a dimension — is skipped: it cannot serve as a rectangular exclusion).
/// Deterministic: source-order walk; the FIRST occurrence of an id wins.
pub(super) fn build_node_boxes(page: &zenith_core::Page) -> BTreeMap<String, (f64, f64, f64, f64)> {
    let mut map: BTreeMap<String, (f64, f64, f64, f64)> = BTreeMap::new();
    collect_node_boxes(&page.children, 0.0, 0.0, &mut map);
    map
}

/// Recursive worker for [`build_node_boxes`]. `dx`/`dy` are the accumulated
/// ancestor translation in pixels.
fn collect_node_boxes(
    children: &[Node],
    dx: f64,
    dy: f64,
    map: &mut BTreeMap<String, (f64, f64, f64, f64)>,
) {
    use zenith_core::dim_to_px;
    for child in children {
        if let Some(id) = node_id(child)
            && let Some((x, y, w, h)) = node_rect(child)
        {
            map.entry(id.to_owned()).or_insert((dx + x, dy + y, w, h));
        }
        match child {
            // A frame is clip-only: its children are NOT translated by its origin.
            Node::Frame(f) => collect_node_boxes(&f.children, dx, dy, map),
            // A group translates its children by its own x/y (absent/bad-unit → 0).
            Node::Group(g) => {
                let gx = g.x.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
                let gy = g.y.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
                collect_node_boxes(
                    &g.children,
                    dx + gx.unwrap_or(0.0),
                    dy + gy.unwrap_or(0.0),
                    map,
                );
            }
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// A node's LOCAL `(x, y, w, h)` rectangle in pixels, when all four resolve.
///
/// Returns `None` for a node kind without a rectangular box (`line`/`polygon`/
/// `polyline`/`footnote`/`unknown`) or one missing any of x/y/w/h.
fn node_rect(node: &Node) -> Option<(f64, f64, f64, f64)> {
    use zenith_core::dim_to_px;
    let rect = |x: &Option<zenith_core::Dimension>,
                y: &Option<zenith_core::Dimension>,
                w: &Option<zenith_core::Dimension>,
                h: &Option<zenith_core::Dimension>|
     -> Option<(f64, f64, f64, f64)> {
        let x = x.as_ref().and_then(|d| dim_to_px(d.value, &d.unit))?;
        let y = y.as_ref().and_then(|d| dim_to_px(d.value, &d.unit))?;
        let w = w.as_ref().and_then(|d| dim_to_px(d.value, &d.unit))?;
        let h = h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit))?;
        Some((x, y, w, h))
    };
    match node {
        Node::Rect(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Ellipse(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Text(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Code(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Frame(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Group(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Image(n) => rect(&n.x, &n.y, &n.w, &n.h),
        Node::Field(n) => rect(&n.x, &n.y, &n.w, &n.h),
        // An `instance` has no intrinsic w/h (its box is the expanded subtree),
        // and line/polygon/polyline have no rectangular box — none can serve as a
        // rectangular exclusion, so they are skipped.
        Node::Instance(_)
        | Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => None,
    }
}

/// The id of a node, or `None` for `Unknown`.
fn node_id(node: &Node) -> Option<&str> {
    match node {
        Node::Rect(n) => Some(&n.id),
        Node::Ellipse(n) => Some(&n.id),
        Node::Line(n) => Some(&n.id),
        Node::Text(n) => Some(&n.id),
        Node::Code(n) => Some(&n.id),
        Node::Frame(n) => Some(&n.id),
        Node::Group(n) => Some(&n.id),
        Node::Image(n) => Some(&n.id),
        Node::Polygon(n) => Some(&n.id),
        Node::Polyline(n) => Some(&n.id),
        Node::Instance(n) => Some(&n.id),
        Node::Field(n) => Some(&n.id),
        Node::Footnote(n) => Some(&n.id),
        Node::Unknown(_) => None,
    }
}

/// Compute a page's live area `(x, y, w, h)` in AUTHORED coordinates, mirroring
/// the validator's `margin.rs` formula.
///
/// Returns `None` unless all four EFFECTIVE margins (`inner`/`outer`/`top`/
/// `bottom`) resolve to pixels — the same all-or-nothing gate the validator uses.
///
/// Each side's effective margin is the page's own value when set, else the
/// document-level default ([`zenith_core::Document::effective_margins`]) — the
/// single source of truth for the document→page margin cascade. With no document
/// margins set this reads exactly the page's own values, so the default-off path
/// is byte-identical.
///
/// LTR book — recto (odd, 1-based): `live_x = margin_inner`; verso (even) with
/// `mirror_margins`: `live_x = margin_outer`; otherwise `live_x = margin_inner`.
/// RTL book (`rtl == true`): the parity is MIRRORED — recto with `mirror_margins`
/// → `live_x = margin_outer` (binding on the right), verso → `live_x =
/// margin_inner`. `live_y = margin_top`, `live_w = page_w - inner - outer`,
/// `live_h = page_h - top - bottom`.
pub(super) fn compute_live_area(
    doc: &zenith_core::Document,
    page: &zenith_core::Page,
    page_w: f64,
    page_h: f64,
    is_recto: bool,
    mirror_margins: bool,
    rtl: bool,
) -> Option<(f64, f64, f64, f64)> {
    use zenith_core::dim_to_px;
    let (inner_opt, outer_opt, top_opt, bottom_opt) = doc.effective_margins(page);
    let inner_dim = inner_opt.as_ref()?;
    let outer_dim = outer_opt.as_ref()?;
    let top_dim = top_opt.as_ref()?;
    let bottom_dim = bottom_opt.as_ref()?;

    let inner = dim_to_px(inner_dim.value, &inner_dim.unit)?;
    let outer = dim_to_px(outer_dim.value, &outer_dim.unit)?;
    let top = dim_to_px(top_dim.value, &top_dim.unit)?;
    let bottom = dim_to_px(bottom_dim.value, &bottom_dim.unit)?;

    // Inner (binding) is on the RIGHT for verso under LTR, and for recto under
    // RTL (the spread is mirrored). When it is on the right, OUTER insets the
    // left edge.
    let inner_on_right = if rtl { is_recto } else { !is_recto };
    let left_inset = if mirror_margins && inner_on_right {
        outer
    } else {
        inner
    };

    Some((
        left_inset,
        top,
        page_w - inner - outer,
        page_h - top - bottom,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_core::Document;
    use zenith_core::Page;

    fn margined_page() -> Page {
        Page {
            id: "p".to_owned(),
            name: None,
            width: px(1200.0),
            height: px(1900.0),
            background: None,
            bleed: None,
            margin_inner: Some(px(160.0)),
            margin_outer: Some(px(100.0)),
            margin_top: Some(px(80.0)),
            margin_bottom: Some(px(80.0)),
            baseline_grid: None,
            parity: None,
            master: None,
            safe_zones: Vec::new(),
            folds: Vec::new(),
            children: Vec::new(),
            source_span: None,
        }
    }

    fn bare_page() -> Page {
        let mut p = margined_page();
        p.margin_inner = None;
        p.margin_outer = None;
        p.margin_top = None;
        p.margin_bottom = None;
        p
    }

    /// A document with no margins set — the default-off cascade reads page values
    /// verbatim, so `compute_live_area(&bare_doc(), page, …)` matches the
    /// pre-cascade behavior of reading `page.margin_*` directly.
    fn bare_doc() -> Document {
        use zenith_core::{KdlAdapter, KdlSource};
        // Parse the minimal valid document; all doc margins are None.
        KdlAdapter
            .parse(b"zenith version=1 { document id=\"d\" { } }")
            .expect("minimal test document must parse")
    }

    /// A minimal `page-count` field node (no geometry, no styling) for the
    /// resolution unit test.
    fn page_count_field() -> FieldNode {
        FieldNode {
            id: "total".to_owned(),
            name: None,
            role: None,
            field_type: "page-count".to_owned(),
            recto: None,
            verso: None,
            target: None,
            x: None,
            y: None,
            w: None,
            h: None,
            style: None,
            fill: None,
            font_family: None,
            font_size: None,
            opacity: None,
            visible: None,
            locked: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        }
    }

    #[test]
    fn page_count_field_resolves_to_total_page_count() {
        let by_id: BTreeMap<String, usize> = BTreeMap::new();
        let markers: BTreeMap<String, String> = BTreeMap::new();
        let boxes: BTreeMap<String, (f64, f64, f64, f64)> = BTreeMap::new();
        let ctx = FieldCtx {
            page_index_1based: 2,
            is_recto: false,
            live_area: None,
            page_index_by_node_id: &by_id,
            footnote_markers: &markers,
            node_boxes: &boxes,
            total_pages: 5,
        };
        let text = resolve_field_to_text(&page_count_field(), &ctx)
            .expect("a page-count field must resolve to a text node");
        assert_eq!(text.spans.len(), 1, "a field is a single span");
        assert_eq!(
            text.spans.first().map(|s| s.text.as_str()),
            Some("5"),
            "page-count resolves to the total page count as a decimal string"
        );
    }

    #[test]
    fn live_area_recto_uses_inner_margin() {
        // LTR recto (is_recto = true): left inset = margin_inner = 160.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            true,
            true,
            false,
        );
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_verso_mirrors_to_outer_margin() {
        // LTR verso (is_recto = false) with mirror: left inset = margin_outer = 100.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            false,
            true,
            false,
        );
        assert_eq!(la, Some((100.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_unmirrored_verso_keeps_inner() {
        // Without mirroring, verso still uses inner as the left inset.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            false,
            false,
            false,
        );
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_rtl_recto_mirrors_to_outer_margin() {
        // RTL recto: binding on the RIGHT, so left inset = margin_outer = 100
        // (the mirror of the LTR recto). Width/top/bottom unchanged.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            true,
            true,
            true,
        );
        assert_eq!(la, Some((100.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_rtl_verso_uses_inner_margin() {
        // RTL verso: binding on the LEFT, so left inset = margin_inner = 160.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            false,
            true,
            true,
        );
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_rtl_unmirrored_keeps_inner() {
        // Without mirroring, RTL recto still uses inner as the left inset.
        let la = compute_live_area(
            &bare_doc(),
            &margined_page(),
            1200.0,
            1900.0,
            true,
            false,
            true,
        );
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_requires_all_four_margins() {
        let mut page = margined_page();
        page.margin_bottom = None;
        assert_eq!(
            compute_live_area(&bare_doc(), &page, 1200.0, 1900.0, true, true, false),
            None,
            "an incomplete margin set yields no live area"
        );
    }

    #[test]
    fn live_area_cascades_doc_margins_to_bare_page() {
        // Doc sets all four margins; the page declares none → the page inherits
        // the doc defaults and a live area is computed (LTR recto).
        let mut doc = bare_doc();
        doc.margin_inner = Some(px(160.0));
        doc.margin_outer = Some(px(100.0));
        doc.margin_top = Some(px(80.0));
        doc.margin_bottom = Some(px(80.0));
        let la = compute_live_area(&doc, &bare_page(), 1200.0, 1900.0, true, true, false);
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_page_inner_overrides_doc_default() {
        // Doc sets all four; the page overrides only inner → page inner (200) +
        // doc outer/top/bottom. LTR recto uses inner as the left inset.
        let mut doc = bare_doc();
        doc.margin_inner = Some(px(160.0));
        doc.margin_outer = Some(px(100.0));
        doc.margin_top = Some(px(80.0));
        doc.margin_bottom = Some(px(80.0));
        let mut page = bare_page();
        page.margin_inner = Some(px(200.0));
        let la = compute_live_area(&doc, &page, 1200.0, 1900.0, true, true, false);
        // left inset = page inner = 200; width = 1200 - 200 - 100 = 900.
        assert_eq!(la, Some((200.0, 80.0, 900.0, 1740.0)));
    }
}
