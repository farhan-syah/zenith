//! Content-based table sizing: the SINGLE column-width / row-height computation
//! shared by [`super::emit::compile_table`] and the multi-page flow pre-pass, plus
//! the per-cell natural-width / wrapped-height measurers.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, FontProvider, Node, ResolvedToken, Style, TableColumn, dim_to_px};

use super::super::text::{
    MeasureEnv, measure_text_natural, measure_text_wrapped_height, resolve_text_families,
};
use super::super::util::resolve_geometry_px;
use super::place::{PlacedCell, child_declared_box, child_declared_y};

/// Lower bound (px) a shrunk AUTO column is clamped to, so proportional shrink
/// to fit never collapses a column to zero width (which would hide its border).
const MIN_AUTO_COL_W: f64 = 2.0;

/// Scalar grid geometry for [`compute_table_layout`]: the resolved grid extents,
/// inter-cell gap, and cell padding plus the table box width/height in pixels.
/// Bundled into a single `Copy` value so the sizing entry point stays under the
/// argument-count lint without suppression.
#[derive(Clone, Copy)]
pub(in crate::compile) struct GridDims {
    pub(in crate::compile) col_count: usize,
    pub(in crate::compile) row_count: usize,
    pub(in crate::compile) gap: f64,
    pub(in crate::compile) pad: f64,
    pub(in crate::compile) table_w: f64,
    pub(in crate::compile) table_h: f64,
}

/// The resolved per-column widths and per-row heights of a table grid, computed
/// ONCE from the placed cells and reused by both [`super::emit::compile_table`]
/// (for emission) and the multi-page flow pre-pass in [`super::super::table_flow`]
/// (for fit decisions).
///
/// `col_widths`/`row_heights` are FINAL (auto-column + row shrink-to-fit applied),
/// while `row_natural` holds the per-row content height BEFORE any vertical shrink
/// — the pre-pass measures row fit against the natural heights so a member's box
/// height decides the body-row slice deterministically.
pub(in crate::compile) struct TableLayout {
    pub(in crate::compile) col_widths: Vec<f64>,
    pub(in crate::compile) row_heights: Vec<f64>,
    pub(in crate::compile) row_natural: Vec<f64>,
}

