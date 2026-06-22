//! Per-page section assignment: which declared `section` a page belongs to and
//! its position within that section, used to resolve section-relative folio
//! fields (`section-page-number`/`section-page-count`/`section-name`).

use zenith_core::Document;

/// Per-page section assignment: the section this page belongs to and its
/// position within that section. `Copy` because all string data is borrowed
/// from the source `Document` (whose lifetime outlives the whole compile).
#[derive(Clone, Copy)]
pub(in crate::compile) struct SectionAssignment<'a> {
    /// 0-based index of this page within its section.
    pub(in crate::compile) page_index_in_section: usize,
    /// Total number of pages in this section.
    pub(in crate::compile) page_count: usize,
    /// First folio number for this section (1 when the section omits it).
    pub(in crate::compile) folio_start: usize,
    /// Folio style declared on the section (`None` defaults to decimal).
    pub(in crate::compile) folio_style: Option<&'a str>,
    /// Human-readable section name.
    pub(in crate::compile) name: &'a str,
}

/// Build a per-page section-assignment vector for the document.
///
/// Returns a `Vec` with one entry per page (indexed by 0-based page index,
/// same length as `doc.body.pages`). Each entry is `Some(SectionAssignment)`
/// when the page falls within a declared section range, or `None` when it
/// precedes the first section (or the document declares no sections).
///
/// Algorithm:
/// 1. Build a `page id → 0-based page index` map.
/// 2. Resolve each section's start page to an index (skip sections whose
///    `start_page` id is not found — the validator already flags these).
/// 3. Stable-sort the resolved sections by start index ascending.
/// 4. Walk sorted sections to compute `[start, end)` ranges, where `end` is
///    the next section's start (or `doc.body.pages.len()` for the last).
/// 5. Fill the output vector: pages in a range get an assignment; pages before
///    the first section start get `None`.
pub(in crate::compile) fn build_section_assignments(
    doc: &Document,
) -> Vec<Option<SectionAssignment<'_>>> {
    use std::collections::BTreeMap;

    let total_pages = doc.body.pages.len();

    // Build page-id → 0-based index map (ordered for determinism).
    let page_index_map: BTreeMap<&str, usize> = doc
        .body
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();

    // Resolve sections to (start_index, &SectionDef), skipping unknowns.
    let mut resolved: Vec<(usize, &zenith_core::SectionDef)> = doc
        .sections
        .iter()
        .filter_map(|sec| {
            let idx = page_index_map.get(sec.start_page.as_str()).copied()?;
            Some((idx, sec))
        })
        .collect();

    // Stable-sort by start index (ties keep declaration order).
    resolved.sort_by_key(|(idx, _)| *idx);

    // Pre-allocate output with all None.
    let mut out: Vec<Option<SectionAssignment<'_>>> = vec![None; total_pages];

    for (i, &(start_idx, sec)) in resolved.iter().enumerate() {
        // end_idx: next section's start, or end of doc.
        let end_idx = resolved
            .get(i + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(total_pages);

        let page_count = end_idx.saturating_sub(start_idx);
        let folio_start = sec.folio_start.unwrap_or(1);
        let folio_style = sec.folio_style.as_deref();
        let name = sec.name.as_str();

        for page_idx in start_idx..end_idx {
            if let Some(slot) = out.get_mut(page_idx) {
                *slot = Some(SectionAssignment {
                    page_index_in_section: page_idx - start_idx,
                    page_count,
                    folio_start,
                    folio_style,
                    name,
                });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::util::px;

    /// Build a minimal Document with N pages and the provided sections.
    ///
    /// Pages are given sequential ids "p0", "p1", … "pN-1". The document body
    /// id is "body". Sections are appended in declaration order.
    fn doc_with_pages_and_sections(
        page_count: usize,
        sections: Vec<zenith_core::SectionDef>,
    ) -> Document {
        use zenith_core::{KdlAdapter, KdlSource};
        // Parse a minimal valid skeleton (no pages) and then patch it.
        let mut doc = KdlAdapter
            .parse(b"zenith version=1 { document id=\"d\" { } }")
            .expect("minimal test document must parse");
        for i in 0..page_count {
            doc.body.pages.push(zenith_core::Page {
                id: format!("p{i}"),
                name: None,
                width: px(100.0),
                height: px(100.0),
                background: None,
                bleed: None,
                margin_inner: None,
                margin_outer: None,
                margin_top: None,
                margin_bottom: None,
                baseline_grid: None,
                parity: None,
                master: None,
                safe_zones: Vec::new(),
                folds: Vec::new(),
                children: Vec::new(),
                source_span: None,
            });
        }
        doc.sections = sections;
        doc
    }

    fn make_section(
        id: &str,
        name: &str,
        start_page: &str,
        folio_start: Option<usize>,
        folio_style: Option<&str>,
    ) -> zenith_core::SectionDef {
        zenith_core::SectionDef {
            id: id.to_owned(),
            name: name.to_owned(),
            folio_start,
            folio_style: folio_style.map(str::to_owned),
            start_page: start_page.to_owned(),
            source_span: None,
        }
    }

    #[test]
    fn build_section_assignments_two_sections_five_pages() {
        // front matter: pages 0–1 (ids "p0","p1"), body: pages 2–4 ("p2","p3","p4")
        let doc = doc_with_pages_and_sections(
            5,
            vec![
                make_section(
                    "sec.front",
                    "Front Matter",
                    "p0",
                    Some(1),
                    Some("lower-roman"),
                ),
                make_section("sec.body", "Body", "p2", Some(1), None),
            ],
        );
        let assignments = build_section_assignments(&doc);
        assert_eq!(assignments.len(), 5);

        // Front matter pages
        let a0 = assignments[0].expect("p0 must have an assignment");
        assert_eq!(a0.page_index_in_section, 0);
        assert_eq!(a0.page_count, 2);
        assert_eq!(a0.folio_start, 1);
        assert_eq!(a0.folio_style, Some("lower-roman"));
        assert_eq!(a0.name, "Front Matter");

        let a1 = assignments[1].expect("p1 must have an assignment");
        assert_eq!(a1.page_index_in_section, 1);
        assert_eq!(a1.page_count, 2);
        assert_eq!(a1.name, "Front Matter");

        // Body pages
        let a2 = assignments[2].expect("p2 must have an assignment");
        assert_eq!(a2.page_index_in_section, 0);
        assert_eq!(a2.page_count, 3);
        assert_eq!(a2.folio_start, 1);
        assert_eq!(a2.folio_style, None);
        assert_eq!(a2.name, "Body");

        let a4 = assignments[4].expect("p4 must have an assignment");
        assert_eq!(a4.page_index_in_section, 2);
        assert_eq!(a4.page_count, 3);
    }

    #[test]
    fn build_section_assignments_page_before_first_section_is_none() {
        // Section starts at p2; pages p0 and p1 are before it → None.
        let doc = doc_with_pages_and_sections(
            4,
            vec![make_section("sec.body", "Body", "p2", None, None)],
        );
        let assignments = build_section_assignments(&doc);
        assert!(assignments[0].is_none(), "p0 is before the first section");
        assert!(assignments[1].is_none(), "p1 is before the first section");
        assert!(assignments[2].is_some(), "p2 starts the section");
        assert!(assignments[3].is_some(), "p3 is in the section");
    }

    #[test]
    fn build_section_assignments_unknown_start_page_is_skipped() {
        // A section referencing a non-existent page id must be silently ignored.
        let doc = doc_with_pages_and_sections(
            2,
            vec![make_section("sec.x", "X", "no-such-page", None, None)],
        );
        let assignments = build_section_assignments(&doc);
        assert!(assignments[0].is_none());
        assert!(assignments[1].is_none());
    }
}
