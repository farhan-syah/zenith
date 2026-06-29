//! Multi-page table flow ("table chain") pre-pass.
//!
//! A *table flow* is the set of `table` nodes that share the same `flows` id. A
//! single logical table whose rows overflow its artboard continues onto
//! author-placed continuation `table` boxes on later pages, repeating its header
//! rows on every member. This mirrors the threaded text-chain system in
//! [`super::chain`]: the FIRST member (page-order, then source-order) is the
//! SOURCE — it carries ALL the body rows plus the column definitions; each
//! continuation member declares the same `flows` id with EMPTY rows and receives
//! the body-row slice that fits its own box height, with the source's header
//! rows cloned in front.
//!
//! This module runs ONCE per document (across ALL pages), BEFORE the main
//! compile walk, producing a single [`TableFlowAssignments`] map keyed by member
//! table id. [`super::table::compile_table`] consults that map: a flow member
//! renders its ASSIGNED rows + columns instead of its own; a non-flow table is
//! wholly unaffected (byte-identical).
//!
//! ## v0 design choices (documented)
//!
//! - **Content source.** The flow's content is the rows of the FIRST member
//!   (page-then-source order). Continuation members declare `flows=id` with empty
//!   `row` children. Only the source's rows are distributed; continuation rows
//!   (if any author error leaves some) are ignored in favor of the assigned
//!   slice.
//! - **Header repetition.** The first `header-rows` rows of the source are the
//!   header block, cloned to the front of every member's assigned slice so the
//!   existing `is_header` logic in `compile_table` styles them on every page.
//! - **Row granularity.** Rows are never split mid-row. A `rowspan` group that
//!   would straddle a member boundary is pushed WHOLE to the next member. If a
//!   single group is taller than an entire member box it is placed anyway and
//!   clipped per-cell (a `table.flow_overflow` advisory is emitted).
//! - **Last member.** The final member takes all remaining body rows even if they
//!   overflow its box (its per-cell clip handles the overflow), mirroring the
//!   chain's last-member policy.
//!
//! ## Determinism
//!
//! Members are collected in document (page, then source) order via a depth-first
//! walk into frames/groups, grouped by id in a [`BTreeMap`]. The result is keyed
//! by node id. Measurement reuses the deterministic `compute_table_layout`. No
//! `HashMap`/time/random reaches output.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, Node, ResolvedToken, Style, TableColumn, TableNode, TableRow,
};
use zenith_layout::RustybuzzEngine;

use super::table::{GridDims, compute_table_layout, place_cells};
use super::text::MeasureEnv;
use super::util::{resolve_geometry_px, resolve_property_dimension_px};

/// The rows + columns a single flow member must render: the source's header rows
/// cloned in front of this member's assigned body-row slice, plus the source's
/// column definitions (so every member shares the same column structure).
pub(crate) struct TableFlowAssignment {
    pub(super) columns: Vec<TableColumn>,
    pub(super) rows: Vec<TableRow>,
}

/// Map from member table id → its assigned rows/columns. A table whose id is
/// absent is NOT a flow member and renders exactly as before.
pub(crate) type TableFlowAssignments = BTreeMap<String, TableFlowAssignment>;

/// A collected flow member: its node id and the resolved box width/height (px)
/// plus gap/pad used to distribute rows. The member's draw geometry is resolved
/// independently inside `compile_table`; only the extents needed for the fit
/// decision are carried here.
struct Member<'a> {
    table: &'a TableNode,
    w: f64,
    h: f64,
    gap: f64,
    pad: f64,
}

/// Resolve a table node's explicit box to pixels, or `None` if any of
/// `x`/`y`/`w`/`h` is absent, a non-dimension, an unresolved token, or uses an
/// unsupported unit. Raw `(px)` dims are byte-identical to the prior read;
/// dimension token refs resolve via the token table.
fn member_box(table: &TableNode, resolved: &BTreeMap<String, ResolvedToken>) -> Option<(f64, f64)> {
    // Full geometry must resolve to be a valid member box (mirrors the chain
    // pre-pass), even though only w/h drive the fit decision.
    resolve_geometry_px(table.x.as_ref(), resolved)?;
    resolve_geometry_px(table.y.as_ref(), resolved)?;
    Some((
        resolve_geometry_px(table.w.as_ref(), resolved)?,
        resolve_geometry_px(table.h.as_ref(), resolved)?,
    ))
}

