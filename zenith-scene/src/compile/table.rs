//! Table-node compilation: single-page tables with EXPLICIT and CONTENT-BASED
//! column widths, CONTENT-BASED row heights, and SEPARATE or COLLAPSE borders.
//!
//! This unit lays a `table` out as a grid of cells inside its declared
//! `[x, y, w, h]` box, honoring `colspan`/`rowspan` (HTML-table cell flow).
//! AUTO columns (a `column` with no `width`) size to their widest cell's
//! measured natural content; rows size to their tallest cell's wrapped content
//! height at the assigned column width. Both passes reuse the production
//! text-shaping pipeline (`shape_words`/`pack_lines`) via the measurer helpers
//! in [`super::text`]. `border-collapse="separate"` (the default) draws each
//! cell's four edges independently. `border-collapse="collapse"` deduplicates
//! shared edges so adjacent cells never double-draw their shared border.
//!
//! Each cell emits, in order: an optional background `FillRect` (cell.fill or
//! table.fill), then an optional border (four independent `StrokeLine`s in
//! separate mode; accumulated and deduplicated in collapse mode), then its
//! compiled child content clipped to and translated into the cell content box
//! (cell padding inset). The CELL provides each child's geometry: a `text`
//! child auto-wraps to the content width (unless it sets `w`), is horizontally
//! aligned by the cell/table `h-align` (via the text's own `align`), and is
//! offset vertically by `v-align` against its measured wrapped height — so
//! authors never hand-size cell text. Author-specified `w`/`x`/`y`/`align` win.
//! Non-text children keep the prior declared-box align-slack placement. Opacity
//! cascades (table.opacity × ctx.opacity).

use std::collections::{BTreeMap, BTreeSet};

use zenith_core::{
    Diagnostic, FontProvider, Node, PropertyValue, ResolvedToken, ResolvedValue, Style,
    TableColumn, TableNode, TableRow, dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::SceneCommand;

use super::anchor::AnchorMap;
use super::chain::ChainAssignments;
use super::field::FieldCtx;
use super::paint::resolve_property_color;
use super::table_flow::TableFlowAssignments;
use super::text::{
    MeasureEnv, measure_text_natural, measure_text_wrapped_height, resolve_text_families,
};
use super::util::resolve_property_dimension_px;
use super::{ComponentMap, RenderCtx, compile_node};

/// Lower bound (px) a shrunk AUTO column is clamped to, so proportional shrink
/// to fit never collapses a column to zero width (which would hide its border).
const MIN_AUTO_COL_W: f64 = 2.0;

// ── Collapse-mode edge deduplication ────────────────────────────────────────

/// Sub-pixel quantization grid used for edge-key comparison: 0.01 px resolution.
/// Two endpoints that differ by less than 0.01 px map to the same integer bucket,
/// preventing floating-point jitter from creating spurious duplicate edges.
const QUANTIZE: f64 = 100.0;

/// Quantize a coordinate to an integer on the 0.01 px grid.
///
/// The `as i64` cast is a Rust 1.45+ saturating cast: `f64::NAN` maps to `0`,
/// `f64::INFINITY` to `i64::MAX`, and `f64::NEG_INFINITY` to `i64::MIN`.
/// Wild coordinates (NaN/inf) therefore land on a degenerate but safe key;
/// they cannot appear in normal layout output since `dim_to_px` only produces
/// finite values and the geometry guards above reject non-finite table bounds.
#[inline]
fn quantize(v: f64) -> i64 {
    (v * QUANTIZE).round() as i64
}

/// A canonical, float-safe key for one border segment in collapse mode.
///
/// Endpoints are quantized to a 0.01 px integer grid and stored in canonical
/// order (`(min, max)` by (ax, ay) first then (bx, by)) so the same physical
/// edge contributed by two adjacent cells maps to a single key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    ax: i64,
    ay: i64,
    bx: i64,
    by: i64,
}

