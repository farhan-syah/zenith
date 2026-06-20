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
        style: field.style.clone(),
        fill: field.fill.clone(),
        font_family: field.font_family.clone(),
        font_size: field.font_size.clone(),
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
            _ => {}
        }
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
/// Returns `None` unless all four margins (`inner`/`outer`/`top`/`bottom`)
/// resolve to pixels — the same all-or-nothing gate the validator uses.
///
/// recto (odd, 1-based): `live_x = margin_inner`; verso (even) with
/// `mirror_margins`: `live_x = margin_outer`; otherwise `live_x = margin_inner`.
/// `live_y = margin_top`, `live_w = page_w - inner - outer`,
/// `live_h = page_h - top - bottom`.
pub(super) fn compute_live_area(
    page: &zenith_core::Page,
    page_w: f64,
    page_h: f64,
    is_recto: bool,
    mirror_margins: bool,
) -> Option<(f64, f64, f64, f64)> {
    use zenith_core::dim_to_px;
    let inner_dim = page.margin_inner.as_ref()?;
    let outer_dim = page.margin_outer.as_ref()?;
    let top_dim = page.margin_top.as_ref()?;
    let bottom_dim = page.margin_bottom.as_ref()?;

    let inner = dim_to_px(inner_dim.value, &inner_dim.unit)?;
    let outer = dim_to_px(outer_dim.value, &outer_dim.unit)?;
    let top = dim_to_px(top_dim.value, &top_dim.unit)?;
    let bottom = dim_to_px(bottom_dim.value, &bottom_dim.unit)?;

    let left_inset = if mirror_margins && !is_recto {
        // verso: binding (inner) is on the RIGHT, so OUTER insets the left edge.
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
            master: None,
            safe_zones: Vec::new(),
            folds: Vec::new(),
            children: Vec::new(),
            source_span: None,
        }
    }

    #[test]
    fn live_area_recto_uses_inner_margin() {
        // Recto (is_recto = true): left inset = margin_inner = 160.
        let la = compute_live_area(&margined_page(), 1200.0, 1900.0, true, true);
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_verso_mirrors_to_outer_margin() {
        // Verso (is_recto = false) with mirror: left inset = margin_outer = 100.
        let la = compute_live_area(&margined_page(), 1200.0, 1900.0, false, true);
        assert_eq!(la, Some((100.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_unmirrored_verso_keeps_inner() {
        // Without mirroring, verso still uses inner as the left inset.
        let la = compute_live_area(&margined_page(), 1200.0, 1900.0, false, false);
        assert_eq!(la, Some((160.0, 80.0, 940.0, 1740.0)));
    }

    #[test]
    fn live_area_requires_all_four_margins() {
        let mut page = margined_page();
        page.margin_bottom = None;
        assert_eq!(
            compute_live_area(&page, 1200.0, 1900.0, true, true),
            None,
            "an incomplete margin set yields no live area"
        );
    }
}
