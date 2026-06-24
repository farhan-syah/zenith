//! Per-kind checks for the container nodes `frame`, `group`, and `table`.
//!
//! These functions emit each container's OWN diagnostics. The child recursion
//! (building a fresh [`super::super::nodes::WalkPos`] and descending) stays in
//! the dispatcher [`super::super::nodes::walk_node`] so traversal/emit order is
//! unchanged.

use std::collections::BTreeSet;

use crate::ast::node::{FrameNode, GroupNode, TableNode};
use crate::diagnostics::Diagnostic;

use super::shared::{
    AnchorParentCtx, AnchorProps, check_anchor, check_optional_dim, check_style_ref,
    is_valid_blend_mode,
};
use super::suggest::check_unknown_props;
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

pub(in crate::validate::check) fn check_frame(
    f: &FrameNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    geom_required: bool,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&f.id, seen_ids, diagnostics);
    if let Some(bm) = f.blend_mode.as_deref()
        && !is_valid_blend_mode(bm)
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "frame '{}': blend-mode '{bm}' is not a recognized value",
                f.id
            ),
            f.source_span,
            Some(f.id.clone()),
        ));
    }
    check_style_ref(
        &f.id,
        f.style.as_deref(),
        declared_style_ids,
        f.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y.
    let anchor_active = check_anchor(
        &f.id,
        AnchorProps {
            anchor: f.anchor.as_deref(),
            anchor_zone: f.anchor_zone.as_deref(),
            anchor_sibling: f.anchor_sibling.as_deref(),
            anchor_parent: f.anchor_parent == Some(true),
            anchor_edge: f.anchor_edge.as_deref(),
            anchor_gap: f.anchor_gap.as_ref(),
        },
        parent_ctx,
        zone_ids,
        f.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    // Frames REQUIRE all four geometry dimensions (unlike groups).
    check_optional_dim(
        &f.id,
        "x",
        f.x.as_ref(),
        xy_required,
        f.source_span,
        diagnostics,
    );
    check_optional_dim(
        &f.id,
        "y",
        f.y.as_ref(),
        xy_required,
        f.source_span,
        diagnostics,
    );
    check_optional_dim(
        &f.id,
        "w",
        f.w.as_ref(),
        geom_required,
        f.source_span,
        diagnostics,
    );
    check_optional_dim(
        &f.id,
        "h",
        f.h.as_ref(),
        geom_required,
        f.source_span,
        diagnostics,
    );

    if let Some(d) = f.blur.as_ref()
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("frame '{}': blur must be >= 0", f.id),
            f.source_span,
            Some(f.id.clone()),
        ));
    }

    // Grid layout advisory: `layout="grid"` without a positive `columns`
    // defaults the scene to a single column. Non-fatal.
    if f.layout.as_deref() == Some("grid") && f.columns.unwrap_or(0) == 0 {
        diagnostics.push(Diagnostic::advisory(
            "grid.missing_columns",
            format!(
                "frame '{}' uses layout=\"grid\" without a positive `columns`; \
                 defaulting to 1 column",
                f.id
            ),
            f.source_span,
            Some(f.id.clone()),
        ));
    }

    // Unknown properties.
    check_unknown_props("frame", &f.id, &f.unknown_props, f.source_span, diagnostics);
}

