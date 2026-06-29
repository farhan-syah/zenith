//! TOC-node resolution: turn a [`TocNode`] into a multi-line tab-leader
//! [`TextNode`] by collecting heading nodes across the whole document.
//!
//! A `toc` is a LEAF that, at compile time, scans every page's nodes for
//! `text` nodes that match its selector (`match-role` and/or `match-style`).
//! Each matched node contributes one row to the output:
//!
//! ```text
//! {heading text}\t{page number}
//! ```
//!
//! Rows are joined with `"\n"`. The synthesised [`TextNode`] enables
//! `tab-leader` mode so the text engine fills the tab gap with the configured
//! leader glyph (default `"."`) and right-aligns the page number. This reuses
//! the existing tab-leader rendering path verbatim.

use std::collections::BTreeMap;

use zenith_core::{Node, Page, TextNode, TextSpan, TocNode};

use super::field::format_folio;

/// Resolve a [`TocNode`] to a multi-line tab-leader [`TextNode`], or `None`
/// when the toc should produce no output.
///
/// Returns `None` when:
/// - `toc.visible == Some(false)`
/// - both `match_role` and `match_style` are `None` (no selector)
/// - no heading entries are found after walking all pages
pub(super) fn resolve_toc_to_text(
    toc: &TocNode,
    pages: &[Page],
    page_index_by_node_id: &BTreeMap<String, usize>,
) -> Option<TextNode> {
    // Invisible toc nodes produce nothing.
    if toc.visible == Some(false) {
        return None;
    }

    // Without a selector the toc collects nothing (the validator warns separately
    // via `toc.no_selector`).
    if toc.match_role.is_none() && toc.match_style.is_none() {
        return None;
    }

    // Walk all pages in order, recursing into Frame/Group children, collecting
    // text nodes that match the selector.
    let mut entries: Vec<(String, usize)> = Vec::new(); // (title, 1-based page index)
    for (page_idx0, page) in pages.iter().enumerate() {
        let page_1based = page_idx0 + 1;
        collect_entries(
            &page.children,
            page_1based,
            toc.match_role.as_deref(),
            toc.match_style.as_deref(),
            page_index_by_node_id,
            &mut entries,
        );
    }

    if entries.is_empty() {
        return None;
    }

    // Format rows: "title\tfolio", joined by newlines.
    let folio_style = toc.folio_style.as_deref();
    let rows: Vec<String> = entries
        .into_iter()
        .map(|(title, page_n)| {
            let folio = format_folio(page_n, folio_style);
            format!("{title}\t{folio}")
        })
        .collect();
    let combined = rows.join("\n");

    // Synthesise a TextNode that uses tab-leader mode. Geometry falls back to
    // toc.x/y/w/h (the caller has no live-area context here; the toc must
    // declare its own geometry for correct positioning).
    let leader = toc
        .leader
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".to_owned());

    Some(TextNode {
        id: toc.id.clone(),
        name: toc.name.clone(),
        role: toc.role.clone(),
        x: toc.x.clone(),
        y: toc.y.clone(),
        w: toc.w.clone(),
        h: toc.h.clone(),
        align: Some("start".to_owned()),
        v_align: None,
        direction: None,
        overflow: Some("clip".to_owned()),
        overflow_wrap: None,
        style: toc.style.clone(),
        fill: toc.fill.clone(),
        stroke: None,
        stroke_width: None,
        contrast_bg: None,
        font_family: toc.font_family.clone(),
        font_size: toc.font_size.clone(),
        font_size_min: None,
        font_weight: None,
        shadow: None,
        filter: None,
        mask: None,
        blend_mode: None,
        blur: None,
        opacity: toc.opacity,
        visible: toc.visible,
        locked: toc.locked,
        selectable: None,
        rotate: None,
        chain: None,
        drop_cap_lines: None,
        hyphenate: None,
        widow_orphan: None,
        tab_leader: Some(leader),
        text_exclusion: None,
        padding_left: None,
        text_indent: None,
        content_format: None,
        src: None,
        bullet: None,
        bullet_gap: None,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        block_styles: Vec::new(),
        spans: vec![TextSpan {
            text: combined,
            fill: None,
            font_weight: None,
            italic: None,
            underline: None,
            strikethrough: None,
            vertical_align: None,
            footnote_ref: None,
            data_ref: None,
            data_format: None,
            highlight: None,
            code: None,
            link: None,
        }],
        source_span: toc.source_span,
        unknown_props: BTreeMap::new(),
    })
}

