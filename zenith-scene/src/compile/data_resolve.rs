//! Data-binding pre-pass: substitute every `PropertyValue::DataRef` and every
//! `TextSpan.data_ref` in a document with the concrete value from a
//! [`DataContext`], BEFORE node compilation.
//!
//! Running this once over the whole document means every downstream resolver
//! (`resolve_property_color`, `resolve_property_dimension_px`, the text-span
//! loop) only ever sees `Literal` / `TokenRef` / `Dimension` values — none of
//! them need to know about data binding.
//!
//! ## Two modes
//!
//! - `data = Some(ctx)`: WALK every node in every page (and the page background),
//!   exhaustively over the [`Node`] enum, and REWRITE each data ref in place:
//!   color-typed props become a `Literal`, dimension-typed props become a
//!   `Dimension` (parsed from the field's number), and span text is replaced with
//!   the resolved (optionally formatted) value. A missing field emits
//!   `data.missing_field` and leaves the ref/text untouched (the authored
//!   fallback).
//! - `data = None`: do NOT mutate. [`scan_for_data_refs`] reports whether any ref
//!   exists; [`compile_page`] uses it to emit a single `data.no_context` advisory
//!   and compiles the original document by reference (byte-identical to before).
//!
//! Determinism: the walk is document-order; lookups go through the
//! [`DataContext`]'s `BTreeMap`. No `HashMap`, no time, no randomness.
//!
//! [`compile_page`]: super::compile_page

use zenith_core::{
    ChartNode, CodeNode, ConnectorNode, DataContext, Diagnostic, Document, EllipseNode, FieldNode,
    FootnoteNode, ImageNode, InstanceNode, LineNode, Node, PatternNode, PropertyValue, RectNode,
    ShapeNode, TableCell, TableNode, TextNode, TextSpan, TocNode, UnknownNode, format_data_value,
};

use super::util::px;

/// Whether the document contains ANY `(data)` property reference or any span
/// `data-ref` — anywhere in the page tree (children + page backgrounds). Used by
/// the `data = None` compile path to decide whether to emit a single
/// `data.no_context` advisory WITHOUT cloning or mutating the document.
pub(super) fn scan_for_data_refs(doc: &Document) -> bool {
    for page in &doc.body.pages {
        if let Some(bg) = &page.background
            && matches!(bg, PropertyValue::DataRef(_))
        {
            return true;
        }
        for node in &page.children {
            if node_has_data_ref(node) {
                return true;
            }
        }
    }
    false
}

/// Substitute every data reference in `doc` against `ctx` (the `data = Some`
/// path). Walks every page background + node tree in document order, rewriting
/// each ref to a resolved value and emitting `data.missing_field` for any
/// unresolved field. The `data = None` path is handled separately by
/// [`scan_for_data_refs`] in [`compile_page`] (advisory-only, no mutation).
///
/// [`compile_page`]: super::compile_page
pub(super) fn substitute_data_refs(
    doc: &mut Document,
    ctx: &DataContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for page in &mut doc.body.pages {
        if let Some(bg) = &mut page.background {
            substitute_color_prop(bg, ctx, &page.id, "background", diagnostics);
        }
        for node in &mut page.children {
            substitute_node(node, ctx, diagnostics);
        }
    }
}

// ── Scan helpers (read-only, `data = None` path) ────────────────────────────────

/// Test a slice of optional property fields for any `DataRef`.
fn any_prop(props: &[&Option<PropertyValue>]) -> bool {
    props
        .iter()
        .any(|p| matches!(p, Some(PropertyValue::DataRef(_))))
}

/// Test a span list for any span carrying a `data_ref`.
fn any_span(spans: &[TextSpan]) -> bool {
    spans.iter().any(|s| s.data_ref.is_some())
}