impl EdgeKey {
    /// Build a canonical `EdgeKey` from two (possibly unordered) endpoints.
    /// The pair `(ax,ay)` is always the lesser endpoint so the key is symmetric.
    fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        let (qx1, qy1, qx2, qy2) = (quantize(x1), quantize(y1), quantize(x2), quantize(y2));
        // Canonical order: lexicographic by (ax, ay) ≤ (bx, by).
        if (qx1, qy1) <= (qx2, qy2) {
            EdgeKey {
                ax: qx1,
                ay: qy1,
                bx: qx2,
                by: qy2,
            }
        } else {
            EdgeKey {
                ax: qx2,
                ay: qy2,
                bx: qx1,
                by: qy1,
            }
        }
    }
}

/// The resolved style for one border segment in collapse mode.
///
/// We store the original f64 endpoints alongside the resolved color and width
/// so we can emit the `StrokeLine` with exact coordinates (no rounding drift).
struct EdgeStyle {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    color: crate::ir::Color,
    stroke_width: f64,
    /// Whether this edge was contributed by a cell with an OWN explicit `border`
    /// property (not inherited from the table). Used for tie-breaking: explicit
    /// wins over inherited; if both are the same kind the later-in-placed-order
    /// writer wins (last-writer rule, BTreeMap insert overwrites).
    is_explicit: bool,
}

/// Try to insert `candidate` for `key` into the accumulator, applying the
/// collapse tie-break rule:
///
/// 1. A cell with its OWN explicit `border`/`border_width` wins over a cell
///    that inherited those values from the table default.
/// 2. If both are explicit OR both are inherited, the later-in-placed-order
///    cell wins (last-writer: simply overwrite the existing entry).
fn try_insert_edge(acc: &mut BTreeMap<EdgeKey, EdgeStyle>, key: EdgeKey, candidate: EdgeStyle) {
    // Determine whether to overwrite without holding a simultaneous immutable
    // borrow on `acc` (which would conflict with the mutable borrow needed for
    // `insert`). We read the `is_explicit` flag we need, then drop the borrow.
    let should_insert = match acc.get(&key) {
        None => true,
        Some(existing) => {
            // Explicit beats inherited; equal explicitness → last writer wins.
            candidate.is_explicit || !existing.is_explicit
        }
    };
    if should_insert {
        acc.insert(key, candidate);
    }
}

/// Resolve a `border-width` property to pixels without requiring an owned
/// `Option<PropertyValue>`. Mirrors the logic in [`resolve_property_dimension_px`]
/// but accepts an `Option<&PropertyValue>` so callers can use `as_ref().or()`
/// to pick cell-over-table without cloning.
fn resolve_border_width(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: f64,
) -> f64 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::Dimension(dim) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
                _ => default,
            },
            None => default,
        },
        Some(PropertyValue::Dimension(dim)) => dim_to_px(dim.value, &dim.unit).unwrap_or(default),
        _ => default,
    }
}

/// One placed cell after the HTML-table occupancy walk: its top-left grid
/// position plus resolved column/row spans. Shared by the width, height, and
/// emission passes so cell placement is byte-identical across all three.
pub(super) struct PlacedCell<'a> {
    /// 0-based starting row.
    row: usize,
    /// 0-based starting column.
    col: usize,
    /// Column span (≥1, clamped to the grid).
    cs: usize,
    /// Row span (≥1, clamped to the grid).
    rs: usize,
    cell: &'a zenith_core::TableCell,
}

