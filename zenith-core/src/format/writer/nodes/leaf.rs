//! Leaf and vector node writers: rect, ellipse, line, text, code, image,
//! polygon, and polyline.

use std::fmt::Write as _;

use crate::ast::{
    ChartNode, CodeNode, EllipseNode, ImageNode, LineNode, PatternNode, PolygonNode, PolylineNode,
    RectNode, TextNode,
};

use crate::format::writer::{
    escape_kdl_string, fmt_property_value, fmt_unknown_property, indent, write_opt_bool,
    write_opt_dimension, write_opt_f64, write_opt_object_position, write_opt_property_value,
    write_opt_str,
};

use super::helpers::{write_points, write_span};
use super::write_node;

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
    write_opt_str(out, "anchor-sibling", &r.anchor_sibling);
    write_opt_str(out, "anchor-edge", &r.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &r.anchor_gap);
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
    write_opt_str(out, "anchor-sibling", &i.anchor_sibling);
    write_opt_str(out, "anchor-edge", &i.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &i.anchor_gap);
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
    write_opt_str(out, "anchor-sibling", &e.anchor_sibling);
    write_opt_str(out, "anchor-edge", &e.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &e.anchor_gap);
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
    write_opt_str(out, "anchor-sibling", &t.anchor_sibling);
    write_opt_str(out, "anchor-edge", &t.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &t.anchor_gap);
    write_opt_bool(out, "anchor-parent", &t.anchor_parent);
    write_opt_dimension(out, "x", &t.x);
    write_opt_dimension(out, "y", &t.y);
    write_opt_dimension(out, "w", &t.w);
    write_opt_dimension(out, "h", &t.h);
    write_opt_str(out, "align", &t.align);
    write_opt_str(out, "v-align", &t.v_align);
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
    write_opt_str(out, "anchor-sibling", &c.anchor_sibling);
    write_opt_str(out, "anchor-edge", &c.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &c.anchor_gap);
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