/// Recursively test whether a node (or any descendant) carries a data ref in any
/// of its property fields or span text. EXHAUSTIVE over the [`Node`] enum so a
/// new node kind forces this scan to be revisited alongside the substitutor.
fn node_has_data_ref(node: &Node) -> bool {
    match node {
        Node::Rect(n) => any_prop(&rect_color_props(n)) || any_prop(&rect_dim_props(n)),
        Node::Ellipse(n) => any_prop(&ellipse_color_props(n)) || any_prop(&ellipse_dim_props(n)),
        Node::Line(n) => any_prop(&[&n.stroke, &n.stroke_width]),
        Node::Text(n) => {
            any_prop(&text_color_props(n)) || any_prop(&text_dim_props(n)) || any_span(&n.spans)
        }
        Node::Code(n) => any_prop(&[&n.fill, &n.font_family, &n.font_size]),
        Node::Polygon(n) => any_prop(&[&n.fill, &n.stroke, &n.stroke_width]),
        Node::Polyline(n) => any_prop(&[&n.fill, &n.stroke, &n.stroke_width]),
        Node::Image(n) => any_prop(&[&n.shadow, &n.filter, &n.mask, &n.clip_radius]),
        Node::Field(n) => any_prop(&[&n.fill, &n.font_family, &n.font_size]),
        Node::Footnote(n) => {
            any_prop(&[&n.fill, &n.font_family, &n.font_size]) || any_span(&n.spans)
        }
        Node::Toc(n) => any_prop(&[&n.fill, &n.font_family, &n.font_size]),
        Node::Shape(n) => {
            any_prop(&shape_color_props(n)) || any_prop(&shape_dim_props(n)) || any_span(&n.spans)
        }
        Node::Connector(n) => any_prop(&[&n.stroke, &n.stroke_width]) || any_span(&n.spans),
        Node::Pattern(n) => {
            any_prop(&pattern_color_props(n))
                || any_prop(&pattern_dim_props(n))
                || node_has_data_ref(&n.motif)
        }
        Node::Chart(n) => {
            any_prop(&chart_color_props(n))
                || any_prop(&chart_dim_props(n))
                || n.label_colors
                    .iter()
                    .any(|p| matches!(p, PropertyValue::DataRef(_)))
                || n.series.iter().any(|s| {
                    matches!(s.color, Some(PropertyValue::DataRef(_)))
                        || matches!(&s.label_color, Some(PropertyValue::DataRef(_)))
                        || s.data_ref.is_some()
                })
        }
        Node::Frame(n) => n.children.iter().any(node_has_data_ref),
        Node::Group(n) => n.children.iter().any(node_has_data_ref),
        Node::Instance(n) => instance_has_data_ref(n),
        Node::Table(n) => table_has_data_ref(n),
        Node::Unknown(n) => unknown_has_data_ref(n),
    }
}

fn instance_has_data_ref(n: &InstanceNode) -> bool {
    n.overrides.iter().any(|o| {
        matches!(o.fill, Some(PropertyValue::DataRef(_)))
            || o.spans.as_ref().is_some_and(|s| any_span(s))
    })
}

fn table_has_data_ref(n: &TableNode) -> bool {
    if any_prop(&[
        &n.gap,
        &n.cell_padding,
        &n.fill,
        &n.border,
        &n.border_width,
        &n.header_fill,
    ]) {
        return true;
    }
    n.rows.iter().any(|row| {
        row.cells.iter().any(|cell| {
            any_prop(&[&cell.fill, &cell.border, &cell.border_width])
                || cell.children.iter().any(node_has_data_ref)
        })
    })
}

fn unknown_has_data_ref(n: &UnknownNode) -> bool {
    n.children.iter().any(node_has_data_ref)
}

// ── Per-node substitution (mutating, `data = Some` path) ─────────────────────────