/// Walk the table's rows with a deterministic occupancy grid (HTML-table cell
/// flow honoring `colspan`/`rowspan`) and return every placed cell in emission
/// order. This is the SINGLE placement walk reused by the auto-width pass, the
/// row-height pass, and the emit pass, so a cell occupies byte-identical slots
/// in measurement and rendering.
pub(super) fn place_cells(
    rows: &[TableRow],
    col_count: usize,
    row_count: usize,
) -> Vec<PlacedCell<'_>> {
    let mut placed: Vec<PlacedCell> = Vec::new();
    let mut occupied: BTreeSet<(usize, usize)> = BTreeSet::new();

    for (r, row) in rows.iter().enumerate() {
        let mut col_cursor = 0usize;
        for cell in &row.cells {
            while col_cursor < col_count && occupied.contains(&(r, col_cursor)) {
                col_cursor += 1;
            }
            if col_cursor >= col_count {
                break;
            }
            let cs = (cell.colspan.max(1) as usize).min(col_count - col_cursor);
            let rs = (cell.rowspan.max(1) as usize).min(row_count - r);
            for dr in 0..rs {
                for dc in 0..cs {
                    occupied.insert((r + dr, col_cursor + dc));
                }
            }
            placed.push(PlacedCell {
                row: r,
                col: col_cursor,
                cs,
                rs,
                cell,
            });
            col_cursor += cs;
        }
    }
    placed
}

/// Geometry of one placed cell in absolute page pixels (already including the
/// table origin but NOT the cell-padding inset).
struct CellRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// The resolved per-column widths and per-row heights of a table grid, computed
/// ONCE from the placed cells and reused by both [`compile_table`] (for emission)
/// and the multi-page flow pre-pass in [`super::table_flow`] (for fit decisions).
///
/// `col_widths`/`row_heights` are FINAL (auto-column + row shrink-to-fit applied),
/// while `row_natural` holds the per-row content height BEFORE any vertical shrink
/// — the pre-pass measures row fit against the natural heights so a member's box
/// height decides the body-row slice deterministically.
pub(super) struct TableLayout {
    pub(super) col_widths: Vec<f64>,
    pub(super) row_heights: Vec<f64>,
    pub(super) row_natural: Vec<f64>,
}