pub(in crate::validate::check) fn check_group(
    g: &GroupNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&g.id, seen_ids, diagnostics);
    if let Some(bm) = g.blend_mode.as_deref()
        && !is_valid_blend_mode(bm)
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "group '{}': blend-mode '{bm}' is not a recognized value",
                g.id
            ),
            g.source_span,
            Some(g.id.clone()),
        ));
    }
    check_style_ref(
        &g.id,
        g.style.as_deref(),
        declared_style_ids,
        g.source_span,
        diagnostics,
    );

    // Groups have NO required geometry — x/y/w/h are all advisory.
    // Still validate the anchor value if present.
    check_anchor(
        &g.id,
        AnchorProps {
            anchor: g.anchor.as_deref(),
            anchor_zone: g.anchor_zone.as_deref(),
            anchor_sibling: g.anchor_sibling.as_deref(),
            anchor_parent: g.anchor_parent == Some(true),
            anchor_edge: g.anchor_edge.as_deref(),
            anchor_gap: g.anchor_gap.as_ref(),
        },
        parent_ctx,
        zone_ids,
        g.source_span,
        diagnostics,
    );

    if let Some(d) = g.blur.as_ref()
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("group '{}': blur must be >= 0", g.id),
            g.source_span,
            Some(g.id.clone()),
        ));
    }

    if let Some(v) = g.intensity
        && !(0.0..=1.0).contains(&v)
    {
        diagnostics.push(Diagnostic::warning(
            "group.invalid_intensity",
            format!("group '{}': intensity {v} is out of range 0.0..=1.0", g.id),
            g.source_span,
            Some(g.id.clone()),
        ));
    }

    // Unknown properties.
    check_unknown_props("group", &g.id, &g.unknown_props, g.source_span, diagnostics);
}