/// Rewrite every data ref on a single node (and recurse into children), against
/// the live `DataContext`. EXHAUSTIVE over the [`Node`] enum so a new node kind
/// forces a compile error here (the coverage guarantee).
fn substitute_node(node: &mut Node, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    match node {
        Node::Rect(n) => substitute_rect(n, ctx, diagnostics),
        Node::Ellipse(n) => substitute_ellipse(n, ctx, diagnostics),
        Node::Line(n) => substitute_line(n, ctx, diagnostics),
        Node::Text(n) => substitute_text(n, ctx, diagnostics),
        Node::Code(n) => substitute_code(n, ctx, diagnostics),
        Node::Polygon(n) => {
            let id = n.id.clone();
            substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
            substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
            substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
        }
        Node::Polyline(n) => {
            let id = n.id.clone();
            substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
            substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
            substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
        }
        Node::Image(n) => substitute_image(n, ctx, diagnostics),
        Node::Field(n) => substitute_field(n, ctx, diagnostics),
        Node::Footnote(n) => substitute_footnote(n, ctx, diagnostics),
        Node::Toc(n) => substitute_toc(n, ctx, diagnostics),
        Node::Shape(n) => substitute_shape(n, ctx, diagnostics),
        Node::Connector(n) => substitute_connector(n, ctx, diagnostics),
        Node::Pattern(n) => substitute_pattern(n, ctx, diagnostics),
        Node::Chart(n) => substitute_chart(n, ctx, diagnostics),
        Node::Frame(n) => substitute_children(&mut n.children, ctx, diagnostics),
        Node::Group(n) => substitute_children(&mut n.children, ctx, diagnostics),
        Node::Instance(n) => substitute_instance(n, ctx, diagnostics),
        Node::Table(n) => substitute_table(n, ctx, diagnostics),
        Node::Unknown(n) => substitute_children(&mut n.children, ctx, diagnostics),
    }
}

fn substitute_children(
    children: &mut [Node],
    ctx: &DataContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for child in children.iter_mut() {
        substitute_node(child, ctx, diagnostics);
    }
}

fn substitute_rect(n: &mut RectNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    // Color-typed properties.
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_color_prop_opt(&mut n.border_top, ctx, &id, "border-top", diagnostics);
    substitute_color_prop_opt(&mut n.border_bottom, ctx, &id, "border-bottom", diagnostics);
    substitute_color_prop_opt(&mut n.border_left, ctx, &id, "border-left", diagnostics);
    substitute_color_prop_opt(&mut n.border_right, ctx, &id, "border-right", diagnostics);
    substitute_color_prop_opt(&mut n.stroke_outer, ctx, &id, "stroke-outer", diagnostics);
    // Dimension-typed properties.
    substitute_dim_prop_opt(&mut n.radius, ctx, &id, "radius", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tl, ctx, &id, "radius-tl", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tr, ctx, &id, "radius-tr", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_br, ctx, &id, "radius-br", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_bl, ctx, &id, "radius-bl", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_dim_prop_opt(&mut n.border_width, ctx, &id, "border-width", diagnostics);
    substitute_dim_prop_opt(
        &mut n.stroke_outer_width,
        ctx,
        &id,
        "stroke-outer-width",
        diagnostics,
    );
}

fn substitute_ellipse(n: &mut EllipseNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_dim_prop_opt(&mut n.rx, ctx, &id, "rx", diagnostics);
    substitute_dim_prop_opt(&mut n.ry, ctx, &id, "ry", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
}

fn substitute_line(n: &mut LineNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
}

fn substitute_text(n: &mut TextNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_color_prop_opt(&mut n.contrast_bg, ctx, &id, "contrast-bg", diagnostics);
    // font-family is a NAME string; resolve like a literal (not a dimension).
    substitute_color_prop_opt(&mut n.font_family, ctx, &id, "font-family", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size, ctx, &id, "font-size", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size_min, ctx, &id, "font-size-min", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_spans(&mut n.spans, ctx, &id, diagnostics);
}

fn substitute_code(n: &mut CodeNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.font_family, ctx, &id, "font-family", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size, ctx, &id, "font-size", diagnostics);
}

fn substitute_image(n: &mut ImageNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    // image carries only token-typed effect props (shadow/filter/mask) and a
    // dimension clip-radius. The effect refs resolve like color props (Literal
    // substitution); clip-radius resolves as a dimension.
    substitute_color_prop_opt(&mut n.shadow, ctx, &id, "shadow", diagnostics);
    substitute_color_prop_opt(&mut n.filter, ctx, &id, "filter", diagnostics);
    substitute_color_prop_opt(&mut n.mask, ctx, &id, "mask", diagnostics);
    substitute_dim_prop_opt(&mut n.clip_radius, ctx, &id, "clip-radius", diagnostics);
}

fn substitute_field(n: &mut FieldNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.font_family, ctx, &id, "font-family", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size, ctx, &id, "font-size", diagnostics);
}

fn substitute_footnote(n: &mut FootnoteNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.font_family, ctx, &id, "font-family", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size, ctx, &id, "font-size", diagnostics);
    substitute_spans(&mut n.spans, ctx, &id, diagnostics);
}

