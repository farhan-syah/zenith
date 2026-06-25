//! Table emission: the [`compile_table`] entry point plus the per-cell fill,
//! border, and clipped-content emission for both separate and collapse modes.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, Node, PropertyValue, ResolvedToken, Style, TableColumn, TableNode,
    TableRow, dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::{Paint, SceneCommand};

use super::super::anchor::AnchorMap;
use super::super::chain::ChainAssignments;
use super::super::field::FieldCtx;
use super::super::paint::resolve_property_color;
use super::super::table_flow::TableFlowAssignments;
use super::super::text::{MeasureEnv, empty_md_blocks, measure_text_wrapped_height};
use super::super::util::resolve_property_dimension_px;
use super::super::{ComponentMap, NodeCtx, RenderCtx, compile_node};

use super::collapse::{EdgeKey, EdgeStyle, accumulate_cell_edges, resolve_border_width};
use super::layout::{
    GridDims, TableLayout, cached_families, compute_table_layout, header_styled_text,
};
use super::place::{CellRect, child_declared_box, place_cells};

/// The shared immutable borrow bundle threaded through table emission: the table
/// node itself plus every resolver/lookup the per-cell passes need. Every field
/// is a reference, so the struct is itself `Copy` and forwards without clones.
#[derive(Clone, Copy)]
pub(in crate::compile) struct TableEmitCtx<'a> {
    /// The table node being compiled (its properties drive fill/border/align).
    pub(in crate::compile) table: &'a TableNode,
    /// Resolved token table for dimension / color property resolution.
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    /// Style id → style lookup for the style cascade.
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    /// Component definitions, for descendant `instance` expansion.
    pub(in crate::compile) components: &'a ComponentMap<'a>,
    /// Font provider used to shape and measure text descendants.
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    /// Shaping engine used to shape and measure text descendants.
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    /// Text-chain assignments cascaded into text descendants.
    pub(in crate::compile) chains: &'a ChainAssignments,
    /// Table-flow assignments cascaded into table descendants.
    pub(in crate::compile) flows: &'a TableFlowAssignments,
    /// Anchor map for `anchor`/`anchor_zone` derived placement.
    pub(in crate::compile) anchors: &'a AnchorMap,
    /// Per-page field context (page index, live area, footnote markers, …).
    pub(in crate::compile) field_ctx: &'a FieldCtx<'a>,
}

impl<'a> TableEmitCtx<'a> {
    /// The shared shaping environment for the per-cell / per-text measurers.
    fn measure_env(&self) -> MeasureEnv<'a> {
        MeasureEnv {
            resolved: self.resolved,
            style_map: self.style_map,
            fonts: self.fonts,
            engine: self.engine,
        }
    }

    /// The shared immutable node-compile context for descendant `compile_node`
    /// calls (cell children). Carries the same borrows as this struct minus the
    /// table node itself.
    fn node_ctx(&self) -> NodeCtx<'a> {
        NodeCtx {
            resolved: self.resolved,
            style_map: self.style_map,
            components: self.components,
            fonts: self.fonts,
            engine: self.engine,
            chains: self.chains,
            flows: self.flows,
            anchors: self.anchors,
            field_ctx: self.field_ctx,
            // Table-cell markdown block layout is out of scope for this unit; a
            // cell's markdown text node keeps the inline path (no regression).
            md_blocks: empty_md_blocks(),
            page_block_styles: &[],
            doc_block_styles: &[],
        }
    }
}

/// Per-cell scalar emission inputs: cell-padding inset, the cascaded opacity, and
/// whether this placed cell is a header (for fill/style precedence). Bundled into
/// one `Copy` value so the per-cell emitters stay under the argument-count lint.
#[derive(Clone, Copy)]
struct CellEmit {
    pad: f64,
    opacity: f64,
    is_header: bool,
}