/// Recursively collect `(title, page_1based)` entries from `children`,
/// descending into Frame and Group containers.
///
/// A node is collected when it is a `Node::Text` AND it satisfies the
/// selector: `match_role` matches the text node's `role` (when set) AND
/// `match_style` matches the text node's `style` (when set). At least one of
/// `match_role`/`match_style` is non-None (the no-selector case is handled
/// by the caller before this function is reached).
///
/// Title: concatenation of the text node's spans' `text` fields.
/// Entries with an empty title are skipped.
fn collect_entries(
    children: &[Node],
    page_1based: usize,
    match_role: Option<&str>,
    match_style: Option<&str>,
    page_index_by_node_id: &BTreeMap<String, usize>,
    entries: &mut Vec<(String, usize)>,
) {
    for node in children {
        match node {
            Node::Text(t) => {
                let role_match = match_role.is_none() || t.role.as_deref() == match_role;
                let style_match = match_style.is_none() || t.style.as_deref() == match_style;
                if role_match && style_match {
                    // Concatenate all span texts for the title.
                    let title: String = t.spans.iter().map(|s| s.text.as_str()).collect();
                    if !title.is_empty() {
                        // Use the node's page index from the pre-built map when
                        // available (handles master-page projections); fall back to
                        // the current page's index.
                        let page_n = page_index_by_node_id
                            .get(&t.id)
                            .copied()
                            .unwrap_or(page_1based);
                        entries.push((title, page_n));
                    }
                }
            }
            Node::Frame(f) => collect_entries(
                &f.children,
                page_1based,
                match_role,
                match_style,
                page_index_by_node_id,
                entries,
            ),
            Node::Group(g) => collect_entries(
                &g.children,
                page_1based,
                match_role,
                match_style,
                page_index_by_node_id,
                entries,
            ),
            Node::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_entries(
                            &cell.children,
                            page_1based,
                            match_role,
                            match_style,
                            page_index_by_node_id,
                            entries,
                        );
                    }
                }
            }
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Toc(_)
            | Node::Footnote(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Unknown(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zenith_core::{Node, Page, TextNode, TextSpan, TocNode};

    use super::resolve_toc_to_text;
    use crate::compile::util::{px, px_prop};

    fn make_span(text: &str) -> TextSpan {
        TextSpan {
            text: text.to_owned(),
            fill: None,
            font_weight: None,
            italic: None,
            underline: None,
            strikethrough: None,
            vertical_align: None,
            footnote_ref: None,
            data_ref: None,
            data_format: None,
            highlight: None,
            code: None,
            link: None,
        }
    }

    fn heading_text(id: &str, role: &str, text: &str) -> Node {
        Node::Text(Box::new(TextNode {
            id: id.to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_sibling: None,
            anchor_edge: None,
            anchor_gap: None,
            anchor_parent: None,
            name: None,
            role: Some(role.to_owned()),
            x: Some(px_prop(0.0)),
            y: Some(px_prop(0.0)),
            w: Some(px_prop(100.0)),
            h: Some(px_prop(20.0)),
            align: None,
            v_align: None,
            direction: None,
            overflow: None,
            overflow_wrap: None,
            style: None,
            fill: None,
            stroke: None,
            stroke_width: None,
            contrast_bg: None,
            font_family: None,
            font_size: None,
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
            selectable: None,
            rotate: None,
            chain: None,
            drop_cap_lines: None,
            hyphenate: None,
            widow_orphan: None,
            tab_leader: None,
            text_exclusion: None,
            padding_left: None,
            text_indent: None,
            content_format: None,
            src: None,
            bullet: None,
            bullet_gap: None,
            spans: vec![make_span(text)],
            block_styles: Vec::new(),
            source_span: None,
            unknown_props: BTreeMap::new(),
        }))
    }

    fn bare_page(id: &str, children: Vec<Node>) -> Page {
        Page {
            id: id.to_owned(),
            name: None,
            width: px(595.0),
            height: px(842.0),
            background: None,
            bleed: None,
            margin_inner: None,
            margin_outer: None,
            margin_top: None,
            margin_bottom: None,
            baseline_grid: None,
            line_jumps: None,
            parity: None,
            master: None,
            safe_zones: Vec::new(),
            folds: Vec::new(),
            block_styles: Vec::new(),
            children,
            source_span: None,
        }
    }

    fn toc_node(match_role: Option<&str>, match_style: Option<&str>) -> TocNode {
        TocNode {
            id: "toc.main".to_owned(),
            anchor: None,
            anchor_zone: None,
            anchor_sibling: None,
            anchor_edge: None,
            anchor_gap: None,
            anchor_parent: None,
            name: None,
            role: None,
            match_role: match_role.map(str::to_owned),
            match_style: match_style.map(str::to_owned),
            leader: None,
            folio_style: None,
            x: Some(px_prop(50.0)),
            y: Some(px_prop(100.0)),
            w: Some(px_prop(400.0)),
            h: Some(px_prop(300.0)),
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

    fn build_page_index(pages: &[Page]) -> BTreeMap<String, usize> {
        let mut map = BTreeMap::new();
        for (idx0, page) in pages.iter().enumerate() {
            index_nodes_for_test(&page.children, idx0 + 1, &mut map);
        }
        map
    }

    fn index_nodes_for_test(nodes: &[Node], page_1based: usize, map: &mut BTreeMap<String, usize>) {
        for node in nodes {
            match node {
                Node::Text(t) => {
                    map.entry(t.id.clone()).or_insert(page_1based);
                }
                Node::Frame(f) => index_nodes_for_test(&f.children, page_1based, map),
                Node::Group(g) => index_nodes_for_test(&g.children, page_1based, map),
                _ => {}
            }
        }
    }

    #[test]
    fn toc_three_headings_across_three_pages() {
        let pages = vec![
            bare_page("p1", vec![heading_text("h1", "heading", "Heading A")]),
            bare_page("p2", vec![heading_text("h2", "heading", "Heading B")]),
            bare_page("p3", vec![heading_text("h3", "heading", "Heading C")]),
        ];
        let page_index = build_page_index(&pages);
        let toc = toc_node(Some("heading"), None);

        let result = resolve_toc_to_text(&toc, &pages, &page_index)
            .expect("toc with matching headings must resolve");

        assert_eq!(result.spans.len(), 1);
        assert_eq!(
            result.spans[0].text,
            "Heading A\t1\nHeading B\t2\nHeading C\t3"
        );
        assert_eq!(result.tab_leader.as_deref(), Some("."));
    }

    #[test]
    fn toc_lower_roman_folio_style() {
        let pages = vec![
            bare_page("p1", vec![heading_text("h1", "heading", "Intro")]),
            bare_page("p2", vec![heading_text("h2", "heading", "Chapter")]),
            bare_page("p3", vec![heading_text("h3", "heading", "Epilogue")]),
        ];
        let page_index = build_page_index(&pages);
        let mut toc = toc_node(Some("heading"), None);
        toc.folio_style = Some("lower-roman".to_owned());

        let result = resolve_toc_to_text(&toc, &pages, &page_index)
            .expect("toc with lower-roman must resolve");

        assert_eq!(result.spans[0].text, "Intro\ti\nChapter\tii\nEpilogue\tiii");
    }

    #[test]
    fn toc_no_matching_headings_returns_none() {
        let pages = vec![bare_page(
            "p1",
            vec![heading_text("h1", "body", "Regular text")],
        )];
        let page_index = build_page_index(&pages);
        let toc = toc_node(Some("heading"), None);

        assert!(
            resolve_toc_to_text(&toc, &pages, &page_index).is_none(),
            "toc with no matching headings must return None"
        );
    }

    #[test]
    fn toc_no_selector_returns_none() {
        let pages = vec![bare_page(
            "p1",
            vec![heading_text("h1", "heading", "Heading A")],
        )];
        let page_index = build_page_index(&pages);
        let toc = toc_node(None, None);

        assert!(
            resolve_toc_to_text(&toc, &pages, &page_index).is_none(),
            "toc with neither match-role nor match-style must return None"
        );
    }

    #[test]
    fn toc_visible_false_returns_none() {
        let pages = vec![bare_page(
            "p1",
            vec![heading_text("h1", "heading", "Heading A")],
        )];
        let page_index = build_page_index(&pages);
        let mut toc = toc_node(Some("heading"), None);
        toc.visible = Some(false);

        assert!(
            resolve_toc_to_text(&toc, &pages, &page_index).is_none(),
            "toc with visible=false must return None"
        );
    }

    #[test]
    fn toc_custom_leader_glyph() {
        let pages = vec![bare_page(
            "p1",
            vec![heading_text("h1", "heading", "Chapter One")],
        )];
        let page_index = build_page_index(&pages);
        let mut toc = toc_node(Some("heading"), None);
        toc.leader = Some("·".to_owned());

        let result = resolve_toc_to_text(&toc, &pages, &page_index)
            .expect("toc with custom leader must resolve");

        assert_eq!(result.tab_leader.as_deref(), Some("·"));
    }
}
