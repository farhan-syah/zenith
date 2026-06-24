//! Container node writers: frame, group, and table (with its `column` / `row` /
//! `cell` children).

use std::fmt::Write as _;

use crate::ast::{FrameNode, GroupNode, TableCell, TableNode, TableRow};

use crate::format::writer::{
    fmt_unknown_property, indent, write_opt_bool, write_opt_dimension, write_opt_f64,
    write_opt_property_value, write_opt_str,
};

use super::write_children_block;

pub(super) fn write_frame(f: &FrameNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("frame");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, layout, columns,
    // rows, opacity, visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
    write_opt_str(out, "anchor", &f.anchor);
    write_opt_str(out, "anchor-zone", &f.anchor_zone);
    write_opt_str(out, "anchor-sibling", &f.anchor_sibling);
    write_opt_str(out, "anchor-edge", &f.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &f.anchor_gap);
    write_opt_bool(out, "anchor-parent", &f.anchor_parent);
    write_opt_dimension(out, "x", &f.x);
    write_opt_dimension(out, "y", &f.y);
    write_opt_dimension(out, "w", &f.w);
    write_opt_dimension(out, "h", &f.h);
    write_opt_str(out, "layout", &f.layout);
    if let Some(n) = f.columns {
        let _ = write!(out, " columns={n}");
    }
    if let Some(n) = f.rows {
        let _ = write!(out, " rows={n}");
    }
    write_opt_f64(out, "opacity", &f.opacity);
    write_opt_bool(out, "visible", &f.visible);
    write_opt_bool(out, "locked", &f.locked);
    write_opt_dimension(out, "rotate", &f.rotate);
    write_opt_str(out, "blend-mode", &f.blend_mode);
    write_opt_dimension(out, "blur", &f.blur);
    write_opt_str(out, "style", &f.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &f.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    write_children_block(&f.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_group(g: &GroupNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("group");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, opacity,
    // visible, locked, rotate, blend-mode, blur, style, semantic-role, intensity,
    // layer-priority, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&g.id);
    out.push('"');
    write_opt_str(out, "name", &g.name);
    write_opt_str(out, "role", &g.role);
    write_opt_str(out, "anchor", &g.anchor);
    write_opt_str(out, "anchor-zone", &g.anchor_zone);
    write_opt_str(out, "anchor-sibling", &g.anchor_sibling);
    write_opt_str(out, "anchor-edge", &g.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &g.anchor_gap);
    write_opt_bool(out, "anchor-parent", &g.anchor_parent);
    write_opt_dimension(out, "x", &g.x);
    write_opt_dimension(out, "y", &g.y);
    write_opt_dimension(out, "w", &g.w);
    write_opt_dimension(out, "h", &g.h);
    write_opt_f64(out, "opacity", &g.opacity);
    write_opt_bool(out, "visible", &g.visible);
    write_opt_bool(out, "locked", &g.locked);
    write_opt_dimension(out, "rotate", &g.rotate);
    write_opt_str(out, "blend-mode", &g.blend_mode);
    write_opt_dimension(out, "blur", &g.blur);
    write_opt_str(out, "style", &g.style);
    write_opt_str(out, "semantic-role", &g.semantic_role);
    write_opt_f64(out, "intensity", &g.intensity);
    if let Some(n) = g.layer_priority {
        let _ = write!(out, " layer-priority={n}");
    }

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &g.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    write_children_block(&g.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_table(t: &TableNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("table");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, header-rows, flows,
    // gap, cell-padding, border-collapse, fill, border, border-width, header-fill,
    // header-style, h-align, v-align, opacity, visible, locked, rotate, style,
    // then unknown props (sorted), then the column/row children block.
    out.push_str(" id=\"");
    out.push_str(&t.id);
    out.push('"');
    write_opt_str(out, "name", &t.name);
    write_opt_str(out, "role", &t.role);
    write_opt_str(out, "anchor", &t.anchor);
    write_opt_str(out, "anchor-zone", &t.anchor_zone);
    write_opt_str(out, "anchor-sibling", &t.anchor_sibling);
    write_opt_str(out, "anchor-edge", &t.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &t.anchor_gap);
    write_opt_bool(out, "anchor-parent", &t.anchor_parent);
    write_opt_dimension(out, "x", &t.x);
    write_opt_dimension(out, "y", &t.y);
    write_opt_dimension(out, "w", &t.w);
    write_opt_dimension(out, "h", &t.h);
    if let Some(n) = t.header_rows {
        let _ = write!(out, " header-rows={n}");
    }
    write_opt_str(out, "flows", &t.flows);
    write_opt_property_value(out, "gap", &t.gap);
    write_opt_property_value(out, "cell-padding", &t.cell_padding);
    write_opt_str(out, "border-collapse", &t.border_collapse);
    write_opt_property_value(out, "fill", &t.fill);
    write_opt_property_value(out, "border", &t.border);
    write_opt_property_value(out, "border-width", &t.border_width);
    write_opt_property_value(out, "header-fill", &t.header_fill);
    write_opt_str(out, "header-style", &t.header_style);
    write_opt_str(out, "h-align", &t.h_align);
    write_opt_str(out, "v-align", &t.v_align);
    write_opt_f64(out, "opacity", &t.opacity);
    write_opt_bool(out, "visible", &t.visible);
    write_opt_bool(out, "locked", &t.locked);
    write_opt_dimension(out, "rotate", &t.rotate);
    write_opt_str(out, "style", &t.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &t.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    for col in &t.columns {
        indent(out, depth + 1);
        out.push_str("column");
        write_opt_dimension(out, "width", &col.width);
        // Unknown properties in sorted key order (BTreeMap iteration is sorted).
        for (key, prop) in &col.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push('\n');
    }
    for row in &t.rows {
        write_table_row(row, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_table_row(r: &TableRow, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("row");

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &r.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    for cell in &r.cells {
        write_table_cell(cell, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit a `table` node's `cell` child (with its own node children block).
fn write_table_cell(c: &TableCell, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("cell");
    if c.colspan != 1 {
        let _ = write!(out, " colspan={}", c.colspan);
    }
    if c.rowspan != 1 {
        let _ = write!(out, " rowspan={}", c.rowspan);
    }
    write_opt_property_value(out, "fill", &c.fill);
    write_opt_property_value(out, "border", &c.border);
    write_opt_property_value(out, "border-width", &c.border_width);
    write_opt_str(out, "h-align", &c.h_align);
    write_opt_str(out, "v-align", &c.v_align);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &c.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    write_children_block(&c.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}