/// Compute the content-based column widths and row heights for a table grid laid
/// out as `columns` × rows inside the box described by `dims` (its width/height,
/// gap, and padding). This is the SINGLE sizing implementation shared by
/// [`super::emit::compile_table`] and the multi-page flow pre-pass; it is
/// origin-independent (it produces widths/heights, not absolute positions).
///
/// `placed` must be the result of [`super::place::place_cells`] over the SAME
/// `rows` slice so cell occupancy is byte-identical to the emit pass.
pub(in crate::compile) fn compute_table_layout(
    columns: &[TableColumn],
    placed: &[PlacedCell<'_>],
    dims: GridDims,
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    header_rows: usize,
    header_style: Option<&str>,
) -> TableLayout {
    let GridDims {
        col_count,
        row_count,
        gap,
        pad,
        table_w,
        table_h,
    } = dims;

    // ── Column widths (CONTENT-BASED auto-sizing) ────────────────────────
    let mut explicit_w: Vec<Option<f64>> = Vec::with_capacity(col_count);
    for i in 0..col_count {
        let w = columns
            .get(i)
            .and_then(|c| c.width.as_ref())
            .and_then(|d| dim_to_px(d.value, &d.unit))
            .map(|v| v.max(0.0));
        explicit_w.push(w);
    }
    let sum_explicit: f64 = explicit_w.iter().filter_map(|w| *w).sum();

    // Resolve the node-level font families ONCE per text node we measure (cached
    // by node id) so repeated cells of the same node don't re-probe the provider
    // or re-emit advisories.
    let mut family_cache: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // Natural content width demanded for each AUTO column.
    let mut auto_natural: Vec<f64> = vec![0.0; col_count];
    for pc in placed {
        // Header cells measure with `header_style` applied so their bold/styled
        // width matches what is rendered; non-header cells pass None.
        let eff_hs = if pc.row < header_rows {
            header_style
        } else {
            None
        };
        let cell_natural =
            cell_natural_width(pc.cell, pad, env, diagnostics, &mut family_cache, eff_hs);
        let is_auto = |c: usize| explicit_w.get(c).is_some_and(|w| w.is_none());
        let auto_col_count = (pc.col..pc.col + pc.cs).filter(|&c| is_auto(c)).count();
        if auto_col_count == 0 {
            continue;
        }
        let explicit_in_span: f64 = (pc.col..pc.col + pc.cs)
            .filter_map(|c| explicit_w.get(c).copied().flatten())
            .sum();
        let span_gaps = gap * (pc.cs.saturating_sub(1)) as f64;
        let auto_demand = (cell_natural - explicit_in_span - span_gaps).max(0.0);
        let per_col = auto_demand / auto_col_count as f64;
        for c in (pc.col..pc.col + pc.cs).filter(|&c| is_auto(c)) {
            if let Some(slot) = auto_natural.get_mut(c) {
                *slot = slot.max(per_col);
            }
        }
    }

    let total_gap_w = gap * (col_count.saturating_sub(1)) as f64;
    let avail_auto = (table_w - sum_explicit - total_gap_w - 2.0 * pad).max(0.0);
    let sum_auto_natural: f64 = explicit_w
        .iter()
        .enumerate()
        .filter(|(_, w)| w.is_none())
        .map(|(i, _)| auto_natural.get(i).copied().unwrap_or(0.0))
        .sum();
    let auto_scale = if sum_auto_natural > avail_auto && sum_auto_natural > 0.0 {
        avail_auto / sum_auto_natural
    } else {
        1.0
    };
    let col_widths: Vec<f64> = explicit_w
        .iter()
        .enumerate()
        .map(|(i, w)| match w {
            Some(px) => px.max(0.0),
            None => {
                let nat = auto_natural.get(i).copied().unwrap_or(0.0) * auto_scale;
                if auto_scale < 1.0 {
                    nat.max(MIN_AUTO_COL_W)
                } else {
                    nat.max(0.0)
                }
            }
        })
        .collect();

    // ── Row heights (CONTENT-BASED) ──────────────────────────────────────
    let mut row_natural: Vec<f64> = vec![0.0; row_count];
    for pc in placed {
        let mut span_w = 0.0;
        for c in pc.col..pc.col + pc.cs {
            span_w += col_widths.get(c).copied().unwrap_or(0.0);
        }
        span_w += gap * (pc.cs.saturating_sub(1)) as f64;
        let content_w = (span_w - 2.0 * pad).max(0.0);
        let eff_hs = if pc.row < header_rows {
            header_style
        } else {
            None
        };

        let cell_h = cell_content_height(
            pc.cell,
            content_w,
            pad,
            env,
            diagnostics,
            &mut family_cache,
            eff_hs,
        );
        let per_row = cell_h / pc.rs as f64;
        for dr in 0..pc.rs {
            if let Some(slot) = row_natural.get_mut(pc.row + dr) {
                *slot = slot.max(per_row);
            }
        }
    }

    let total_gap_h = gap * (row_count.saturating_sub(1)) as f64;
    let avail_h = (table_h - total_gap_h - 2.0 * pad).max(0.0);
    let sum_rows: f64 = row_natural.iter().sum();
    let row_scale = if sum_rows > avail_h && sum_rows > 0.0 {
        avail_h / sum_rows
    } else {
        1.0
    };
    let row_heights: Vec<f64> = row_natural
        .iter()
        .map(|h| (h * row_scale).max(0.0))
        .collect();

    TableLayout {
        col_widths,
        row_heights,
        row_natural,
    }
}

/// Natural (unwrapped) content width a cell demands, in pixels: the max over the
/// cell's children of the child's natural width, plus the two cell-padding
/// insets. A `Node::Text` child measures via the shared text pipeline; any other
/// kind uses its declared box width (or 0). `family_cache` memoizes per-text-node
/// family resolution so a repeated node id is not re-probed/re-diagnosed.
fn cell_natural_width(
    cell: &zenith_core::TableCell,
    pad: f64,
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    family_cache: &mut BTreeMap<String, Vec<String>>,
    header_style: Option<&str>,
) -> f64 {
    let mut widest = 0.0_f64;
    for child in &cell.children {
        let w = match child {
            Node::Text(t) => {
                let eff = header_styled_text(t, header_style);
                let families = cached_families(
                    &eff,
                    env.resolved,
                    env.style_map,
                    env.fonts,
                    diagnostics,
                    family_cache,
                );
                measure_text_natural(&eff, families, env, diagnostics).unwrap_or(0.0)
            }
            other @ (Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Code(_)
            | Node::Frame(_)
            | Node::Group(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Table(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Unknown(_)) => child_declared_box(other, env.resolved).0.unwrap_or(0.0),
        };
        widest = widest.max(w);
    }
    widest + 2.0 * pad
}

/// Wrapped content height a cell demands at content width `content_w`, in pixels:
/// the max over the cell's children of the child's height, plus the two
/// cell-padding insets. A `Node::Text` child measures its wrapped block height at
/// `content_w` via the shared pipeline; any other kind uses its declared box
/// height (or 0).
fn cell_content_height(
    cell: &zenith_core::TableCell,
    content_w: f64,
    pad: f64,
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    family_cache: &mut BTreeMap<String, Vec<String>>,
    header_style: Option<&str>,
) -> f64 {
    let mut tallest = 0.0_f64;
    for child in &cell.children {
        // Bottom extent of a child within the cell content box =
        //   declared_y + height_contribution
        // When neither is set: y0=0.0, h=nat_h → same as before (byte-identical).
        let h = match child {
            Node::Text(t) => {
                let eff = header_styled_text(t, header_style);
                let families = cached_families(
                    &eff,
                    env.resolved,
                    env.style_map,
                    env.fonts,
                    diagnostics,
                    family_cache,
                );
                let nat_h =
                    measure_text_wrapped_height(&eff, content_w, families, env, diagnostics)
                        .unwrap_or(0.0);
                let y0 = resolve_geometry_px(t.y.as_ref(), env.resolved).unwrap_or(0.0);
                let h_decl = resolve_geometry_px(t.h.as_ref(), env.resolved).unwrap_or(nat_h);
                y0 + h_decl
            }
            other @ (Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Code(_)
            | Node::Frame(_)
            | Node::Group(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Table(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Unknown(_)) => {
                let y0 = child_declared_y(other, env.resolved).unwrap_or(0.0);
                let h_decl = child_declared_box(other, env.resolved).1.unwrap_or(0.0);
                y0 + h_decl
            }
        };
        tallest = tallest.max(h);
    }
    tallest + 2.0 * pad
}

/// Return the effective [`zenith_core::TextNode`] for measurement and rendering,
/// applying the table `header_style` when appropriate.
///
/// `header_style` is the EFFECTIVE header style for this cell (the caller passes
/// `None` for non-header cells), so this helper applies it whenever it is `Some`.
///
/// Returns `Cow::Borrowed(t)` (zero clone) when `header_style` is `None` or the
/// text node already has its own `style` set. Returns `Cow::Owned(clone)` only
/// when injecting (`header_style.is_some() && t.style.is_none()`), setting
/// `clone.style = Some(header_style)`. Used by BOTH the measurement passes and
/// the emit path so a styled (e.g. bold) header measures the width it renders at.
pub(super) fn header_styled_text<'a>(
    t: &'a zenith_core::TextNode,
    header_style: Option<&str>,
) -> std::borrow::Cow<'a, zenith_core::TextNode> {
    if let Some(style_id) = header_style
        && t.style.is_none()
    {
        let mut cloned = t.clone();
        cloned.style = Some(style_id.to_owned());
        std::borrow::Cow::Owned(cloned)
    } else {
        std::borrow::Cow::Borrowed(t)
    }
}

/// Resolve (and memoize) a text node's font families through [`resolve_text_families`].
/// The advisory inside that helper fires at most once per node id because a cache
/// hit skips the resolution entirely.
pub(super) fn cached_families<'c>(
    text: &zenith_core::TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    diagnostics: &mut Vec<Diagnostic>,
    family_cache: &'c mut BTreeMap<String, Vec<String>>,
) -> &'c [String] {
    family_cache
        .entry(text.id.clone())
        .or_insert_with(|| resolve_text_families(text, resolved, style_map, fonts, diagnostics))
}