pub(in crate::compile) fn compile_table(
    cx: TableEmitCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    let table = cx.table;

    // Entire subtree excluded when visible=false (no commands emitted).
    if table.visible == Some(false) {
        return;
    }

    // Multi-page flow: if this table id was assigned a slice by the document-wide
    // pre-pass, render that slice's rows + columns (the member box's own geometry,
    // gap, padding, border, fill, header styling still apply). A table NOT in the
    // map (the common case) uses its own rows/columns — byte-identical to before.
    let flow = cx.flows.get(&table.id);
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
    let gap = resolve_property_dimension_px(table.gap.as_ref(), cx.resolved, 0.0).max(0.0);
    let pad = resolve_property_dimension_px(table.cell_padding.as_ref(), cx.resolved, 0.0).max(0.0);

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
    let env = cx.measure_env();

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
        GridDims {
            col_count,
            row_count,
            gap,
            pad,
            table_w,
            table_h,
        },
        env,
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

        let cell_emit = CellEmit {
            pad,
            opacity,
            is_header: pc.row < header_rows,
        };

        if collapse_mode {
            // Accumulate this cell's four edges into the dedup map; fill and
            // content are still emitted immediately via emit_cell_no_border.
            emit_cell_no_border(pc.cell, &rect, cell_emit, cx, commands, diagnostics, ctx);
            accumulate_cell_edges(
                table,
                pc.cell,
                &rect,
                opacity,
                cx.resolved,
                diagnostics,
                &mut edge_acc,
            );
        } else {
            emit_cell(pc.cell, &rect, cell_emit, cx, commands, diagnostics, ctx);
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

/// Emit one cell in collapse mode: background fill and clipped content only
/// (border edges are accumulated separately by [`accumulate_cell_edges`]).
fn emit_cell_no_border(
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    cell_emit: CellEmit,
    cx: TableEmitCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    emit_cell_fill(cell, rect, cell_emit, cx, commands, diagnostics);
    emit_cell_children(cell, rect, cell_emit, cx, commands, diagnostics, ctx);
}

/// Emit one cell: background fill, separate border, and clipped/aligned content.
fn emit_cell(
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    cell_emit: CellEmit,
    cx: TableEmitCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    emit_cell_fill(cell, rect, cell_emit, cx, commands, diagnostics);

    let table = cx.table;
    // ── Separate border: each cell draws its own four edges independently ─
    let border_prop: Option<&PropertyValue> = cell.border.as_ref().or(table.border.as_ref());
    if let Some(prop) = border_prop
        && let Some(mut color) = resolve_property_color(prop, cx.resolved, diagnostics, &table.id)
    {
        color.a = (color.a as f64 * cell_emit.opacity).round() as u8;
        // Width: cell.border-width else table.border-width else 1px.
        let bw = resolve_border_width(
            cell.border_width.as_ref().or(table.border_width.as_ref()),
            cx.resolved,
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

    emit_cell_children(cell, rect, cell_emit, cx, commands, diagnostics, ctx);
}

/// Emit a cell's optional background fill.
///
/// Precedence: cell.fill > header_fill (header only) > table.fill. When the cell
/// is not a header, the default falls back to table.fill directly — byte-identical
/// to the pre-header code path.
fn emit_cell_fill(
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    cell_emit: CellEmit,
    cx: TableEmitCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let table = cx.table;
    let fill_prop: Option<&PropertyValue> = cell.fill.as_ref().or_else(|| {
        if cell_emit.is_header {
            table.header_fill.as_ref().or(table.fill.as_ref())
        } else {
            table.fill.as_ref()
        }
    });
    if let Some(prop) = fill_prop
        && let Some(mut color) = resolve_property_color(prop, cx.resolved, diagnostics, &table.id)
    {
        color.a = (color.a as f64 * cell_emit.opacity).round() as u8;
        commands.push(SceneCommand::FillRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            paint: Paint::solid(color),
        });
    }
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
fn emit_cell_children(
    cell: &zenith_core::TableCell,
    rect: &CellRect,
    cell_emit: CellEmit,
    cx: TableEmitCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    let table = cx.table;
    let node_cx = cx.node_ctx();
    let pad = cell_emit.pad;
    let opacity = cell_emit.opacity;
    let is_header = cell_emit.is_header;

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
    let env = cx.measure_env();
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
                cx.resolved,
                cx.style_map,
                cx.fonts,
                diagnostics,
                &mut family_cache,
            );
            let nat_h = measure_text_wrapped_height(&styled, wrap_w, families, env, diagnostics)
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
                cloned.w = Some(super::super::util::px(wrap_w));
            }
            if cloned.x.is_none() {
                cloned.x = Some(super::super::util::px(0.0));
            }
            if cloned.y.is_none() {
                cloned.y = Some(super::super::util::px(0.0));
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
                node_cx,
                commands,
                diagnostics,
                &mut Vec::new(),
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
        let _ = compile_node(
            child,
            node_cx,
            commands,
            diagnostics,
            &mut Vec::new(),
            child_ctx,
        );
    }

    commands.push(SceneCommand::PopClip);
}