/// Depth-first walk in source order collecting `(flow_id → ordered members)`.
/// Recurses into frame/group children and table cells, mirroring how the chain
/// collection walks the tree.
fn collect_flows<'a>(
    nodes: &'a [Node],
    resolved: &BTreeMap<String, ResolvedToken>,
    members: &mut BTreeMap<String, Vec<Member<'a>>>,
) {
    for node in nodes {
        match node {
            Node::Table(t) => {
                if let Some(flow_id) = &t.flows
                    && let Some((w, h)) = member_box(t, resolved)
                {
                    let gap = resolve_property_dimension_px(t.gap.as_ref(), resolved, 0.0).max(0.0);
                    let pad = resolve_property_dimension_px(t.cell_padding.as_ref(), resolved, 0.0)
                        .max(0.0);
                    members.entry(flow_id.clone()).or_default().push(Member {
                        table: t,
                        w,
                        h,
                        gap,
                        pad,
                    });
                }
                // A flow table's own cells may host nested flow tables; recurse.
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_flows(&cell.children, resolved, members);
                    }
                }
            }
            Node::Frame(f) => collect_flows(&f.children, resolved, members),
            Node::Group(g) => collect_flows(&g.children, resolved, members),
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
            | Node::Toc(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Mesh(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Build the DOCUMENT-WIDE table-flow assignment map across every page.
///
/// Members are collected in (page-order, then source-order). For each flow id the
/// first member is the SOURCE: its header rows repeat on every member and its
/// body rows are distributed greedily so each member takes the rows that fit its
/// own box height; the last member keeps any remainder. Returns an empty map when
/// no `flows` members are present, in which case `compile_table` behaves exactly
/// as before for every table.
pub(super) fn resolve_table_flows<'a>(
    doc: &'a zenith_core::Document,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    diagnostics: &mut Vec<Diagnostic>,
) -> TableFlowAssignments {
    let mut members: BTreeMap<String, Vec<Member<'a>>> = BTreeMap::new();
    for page in &doc.body.pages {
        collect_flows(&page.children, resolved, &mut members);
    }

    let env = MeasureEnv {
        resolved,
        style_map,
        fonts,
        engine,
    };

    let mut assignments: TableFlowAssignments = BTreeMap::new();
    for flow_members in members.values() {
        distribute_one_flow(flow_members, env, diagnostics, &mut assignments);
    }
    assignments
}

/// Total natural height (incl. interior gaps) of `count` consecutive natural row
/// heights starting at `start`, using `gap` between rows.
fn block_height(row_natural: &[f64], start: usize, count: usize, gap: f64) -> f64 {
    if count == 0 {
        return 0.0;
    }
    let mut h = 0.0;
    for i in 0..count {
        h += row_natural.get(start + i).copied().unwrap_or(0.0);
    }
    h + gap * (count.saturating_sub(1)) as f64
}

/// The rowspan extent (number of consecutive body rows that must stay together)
/// of the body row at offset `i` within `body`, clamped to `[1, remaining]`. A
/// row whose cells declare `rowspan>1` forms a group with the following rows so a
/// `rowspan` cell is never split across a member boundary (v0: single-level
/// grouping by the row's own max rowspan).
fn rowspan_extent(body: &[TableRow], i: usize, remaining: usize) -> usize {
    let span = body
        .get(i)
        .map(|r| {
            r.cells
                .iter()
                .map(|c| c.rowspan.max(1) as usize)
                .max()
                .unwrap_or(1)
        })
        .unwrap_or(1);
    span.clamp(1, remaining.max(1))
}

/// Distribute one flow group's source body rows across its members, writing a
/// [`TableFlowAssignment`] per member into `assignments`.
fn distribute_one_flow(
    flow_members: &[Member<'_>],
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
    assignments: &mut TableFlowAssignments,
) {
    let Some(source) = flow_members.first() else {
        return;
    };
    let src = source.table;
    let columns: Vec<TableColumn> = src.columns.clone();
    let header_rows = (src.header_rows.unwrap_or(0) as usize).min(src.rows.len());
    let header: Vec<TableRow> = src.rows.get(..header_rows).unwrap_or(&[]).to_vec();
    let body: &[TableRow] = src.rows.get(header_rows..).unwrap_or(&[]);

    let last_index = flow_members.len().saturating_sub(1);
    // Cursor into `body`: the next body row this member's slice begins at.
    let mut cursor = 0usize;

    for (mi, member) in flow_members.iter().enumerate() {
        let is_last = mi == last_index;
        let col_count = columns.len().max(1);

        // The body rows still available to this (and later) members.
        let remaining_body: &[TableRow] = body.get(cursor..).unwrap_or(&[]);

        let take = if is_last {
            // Last member keeps everything that remains (may overflow + clip).
            remaining_body.len()
        } else {
            // Measure header + remaining body at THIS member's box to get the
            // natural per-row heights, then greedily accumulate whole rowspan
            // groups whose total height fits the member's body capacity.
            let mut candidate: Vec<TableRow> = header.clone();
            candidate.extend_from_slice(remaining_body);
            let row_count = candidate.len();
            if row_count == 0 {
                0
            } else {
                let placed = place_cells(&candidate, col_count, row_count);
                let layout = compute_table_layout(
                    &columns,
                    &placed,
                    GridDims {
                        col_count,
                        row_count,
                        gap: member.gap,
                        pad: member.pad,
                        table_w: member.w,
                        table_h: member.h,
                    },
                    env,
                    diagnostics,
                    header_rows,
                    src.header_style.as_deref(),
                );
                let row_natural = &layout.row_natural;

                // Header always rides on every member; its height (plus the gap
                // separating it from the body) is subtracted from the box first.
                let header_h = block_height(row_natural, 0, header_rows, member.gap);
                let avail = (member.h - 2.0 * member.pad).max(0.0);
                let after_header_gap = if header_rows > 0 { member.gap } else { 0.0 };
                let mut used = header_h + after_header_gap;

                greedy_take(
                    remaining_body,
                    row_natural,
                    GreedyCtx {
                        header_rows,
                        gap: member.gap,
                        avail,
                        table_id: &src.id,
                        source_span: src.source_span,
                    },
                    &mut used,
                    diagnostics,
                )
            }
        };

        let take = take.min(remaining_body.len());
        let mut rows: Vec<TableRow> = header.clone();
        rows.extend_from_slice(remaining_body.get(..take).unwrap_or(&[]));

        assignments.insert(
            member.table.id.clone(),
            TableFlowAssignment {
                columns: columns.clone(),
                rows,
            },
        );

        cursor = cursor.saturating_add(take);
    }
}

/// Scalar inputs for [`greedy_take`]: the header-row offset into the candidate
/// layout, the inter-row gap, the member's available body height, and the source
/// table id / span used for the overflow advisory. Bundled into one `Copy` value
/// so the greedy walk stays under the argument-count lint without suppression.
#[derive(Clone, Copy)]
struct GreedyCtx<'a> {
    header_rows: usize,
    gap: f64,
    avail: f64,
    table_id: &'a str,
    source_span: Option<zenith_core::Span>,
}

/// Greedily accumulate whole rowspan groups of `body` into a non-last member.
///
/// `row_natural` is the natural per-row height of the candidate layout
/// `header ++ body` (so body row `j` is at index `gc.header_rows + j`). Groups are
/// added while their height keeps `used <= gc.avail`. Termination + non-empty
/// guarantee: if not even the FIRST group fits, that group is taken anyway (so a
/// member never stalls the flow with a zero slice) and a `table.flow_overflow`
/// advisory is emitted. Returns the number of body rows taken (≥1 when body is
/// non-empty, capped at `body.len()`).
fn greedy_take(
    body: &[TableRow],
    row_natural: &[f64],
    gc: GreedyCtx<'_>,
    used: &mut f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let GreedyCtx {
        header_rows,
        gap,
        avail,
        table_id,
        source_span,
    } = gc;
    let mut taken = 0usize;
    while taken < body.len() {
        let remaining = body.len() - taken;
        let extent = rowspan_extent(body, taken, remaining);
        // Height of this group within the candidate layout's natural rows.
        let group_h = block_height(row_natural, header_rows + taken, extent, gap);
        // The inter-row gap that would precede this group (if rows precede it).
        let lead_gap = if taken > 0 { gap } else { 0.0 };

        if *used + lead_gap + group_h <= avail || taken == 0 {
            // Fits — OR this is the first group and we MUST place ≥1 to advance.
            if taken == 0 && *used + lead_gap + group_h > avail {
                // First group does not fit even alone: place + clip, advise once.
                diagnostics.push(Diagnostic::advisory(
                    "table.flow_overflow",
                    format!(
                        "table flow '{table_id}': a rowspan group is taller than a \
                         continuation box; placed and clipped"
                    ),
                    source_span,
                    Some(table_id.to_owned()),
                ));
            }
            *used += lead_gap + group_h;
            taken += extent;
        } else {
            break;
        }
    }
    taken.min(body.len())
}
