//! Writers for effect-producing nodes.

use crate::ast::node::{LightNode, MeshNode};
use crate::format::writer::{
    fmt_unknown_property, indent, write_opt_bool, write_opt_dimension, write_opt_f64,
    write_opt_property_value, write_opt_str,
};

pub(super) fn write_light(l: &LightNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("light id=\"");
    out.push_str(&l.id);
    out.push('"');
    write_opt_str(out, "name", &l.name);
    write_opt_str(out, "role", &l.role);
    write_opt_str(out, "kind", &l.kind);
    write_opt_property_value(out, "x", &l.x);
    write_opt_property_value(out, "y", &l.y);
    write_opt_property_value(out, "radius", &l.radius);
    write_opt_property_value(out, "color", &l.color);
    write_opt_f64(out, "opacity", &l.opacity);
    write_opt_bool(out, "visible", &l.visible);
    write_opt_bool(out, "locked", &l.locked);
    write_opt_dimension(out, "angle", &l.angle);
    for (key, prop) in &l.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }
    out.push('\n');
}

pub(super) fn write_mesh(m: &MeshNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("mesh id=\"");
    out.push_str(&m.id);
    out.push('"');
    write_opt_str(out, "name", &m.name);
    write_opt_str(out, "role", &m.role);
    write_opt_str(out, "kind", &m.kind);
    write_opt_property_value(out, "x", &m.x);
    write_opt_property_value(out, "y", &m.y);
    write_opt_property_value(out, "w", &m.w);
    write_opt_property_value(out, "h", &m.h);
    if let Some(rows) = m.rows {
        out.push_str(" rows=");
        out.push_str(&rows.to_string());
    }
    if let Some(columns) = m.columns {
        out.push_str(" columns=");
        out.push_str(&columns.to_string());
    }
    write_opt_property_value(out, "vanishing-x", &m.vanishing_x);
    write_opt_property_value(out, "vanishing-y", &m.vanishing_y);
    write_opt_property_value(out, "extend", &m.extend);
    write_opt_property_value(out, "stroke", &m.stroke);
    write_opt_property_value(out, "stroke-width", &m.stroke_width);
    write_opt_property_value(out, "stroke-dash", &m.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &m.stroke_gap);
    write_opt_str(out, "stroke-linecap", &m.stroke_linecap);
    write_opt_f64(out, "opacity", &m.opacity);
    write_opt_bool(out, "visible", &m.visible);
    write_opt_bool(out, "locked", &m.locked);
    for (key, prop) in &m.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }
    out.push('\n');
}
