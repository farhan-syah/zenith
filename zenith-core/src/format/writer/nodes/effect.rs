//! Writers for effect-producing nodes.

use crate::ast::node::LightNode;
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