/// Compute the content-based column widths and row heights for a table grid laid
/// out as `columns` × `rows` inside a box of `table_w` × `table_h` pixels with the
/// given `gap` and `pad`. This is the SINGLE sizing implementation shared by
/// [`compile_table`] and the multi-page flow pre-pass; it is origin-independent
/// (it produces widths/heights, not absolute positions).
///
/// `placed` must be the result of [`place_cells`] over the SAME `rows` slice so
/// cell occupancy is byte-identical to the emit pass.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_table_layout(
    columns: &[TableColumn],
    placed: &[PlacedCell<'_>],
    col_count: usize,
    row_count: usize,
    gap: f64,
    pad: f64,
    table_w: f64,
    table_h: f64,
    env: &MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    header_rows: usize,
    header_style: Option<&str>,
) -> TableLayout {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_table(
    table: &TableNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    flows: &TableFlowAssignments,
    anchors: &AnchorMap,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false (no commands emitted).
    if table.visible == Some(false) {
        return;
    }

    // Multi-page flow: if this table id was assigned a slice by the document-wide
    // pre-pass, render that slice's rows + columns (the member box's own geometry,
    // gap, padding, border, fill, header styling still apply). A table NOT in the
    // map (the common case) uses its own rows/columns — byte-identical to before.
    let flow = flows.get(&table.id);
    let rows: &[TableRow] = flow.map(|a| a.rows.as_slice()).unwrap_or(&table.rows);
    let columns: &[TableColumn] = flow.map(|a| a.columns.as_slice()).unwrap_or(&table.columns);

    // ── Resolve table geometry ───────────────────────────────────────────
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
        (&table.x, &table.y, &table.w, &table.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "table '{}' is missing one or more geometry properties (x, y, w, h); skipped",
                table.id
            ),
            table.source_span,
            Some(table.id.clone()),
        ));
        return;
    };
    let (Some(table_x), Some(table_y), Some(table_w), Some(table_h)) = (
        dim_to_px(x_dim.value, &x_dim.unit),
        dim_to_px(y_dim.value, &y_dim.unit),
        dim_to_px(w_dim.value, &w_dim.unit),
        dim_to_px(h_dim.value, &h_dim.unit),
    ) else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "table '{}' has an unresolvable geometry unit (x, y, w, h); skipped",
                table.id
            ),
            table.source_span,
            Some(table.id.clone()),
        ));
        return;
    };

    // Absolute page origin (cascade translation applied).
    let origin_x = ctx.dx + table_x;
    let origin_y = ctx.dy + table_y;

    // ── Resolve gap + cell padding (token or literal), default 0 ─────────
    let gap = resolve_property_dimension_px(&table.gap, resolved, 0.0).max(0.0);
    let pad = resolve_property_dimension_px(&table.cell_padding, resolved, 0.0).max(0.0);

    // Opacity cascade.
    let opacity = (table.opacity.unwrap_or(1.0).clamp(0.0, 1.0)) * ctx.opacity;

    // ── Grid dimensions ──────────────────────────────────────────────────
    let col_count = columns.len().max(1);
    let row_count = rows.len();
    if row_count == 0 {
        // No rows → nothing to draw (the table box itself has no fill in v0). A
        // flow continuation member with an empty assigned slice also lands here.
        return;
    }

    // ── Cell placement (shared occupancy walk) ───────────────────────────
    // Computed ONCE here and reused by the layout pass and the emit pass so a
    // cell occupies byte-identical slots throughout.
    let placed = place_cells(rows, col_count, row_count);

    // Shared shaping environment for the per-cell measurers.
    let env = MeasureEnv {
        resolved,
        style_map,
        fonts,
        engine,
    };

    // First `header_rows` rows are header rows (clamped to the row count); their
    // text is measured AND rendered with `header_style`, so both agree.
    let header_rows = (table.header_rows.unwrap_or(0) as usize).min(row_count);

    // ── Column widths + row heights (shared sizing math) ─────────────────
    // The SINGLE sizing implementation, also used by the multi-page flow
    // pre-pass for its fit decisions (see `compute_table_layout`).
    let TableLayout {
        col_widths,
        row_heights,
        row_natural: _,
    } = compute_table_layout(
        columns,
        &placed,
        col_count,
        row_count,
        gap,
        pad,
        table_w,
        table_h,
        &env,
        diagnostics,
        header_rows,
        table.header_style.as_deref(),
    );

    // Left edge of each column (content-box left = origin + pad).
    let mut col_left: Vec<f64> = Vec::with_capacity(col_count);
    let mut cursor = origin_x + pad;
    for w in &col_widths {
        col_left.push(cursor);
        cursor += w + gap;
    }

    let mut row_top: Vec<f64> = Vec::with_capacity(row_count);
    let mut rcursor = origin_y + pad;
    for h in &row_heights {
        row_top.push(rcursor);
        rcursor += h + gap;
    }

    // ── Cell emission (reusing the shared placement walk) ────────────────
    // When border-collapse="collapse" we accumulate all border edges into a
    // BTreeMap (deterministic, float-safe key) and emit them in a single pass
    // after the cell loop. Any other value (including None/"separate"/unknown)
    // uses the default per-cell four-StrokeLine emission unchanged.
    let collapse_mode = matches!(table.border_collapse.as_deref(), Some("collapse"));

    // Accumulator for collapse mode: EdgeKey → EdgeStyle.
    // Populated during the cell loop below; empty (and unused) in separate mode.
    let mut edge_acc: BTreeMap<EdgeKey, EdgeStyle> = BTreeMap::new();

    // (A placed cell is a header when its starting row index < `header_rows`,
    // computed above. When header_rows is 0/absent, no cell is a header and the
    // output is byte-identical to the pre-header-styling code path.)
    for pc in &placed {
        // Cell rect: from column `pc.col` left to the right edge of the last
        // spanned column (including interior gaps); similarly for rows.
        let left = col_left.get(pc.col).copied().unwrap_or(origin_x + pad);
        let mut span_w = 0.0;
        for c in pc.col..pc.col + pc.cs {
            span_w += col_widths.get(c).copied().unwrap_or(0.0);
        }
        span_w += gap * (pc.cs.saturating_sub(1)) as f64;

        let top = row_top.get(pc.row).copied().unwrap_or(origin_y + pad);
        let mut span_h = 0.0;
        for dr in 0..pc.rs {
            span_h += row_heights.get(pc.row + dr).copied().unwrap_or(0.0);
        }
        span_h += gap * (pc.rs.saturating_sub(1)) as f64;

        let rect = CellRect {
            x: left,
            y: top,
            w: span_w.max(0.0),
            h: span_h.max(0.0),
        };

        let is_header = pc.row < header_rows;

        if collapse_mode {
            // Accumulate this cell's four edges into the dedup map; fill and
            // content are still emitted immediately via emit_cell_no_border.
            emit_cell_no_border(
                table,
                pc.cell,
                &rect,
                pad,
                opacity,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                flows,
                anchors,
                field_ctx,
                ctx,
                is_header,
            );
            accumulate_cell_edges(
                table,
                pc.cell,
                &rect,
                opacity,
                resolved,
                diagnostics,
                &mut edge_acc,
            );
        } else {
            emit_cell(
                table,
                pc.cell,
                &rect,
                pad,
                opacity,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                flows,
                anchors,
                field_ctx,
                ctx,
                is_header,
            );
        }
    }

    // Collapse mode: emit every unique edge exactly once, in BTreeMap order
    // (deterministic: sorted by quantized endpoint coordinates).
    for edge in edge_acc.values() {
        commands.push(SceneCommand::StrokeLine {
            x1: edge.x1,
            y1: edge.y1,
            x2: edge.x2,
            y2: edge.y2,
            color: edge.color,
            stroke_width: edge.stroke_width,
            stroke_dash: None,
            stroke_gap: None,
            stroke_linecap: None,
        });
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
    env: &MeasureEnv,
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
            other => child_declared_box(other).0.unwrap_or(0.0),
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
    env: &MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    family_cache: &mut BTreeMap<String, Vec<String>>,
    header_style: Option<&str>,
) -> f64 {
    let mut tallest = 0.0_f64;
    for child in &cell.children {
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
                measure_text_wrapped_height(&eff, content_w, families, env, diagnostics)
                    .unwrap_or(0.0)
            }
            other => child_declared_box(other).1.unwrap_or(0.0),
        };
        tallest = tallest.max(h);
    }
    tallest + 2.0 * pad
}

