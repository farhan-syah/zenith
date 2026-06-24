//! Page-structure ops: `AddPage` / `DeletePage` / `SetPageSize` / `ReorderPages`,
//! plus the canonical dimension-string parser they share with geometry/token ops.

use zenith_core::{Diagnostic, Dimension, Document, Page, PropertyValue, Unit};

use super::super::record_affected;

/// Parse a canonical `"(unit)value"` dimension string (e.g. `"(px)1800"`) into
/// a [`Dimension`], preserving its unit.
///
/// Mirrors the parser used by geometry ops but keeps the unit (rather than
/// collapsing to px) because [`Page::width`]/[`Page::height`] store a full
/// `Dimension`. Returns `None` if the string is not parenthesized-unit-prefixed
/// or the numeric tail is not a finite number.
pub(in crate::engine) fn parse_dimension_str(s: &str) -> Option<Dimension> {
    let rest = s.strip_prefix('(')?;
    let (unit_str, value_str) = rest.split_once(')')?;
    let unit = Unit::from_annotation(unit_str);
    let value: f64 = value_str.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(Dimension { value, unit })
}

/// The borrowed fields of an [`crate::op::Op::AddPage`], grouped so the apply
/// function stays under the argument-count lint while keeping each field named.
pub(in crate::engine) struct AddPageSpec<'a> {
    /// Stable id for the new page.
    pub id: &'a str,
    /// Width dimension string, e.g. `"(px)1800"`.
    pub w: &'a str,
    /// Height dimension string, e.g. `"(px)1200"`.
    pub h: &'a str,
    /// Optional background token-ref id.
    pub background: Option<&'a str>,
    /// 0-based insert position; `None` appends.
    pub index: Option<usize>,
}

pub(in crate::engine) fn apply_add_page(
    spec: &AddPageSpec<'_>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let AddPageSpec {
        id,
        w,
        h,
        background,
        index,
    } = *spec;
    // 1. Reject a page id that collides with an existing page id. (A collision
    //    with a non-page node id is also caught by post-validation's
    //    id.duplicate; the page-level check here gives a precise message.)
    if doc.body.pages.iter().any(|p| p.id == id) {
        diagnostics.push(Diagnostic::error(
            "tx.duplicate_id",
            format!("add_page: a page with id {:?} already exists", id),
            None,
            Some(id.to_owned()),
        ));
        return;
    }

    // 2. Parse the width/height dimension strings.
    let Some(width) = parse_dimension_str(w) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "add_page: width {:?} is not a valid dimension (expected e.g. \"(px)1800\")",
                w
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };
    let Some(height) = parse_dimension_str(h) else {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "add_page: height {:?} is not a valid dimension (expected e.g. \"(px)1200\")",
                h
            ),
            None,
            Some(id.to_owned()),
        ));
        return;
    };

    // 3. Resolve the insert position. `None` appends; an explicit index must be
    //    within `0..=len` (len = append).
    let len = doc.body.pages.len();
    let at = match index {
        None => len,
        Some(i) => {
            if i > len {
                diagnostics.push(Diagnostic::error(
                    "tx.out_of_range",
                    format!(
                        "add_page: index {} is out of range (document has {} page(s))",
                        i, len
                    ),
                    None,
                    Some(id.to_owned()),
                ));
                return;
            }
            i
        }
    };

    // 4. Build the empty page with all optional fields at their defaults.
    let page = Page {
        id: id.to_owned(),
        name: None,
        width,
        height,
        background: background.map(|b| PropertyValue::TokenRef(b.to_owned())),
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
        children: Vec::new(),
        source_span: None,
    };

    doc.body.pages.insert(at, page);
    record_affected(id, affected);
}

pub(in crate::engine) fn apply_delete_page(
    page_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let Some(pos) = doc.body.pages.iter().position(|p| p.id == page_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("delete_page: page {:?} not found", page_id),
            None,
            Some(page_id.to_owned()),
        ));
        return;
    };
    doc.body.pages.remove(pos);
    record_affected(page_id, affected);
}

/// Resize a page by replacing its `width`/`height` dimensions.
///
/// Parses `w` and `h` via [`parse_dimension_str`], then locates the page by id
/// and overwrites its size. Children are NOT reflowed — any child that now falls
/// outside the new bounds will generate an `off_canvas` advisory at validation.
pub(in crate::engine) fn apply_set_page_size(
    page_id: &str,
    w: &str,
    h: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Parse width — must be a valid dimension and strictly positive.
    let w_dim = match parse_dimension_str(w) {
        Some(d) if d.value > 0.0 => d,
        Some(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!("set_page_size: w {:?} must be a finite value > 0", w),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "set_page_size: w {:?} is not a valid dimension (expected e.g. \"(px)794\")",
                    w
                ),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
    };

    // Parse height — must be a valid dimension and strictly positive.
    let h_dim = match parse_dimension_str(h) {
        Some(d) if d.value > 0.0 => d,
        Some(_) => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!("set_page_size: h {:?} must be a finite value > 0", h),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.invalid_value",
                format!(
                    "set_page_size: h {:?} is not a valid dimension (expected e.g. \"(px)1123\")",
                    h
                ),
                None,
                Some(page_id.to_owned()),
            ));
            return;
        }
    };

    // Locate the page by id.
    let Some(page) = doc.body.pages.iter_mut().find(|p| p.id == page_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("set_page_size: page {:?} not found", page_id),
            None,
            Some(page_id.to_owned()),
        ));
        return;
    };

    page.width = w_dim;
    page.height = h_dim;
    record_affected(page_id, affected);
}

pub(in crate::engine) fn apply_reorder_pages(
    order: &[String],
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // The current page ids in document order.
    let current: Vec<String> = doc.body.pages.iter().map(|p| p.id.clone()).collect();

    // `order` must be a permutation of `current`: same length, no duplicates,
    // and every requested id must exist exactly once. We verify via sorted
    // comparison (deterministic, no HashMap).
    let mut order_sorted: Vec<&String> = order.iter().collect();
    order_sorted.sort();
    let dup = order_sorted.windows(2).any(|w| w[0] == w[1]);
    let mut current_sorted: Vec<&String> = current.iter().collect();
    current_sorted.sort();

    if dup || order_sorted != current_sorted {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "reorder_pages: order {:?} is not a permutation of the existing \
                 page ids {:?}",
                order, current
            ),
            None,
            None,
        ));
        return;
    }

    // Rebuild the page vec in the requested order. Each id resolves to exactly
    // one page (guaranteed by the permutation check above). We drain the old
    // pages into a lookup-by-index without HashMap: for each target id, find and
    // take its page from a slot-tracking vec.
    let mut slots: Vec<Option<Page>> = doc.body.pages.drain(..).map(Some).collect();
    let mut reordered: Vec<Page> = Vec::with_capacity(order.len());
    for id in order {
        // Find the first remaining slot whose page id matches. The permutation
        // check guarantees a match exists for every id.
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.as_ref().map(|p| p.id.as_str()) == Some(id.as_str()))
            && let Some(page) = slot.take()
        {
            reordered.push(page);
        }
    }
    doc.body.pages = reordered;

    // Record every page id as affected (the whole list was restructured).
    for id in order {
        record_affected(id, affected);
    }
}
