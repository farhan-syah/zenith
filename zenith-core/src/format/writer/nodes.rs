//! Node-tree writing: the `document` body, `page`, the per-node writers
//! (rect/ellipse/line/text/code/image/group/frame/polygon/polyline), and the
//! `span` / `point` / `content` leaf emitters.

use std::fmt::Write as _;

use crate::ast::{
    CodeNode, DocumentBody, EllipseNode, FieldNode, Fold, FootnoteNode, FrameNode, GroupNode,
    ImageNode, InstanceNode, LineNode, Node, Override, Page, Point, PolygonNode, PolylineNode,
    RectNode, SafeZone, SafeZoneType, TextNode, TextSpan,
};

use super::{
    fmt_dimension, fmt_unknown_value, indent, write_opt_bool, write_opt_dimension, write_opt_f64,
    write_opt_object_position, write_opt_property_value, write_opt_str,
};

// ---------------------------------------------------------------------------
// Document body
// ---------------------------------------------------------------------------

pub(super) fn write_document_body(body: &DocumentBody, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("document");
    out.push_str(" id=\"");
    out.push_str(&body.id);
    out.push('"');
    write_opt_str(out, "title", &body.title);
    out.push_str(" {\n");

    for page in &body.pages {
        write_page(page, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

fn write_page(page: &Page, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("page");
    // Canonical order: id, name, w, h, background
    out.push_str(" id=\"");
    out.push_str(&page.id);
    out.push('"');
    write_opt_str(out, "name", &page.name);
    out.push_str(" w=");
    out.push_str(&fmt_dimension(&page.width));
    out.push_str(" h=");
    out.push_str(&fmt_dimension(&page.height));
    write_opt_property_value(out, "background", &page.background);
    write_opt_dimension(out, "bleed", &page.bleed);
    write_opt_dimension(out, "baseline-grid", &page.baseline_grid);
    write_opt_dimension(out, "margin-inner", &page.margin_inner);
    write_opt_dimension(out, "margin-outer", &page.margin_outer);
    write_opt_dimension(out, "margin-top", &page.margin_top);
    write_opt_dimension(out, "margin-bottom", &page.margin_bottom);
    write_opt_str(out, "parity", &page.parity);
    write_opt_str(out, "master", &page.master);

    out.push_str(" {\n");
    // Safe-zones and folds are page metadata, emitted before the renderable
    // children.
    for zone in &page.safe_zones {
        write_safe_zone(zone, out, depth + 1);
    }
    for fold in &page.folds {
        write_fold(fold, out, depth + 1);
    }
    write_children_block(&page.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit a single `safe-zone` line:
/// `safe-zone id="..." type="exclusion|required" x=(px)N y=(px)N w=(px)N h=(px)N label="..."`
/// (`label` is omitted when `None`).
fn write_safe_zone(zone: &SafeZone, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("safe-zone");
    out.push_str(" id=\"");
    out.push_str(&zone.id);
    out.push('"');
    out.push_str(" type=\"");
    out.push_str(match zone.zone_type {
        SafeZoneType::Exclusion => "exclusion",
        SafeZoneType::Required => "required",
    });
    out.push('"');
    out.push_str(" x=");
    out.push_str(&fmt_dimension(&zone.x));
    out.push_str(" y=");
    out.push_str(&fmt_dimension(&zone.y));
    out.push_str(" w=");
    out.push_str(&fmt_dimension(&zone.w));
    out.push_str(" h=");
    out.push_str(&fmt_dimension(&zone.h));
    write_opt_str(out, "label", &zone.label);
    out.push('\n');
}

/// Emit a single `fold` line:
/// `fold id="..." orientation="vertical|horizontal" position=(px)N`
/// (`position` is omitted when `None`).
fn write_fold(fold: &Fold, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("fold");
    out.push_str(" id=\"");
    out.push_str(&fold.id);
    out.push('"');
    out.push_str(" orientation=\"");
    out.push_str(&fold.orientation);
    out.push('"');
    write_opt_dimension(out, "position", &fold.position);
    out.push('\n');
}

/// Emit each child node in source order at `depth + 1` indentation.
///
/// Used by `write_page`, `write_group`, and `write_frame` so the child-block
/// logic lives in exactly one place.
///
/// # Known limitation
/// Frames and groups nest recursively via `write_node` → `write_frame` /
/// `write_group` → `write_children_block` with no depth guard.  This is an
/// accepted v0 limitation; stack overflow is only possible with pathologically
/// deep trees.
fn write_children_block(children: &[Node], out: &mut String, depth: usize) {
    for child in children {
        write_node(child, out, depth + 1);
    }
}

/// Emit a component definition's child nodes at `depth + 1` indentation.
///
/// Public to the writer module so the `components` block writer in the module
/// root can reuse the exact same per-node serialization the page/group/frame
/// child blocks use. (`write_children_block` indents relative to a container
/// node's own depth; here `depth` is the `component` node's depth.)
pub(super) fn write_component_children(children: &[Node], out: &mut String, depth: usize) {
    write_children_block(children, out, depth);
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

fn write_node(node: &Node, out: &mut String, depth: usize) {
    match node {
        Node::Rect(r) => write_rect(r, out, depth),
        Node::Ellipse(e) => write_ellipse(e, out, depth),
        Node::Line(l) => write_line(l, out, depth),
        Node::Text(t) => write_text(t, out, depth),
        Node::Code(c) => write_code(c, out, depth),
        Node::Frame(f) => write_frame(f, out, depth),
        Node::Group(g) => write_group(g, out, depth),
        Node::Image(i) => write_image(i, out, depth),
        Node::Polygon(p) => write_polygon(p, out, depth),
        Node::Polyline(p) => write_polyline(p, out, depth),
        Node::Instance(i) => write_instance(i, out, depth),
        Node::Field(f) => write_field(f, out, depth),
        Node::Footnote(f) => write_footnote(f, out, depth),
        Node::Unknown(u) => write_unknown_node(u, out, depth),
    }
}

fn write_field(f: &FieldNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("field");

    // Canonical property order: id, name, role, type, recto, verso, target,
    // x, y, w, h, fill, font-family, font-size, opacity, visible, locked, style,
    // then unknown props (sorted). A field is a leaf — no child block.
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_instance(i: &InstanceNode, out: &mut String, depth: usize) {
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
        out.push_str(&fmt_unknown_value(&prop.value));
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

fn write_rect(r: &RectNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("rect");

    // Canonical property order: id, name, role, x, y, w, h, radius, fill,
    // stroke, stroke-width, stroke-alignment, opacity, visible, locked, rotate, style
    out.push_str(" id=\"");
    out.push_str(&r.id);
    out.push('"');
    write_opt_str(out, "name", &r.name);
    write_opt_str(out, "role", &r.role);
    write_opt_dimension(out, "x", &r.x);
    write_opt_dimension(out, "y", &r.y);
    write_opt_dimension(out, "w", &r.w);
    write_opt_dimension(out, "h", &r.h);
    write_opt_property_value(out, "radius", &r.radius);
    write_opt_property_value(out, "fill", &r.fill);
    write_opt_property_value(out, "stroke", &r.stroke);
    write_opt_property_value(out, "stroke-width", &r.stroke_width);
    write_opt_str(out, "stroke-alignment", &r.stroke_alignment);
    write_opt_property_value(out, "stroke-dash", &r.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &r.stroke_gap);
    write_opt_str(out, "stroke-linecap", &r.stroke_linecap);
    write_opt_property_value(out, "shadow", &r.shadow);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_image(i: &ImageNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("image");

    // Canonical property order: id, name, role, asset, x, y, w, h,
    // src-x, src-y, src-w, src-h, fit, clip, clip-radius,
    // object-position-x, object-position-y, shadow, opacity, visible, locked,
    // rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&i.id);
    out.push('"');
    write_opt_str(out, "name", &i.name);
    write_opt_str(out, "role", &i.role);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_ellipse(e: &EllipseNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("ellipse");

    // Canonical property order: id, name, role, x, y, w, h, fill, stroke,
    // stroke-width, opacity, visible, locked, rotate, style.
    // NOTE: stroke-alignment is not supported for ellipse in v0 (centered only).
    out.push_str(" id=\"");
    out.push_str(&e.id);
    out.push('"');
    write_opt_str(out, "name", &e.name);
    write_opt_str(out, "role", &e.role);
    write_opt_dimension(out, "x", &e.x);
    write_opt_dimension(out, "y", &e.y);
    write_opt_dimension(out, "w", &e.w);
    write_opt_dimension(out, "h", &e.h);
    write_opt_property_value(out, "fill", &e.fill);
    write_opt_property_value(out, "stroke", &e.stroke);
    write_opt_property_value(out, "stroke-width", &e.stroke_width);
    write_opt_property_value(out, "stroke-dash", &e.stroke_dash);
    write_opt_property_value(out, "stroke-gap", &e.stroke_gap);
    write_opt_str(out, "stroke-linecap", &e.stroke_linecap);
    write_opt_property_value(out, "shadow", &e.shadow);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_line(l: &LineNode, out: &mut String, depth: usize) {
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push('\n');
}

fn write_frame(f: &FrameNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("frame");

    // Canonical property order: id, name, role, x, y, w, h, layout, columns,
    // rows, opacity, visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&f.id);
    out.push('"');
    write_opt_str(out, "name", &f.name);
    write_opt_str(out, "role", &f.role);
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
    write_opt_str(out, "style", &f.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &f.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    write_children_block(&f.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_group(g: &GroupNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("group");

    // Canonical property order: id, name, role, x, y, w, h, opacity,
    // visible, locked, rotate, style, then unknown props (sorted).
    out.push_str(" id=\"");
    out.push_str(&g.id);
    out.push('"');
    write_opt_str(out, "name", &g.name);
    write_opt_str(out, "role", &g.role);
    write_opt_dimension(out, "x", &g.x);
    write_opt_dimension(out, "y", &g.y);
    write_opt_dimension(out, "w", &g.w);
    write_opt_dimension(out, "h", &g.h);
    write_opt_f64(out, "opacity", &g.opacity);
    write_opt_bool(out, "visible", &g.visible);
    write_opt_bool(out, "locked", &g.locked);
    write_opt_dimension(out, "rotate", &g.rotate);
    write_opt_str(out, "style", &g.style);

    // Unknown properties in sorted key order (BTreeMap iteration is sorted).
    for (key, prop) in &g.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    write_children_block(&g.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_text(t: &TextNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("text");

    // Canonical property order: id, name, role, x, y, w, h, align, direction,
    // overflow, fill, font-family, font-size, font-weight, opacity, visible,
    // locked, rotate, style, chain
    out.push_str(" id=\"");
    out.push_str(&t.id);
    out.push('"');
    write_opt_str(out, "name", &t.name);
    write_opt_str(out, "role", &t.role);
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
    write_opt_property_value(out, "contrast-bg", &t.contrast_bg);
    write_opt_property_value(out, "font-family", &t.font_family);
    write_opt_property_value(out, "font-size", &t.font_size);
    write_opt_property_value(out, "font-size-min", &t.font_size_min);
    write_opt_property_value(out, "font-weight", &t.font_weight);
    write_opt_property_value(out, "shadow", &t.shadow);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    for span in &t.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_span(span: &TextSpan, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("span \"");
    out.push_str(&super::escape_kdl_string(&span.text));
    out.push('"');

    // Inline props: fill, font-weight, italic, underline, strikethrough,
    // vertical-align.
    write_opt_property_value(out, "fill", &span.fill);
    write_opt_property_value(out, "font-weight", &span.font_weight);
    write_opt_bool(out, "italic", &span.italic);
    write_opt_bool(out, "underline", &span.underline);
    write_opt_bool(out, "strikethrough", &span.strikethrough);
    write_opt_str(out, "vertical-align", &span.vertical_align);
    write_opt_str(out, "footnote-ref", &span.footnote_ref);

    out.push('\n');
}

fn write_footnote(f: &FootnoteNode, out: &mut String, depth: usize) {
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    out.push_str(" {\n");
    for span in &f.spans {
        write_span(span, out, depth + 1);
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_code(c: &CodeNode, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("code");

    // Canonical property order: id, name, role, x, y, w, h, overflow, language,
    // line-numbers, tab-width, style, fill, font-family, font-size, font-weight,
    // syntax-theme, opacity, visible, locked, rotate, then unknown props.
    out.push_str(" id=\"");
    out.push_str(&c.id);
    out.push('"');
    write_opt_str(out, "name", &c.name);
    write_opt_str(out, "role", &c.role);
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // The verbatim source is emitted as a single escaped `content` child line.
    // It is NEVER re-indented/trimmed: the content is one escaped single-line
    // KDL string (KDL v2 multi-line dedent rules would otherwise mutate it).
    out.push_str(" {\n");
    indent(out, depth + 1);
    out.push_str("content \"");
    out.push_str(&super::escape_kdl_string(&c.content));
    out.push_str("\"\n");
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit a `point x=(unit)N y=(unit)N` line for each vertex in the list.
///
/// The block is always emitted (even for zero points) to maintain a consistent
/// brace-block style, mirroring how `write_text` always emits its `{ … }`.
fn write_points(points: &[Point], out: &mut String, depth: usize) {
    for pt in points {
        indent(out, depth);
        out.push_str("point");
        write_opt_dimension(out, "x", &pt.x);
        write_opt_dimension(out, "y", &pt.y);
        out.push('\n');
    }
}

fn write_polygon(p: &PolygonNode, out: &mut String, depth: usize) {
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // Points block: always emit braces (container style).
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_polyline(p: &PolylineNode, out: &mut String, depth: usize) {
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
        out.push_str(&fmt_unknown_value(&prop.value));
    }

    // Points block.
    out.push_str(" {\n");
    write_points(&p.points, out, depth + 1);
    indent(out, depth);
    out.push_str("}\n");
}

fn write_unknown_node(u: &crate::ast::UnknownNode, out: &mut String, depth: usize) {
    // Emit `<kind>` as a leaf (UnknownNode has no property map in current AST).
    indent(out, depth);
    out.push_str(&u.kind);
    out.push('\n');
}