/// Return the effective [`TextNode`] for measurement and rendering, applying the
/// table `header_style` when appropriate.
///
/// `header_style` is the EFFECTIVE header style for this cell (the caller passes
/// `None` for non-header cells), so this helper applies it whenever it is `Some`.
///
/// Returns `Cow::Borrowed(t)` (zero clone) when `header_style` is `None` or the
/// text node already has its own `style` set. Returns `Cow::Owned(clone)` only
/// when injecting (`header_style.is_some() && t.style.is_none()`), setting
/// `clone.style = Some(header_style)`. Used by BOTH the measurement passes and
/// the emit path so a styled (e.g. bold) header measures the width it renders at.
fn header_styled_text<'a>(
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
fn cached_families<'c>(
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

/// Emit one cell in collapse mode: background fill and clipped content only
/// (border edges are accumulated separately by [`accumulate_cell_edges`]).
#[allow(clippy::too_many_arguments)]
fn emit_cell_no_border(
    table: &TableNode,
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    pad: f64,
    opacity: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    flows: &TableFlowAssignments,
    anchors: &AnchorMap,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
    is_header: bool,
) {
    // ── Background fill ───────────────────────────────────────────────────
    // Precedence: cell.fill > header_fill (header only) > table.fill.
    // When is_header=false, the default falls back to table.fill directly —
    // byte-identical to the pre-header code path.
    let fill_prop: Option<&PropertyValue> = cell.fill.as_ref().or_else(|| {
        if is_header {
            table.header_fill.as_ref().or(table.fill.as_ref())
        } else {
            table.fill.as_ref()
        }
    });
    if let Some(prop) = fill_prop
        && let Some(mut color) = resolve_property_color(prop, resolved, diagnostics, &table.id)
    {
        color.a = (color.a as f64 * opacity).round() as u8;
        commands.push(SceneCommand::FillRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            color,
        });
    }

    emit_cell_children(
        table,
        cell,
        rect,
        pad,
        opacity,
        is_header,
        resolved,
        style_map,
        components,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        flows,
        anchors,
        field_ctx,
        ctx,
    );
}