fn substitute_toc(n: &mut TocNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.font_family, ctx, &id, "font-family", diagnostics);
    substitute_dim_prop_opt(&mut n.font_size, ctx, &id, "font-size", diagnostics);
}

fn substitute_shape(n: &mut ShapeNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_dim_prop_opt(&mut n.radius, ctx, &id, "radius", diagnostics);
    substitute_dim_prop_opt(&mut n.padding, ctx, &id, "padding", diagnostics);
    substitute_spans(&mut n.spans, ctx, &id, diagnostics);
}

fn substitute_connector(
    n: &mut ConnectorNode,
    ctx: &DataContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_spans(&mut n.spans, ctx, &id, diagnostics);
}

fn substitute_pattern(n: &mut PatternNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_color_prop_opt(&mut n.border_top, ctx, &id, "border-top", diagnostics);
    substitute_color_prop_opt(&mut n.border_bottom, ctx, &id, "border-bottom", diagnostics);
    substitute_color_prop_opt(&mut n.border_left, ctx, &id, "border-left", diagnostics);
    substitute_color_prop_opt(&mut n.border_right, ctx, &id, "border-right", diagnostics);
    substitute_color_prop_opt(&mut n.stroke_outer, ctx, &id, "stroke-outer", diagnostics);
    substitute_dim_prop_opt(&mut n.radius, ctx, &id, "radius", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tl, ctx, &id, "radius-tl", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tr, ctx, &id, "radius-tr", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_br, ctx, &id, "radius-br", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_bl, ctx, &id, "radius-bl", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_dim_prop_opt(&mut n.border_width, ctx, &id, "border-width", diagnostics);
    substitute_dim_prop_opt(
        &mut n.stroke_outer_width,
        ctx,
        &id,
        "stroke-outer-width",
        diagnostics,
    );
    // The motif is a template that expands into native shapes; resolve its refs
    // so the expanded copies paint resolved values.
    substitute_node(&mut n.motif, ctx, diagnostics);
}

fn substitute_chart(n: &mut ChartNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.stroke, ctx, &id, "stroke", diagnostics);
    substitute_color_prop_opt(&mut n.border_top, ctx, &id, "border-top", diagnostics);
    substitute_color_prop_opt(&mut n.border_bottom, ctx, &id, "border-bottom", diagnostics);
    substitute_color_prop_opt(&mut n.border_left, ctx, &id, "border-left", diagnostics);
    substitute_color_prop_opt(&mut n.border_right, ctx, &id, "border-right", diagnostics);
    substitute_color_prop_opt(&mut n.stroke_outer, ctx, &id, "stroke-outer", diagnostics);
    substitute_color_prop_opt(&mut n.value_color, ctx, &id, "value-color", diagnostics);
    substitute_dim_prop_opt(&mut n.radius, ctx, &id, "radius", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tl, ctx, &id, "radius-tl", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_tr, ctx, &id, "radius-tr", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_br, ctx, &id, "radius-br", diagnostics);
    substitute_dim_prop_opt(&mut n.radius_bl, ctx, &id, "radius-bl", diagnostics);
    substitute_dim_prop_opt(&mut n.stroke_width, ctx, &id, "stroke-width", diagnostics);
    substitute_dim_prop_opt(&mut n.border_width, ctx, &id, "border-width", diagnostics);
    substitute_dim_prop_opt(
        &mut n.stroke_outer_width,
        ctx,
        &id,
        "stroke-outer-width",
        diagnostics,
    );
    // label-colors: per-slice label color refs; substitute each entry in place.
    for pv in &mut n.label_colors {
        substitute_color_prop(pv, ctx, &id, "label-colors", diagnostics);
    }
    // Series color props and per-series label-color are token refs; substitute in place.
    for s in &mut n.series {
        substitute_color_prop_opt(&mut s.color, ctx, &id, "series.color", diagnostics);
        substitute_color_prop_opt(
            &mut s.label_color,
            ctx,
            &id,
            "series.label-color",
            diagnostics,
        );
    }
}