pub(in crate::validate::check) fn check_table(
    t: &TableNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    geom_required: bool,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&t.id, seen_ids, diagnostics);
    check_style_ref(
        &t.id,
        t.style.as_deref(),
        declared_style_ids,
        t.source_span,
        diagnostics,
    );
    // `header-style` is a style ref carried for a later unit; validate it
    // against the declared styles now so authoring errors surface early.
    check_style_ref(
        &t.id,
        t.header_style.as_deref(),
        declared_style_ids,
        t.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y.
    let anchor_active = check_anchor(
        &t.id,
        AnchorProps {
            anchor: t.anchor.as_deref(),
            anchor_zone: t.anchor_zone.as_deref(),
            anchor_sibling: t.anchor_sibling.as_deref(),
            anchor_parent: t.anchor_parent == Some(true),
            anchor_edge: t.anchor_edge.as_deref(),
            anchor_gap: t.anchor_gap.as_ref(),
        },
        parent_ctx,
        zone_ids,
        t.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    // Required geometry: x, y, w, h must all be present (mirror frame).
    check_optional_dim(
        &t.id,
        "x",
        t.x.as_ref(),
        xy_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "y",
        t.y.as_ref(),
        xy_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "w",
        t.w.as_ref(),
        geom_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "h",
        t.h.as_ref(),
        geom_required,
        t.source_span,
        diagnostics,
    );

    // Token-typed visual props: colors and dimensions.
    for (prop_name, prop_val) in [
        ("fill", t.fill.as_ref()),
        ("border", t.border.as_ref()),
        ("header-fill", t.header_fill.as_ref()),
    ] {
        check_visual_prop(
            &t.id,
            prop_name,
            prop_val,
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }
    for (prop_name, prop_val) in [
        ("border-width", t.border_width.as_ref()),
        ("gap", t.gap.as_ref()),
        ("cell-padding", t.cell_padding.as_ref()),
    ] {
        check_visual_prop(
            &t.id,
            prop_name,
            prop_val,
            VisualExpect::Dimension,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
    }

    // Enum-value checks (Warnings on unrecognized values, not errors).
    if let Some(ha) = t.h_align.as_deref()
        && !matches!(ha, "start" | "center" | "end")
    {
        diagnostics.push(Diagnostic::warning(
            "table.invalid_h_align",
            format!(
                "table '{}': h-align '{ha}' is not one of start/center/end",
                t.id
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }
    if let Some(va) = t.v_align.as_deref()
        && !matches!(va, "top" | "middle" | "bottom")
    {
        diagnostics.push(Diagnostic::warning(
            "table.invalid_v_align",
            format!(
                "table '{}': v-align '{va}' is not one of top/middle/bottom",
                t.id
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }
    if let Some(bc) = t.border_collapse.as_deref()
        && !matches!(bc, "separate" | "collapse")
    {
        diagnostics.push(Diagnostic::warning(
            "table.invalid_border_collapse",
            format!(
                "table '{}': border-collapse '{bc}' is not one of separate/collapse",
                t.id
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }

    // Per-cell enum checks, mirroring the table-level checks.
    for row in &t.rows {
        for cell in &row.cells {
            if let Some(ha) = cell.h_align.as_deref()
                && !matches!(ha, "start" | "center" | "end")
            {
                diagnostics.push(Diagnostic::warning(
                    "table.invalid_h_align",
                    format!(
                        "table '{}': cell h-align '{ha}' is not one of start/center/end",
                        t.id
                    ),
                    cell.source_span,
                    Some(t.id.clone()),
                ));
            }
            if let Some(va) = cell.v_align.as_deref()
                && !matches!(va, "top" | "middle" | "bottom")
            {
                diagnostics.push(Diagnostic::warning(
                    "table.invalid_v_align",
                    format!(
                        "table '{}': cell v-align '{va}' is not one of top/middle/bottom",
                        t.id
                    ),
                    cell.source_span,
                    Some(t.id.clone()),
                ));
            }
            // Per-cell token-typed visual props.
            check_visual_prop(
                &t.id,
                "cell fill",
                cell.fill.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "cell border",
                cell.border.as_ref(),
                VisualExpect::Color,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
            check_visual_prop(
                &t.id,
                "cell border-width",
                cell.border_width.as_ref(),
                VisualExpect::Dimension,
                referenced_token_ids,
                resolved_tokens,
                diagnostics,
            );
        }
    }

    // Unknown properties on the table node itself.
    check_unknown_props("table", &t.id, &t.unknown_props, t.source_span, diagnostics);

    // Unknown properties on each column declaration.
    for col in &t.columns {
        check_unknown_props(
            "column",
            &t.id,
            &col.unknown_props,
            col.source_span,
            diagnostics,
        );
    }

    // Unknown properties on each row and cell.
    for row in &t.rows {
        check_unknown_props(
            "row",
            &t.id,
            &row.unknown_props,
            row.source_span,
            diagnostics,
        );
        for cell in &row.cells {
            check_unknown_props(
                "cell",
                &t.id,
                &cell.unknown_props,
                cell.source_span,
                diagnostics,
            );
        }
    }

    // ── Cell-span consistency ──────────────────────────────────────
    // HTML-table cell flow: place each cell in the next free column of
    // its row, honoring colspan/rowspan via a BTreeSet occupancy grid
    // keyed by (row, col). A colspan that would run past the column
    // count, or a rowspan past the last row, is a hard error.
    let col_count = t.columns.len().max(1);
    let row_count = t.rows.len();
    let mut occupied: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (r, row) in t.rows.iter().enumerate() {
        let mut col_cursor = 0usize;
        for cell in &row.cells {
            // Advance to the next column free at this row.
            while col_cursor < col_count && occupied.contains(&(r, col_cursor)) {
                col_cursor += 1;
            }
            let cs = cell.colspan.max(1) as usize;
            let rs = cell.rowspan.max(1) as usize;
            if col_cursor + cs > col_count {
                diagnostics.push(Diagnostic::error(
                    "table.cell_overflow",
                    format!(
                        "table '{}': cell at row {r} starting column {col_cursor} with \
                         colspan {cs} exceeds the column count {col_count}",
                        t.id
                    ),
                    cell.source_span,
                    Some(t.id.clone()),
                ));
            }
            if r + rs > row_count {
                diagnostics.push(Diagnostic::error(
                    "table.cell_overflow",
                    format!(
                        "table '{}': cell at row {r} with rowspan {rs} extends past the \
                         last row (row count {row_count})",
                        t.id
                    ),
                    cell.source_span,
                    Some(t.id.clone()),
                ));
            }
            // Mark the cell's covered slots (clamped to the grid).
            for dr in 0..rs {
                for dc in 0..cs {
                    let cr = r + dr;
                    let cc = col_cursor + dc;
                    if cr < row_count && cc < col_count {
                        occupied.insert((cr, cc));
                    }
                }
            }
            col_cursor += cs;
        }
    }
}