/// Compile a cell's direct children into the cell content box, clipped to it.
///
/// The CELL provides each child's geometry: the content box is the cell rect
/// inset by `pad`. A `Node::Text` child gets auto-box behavior — it wraps to the
/// content width (unless the author set `w`), is aligned horizontally by the
/// cell/table `h-align` (via the text's own `align`), and is offset vertically by
/// the cell/table `v-align` against its measured wrapped height. Author-specified
/// `w`/`x`/`y`/`align` always win, so explicitly-sized cell text is byte-identical
/// to before. Any non-text child keeps the prior declared-box align-slack layout.
#[allow(clippy::too_many_arguments)]
fn emit_cell_children(
    table: &TableNode,
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    pad: f64,
    opacity: f64,
    is_header: bool,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    flows: &TableFlowAssignments,
    anchors: &AnchorMap,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) {
    // ── Content box (cell padding inset) ─────────────────────────────────
    let content_x = rect.x + pad;
    let content_y = rect.y + pad;
    let content_w = (rect.w - 2.0 * pad).max(0.0);
    let content_h = (rect.h - 2.0 * pad).max(0.0);

    // Alignment offsets (cell override else table default). Horizontal shifts
    // the child column within the content width; vertical within its height.
    let h_align = cell
        .h_align
        .as_deref()
        .or(table.h_align.as_deref())
        .unwrap_or("start");
    let v_align = cell
        .v_align
        .as_deref()
        .or(table.v_align.as_deref())
        .unwrap_or("top");

    // Clip cell content to the content box, then compile each child with a
    // RenderCtx translated to the content-box origin (plus the alignment
    // offset) so authored coordinate (0,0) lands at the cell's content corner.
    commands.push(SceneCommand::PushClip {
        x: content_x,
        y: content_y,
        w: content_w,
        h: content_h,
    });

    // Shared shaping environment for the per-text measure pass.
    let env = MeasureEnv {
        resolved,
        style_map,
        fonts,
        engine,
    };
    let mut family_cache: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for child in &cell.children {
        if let Node::Text(t) = child {
            // ── Text: the CELL provides the box (auto-wrap + h/v align) ──
            // Effective wrap width: author `w` else the content width.
            let wrap_w = child_declared_box(child).0.unwrap_or(content_w);
            // Effective text align: author `align` else the cell/table h-align
            // mapped to a text align value.
            let h_align_text = match h_align {
                "center" => "center",
                "end" => "end",
                _ => "start",
            };
            let eff_align: &str = t.align.as_deref().unwrap_or(h_align_text);

            // Build the effective styled text (header-style injection via the
            // shared helper so measurement and rendering always agree).
            let eff_hs = if is_header {
                table.header_style.as_deref()
            } else {
                None
            };
            let styled = header_styled_text(t, eff_hs);

            // Measure wrapped natural height at the effective wrap width.
            let families = cached_families(
                &styled,
                resolved,
                style_map,
                fonts,
                diagnostics,
                &mut family_cache,
            );
            let nat_h = measure_text_wrapped_height(&styled, wrap_w, families, &env, diagnostics)
                .unwrap_or(0.0);
            let v_offset = match v_align {
                "middle" => ((content_h - nat_h) / 2.0).max(0.0),
                "bottom" => (content_h - nat_h).max(0.0),
                _ => 0.0,
            };

            // Override unset geometry/align fields. Author-set w/x/y/align/style
            // are always preserved (header_styled_text never overwrites style when
            // the author already set one).
            let mut cloned = styled.into_owned();
            if cloned.w.is_none() {
                cloned.w = Some(super::util::px(wrap_w));
            }
            if cloned.x.is_none() {
                cloned.x = Some(super::util::px(0.0));
            }
            if cloned.y.is_none() {
                cloned.y = Some(super::util::px(0.0));
            }
            if cloned.align.is_none() {
                cloned.align = Some(eff_align.to_string());
            }
            let effective_child = zenith_core::Node::Text(Box::new(cloned));

            // Horizontal placement is handled by the text's own align, so the
            // ctx carries only the content origin plus the vertical offset.
            let child_ctx = RenderCtx {
                opacity,
                dx: content_x,
                dy: content_y + v_offset,
                baseline_grid: ctx.baseline_grid,
            };
            let _ = compile_node(
                &effective_child,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                flows,
                anchors,
                field_ctx,
                child_ctx,
            );
            continue;
        }

        // ── Non-text: declared-box align-slack into the content box ──────
        let (cw, ch) = child_declared_box(child);
        let dx_align = match h_align {
            "center" => ((content_w - cw.unwrap_or(content_w)) / 2.0).max(0.0),
            "end" => (content_w - cw.unwrap_or(content_w)).max(0.0),
            _ => 0.0,
        };
        let dy_align = match v_align {
            "middle" => ((content_h - ch.unwrap_or(content_h)) / 2.0).max(0.0),
            "bottom" => (content_h - ch.unwrap_or(content_h)).max(0.0),
            _ => 0.0,
        };
        let child_ctx = RenderCtx {
            opacity,
            dx: content_x + dx_align,
            dy: content_y + dy_align,
            baseline_grid: ctx.baseline_grid,
        };
        let effective_child: std::borrow::Cow<zenith_core::Node> =
            std::borrow::Cow::Borrowed(child);
        let _ = compile_node(
            &effective_child,
            resolved,
            style_map,
            components,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            flows,
            anchors,
            field_ctx,
            child_ctx,
        );
    }

    commands.push(SceneCommand::PopClip);
}