fn substitute_instance(n: &mut InstanceNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    // Only what is authored ON the instance (its overrides) is in scope here; the
    // component definition is resolved when its subtree is expanded from the
    // (separately-walked) component declarations. Resolve overrides in doc order.
    let id = n.id.clone();
    for ov in &mut n.overrides {
        substitute_color_prop_opt(&mut ov.fill, ctx, &id, "override.fill", diagnostics);
        if let Some(spans) = ov.spans.as_mut() {
            substitute_spans(spans, ctx, &id, diagnostics);
        }
    }
}

fn substitute_table(n: &mut TableNode, ctx: &DataContext, diagnostics: &mut Vec<Diagnostic>) {
    let id = n.id.clone();
    substitute_color_prop_opt(&mut n.fill, ctx, &id, "fill", diagnostics);
    substitute_color_prop_opt(&mut n.border, ctx, &id, "border", diagnostics);
    substitute_color_prop_opt(&mut n.header_fill, ctx, &id, "header-fill", diagnostics);
    substitute_dim_prop_opt(&mut n.gap, ctx, &id, "gap", diagnostics);
    substitute_dim_prop_opt(&mut n.cell_padding, ctx, &id, "cell-padding", diagnostics);
    substitute_dim_prop_opt(&mut n.border_width, ctx, &id, "border-width", diagnostics);
    for row in &mut n.rows {
        for cell in &mut row.cells {
            substitute_cell(cell, ctx, &id, diagnostics);
        }
    }
}

fn substitute_cell(
    cell: &mut TableCell,
    ctx: &DataContext,
    table_id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    substitute_color_prop_opt(&mut cell.fill, ctx, table_id, "cell.fill", diagnostics);
    substitute_color_prop_opt(&mut cell.border, ctx, table_id, "cell.border", diagnostics);
    substitute_dim_prop_opt(
        &mut cell.border_width,
        ctx,
        table_id,
        "cell.border-width",
        diagnostics,
    );
    substitute_children(&mut cell.children, ctx, diagnostics);
}

// ── Span substitution ───────────────────────────────────────────────────────────

/// Resolve every `data-ref` span's text content against `ctx`, formatting via the
/// span's `data_format` when set. A missing field emits `data.missing_field` and
/// leaves the authored fallback text unchanged.
fn substitute_spans(
    spans: &mut [TextSpan],
    ctx: &DataContext,
    subject_id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for span in spans.iter_mut() {
        let Some(path) = span.data_ref.as_deref() else {
            continue;
        };
        match ctx.get(path) {
            Some(value) => {
                span.text = match &span.data_format {
                    Some(fmt) => format_data_value(value, fmt),
                    None => value.to_owned(),
                };
            }
            None => diagnostics.push(missing_field_diag(subject_id, "span data-ref", path)),
        }
    }
}

// ── Single-property substitution primitives ─────────────────────────────────────

/// Resolve a COLOR-typed property: a resolved field becomes a raw `Literal`
/// (whatever the field holds — typically a hex color or a token id). A missing
/// field emits `data.missing_field` and leaves the ref in place (unresolved).
fn substitute_color_prop_opt(
    prop: &mut Option<PropertyValue>,
    ctx: &DataContext,
    subject_id: &str,
    prop_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(pv) = prop {
        substitute_color_prop(pv, ctx, subject_id, prop_name, diagnostics);
    }
}

fn substitute_color_prop(
    pv: &mut PropertyValue,
    ctx: &DataContext,
    subject_id: &str,
    prop_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let PropertyValue::DataRef(path) = pv else {
        return;
    };
    match ctx.get(path) {
        Some(value) => *pv = PropertyValue::Literal(value.to_owned()),
        None => diagnostics.push(missing_field_diag(subject_id, prop_name, path)),
    }
}