pub(super) fn write_pattern(p: &PatternNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("pattern");

    // Canonical property order mirrors `rect`, with the pattern-specific props
    // (kind, seed, count, spacing, jitter) emitted right after the common
    // geometry/visual spread, then unknown props, then the single motif block.
    out.push_str(" id=\"");
    out.push_str(&p.id);
    out.push('"');
    out.push_str(" kind=\"");
    out.push_str(&escape_kdl_string(&p.kind));
    out.push('"');
    write_opt_str(out, "name", &p.name);
    write_opt_str(out, "role", &p.role);
    write_opt_str(out, "anchor", &p.anchor);
    write_opt_str(out, "anchor-zone", &p.anchor_zone);
    write_opt_str(out, "anchor-sibling", &p.anchor_sibling);
    write_opt_str(out, "anchor-edge", &p.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &p.anchor_gap);
    write_opt_bool(out, "anchor-parent", &p.anchor_parent);
    write_opt_dimension(out, "x", &p.x);
    write_opt_dimension(out, "y", &p.y);
    write_opt_dimension(out, "w", &p.w);
    write_opt_dimension(out, "h", &p.h);
    if let Some(n) = p.seed {
        let _ = write!(out, " seed={n}");
    }
    if let Some(n) = p.count {
        let _ = write!(out, " count={n}");
    }
    write_opt_dimension(out, "spacing", &p.spacing);
    write_opt_f64(out, "jitter", &p.jitter);
    write_opt_property_value(out, "radius", &p.radius);
    write_opt_property_value(out, "radius-tl", &p.radius_tl);
    write_opt_property_value(out, "radius-tr", &p.radius_tr);
    write_opt_property_value(out, "radius-br", &p.radius_br);
    write_opt_property_value(out, "radius-bl", &p.radius_bl);
    write_opt_property_value(out, "fill", &p.fill);
    write_opt_property_value(out, "stroke", &p.stroke);
    write_opt_property_value(out, "stroke-width", &p.stroke_width);
    write_opt_str(out, "stroke-alignment", &p.stroke_alignment);
    write_opt_property_value(out, "stroke-dash", &p.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &p.stroke_gap);
    write_opt_str(out, "stroke-linecap", &p.stroke_linecap);
    write_opt_property_value(out, "border-top", &p.border_top);
    write_opt_property_value(out, "border-bottom", &p.border_bottom);
    write_opt_property_value(out, "border-left", &p.border_left);
    write_opt_property_value(out, "border-right", &p.border_right);
    write_opt_property_value(out, "border-width", &p.border_width);
    write_opt_property_value(out, "stroke-outer", &p.stroke_outer);
    write_opt_property_value(out, "stroke-outer-width", &p.stroke_outer_width);
    write_opt_property_value(out, "shadow", &p.shadow);
    write_opt_property_value(out, "filter", &p.filter);
    write_opt_property_value(out, "mask", &p.mask);
    write_opt_str(out, "blend-mode", &p.blend_mode);
    write_opt_dimension(out, "blur", &p.blur);
    write_opt_f64(out, "opacity", &p.opacity);
    write_opt_bool(out, "visible", &p.visible);
    write_opt_bool(out, "locked", &p.locked);
    write_opt_dimension(out, "rotate", &p.rotate);
    write_opt_str(out, "style", &p.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &p.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }

    // The single motif template, emitted as the sole child of the block.
    out.push_str(" {\n");
    write_node(&p.motif, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}

pub(super) fn write_chart(c: &ChartNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("chart");

    // Canonical property order mirrors `pattern`, with the chart-specific props
    // (kind, title, caption, legend, axis-min, axis-max, axis-style) emitted right
    // after the common geometry/visual spread, then unknown props, then the series
    // block.
    out.push_str(" id=\"");
    out.push_str(&c.id);
    out.push('"');
    out.push_str(" kind=\"");
    out.push_str(&escape_kdl_string(&c.kind));
    out.push('"');
    write_opt_str(out, "name", &c.name);
    write_opt_str(out, "role", &c.role);
    write_opt_str(out, "anchor", &c.anchor);
    write_opt_str(out, "anchor-zone", &c.anchor_zone);
    write_opt_str(out, "anchor-sibling", &c.anchor_sibling);
    write_opt_str(out, "anchor-edge", &c.anchor_edge);
    write_opt_dimension(out, "anchor-gap", &c.anchor_gap);
    write_opt_bool(out, "anchor-parent", &c.anchor_parent);
    write_opt_dimension(out, "x", &c.x);
    write_opt_dimension(out, "y", &c.y);
    write_opt_dimension(out, "w", &c.w);
    write_opt_dimension(out, "h", &c.h);
    write_opt_str(out, "title", &c.title);
    write_opt_str(out, "caption", &c.caption);
    write_opt_bool(out, "legend", &c.legend);
    if let Some(v) = c.axis_min {
        let _ = write!(out, " axis-min={v}");
    }
    if let Some(v) = c.axis_max {
        let _ = write!(out, " axis-max={v}");
    }
    write_opt_str(out, "axis-style", &c.axis_style);
    write_opt_str(out, "bar-mode", &c.bar_mode);
    write_opt_str(out, "point-placement", &c.point_placement);
    write_opt_str(out, "value-labels", &c.value_labels);
    write_opt_property_value(out, "value-color", &c.value_color);
    write_opt_property_value(out, "radius", &c.radius);
    write_opt_property_value(out, "radius-tl", &c.radius_tl);
    write_opt_property_value(out, "radius-tr", &c.radius_tr);
    write_opt_property_value(out, "radius-br", &c.radius_br);
    write_opt_property_value(out, "radius-bl", &c.radius_bl);
    write_opt_property_value(out, "fill", &c.fill);
    write_opt_property_value(out, "stroke", &c.stroke);
    write_opt_property_value(out, "stroke-width", &c.stroke_width);
    write_opt_str(out, "stroke-alignment", &c.stroke_alignment);
    write_opt_property_value(out, "stroke-dash", &c.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &c.stroke_gap);
    write_opt_str(out, "stroke-linecap", &c.stroke_linecap);
    write_opt_property_value(out, "border-top", &c.border_top);
    write_opt_property_value(out, "border-bottom", &c.border_bottom);
    write_opt_property_value(out, "border-left", &c.border_left);
    write_opt_property_value(out, "border-right", &c.border_right);
    write_opt_property_value(out, "border-width", &c.border_width);
    write_opt_property_value(out, "stroke-outer", &c.stroke_outer);
    write_opt_property_value(out, "stroke-outer-width", &c.stroke_outer_width);
    write_opt_property_value(out, "shadow", &c.shadow);
    write_opt_property_value(out, "filter", &c.filter);
    write_opt_property_value(out, "mask", &c.mask);
    write_opt_str(out, "blend-mode", &c.blend_mode);
    write_opt_dimension(out, "blur", &c.blur);
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

    // Child block: always emitted (even when empty) so parse → format → parse is byte-stable.
    // Order: categories line (when non-empty), then label-colors line (when non-empty), then series lines.
    out.push_str(" {\n");

    // Categories child: emitted only when non-empty (empty = absent, byte-identical).
    if !c.categories.is_empty() {
        indent(out, depth + 1);
        out.push_str("categories");
        for label in &c.categories {
            out.push_str(" \"");
            out.push_str(&escape_kdl_string(label));
            out.push('"');
        }
        out.push('\n');
    }

    // Label-colors child: emitted only when non-empty (empty = absent, byte-identical).
    // Each entry is a positional PropertyValue (e.g. `(token)"color.x"`).
    if !c.label_colors.is_empty() {
        indent(out, depth + 1);
        out.push_str("label-colors");
        for pv in &c.label_colors {
            out.push(' ');
            out.push_str(&fmt_property_value(pv));
        }
        out.push('\n');
    }

    // Series children: one `series` line per entry with data values as positional
    // args and label/color/label-color/data-ref as named props.
    for s in &c.series {
        indent(out, depth + 1);
        out.push_str("series");
        // Named props first (canonical order: label, color, label-color, data-ref).
        if let Some(label) = &s.label {
            out.push_str(" label=\"");
            out.push_str(&escape_kdl_string(label));
            out.push('"');
        }
        write_opt_property_value(out, "color", &s.color);
        write_opt_property_value(out, "label-color", &s.label_color);
        if let Some(dr) = &s.data_ref {
            out.push_str(" data-ref=\"");
            out.push_str(&escape_kdl_string(dr));
            out.push('"');
        }
        // Positional f64 values.
        for v in &s.values {
            let _ = write!(out, " {v}");
        }
        out.push('\n');
    }
    indent(out, depth);
    out.push_str("}\n");
}