/// Accumulate the four border edges of one cell into the collapse-mode dedup map.
///
/// Tie-break rule (documented here as the authoritative source):
///   1. A cell with its OWN explicit `border`/`border_width` property (i.e. the
///      `TableCell.border` field is `Some`) wins over a cell that inherited those
///      values from the table-level defaults.
///   2. If both contributing cells are equally explicit or equally inherited,
///      the later-in-placed-order cell wins (last writer overwrites).
///
/// Only inserts edges when the resolved border color is `Some` AND the resolved
/// width is > 0 (same guard as the separate-mode path).
fn accumulate_cell_edges(
    table: &TableNode,
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    opacity: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    diagnostics: &mut Vec<Diagnostic>,
    acc: &mut BTreeMap<EdgeKey, EdgeStyle>,
) {
    let border_prop: Option<&PropertyValue> = cell.border.as_ref().or(table.border.as_ref());
    let Some(prop) = border_prop else { return };
    let Some(mut color) = resolve_property_color(prop, resolved, diagnostics, &table.id) else {
        return;
    };
    color.a = (color.a as f64 * opacity).round() as u8;

    let bw = resolve_border_width(
        cell.border_width.as_ref().or(table.border_width.as_ref()),
        resolved,
        1.0,
    )
    .max(0.0);
    if bw <= 0.0 {
        return;
    }

    // Whether this cell contributes an OWN explicit border (not table fallback).
    let is_explicit = cell.border.is_some();

    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w;
    let y1 = rect.y + rect.h;

    for (ex1, ey1, ex2, ey2) in [
        (x0, y0, x1, y0), // top
        (x0, y1, x1, y1), // bottom
        (x0, y0, x0, y1), // left
        (x1, y0, x1, y1), // right
    ] {
        let key = EdgeKey::new(ex1, ey1, ex2, ey2);
        let candidate = EdgeStyle {
            x1: ex1,
            y1: ey1,
            x2: ex2,
            y2: ey2,
            color,
            stroke_width: bw,
            is_explicit,
        };
        try_insert_edge(acc, key, candidate);
    }
}

