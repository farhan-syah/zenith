//! Specialized node writers: shape, connector, field, toc, footnote, instance
//! (with its `override` children), and the lossless `unknown` forward-compat
//! node.

use crate::ast::{
    ConnectorNode, FieldNode, FootnoteNode, InstanceNode, Override, ShapeNode, TocNode, UnknownNode,
};

use crate::format::writer::{
    fmt_unknown_property, indent, write_opt_bool, write_opt_dimension, write_opt_f64,
    write_opt_property_value, write_opt_str,
};

use super::helpers::write_span;
use super::write_children_block;

pub(super) fn write_shape(s: &ShapeNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("shape");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, kind, fill, stroke,
    // stroke-width, radius, stroke-alignment, padding, h-align, v-align,
    // text-style, style, opacity, visible, locked, rotate, then unknown props
    // (sorted), then the span children.
    out.push_str(" id=\"");
    out.push_str(&s.id);
    out.push('"');
    write_opt_str(out, "name", &s.name);
    write_opt_str(out, "role", &s.role);
    write_opt_str(out, "anchor", &s.anchor);
    write_opt_str(out, "anchor-zone", &s.anchor_zone);
    write_opt_str(out, "anchor-sibling", &s.anchor_sibling);
    write_opt_bool(out, "anchor-parent", &s.anchor_parent);
    write_opt_dimension(out, "x", &s.x);
    write_opt_dimension(out, "y", &s.y);
    write_opt_dimension(out, "w", &s.w);
    write_opt_dimension(out, "h", &s.h);
    write_opt_str(out, "kind", &s.kind);
    write_opt_property_value(out, "fill", &s.fill);
    write_opt_property_value(out, "stroke", &s.stroke);
    write_opt_property_value(out, "stroke-width", &s.stroke_width);
    write_opt_property_value(out, "radius", &s.radius);
    write_opt_str(out, "stroke-alignment", &s.stroke_alignment);
    write_opt_property_value(out, "padding", &s.padding);
    write_opt_str(out, "h-align", &s.h_align);
    write_opt_str(out, "v-align", &s.v_align);
    write_opt_str(out, "text-style", &s.text_style);
    write_opt_str(out, "style", &s.style);
    write_opt_f64(out, "opacity", &s.opacity);
    write_opt_bool(out, "visible", &s.visible);
    write_opt_bool(out, "locked", &s.locked);
    write_opt_dimension(out, "rotate", &s.rotate);

    // Unknown properties in sorted key order.
    for (key, prop) in &s.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    for span in &s.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_connector(c: &ConnectorNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("connector");

    // Canonical property order: id, name, role, from, to, from-anchor,
    // to-anchor, route, marker-start, marker-end, stroke, stroke-width,
    // opacity, visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&c.id);
    out.push('"');
    write_opt_str(out, "name", &c.name);
    write_opt_str(out, "role", &c.role);
    write_opt_str(out, "from", &c.from);
    write_opt_str(out, "to", &c.to);
    write_opt_str(out, "from-anchor", &c.from_anchor);
    write_opt_str(out, "to-anchor", &c.to_anchor);
    write_opt_str(out, "route", &c.route);
    write_opt_str(out, "marker-start", &c.marker_start);
    write_opt_str(out, "marker-end", &c.marker_end);
    write_opt_property_value(out, "stroke", &c.stroke);
    write_opt_property_value(out, "stroke-width", &c.stroke_width);
    write_opt_f64(out, "opacity", &c.opacity);
    write_opt_bool(out, "visible", &c.visible);
    write_opt_bool(out, "locked", &c.locked);
    write_opt_dimension(out, "rotate", &c.rotate);
    write_opt_str(out, "style", &c.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &c.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_field(f: &FieldNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("field");

    // Canonical property order: id, name, role, anchor, anchor-zone, type, recto, verso, target,
    // x, y, w, h, fill, font-family, font-size, opacity, visible, locked, style,
    // then unknown props (sorted). A field is a leaf — no child block.
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
    write_opt_str(out, "anchor", &f.anchor);
    write_opt_str(out, "anchor-zone", &f.anchor_zone);
    write_opt_str(out, "anchor-sibling", &f.anchor_sibling);
    write_opt_bool(out, "anchor-parent", &f.anchor_parent);
    out.push_str(" type=\"");
    out.push_str(&f.field_type);
    out.push('"');
    write_opt_str(out, "recto", &f.recto);
    write_opt_str(out, "verso", &f.verso);
    write_opt_str(out, "target", &f.target);
    write_opt_dimension(out, "x", &f.x);
    write_opt_dimension(out, "y", &f.y);
    write_opt_dimension(out, "w", &f.w);
    write_opt_dimension(out, "h", &f.h);
    write_opt_property_value(out, "fill", &f.fill);
    write_opt_property_value(out, "font-family", &f.font_family);
    write_opt_property_value(out, "font-size", &f.font_size);
    write_opt_f64(out, "opacity", &f.opacity);
    write_opt_bool(out, "visible", &f.visible);
    write_opt_bool(out, "locked", &f.locked);
    write_opt_str(out, "style", &f.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &f.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_toc(t: &TocNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("toc");

    // Canonical property order: id, name, role, anchor, anchor-zone, match-role, match-style,
    // leader, folio-style, x, y, w, h, fill, font-family, font-size, opacity,
    // visible, locked, style, then unknown props (sorted). A toc is a leaf —
    // no child block.
    out.push_str(" id=\"");
    out.push_str(&t.id);
    out.push('"');
    write_opt_str(out, "name", &t.name);
    write_opt_str(out, "role", &t.role);
    write_opt_str(out, "anchor", &t.anchor);
    write_opt_str(out, "anchor-zone", &t.anchor_zone);
    write_opt_str(out, "anchor-sibling", &t.anchor_sibling);
    write_opt_bool(out, "anchor-parent", &t.anchor_parent);
    write_opt_str(out, "match-role", &t.match_role);
    write_opt_str(out, "match-style", &t.match_style);
    write_opt_str(out, "leader", &t.leader);
    write_opt_str(out, "folio-style", &t.folio_style);
    write_opt_dimension(out, "x", &t.x);
    write_opt_dimension(out, "y", &t.y);
    write_opt_dimension(out, "w", &t.w);
    write_opt_dimension(out, "h", &t.h);
    write_opt_property_value(out, "fill", &t.fill);
    write_opt_property_value(out, "font-family", &t.font_family);
    write_opt_property_value(out, "font-size", &t.font_size);
    write_opt_f64(out, "opacity", &t.opacity);
    write_opt_bool(out, "visible", &t.visible);
    write_opt_bool(out, "locked", &t.locked);
    write_opt_str(out, "style", &t.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &t.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_footnote(f: &FootnoteNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("footnote");

    // Canonical property order: id, name, role, marker, fill, font-family,
    // font-size, style, then unknown props (sorted), then the span children.
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
    write_opt_str(out, "marker", &f.marker);
    write_opt_property_value(out, "fill", &f.fill);
    write_opt_property_value(out, "font-family", &f.font_family);
    write_opt_property_value(out, "font-size", &f.font_size);
    write_opt_str(out, "style", &f.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &f.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    for span in &f.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_instance(i: &InstanceNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("instance");

    // Canonical property order: id, name, role, component, x, y, opacity,
    // visible, locked, then unknown props (sorted), then the override children.
    out.push_str(" id=\"");
    out.push_str(&i.id);
    out.push('"');
    write_opt_str(out, "name", &i.name);
    write_opt_str(out, "role", &i.role);
    out.push_str(" component=\"");
    out.push_str(&i.component);
    out.push('"');
    write_opt_dimension(out, "x", &i.x);
    write_opt_dimension(out, "y", &i.y);
    write_opt_f64(out, "opacity", &i.opacity);
    write_opt_bool(out, "visible", &i.visible);
    write_opt_bool(out, "locked", &i.locked);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &i.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    // Always emit a brace block (container style), even with no overrides, so
    // an instance is visually consistent with group/frame container nodes.
    out.push_str(" {\n");
    for ov in &i.overrides {
        write_override(ov, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_override(ov: &Override, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("override ref=\"");
    out.push_str(&ov.ref_id);
    out.push('"');
    write_opt_property_value(out, "fill", &ov.fill);
    write_opt_bool(out, "visible", &ov.visible);

    // Span children (replacement text) live in a brace block; emit one only
    // when the override carries spans, otherwise close the line.
    match &ov.spans {
        Some(spans) => {
            out.push_str(" {\n");
            for span in spans {
                write_span(span, out, depth + 1);
            }
            indent(out, depth);
            out.push_str("}\n");
        }
        None => out.push('\n'),
    }
}

pub(super) fn write_unknown_node(u: &UnknownNode, out: &mut String, depth: usize) {
    // Lossless forward-compat emit: `<kind>` then `id="..."` (if present), then
    // every preserved property (with its value annotation) in sorted key order,
    // then a children block if non-empty — mirroring `write_group`/`write_frame`.
    indent(out, depth);
    out.push_str(&u.kind);

    if let Some(id) = &u.id {
        out.push_str(" id=\"");
        out.push_str(id);
        out.push('"');
    }

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &u.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    if u.children.is_empty() {
        out.push('\n');
    } else {
        out.push_str(" {\n");
        write_children_block(&u.children, out, depth);
        indent(out, depth);
        out.push_str("}\n");
    }
}
