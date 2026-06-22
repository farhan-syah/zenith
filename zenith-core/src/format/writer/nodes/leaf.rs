//! Leaf and vector node writers: rect, ellipse, line, text, code, image,
//! polygon, and polyline.

use std::fmt::Write as _;

use crate::ast::{
    CodeNode, EllipseNode, ImageNode, LineNode, PolygonNode, PolylineNode, RectNode, TextNode,
};

use crate::format::writer::{
    escape_kdl_string, fmt_unknown_property, indent, write_opt_bool, write_opt_dimension,
    write_opt_f64, write_opt_object_position, write_opt_property_value, write_opt_str,
};

use super::helpers::{write_points, write_span};

pub(super) fn write_rect(r: &RectNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("rect");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, radius, fill,
    // stroke, stroke-width, stroke-alignment, opacity, visible, locked, rotate, style
    out.push_str(" id=\"");
    out.push_str(&r.id);
    out.push('"');
    write_opt_str(out, "name", &r.name);
    write_opt_str(out, "role", &r.role);
    write_opt_str(out, "anchor", &r.anchor);
    write_opt_str(out, "anchor-zone", &r.anchor_zone);
    write_opt_bool(out, "anchor-parent", &r.anchor_parent);
    write_opt_dimension(out, "x", &r.x);
    write_opt_dimension(out, "y", &r.y);
    write_opt_dimension(out, "w", &r.w);
    write_opt_dimension(out, "h", &r.h);
    write_opt_property_value(out, "radius", &r.radius);
    write_opt_property_value(out, "radius-tl", &r.radius_tl);
    write_opt_property_value(out, "radius-tr", &r.radius_tr);
    write_opt_property_value(out, "radius-br", &r.radius_br);
    write_opt_property_value(out, "radius-bl", &r.radius_bl);
    write_opt_property_value(out, "fill", &r.fill);
    write_opt_property_value(out, "stroke", &r.stroke);
    write_opt_property_value(out, "stroke-width", &r.stroke_width);
    write_opt_str(out, "stroke-alignment", &r.stroke_alignment);
    write_opt_property_value(out, "stroke-dash", &r.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &r.stroke_gap);
    write_opt_str(out, "stroke-linecap", &r.stroke_linecap);
    write_opt_property_value(out, "border-top", &r.border_top);
    write_opt_property_value(out, "border-bottom", &r.border_bottom);
    write_opt_property_value(out, "border-left", &r.border_left);
    write_opt_property_value(out, "border-right", &r.border_right);
    write_opt_property_value(out, "border-width", &r.border_width);
    write_opt_property_value(out, "stroke-outer", &r.stroke_outer);
    write_opt_property_value(out, "stroke-outer-width", &r.stroke_outer_width);
    write_opt_property_value(out, "shadow", &r.shadow);
    write_opt_property_value(out, "filter", &r.filter);
    write_opt_property_value(out, "mask", &r.mask);
    write_opt_str(out, "blend-mode", &r.blend_mode);
    write_opt_dimension(out, "blur", &r.blur);
    write_opt_f64(out, "opacity", &r.opacity);
    write_opt_bool(out, "visible", &r.visible);
    write_opt_bool(out, "locked", &r.locked);
    write_opt_dimension(out, "rotate", &r.rotate);
    write_opt_str(out, "style", &r.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &r.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_image(i: &ImageNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("image");

    // Canonical property order: id, name, role, anchor, anchor-zone, asset, x, y, w, h,
    // src-x, src-y, src-w, src-h, fit, clip, clip-radius,
    // object-position-x, object-position-y, shadow, opacity, visible, locked,
    // rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&i.id);
    out.push('"');
    write_opt_str(out, "name", &i.name);
    write_opt_str(out, "role", &i.role);
    write_opt_str(out, "anchor", &i.anchor);
    write_opt_str(out, "anchor-zone", &i.anchor_zone);
    write_opt_bool(out, "anchor-parent", &i.anchor_parent);
    out.push_str(" asset=\"");
    out.push_str(&i.asset);
    out.push('"');
    write_opt_dimension(out, "x", &i.x);
    write_opt_dimension(out, "y", &i.y);
    write_opt_dimension(out, "w", &i.w);
    write_opt_dimension(out, "h", &i.h);
    write_opt_dimension(out, "src-x", &i.src_x);
    write_opt_dimension(out, "src-y", &i.src_y);
    write_opt_dimension(out, "src-w", &i.src_w);
    write_opt_dimension(out, "src-h", &i.src_h);
    write_opt_str(out, "fit", &i.fit);
    write_opt_str(out, "clip", &i.clip);
    write_opt_property_value(out, "clip-radius", &i.clip_radius);
    write_opt_object_position(out, "object-position-x", &i.object_position_x);
    write_opt_object_position(out, "object-position-y", &i.object_position_y);
    write_opt_property_value(out, "shadow", &i.shadow);
    write_opt_property_value(out, "filter", &i.filter);
    write_opt_property_value(out, "mask", &i.mask);
    write_opt_str(out, "blend-mode", &i.blend_mode);
    write_opt_dimension(out, "blur", &i.blur);
    write_opt_f64(out, "opacity", &i.opacity);
    write_opt_bool(out, "visible", &i.visible);
    write_opt_bool(out, "locked", &i.locked);
    write_opt_dimension(out, "rotate", &i.rotate);
    write_opt_str(out, "style", &i.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &i.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_ellipse(e: &EllipseNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("ellipse");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, fill, stroke,
    // stroke-width, opacity, visible, locked, rotate, style.
    // NOTE: stroke-alignment is not supported for ellipse in v0 (centered only).
    out.push_str(" id=\"");
    out.push_str(&e.id);
    out.push('"');
    write_opt_str(out, "name", &e.name);
    write_opt_str(out, "role", &e.role);
    write_opt_str(out, "anchor", &e.anchor);
    write_opt_str(out, "anchor-zone", &e.anchor_zone);
    write_opt_bool(out, "anchor-parent", &e.anchor_parent);
    write_opt_dimension(out, "x", &e.x);
    write_opt_dimension(out, "y", &e.y);
    write_opt_dimension(out, "w", &e.w);
    write_opt_dimension(out, "h", &e.h);
    write_opt_property_value(out, "rx", &e.rx);
    write_opt_property_value(out, "ry", &e.ry);
    write_opt_property_value(out, "fill", &e.fill);
    write_opt_property_value(out, "stroke", &e.stroke);
    write_opt_property_value(out, "stroke-width", &e.stroke_width);
    write_opt_property_value(out, "stroke-dash", &e.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &e.stroke_gap);
    write_opt_str(out, "stroke-linecap", &e.stroke_linecap);
    write_opt_property_value(out, "shadow", &e.shadow);
    write_opt_property_value(out, "filter", &e.filter);
    write_opt_property_value(out, "mask", &e.mask);
    write_opt_str(out, "blend-mode", &e.blend_mode);
    write_opt_dimension(out, "blur", &e.blur);
    write_opt_f64(out, "opacity", &e.opacity);
    write_opt_bool(out, "visible", &e.visible);
    write_opt_bool(out, "locked", &e.locked);
    write_opt_dimension(out, "rotate", &e.rotate);
    write_opt_str(out, "style", &e.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &e.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_line(l: &LineNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("line");

    // Canonical property order: id, name, role, x1, y1, x2, y2, stroke,
    // stroke-width, opacity, visible, locked, style, then unknown props.
    out.push_str(" id=\"");
    out.push_str(&l.id);
    out.push('"');
    write_opt_str(out, "name", &l.name);
    write_opt_str(out, "role", &l.role);
    write_opt_dimension(out, "x1", &l.x1);
    write_opt_dimension(out, "y1", &l.y1);
    write_opt_dimension(out, "x2", &l.x2);
    write_opt_dimension(out, "y2", &l.y2);
    write_opt_property_value(out, "stroke", &l.stroke);
    write_opt_property_value(out, "stroke-width", &l.stroke_width);
    write_opt_property_value(out, "stroke-dash", &l.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &l.stroke_gap);
    write_opt_str(out, "stroke-linecap", &l.stroke_linecap);
    write_opt_f64(out, "opacity", &l.opacity);
    write_opt_bool(out, "visible", &l.visible);
    write_opt_bool(out, "locked", &l.locked);
    write_opt_str(out, "style", &l.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &l.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push('\n');
}

pub(super) fn write_text(t: &TextNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("text");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, align, direction,
    // overflow, fill, font-family, font-size, font-weight, opacity, visible,
    // locked, rotate, style, chain
    out.push_str(" id=\"");
    out.push_str(&t.id);
    out.push('"');
    write_opt_str(out, "name", &t.name);
    write_opt_str(out, "role", &t.role);
    write_opt_str(out, "anchor", &t.anchor);
    write_opt_str(out, "anchor-zone", &t.anchor_zone);
    write_opt_bool(out, "anchor-parent", &t.anchor_parent);
    write_opt_dimension(out, "x", &t.x);
    write_opt_dimension(out, "y", &t.y);
    write_opt_dimension(out, "w", &t.w);
    write_opt_dimension(out, "h", &t.h);
    write_opt_str(out, "align", &t.align);
    write_opt_str(out, "direction", &t.direction);
    write_opt_str(out, "overflow", &t.overflow);
    write_opt_str(out, "overflow-wrap", &t.overflow_wrap);
    write_opt_dimension(out, "padding-left", &t.padding_left);
    write_opt_dimension(out, "text-indent", &t.text_indent);
    write_opt_property_value(out, "fill", &t.fill);
    write_opt_property_value(out, "stroke", &t.stroke);
    write_opt_property_value(out, "stroke-width", &t.stroke_width);
    write_opt_property_value(out, "contrast-bg", &t.contrast_bg);
    write_opt_property_value(out, "font-family", &t.font_family);
    write_opt_property_value(out, "font-size", &t.font_size);
    write_opt_property_value(out, "font-size-min", &t.font_size_min);
    write_opt_property_value(out, "font-weight", &t.font_weight);
    write_opt_property_value(out, "shadow", &t.shadow);
    write_opt_property_value(out, "filter", &t.filter);
    write_opt_property_value(out, "mask", &t.mask);
    write_opt_str(out, "blend-mode", &t.blend_mode);
    write_opt_dimension(out, "blur", &t.blur);
    write_opt_f64(out, "opacity", &t.opacity);
    write_opt_bool(out, "visible", &t.visible);
    write_opt_bool(out, "locked", &t.locked);
    write_opt_dimension(out, "rotate", &t.rotate);
    write_opt_str(out, "style", &t.style);
    write_opt_str(out, "chain", &t.chain);
    if let Some(n) = t.drop_cap_lines {
        let _ = write!(out, " drop-cap-lines={n}");
    }
    if let Some(h) = t.hyphenate {
        let _ = write!(out, " hyphenate=#{h}");
    }
    if let Some(n) = t.widow_orphan {
        let _ = write!(out, " widow-orphan={n}");
    }
    write_opt_str(out, "tab-leader", &t.tab_leader);
    write_opt_str(out, "text-exclusion", &t.text_exclusion);
    write_opt_str(out, "bullet", &t.bullet);
    write_opt_dimension(out, "bullet-gap", &t.bullet_gap);

    // Unknown properties in sorted key order.
    for (key, prop) in &t.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    out.push_str(" {\n");
    for span in &t.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_code(c: &CodeNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("code");

    // Canonical property order: id, name, role, anchor, anchor-zone, x, y, w, h, overflow, language,
    // line-numbers, tab-width, style, fill, font-family, font-size, font-weight,
    // syntax-theme, opacity, visible, locked, rotate, then unknown props.
    out.push_str(" id=\"");
    out.push_str(&c.id);
    out.push('"');
    write_opt_str(out, "name", &c.name);
    write_opt_str(out, "role", &c.role);
    write_opt_str(out, "anchor", &c.anchor);
    write_opt_str(out, "anchor-zone", &c.anchor_zone);
    write_opt_bool(out, "anchor-parent", &c.anchor_parent);
    write_opt_dimension(out, "x", &c.x);
    write_opt_dimension(out, "y", &c.y);
    write_opt_dimension(out, "w", &c.w);
    write_opt_dimension(out, "h", &c.h);
    write_opt_str(out, "overflow", &c.overflow);
    write_opt_str(out, "language", &c.language);
    write_opt_bool(out, "line-numbers", &c.line_numbers);
    if let Some(tw) = c.tab_width {
        let _ = write!(out, " tab-width={tw}");
    }
    write_opt_str(out, "style", &c.style);
    write_opt_property_value(out, "fill", &c.fill);
    write_opt_property_value(out, "font-family", &c.font_family);
    write_opt_property_value(out, "font-size", &c.font_size);
    write_opt_property_value(out, "font-weight", &c.font_weight);
    if let Some(t) = c.syntax_theme {
        let _ = write!(out, " syntax-theme=\"{}\"", t.as_str());
    }
    write_opt_f64(out, "opacity", &c.opacity);
    write_opt_bool(out, "visible", &c.visible);
    write_opt_bool(out, "locked", &c.locked);
    write_opt_dimension(out, "rotate", &c.rotate);

    // Unknown properties in sorted key order.
    for (key, prop) in &c.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    // The verbatim source is emitted as a single escaped `content` child line.
    // It is NEVER re-indented/trimmed: the content is one escaped single-line
    // KDL string (KDL v2 multi-line dedent rules would otherwise mutate it).
    out.push_str(" {\n");
    indent(out, depth + 1);
    out.push_str("content \"");
    out.push_str(&escape_kdl_string(&c.content));
    out.push_str("\"\n");
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_polygon(p: &PolygonNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("polygon");

    // Canonical property order: id, name, role, fill, stroke, stroke-width,
    // stroke-alignment, fill-rule, opacity, visible, locked, rotate, style,
    // then unknown props, then the points block.
    out.push_str(" id=\"");
    out.push_str(&p.id);
    out.push('"');
    write_opt_str(out, "name", &p.name);
    write_opt_str(out, "role", &p.role);
    write_opt_property_value(out, "fill", &p.fill);
    write_opt_property_value(out, "stroke", &p.stroke);
    write_opt_property_value(out, "stroke-width", &p.stroke_width);
    // DEFERRED: stroke-alignment offset (rendered centered in v0)
    write_opt_str(out, "stroke-alignment", &p.stroke_alignment);
    write_opt_str(out, "fill-rule", &p.fill_rule);
    write_opt_f64(out, "opacity", &p.opacity);
    write_opt_bool(out, "visible", &p.visible);
    write_opt_bool(out, "locked", &p.locked);
    write_opt_dimension(out, "rotate", &p.rotate);
    write_opt_str(out, "style", &p.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &p.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    // Points block: always emit braces (container style).
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_polyline(p: &PolylineNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("polyline");

    // Canonical property order: id, name, role, fill, stroke, stroke-width,
    // fill-rule, opacity, visible, locked, rotate, style,
    // then unknown props, then the points block.
    // NOTE: polyline has NO stroke-alignment.
    out.push_str(" id=\"");
    out.push_str(&p.id);
    out.push('"');
    write_opt_str(out, "name", &p.name);
    write_opt_str(out, "role", &p.role);
    write_opt_property_value(out, "fill", &p.fill);
    write_opt_property_value(out, "stroke", &p.stroke);
    write_opt_property_value(out, "stroke-width", &p.stroke_width);
    write_opt_str(out, "fill-rule", &p.fill_rule);
    write_opt_f64(out, "opacity", &p.opacity);
    write_opt_bool(out, "visible", &p.visible);
    write_opt_bool(out, "locked", &p.locked);
    write_opt_dimension(out, "rotate", &p.rotate);
    write_opt_str(out, "style", &p.style);

    // Unknown properties in sorted key order.
    for (key, prop) in &p.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    // Points block.
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}
