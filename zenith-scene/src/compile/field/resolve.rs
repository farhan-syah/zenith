//! Field resolution: turn an auto-resolved [`FieldNode`] into a concrete
//! single-line [`TextNode`] against the page it is projected onto, plus the
//! per-page [`FieldCtx`] that threads the resolution inputs.
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

use zenith_core::{FieldNode, Page, TextNode, TextSpan};

use crate::compile::util::px;

use super::folio::format_folio;

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
    pub(in crate::compile) page_index_1based: usize,
    pub(in crate::compile) is_recto: bool,
    pub(in crate::compile) live_area: Option<(f64, f64, f64, f64)>,
    pub(in crate::compile) page_index_by_node_id: &'a BTreeMap<String, usize>,
    /// This page's footnote markers: `footnote_id → marker_string` (auto-number
    /// or explicit override), in id order. A text span whose `footnote_ref` keys
    /// into this map emits that marker as an inline superscript run. Empty when
    /// the page declares no footnotes.
    pub(in crate::compile) footnote_markers: &'a BTreeMap<String, String>,
    /// This page's node id → ABSOLUTE page-coordinate bounding box `(x, y, w, h)`
    /// in pixels, accumulating group/instance translation (frames do not
    /// translate). Drives text-runaround exclusion lookup in `compile_text`. Only
    /// nodes with a fully-resolvable x/y/w/h rect are included. Empty when no node
    /// on the page has a resolvable box.
    pub(in crate::compile) node_boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    /// Total page count in `doc.body.pages`, for `page-count` field resolution.
    /// A `page-count` field resolves to this value as a decimal string (the "M"
    /// in a "Slide N of M" footer, where `page-number` supplies N).
    pub(in crate::compile) total_pages: usize,
    /// All document pages, for document-wide `toc` heading collection (a `toc`
    /// node scans every page). Borrowed from `doc.body.pages`; `compile_node`
    /// only has the `FieldCtx`, so the page list is carried here.
    pub(in crate::compile) pages: &'a [Page],
    /// 0-based index of THIS page within its section (page 1 of the section → 0).
    /// `None` when the page belongs to no declared section.
    pub(in crate::compile) section_page_index: Option<usize>,
    /// Total page count of THIS page's section. `None` when no section.
    pub(in crate::compile) section_page_count: Option<usize>,
    /// The section's `folio_start` (first folio number; defaults to 1 when the
    /// section omits it). `None` when no section.
    pub(in crate::compile) section_folio_start: Option<usize>,
    /// The section's folio numbering style ("decimal"/"lower-roman"/"upper-roman").
    /// `None` when no section or the section omits it.
    pub(in crate::compile) section_folio_style: Option<&'a str>,
    /// The section's display name (for a `section-name` field). `None` when no section.
    pub(in crate::compile) section_name: Option<&'a str>,
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
pub(in crate::compile) fn resolve_field_to_text(
    field: &FieldNode,
    ctx: &FieldCtx,
) -> Option<TextNode> {
    // Skip invisible fields entirely (mirror the text/leaf visible=false path).
    if field.visible == Some(false) {
        return None;
    }

    // Suppress numeric fields on the first page when requested.
    let is_numeric_type = matches!(
        field.field_type.as_str(),
        "page-number" | "page-count" | "page-ref" | "section-page-number" | "section-page-count"
    );
    if field.suppress_first.is_some_and(|v| v) && ctx.page_index_1based == 1 && is_numeric_type {
        return None;
    }

    let style = field.folio_style.as_deref();

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
        "page-number" => (format_folio(ctx.page_index_1based, style), "center"),
        "page-count" => (format_folio(ctx.total_pages, style), "center"),
        "page-ref" => {
            // Resolve the 1-based index of the page that contains `target`.
            let target = field.target.as_deref()?;
            let idx = ctx.page_index_by_node_id.get(target)?;
            (format_folio(*idx, style), "start")
        }
        "section-page-number" => {
            // Resolve a section-relative folio. Effective style: field's own
            // folio_style beats the section's, which beats decimal.
            let rel = ctx.section_page_index?;
            let folio_n = ctx.section_folio_start.unwrap_or(1) + rel;
            let effective_style = style.or(ctx.section_folio_style);
            (format_folio(folio_n, effective_style), "center")
        }
        "section-page-count" => {
            // Render the total page count of this page's section.
            let n = ctx.section_page_count?;
            let effective_style = style.or(ctx.section_folio_style);
            (format_folio(n, effective_style), "center")
        }
        "section-name" => {
            // Render the section's human-readable name.
            let name = ctx.section_name?;
            if name.is_empty() {
                return None;
            }
            (name.to_owned(), "center")
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
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: field.font_family.clone(),
        font_size: field.font_size.clone(),
        font_size_min: None,
        font_weight: None,
        shadow: None,
        filter: None,
        mask: None,
        blend_mode: None,
        blur: None,
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
        bullet: None,
        bullet_gap: None,
        anchor: None,
        anchor_zone: None,
        anchor_parent: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `page-count` field node (no geometry, no styling) for the
    /// resolution unit test.
    fn page_count_field() -> FieldNode {
        FieldNode {
            id: "total".to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_parent: None,
            name: None,
            role: None,
            field_type: "page-count".to_owned(),
            recto: None,
            verso: None,
            target: None,
            folio_style: None,
            suppress_first: None,
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
            pages: &[],
            footnote_markers: &markers,
            node_boxes: &boxes,
            total_pages: 5,
            section_page_index: None,
            section_page_count: None,
            section_folio_start: None,
            section_folio_style: None,
            section_name: None,
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

    // ── resolve_field_to_text: folio_style ────────────────────────────────────

    type CtxStores = (
        BTreeMap<String, usize>,
        BTreeMap<String, String>,
        BTreeMap<String, (f64, f64, f64, f64)>,
    );

    fn make_ctx() -> CtxStores {
        (BTreeMap::new(), BTreeMap::new(), BTreeMap::new())
    }

    /// The five section-relative fields, grouped to keep `ctx_with_section`
    /// under the argument-count lint.
    #[derive(Default)]
    struct SectionArgs<'a> {
        page_index: Option<usize>,
        page_count: Option<usize>,
        folio_start: Option<usize>,
        folio_style: Option<&'a str>,
        name: Option<&'a str>,
    }

    fn field_ctx<'a>(
        page: usize,
        total: usize,
        by_id: &'a BTreeMap<String, usize>,
        markers: &'a BTreeMap<String, String>,
        boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
    ) -> FieldCtx<'a> {
        FieldCtx {
            page_index_1based: page,
            is_recto: page % 2 == 1,
            live_area: None,
            page_index_by_node_id: by_id,
            pages: &[],
            footnote_markers: markers,
            node_boxes: boxes,
            total_pages: total,
            section_page_index: None,
            section_page_count: None,
            section_folio_start: None,
            section_folio_style: None,
            section_name: None,
        }
    }

    fn page_number_field(folio_style: Option<&str>, suppress_first: Option<bool>) -> FieldNode {
        FieldNode {
            id: "pn".to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_parent: None,
            name: None,
            role: None,
            field_type: "page-number".to_owned(),
            recto: None,
            verso: None,
            target: None,
            folio_style: folio_style.map(str::to_owned),
            suppress_first,
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

    fn running_head_field(suppress_first: Option<bool>) -> FieldNode {
        FieldNode {
            id: "rh".to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_parent: None,
            name: None,
            role: None,
            field_type: "running-head".to_owned(),
            recto: Some("Chapter One".to_owned()),
            verso: Some("My Book".to_owned()),
            target: None,
            folio_style: None,
            suppress_first,
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

    fn span_text(node: &TextNode) -> &str {
        node.spans.first().map(|s| s.text.as_str()).unwrap_or("")
    }

    #[test]
    fn page_number_lower_roman_on_page_3() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(3, 10, &by_id, &markers, &boxes);
        let field = page_number_field(Some("lower-roman"), None);
        let text =
            resolve_field_to_text(&field, &ctx).expect("page-number with lower-roman must resolve");
        assert_eq!(span_text(&text), "iii");
    }

    #[test]
    fn page_number_upper_roman_on_page_4() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(4, 10, &by_id, &markers, &boxes);
        let field = page_number_field(Some("upper-roman"), None);
        let text =
            resolve_field_to_text(&field, &ctx).expect("page-number with upper-roman must resolve");
        assert_eq!(span_text(&text), "IV");
    }

    #[test]
    fn page_number_no_folio_style_is_decimal() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(5, 10, &by_id, &markers, &boxes);
        let field = page_number_field(None, None);
        let text = resolve_field_to_text(&field, &ctx)
            .expect("page-number without folio-style must resolve");
        assert_eq!(span_text(&text), "5");
    }

    #[test]
    fn page_number_unknown_folio_style_falls_back_to_decimal() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(7, 10, &by_id, &markers, &boxes);
        let field = page_number_field(Some("klingon"), None);
        let text = resolve_field_to_text(&field, &ctx)
            .expect("page-number with unknown folio-style must resolve (decimal fallback)");
        assert_eq!(span_text(&text), "7");
    }

    // ── resolve_field_to_text: suppress_first ─────────────────────────────────

    #[test]
    fn suppress_first_hides_numeric_field_on_page_1() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(1, 10, &by_id, &markers, &boxes);
        let field = page_number_field(None, Some(true));
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "suppress-first=true on page 1 must return None"
        );
    }

    #[test]
    fn suppress_first_allows_numeric_field_on_page_2() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(2, 10, &by_id, &markers, &boxes);
        let field = page_number_field(None, Some(true));
        let text = resolve_field_to_text(&field, &ctx)
            .expect("suppress-first=true on page 2 must resolve normally");
        assert_eq!(span_text(&text), "2");
    }

    #[test]
    fn suppress_first_does_not_suppress_running_head_on_page_1() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = field_ctx(1, 10, &by_id, &markers, &boxes);
        let field = running_head_field(Some(true));
        // running-head is not a numeric type; suppress_first must be ignored.
        let text = resolve_field_to_text(&field, &ctx)
            .expect("suppress-first must NOT suppress running-head on page 1");
        assert_eq!(span_text(&text), "Chapter One");
    }

    // ── resolve_field_to_text: section-page-number ────────────────────────────

    fn section_field(field_type: &str, folio_style: Option<&str>) -> FieldNode {
        FieldNode {
            id: "sf".to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_parent: None,
            name: None,
            role: None,
            field_type: field_type.to_owned(),
            recto: None,
            verso: None,
            target: None,
            folio_style: folio_style.map(str::to_owned),
            suppress_first: None,
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

    fn ctx_with_section<'a>(
        page_1based: usize,
        total: usize,
        by_id: &'a BTreeMap<String, usize>,
        markers: &'a BTreeMap<String, String>,
        boxes: &'a BTreeMap<String, (f64, f64, f64, f64)>,
        sec: SectionArgs<'a>,
    ) -> FieldCtx<'a> {
        FieldCtx {
            page_index_1based: page_1based,
            is_recto: page_1based % 2 == 1,
            live_area: None,
            page_index_by_node_id: by_id,
            pages: &[],
            footnote_markers: markers,
            node_boxes: boxes,
            total_pages: total,
            section_page_index: sec.page_index,
            section_page_count: sec.page_count,
            section_folio_start: sec.folio_start,
            section_folio_style: sec.folio_style,
            section_name: sec.name,
        }
    }

    #[test]
    fn section_page_number_first_body_page_is_1() {
        let (by_id, markers, boxes) = make_ctx();
        // 3rd page overall (page_1based=3), first page of body section (rel=0),
        // folio_start=1 → folio = 1+0 = 1 → "1"
        let ctx = ctx_with_section(
            3,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(0),
                page_count: Some(3),
                folio_start: Some(1),
                name: Some("Body"),
                ..SectionArgs::default()
            },
        );
        let field = section_field("section-page-number", None);
        let text = resolve_field_to_text(&field, &ctx)
            .expect("section-page-number on first body page must resolve");
        assert_eq!(span_text(&text), "1");
    }

    #[test]
    fn section_page_number_front_matter_second_page_lower_roman() {
        let (by_id, markers, boxes) = make_ctx();
        // 2nd front-matter page (rel=1, folio_start=1): folio = 1+1 = 2 → "ii"
        let ctx = ctx_with_section(
            2,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(1),
                page_count: Some(2),
                folio_start: Some(1),
                folio_style: Some("lower-roman"),
                name: Some("Front"),
            },
        );
        let field = section_field("section-page-number", None);
        let text = resolve_field_to_text(&field, &ctx)
            .expect("section-page-number with lower-roman must resolve");
        assert_eq!(span_text(&text), "ii");
    }

    #[test]
    fn section_page_number_field_folio_style_overrides_section() {
        // Field-level folio_style="upper-roman" beats section style="lower-roman".
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(
            2,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(1),
                page_count: Some(2),
                folio_start: Some(1),
                folio_style: Some("lower-roman"),
                name: Some("Front"),
            },
        );
        let field = section_field("section-page-number", Some("upper-roman"));
        let text =
            resolve_field_to_text(&field, &ctx).expect("field folio_style override must resolve");
        // folio = 1+1 = 2 → "II" (upper-roman)
        assert_eq!(span_text(&text), "II");
    }

    #[test]
    fn section_page_number_no_section_returns_none() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(1, 5, &by_id, &markers, &boxes, SectionArgs::default());
        let field = section_field("section-page-number", None);
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "section-page-number with no section must return None"
        );
    }

    #[test]
    fn section_page_count_returns_section_count() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(
            3,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(0),
                page_count: Some(3),
                folio_start: Some(1),
                name: Some("Body"),
                ..SectionArgs::default()
            },
        );
        let field = section_field("section-page-count", None);
        let text = resolve_field_to_text(&field, &ctx).expect("section-page-count must resolve");
        assert_eq!(span_text(&text), "3");
    }

    #[test]
    fn section_page_count_no_section_returns_none() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(1, 5, &by_id, &markers, &boxes, SectionArgs::default());
        let field = section_field("section-page-count", None);
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "section-page-count with no section must return None"
        );
    }

    #[test]
    fn section_name_returns_name() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(
            3,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(0),
                page_count: Some(3),
                folio_start: Some(1),
                name: Some("Chapter One"),
                ..SectionArgs::default()
            },
        );
        let field = section_field("section-name", None);
        let text = resolve_field_to_text(&field, &ctx).expect("section-name must resolve");
        assert_eq!(span_text(&text), "Chapter One");
    }

    #[test]
    fn section_name_empty_returns_none() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(
            1,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(0),
                page_count: Some(1),
                folio_start: Some(1),
                name: Some(""),
                ..SectionArgs::default()
            },
        );
        let field = section_field("section-name", None);
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "section-name with empty name must return None"
        );
    }

    #[test]
    fn section_name_no_section_returns_none() {
        let (by_id, markers, boxes) = make_ctx();
        let ctx = ctx_with_section(1, 5, &by_id, &markers, &boxes, SectionArgs::default());
        let field = section_field("section-name", None);
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "section-name with no section must return None"
        );
    }

    #[test]
    fn suppress_first_hides_section_page_number_on_page_1() {
        let (by_id, markers, boxes) = make_ctx();
        // Document page 1 is also section page 0 (first page of front matter).
        let mut ctx = ctx_with_section(
            1,
            5,
            &by_id,
            &markers,
            &boxes,
            SectionArgs {
                page_index: Some(0),
                page_count: Some(2),
                folio_start: Some(1),
                name: Some("Front"),
                ..SectionArgs::default()
            },
        );
        // Use page_index_1based = 1 to trigger suppress_first.
        ctx.page_index_1based = 1;
        let mut field = section_field("section-page-number", None);
        field.suppress_first = Some(true);
        assert!(
            resolve_field_to_text(&field, &ctx).is_none(),
            "suppress-first=true on page 1 must suppress section-page-number"
        );
    }
}