/// Resolve a DIMENSION-typed property: the field value is parsed as a number
/// (after stripping a trailing `px` / surrounding whitespace) and becomes a
/// `Dimension` in px. A missing field OR an unparseable value emits
/// `data.missing_field` and leaves the ref in place (unresolved).
fn substitute_dim_prop_opt(
    prop: &mut Option<PropertyValue>,
    ctx: &DataContext,
    subject_id: &str,
    prop_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(pv) = prop else {
        return;
    };
    let PropertyValue::DataRef(path) = pv else {
        return;
    };
    match ctx.get(path) {
        Some(value) => match parse_px(value) {
            Some(n) => *pv = PropertyValue::Dimension(px(n)),
            None => diagnostics.push(missing_field_diag(subject_id, prop_name, path)),
        },
        None => diagnostics.push(missing_field_diag(subject_id, prop_name, path)),
    }
}

/// Parse a data field value as a pixel magnitude: strip surrounding whitespace
/// and an optional trailing `px`, then parse as `f64`. Returns `None` on failure
/// or a non-finite result.
fn parse_px(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    let num = trimmed.strip_suffix("px").unwrap_or(trimmed).trim();
    num.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn missing_field_diag(subject_id: &str, prop_name: &str, path: &str) -> Diagnostic {
    Diagnostic::advisory(
        "data.missing_field",
        format!(
            "node '{subject_id}': property '{prop_name}' references data field \
             '{path}' which is not present in the data context (or is not a valid \
             value for this property); left unresolved"
        ),
        None,
        Some(subject_id.to_owned()),
    )
}

// ── Field-slice accessors used only by the read-only scan ────────────────────────
//
// These mirror the property sets handled by the mutating substitutors so the
// scan and the walk stay in lockstep.

fn rect_color_props(n: &RectNode) -> [&Option<PropertyValue>; 7] {
    [
        &n.fill,
        &n.stroke,
        &n.border_top,
        &n.border_bottom,
        &n.border_left,
        &n.border_right,
        &n.stroke_outer,
    ]
}

fn rect_dim_props(n: &RectNode) -> [&Option<PropertyValue>; 8] {
    [
        &n.radius,
        &n.radius_tl,
        &n.radius_tr,
        &n.radius_br,
        &n.radius_bl,
        &n.stroke_width,
        &n.border_width,
        &n.stroke_outer_width,
    ]
}

fn ellipse_color_props(n: &EllipseNode) -> [&Option<PropertyValue>; 2] {
    [&n.fill, &n.stroke]
}

fn ellipse_dim_props(n: &EllipseNode) -> [&Option<PropertyValue>; 3] {
    [&n.rx, &n.ry, &n.stroke_width]
}

fn text_color_props(n: &TextNode) -> [&Option<PropertyValue>; 4] {
    [&n.fill, &n.stroke, &n.contrast_bg, &n.font_family]
}

fn text_dim_props(n: &TextNode) -> [&Option<PropertyValue>; 3] {
    [&n.font_size, &n.font_size_min, &n.stroke_width]
}

fn shape_color_props(n: &ShapeNode) -> [&Option<PropertyValue>; 2] {
    [&n.fill, &n.stroke]
}

fn shape_dim_props(n: &ShapeNode) -> [&Option<PropertyValue>; 3] {
    [&n.stroke_width, &n.radius, &n.padding]
}

fn pattern_color_props(n: &PatternNode) -> [&Option<PropertyValue>; 7] {
    [
        &n.fill,
        &n.stroke,
        &n.border_top,
        &n.border_bottom,
        &n.border_left,
        &n.border_right,
        &n.stroke_outer,
    ]
}

fn pattern_dim_props(n: &PatternNode) -> [&Option<PropertyValue>; 8] {
    [
        &n.radius,
        &n.radius_tl,
        &n.radius_tr,
        &n.radius_br,
        &n.radius_bl,
        &n.stroke_width,
        &n.border_width,
        &n.stroke_outer_width,
    ]
}

fn chart_color_props(n: &ChartNode) -> [&Option<PropertyValue>; 8] {
    [
        &n.fill,
        &n.stroke,
        &n.border_top,
        &n.border_bottom,
        &n.border_left,
        &n.border_right,
        &n.stroke_outer,
        &n.value_color,
    ]
}

fn chart_dim_props(n: &ChartNode) -> [&Option<PropertyValue>; 8] {
    [
        &n.radius,
        &n.radius_tl,
        &n.radius_tr,
        &n.radius_br,
        &n.radius_bl,
        &n.stroke_width,
        &n.border_width,
        &n.stroke_outer_width,
    ]
}