/// Emit one cell: background fill, separate border, and clipped/aligned content.
#[allow(clippy::too_many_arguments)]
fn emit_cell(
    table: &TableNode,
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    pad: f64,
    opacity: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    flows: &TableFlowAssignments,
    anchors: &AnchorMap,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
    is_header: bool,
) {
    // ── Background fill ───────────────────────────────────────────────────
    // Precedence: cell.fill > header_fill (header only) > table.fill.
    // When is_header=false, the default falls back to table.fill directly —
    // byte-identical to the pre-header code path.
    let fill_prop: Option<&PropertyValue> = cell.fill.as_ref().or_else(|| {
        if is_header {
            table.header_fill.as_ref().or(table.fill.as_ref())
        } else {
            table.fill.as_ref()
        }
    });
    if let Some(prop) = fill_prop
        && let Some(mut color) = resolve_property_color(prop, resolved, diagnostics, &table.id)
    {
        color.a = (color.a as f64 * opacity).round() as u8;
        commands.push(SceneCommand::FillRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            color,
        });
    }

    // ── Separate border: each cell draws its own four edges independently ─
    let border_prop: Option<&PropertyValue> = cell.border.as_ref().or(table.border.as_ref());
    if let Some(prop) = border_prop
        && let Some(mut color) = resolve_property_color(prop, resolved, diagnostics, &table.id)
    {
        color.a = (color.a as f64 * opacity).round() as u8;
        // Width: cell.border-width else table.border-width else 1px.
        let bw = resolve_border_width(
            cell.border_width.as_ref().or(table.border_width.as_ref()),
            resolved,
            1.0,
        )
        .max(0.0);
        if bw > 0.0 {
            let x0 = rect.x;
            let y0 = rect.y;
            let x1 = rect.x + rect.w;
            let y1 = rect.y + rect.h;
            // Four edges as independent stroke lines (centered stroke).
            for (ax, ay, bx, by) in [
                (x0, y0, x1, y0), // top
                (x0, y1, x1, y1), // bottom
                (x0, y0, x0, y1), // left
                (x1, y0, x1, y1), // right
            ] {
                commands.push(SceneCommand::StrokeLine {
                    x1: ax,
                    y1: ay,
                    x2: bx,
                    y2: by,
                    color,
                    stroke_width: bw,
                    stroke_dash: None,
                    stroke_gap: None,
                    stroke_linecap: None,
                });
            }
        }
    }

    emit_cell_children(
        table,
        cell,
        rect,
        pad,
        opacity,
        is_header,
        resolved,
        style_map,
        components,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        flows,
        anchors,
        field_ctx,
        ctx,
    );
}

/// The declared `(w, h)` of a cell child in pixels, when the kind carries a
/// box and the dimensions resolve. Used to compute alignment slack. Kinds
/// without a resolvable box yield `(None, None)`.
fn child_declared_box(node: &zenith_core::Node) -> (Option<f64>, Option<f64>) {
    use zenith_core::Node;
    let px =
        |d: &Option<zenith_core::Dimension>| d.as_ref().and_then(|d| dim_to_px(d.value, &d.unit));
    match node {
        Node::Rect(n) => (px(&n.w), px(&n.h)),
        Node::Ellipse(n) => (px(&n.w), px(&n.h)),
        Node::Text(n) => (px(&n.w), px(&n.h)),
        Node::Code(n) => (px(&n.w), px(&n.h)),
        Node::Image(n) => (px(&n.w), px(&n.h)),
        Node::Frame(n) => (px(&n.w), px(&n.h)),
        Node::Group(n) => (px(&n.w), px(&n.h)),
        Node::Field(n) => (px(&n.w), px(&n.h)),
        Node::Toc(n) => (px(&n.w), px(&n.h)),
        Node::Table(n) => (px(&n.w), px(&n.h)),
        Node::Shape(n) => (px(&n.w), px(&n.h)),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => (None, None),
    }
}
