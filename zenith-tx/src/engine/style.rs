//! Style and text op application: fill, stroke, stroke-width, opacity,
//! text-align, text-direction, find-replace-text, and text-replacement setters,
//! plus the property accessors they use.

use zenith_core::{
    Diagnostic, Document, Node, PropertyValue, TextNode, TextSpan, canonicalize_style_key,
};

use crate::op::OpSpan;

use super::{find_node_any_mut, node_kind_str, record_affected};

// ── Valid align values ────────────────────────────────────────────────────────

const VALID_ALIGNS: &[&str] = &["start", "center", "end", "justify"];

// ── Field accessor helpers ────────────────────────────────────────────────────

/// Return a mutable reference to the `fill` field of a node, or `None` for
/// node variants that do not have a `fill` property
/// (`Line`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_fill_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.fill),
        Node::Ellipse(n) => Some(&mut n.fill),
        Node::Text(n) => Some(&mut n.fill),
        Node::Code(n) => Some(&mut n.fill),
        Node::Polygon(n) => Some(&mut n.fill),
        Node::Polyline(n) => Some(&mut n.fill),
        Node::Field(n) => Some(&mut n.fill),
        Node::Toc(n) => Some(&mut n.fill),
        Node::Footnote(n) => Some(&mut n.fill),
        Node::Table(n) => Some(&mut n.fill),
        Node::Shape(n) => Some(&mut n.fill),
        Node::Pattern(n) => Some(&mut n.fill),
        Node::Chart(n) => Some(&mut n.fill),
        // A connector is stroke-only (like `line`): it has no `fill` field, so
        // set_fill honestly surfaces tx.unsupported_property.
        Node::Line(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
        | Node::Connector(_)
        | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `stroke` field of a node, or `None` for
/// variants that do not have a `stroke` property
/// (`Text`, `Code`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_stroke_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.stroke),
        Node::Line(n) => Some(&mut n.stroke),
        Node::Polygon(n) => Some(&mut n.stroke),
        Node::Polyline(n) => Some(&mut n.stroke),
        Node::Ellipse(n) => Some(&mut n.stroke),
        Node::Shape(n) => Some(&mut n.stroke),
        Node::Connector(n) => Some(&mut n.stroke),
        Node::Pattern(n) => Some(&mut n.stroke),
        Node::Chart(n) => Some(&mut n.stroke),
        Node::Text(_)
        | Node::Code(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Table(_)
        | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `stroke_width` field of a node, or `None`
/// for variants that do not have a `stroke_width` property
/// (`Text`, `Code`, `Frame`, `Group`, `Image`, `Unknown`).
fn node_stroke_width_mut(node: &mut Node) -> Option<&mut Option<PropertyValue>> {
    match node {
        Node::Rect(n) => Some(&mut n.stroke_width),
        Node::Line(n) => Some(&mut n.stroke_width),
        Node::Polygon(n) => Some(&mut n.stroke_width),
        Node::Polyline(n) => Some(&mut n.stroke_width),
        Node::Ellipse(n) => Some(&mut n.stroke_width),
        Node::Shape(n) => Some(&mut n.stroke_width),
        Node::Connector(n) => Some(&mut n.stroke_width),
        Node::Pattern(n) => Some(&mut n.stroke_width),
        Node::Chart(n) => Some(&mut n.stroke_width),
        Node::Text(_)
        | Node::Code(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Table(_)
        | Node::Unknown(_) => None,
    }
}

/// Return a mutable reference to the `opacity` field of a node, or `None`
/// for `Node::Unknown` which carries no `opacity` field.
fn node_opacity_mut(node: &mut Node) -> Option<&mut Option<f64>> {
    match node {
        Node::Rect(n) => Some(&mut n.opacity),
        Node::Ellipse(n) => Some(&mut n.opacity),
        Node::Line(n) => Some(&mut n.opacity),
        Node::Text(n) => Some(&mut n.opacity),
        Node::Code(n) => Some(&mut n.opacity),
        Node::Frame(n) => Some(&mut n.opacity),
        Node::Group(n) => Some(&mut n.opacity),
        Node::Image(n) => Some(&mut n.opacity),
        Node::Polygon(n) => Some(&mut n.opacity),
        Node::Polyline(n) => Some(&mut n.opacity),
        Node::Instance(n) => Some(&mut n.opacity),
        Node::Field(n) => Some(&mut n.opacity),
        Node::Toc(n) => Some(&mut n.opacity),
        Node::Table(n) => Some(&mut n.opacity),
        Node::Shape(n) => Some(&mut n.opacity),
        Node::Connector(n) => Some(&mut n.opacity),
        Node::Pattern(n) => Some(&mut n.opacity),
        Node::Chart(n) => Some(&mut n.opacity),
        // A footnote has no `opacity` field.
        Node::Footnote(_) => None,
        Node::Unknown(_) => None,
    }
}

// ── Valid overflow values ─────────────────────────────────────────────────────

const VALID_OVERFLOWS: &[&str] = &["fit", "clip", "visible"];

/// Return a mutable reference to the `overflow` field of a node, or `None` for
/// variants that do not carry an `overflow` property. Only `Text` and `Code`
/// have one.
fn node_overflow_mut(node: &mut Node) -> Option<&mut Option<String>> {
    match node {
        Node::Text(n) => Some(&mut n.overflow),
        Node::Code(n) => Some(&mut n.overflow),
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Table(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Unknown(_) => None,
    }
}

// ── SetTextAlign ──────────────────────────────────────────────────────────────

pub(super) fn apply_set_text_align(
    node_id: &str,
    align: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate align value before touching the tree.
    if !VALID_ALIGNS.contains(&align) {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "invalid align value {:?}; must be one of: {}",
                align,
                VALID_ALIGNS.join(", ")
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    // Walk the tree looking for `node_id`.
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(Node::Text(text_node)) => {
            text_node.align = Some(align.to_owned());
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.wrong_node_type",
                format!(
                    "set_text_align requires a text node but {:?} is a {}",
                    node_id, kind
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}

// ── SetTextOverflow ───────────────────────────────────────────────────────────

pub(super) fn apply_set_text_overflow(
    node_id: &str,
    overflow: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate overflow value before touching the tree.
    if !VALID_OVERFLOWS.contains(&overflow) {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "invalid overflow value {:?}; must be one of: {}",
                overflow,
                VALID_OVERFLOWS.join(", ")
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            // node_kind_str returns &'static str, so there is no live borrow of
            // `node` after this binding — the mutable borrow below is fine.
            let kind = node_kind_str(node);
            match node_overflow_mut(node) {
                Some(slot) => {
                    *slot = Some(overflow.to_owned());
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.wrong_node_type",
                        format!(
                            "set_text_overflow requires a text or code node but {:?} is a {}",
                            node_id, kind
                        ),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
    }
}

// ── SetFill / SetStroke / SetStrokeWidth ──────────────────────────────────────

/// Shared driver for token-valued property setters (`set_fill`, `set_stroke`,
/// `set_stroke_width`). Finds the node, fetches the `Option<PropertyValue>`
/// slot via `accessor`, and sets it to `TokenRef(token)`. Emits `tx.unknown_node`
/// if the node is missing, or `tx.unsupported_property` (naming `op_label`) if the
/// node kind lacks the property.
fn apply_set_property_token(
    node_id: &str,
    token: &str,
    op_label: &str,
    accessor: fn(&mut Node) -> Option<&mut Option<PropertyValue>>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            // node_kind_str returns &'static str, so there is no live borrow
            // of `node` after this let binding — the mutable borrow below is fine.
            let kind = node_kind_str(node);
            match accessor(node) {
                Some(slot) => {
                    *slot = Some(PropertyValue::TokenRef(token.to_owned()));
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("{} is not supported on a {} node", op_label, kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
    }
}

pub(super) fn apply_set_fill(
    node_id: &str,
    fill_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        fill_token,
        "set_fill",
        node_fill_mut,
        doc,
        diagnostics,
        affected,
    );
}

pub(super) fn apply_set_stroke(
    node_id: &str,
    stroke_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        stroke_token,
        "set_stroke",
        node_stroke_mut,
        doc,
        diagnostics,
        affected,
    );
}

pub(super) fn apply_set_stroke_width(
    node_id: &str,
    stroke_width_token: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    apply_set_property_token(
        node_id,
        stroke_width_token,
        "set_stroke_width",
        node_stroke_width_mut,
        doc,
        diagnostics,
        affected,
    );
}

// ── SetOpacity ────────────────────────────────────────────────────────────────

pub(super) fn apply_set_opacity(
    node_id: &str,
    opacity: f64,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            let kind = node_kind_str(node);
            match node_opacity_mut(node) {
                Some(slot) => {
                    *slot = Some(opacity.clamp(0.0, 1.0));
                    record_affected(node_id, affected);
                }
                None => {
                    diagnostics.push(Diagnostic::error(
                        "tx.unsupported_property",
                        format!("set_opacity is not supported on a {} node", kind),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
        }
    }
}

// ── SetStyleProperty ─────────────────────────────────────────────────────────

/// Set one recognized visual property on a named style to a token reference.
///
/// Canonicalizes `property` (accepting underscore forms), then looks up the
/// style by `style_id`. On success, inserts `PropertyValue::TokenRef(value)`
/// into the style's `properties` map under the canonical key and records the
/// style id as affected. Emits `tx.unsupported_property` for unrecognized keys
/// and `tx.unknown_style` when no style with `style_id` exists.
pub(super) fn apply_set_style_property(
    style_id: &str,
    property: &str,
    value: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Canonicalize the property key; reject immediately if not recognized.
    let Some(canonical_key) = canonicalize_style_key(property) else {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!("property {:?} is not a recognized style key", property),
            None,
            Some(style_id.to_owned()),
        ));
        return;
    };

    // Locate the style by id.
    match doc.styles.styles.iter_mut().find(|s| s.id == style_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_style",
                format!("style {:?} not found in document", style_id),
                None,
                Some(style_id.to_owned()),
            ));
        }
        Some(style) => {
            style.properties.insert(
                canonical_key.to_owned(),
                PropertyValue::TokenRef(value.to_owned()),
            );
            record_affected(style_id, affected);
        }
    }
}

// ── SetTextDirection ──────────────────────────────────────────────────────────

const VALID_DIRECTIONS: &[&str] = &["ltr", "rtl"];

/// Set the `direction` property on a text node.
///
/// Mirrors [`apply_set_text_align`] exactly: eager value validation, then a
/// three-arm match over the found node.
pub(super) fn apply_set_text_direction(
    node_id: &str,
    direction: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate direction value before touching the tree.
    if !VALID_DIRECTIONS.contains(&direction) {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "invalid direction value {:?}; must be one of: {}",
                direction,
                VALID_DIRECTIONS.join(", ")
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    // Walk the tree looking for `node_id`.
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(Node::Text(text_node)) => {
            text_node.direction = Some(direction.to_owned());
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.wrong_node_type",
                format!(
                    "set_text_direction requires a text node but {:?} is a {}",
                    node_id, kind
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}

// ── FindReplaceText ───────────────────────────────────────────────────────────

/// Replace all occurrences of `find` in every span's `text` of `spans`,
/// returning `true` if any span was modified.
///
/// Only `TextSpan::text` is mutated; all formatting fields are preserved.
/// Shared by both text nodes and shape labels (their spans are the same type).
fn replace_in_spans(spans: &mut Vec<TextSpan>, find: &str, replace: &str) -> bool {
    let mut changed = false;
    for span in spans {
        if span.text.contains(find) {
            span.text = span.text.replace(find, replace);
            changed = true;
        }
    }
    changed
}

/// Replace all occurrences of `find` in every span's text of `text_node`,
/// returning `true` if any span was modified.
///
/// Thin wrapper over [`replace_in_spans`] so text and shape paths share one
/// source of truth.
fn replace_in_text_node(text_node: &mut TextNode, find: &str, replace: &str) -> bool {
    replace_in_spans(&mut text_node.spans, find, replace)
}

/// Map a slice of transaction [`OpSpan`]s to owned [`TextSpan`]s.
///
/// `fill` and `font_weight` token ids are wrapped as `PropertyValue::TokenRef`;
/// all other formatting fields are carried verbatim. Shared by `replace_text`'s
/// text-node and shape-label arms so the mapping has a single source of truth.
fn op_spans_to_text_spans(spans: &[OpSpan]) -> Vec<TextSpan> {
    spans
        .iter()
        .map(|s| TextSpan {
            text: s.text.clone(),
            fill: s
                .fill
                .as_ref()
                .map(|id| PropertyValue::TokenRef(id.clone())),
            font_weight: s
                .font_weight
                .as_ref()
                .map(|id| PropertyValue::TokenRef(id.clone())),
            italic: s.italic,
            underline: s.underline,
            strikethrough: s.strikethrough,
            vertical_align: s.vertical_align.clone(),
            footnote_ref: s.footnote_ref.clone(),
            data_ref: None,
            data_format: None,
            highlight: None,
            code: None,
            link: None,
        })
        .collect()
}

/// Collect `(id, is_locked)` pairs for every `TextNode` and `ShapeNode`
/// (whose owned label spans are find-replace targets) reachable in
/// `children`, in document order, recursing into `Frame` and `Group`
/// containers. Capturing the locked flag here avoids a second O(n) tree walk
/// per node in the doc-wide find-replace loop. Exhaustive over all `Node`
/// variants — no wildcard.
fn collect_text_entries(children: &[Node], out: &mut Vec<(String, bool)>) {
    for node in children {
        match node {
            Node::Text(t) => out.push((t.id.clone(), t.locked == Some(true))),
            // A shape owns its label spans directly (not as child Nodes), so it
            // is a leaf for this collection — do not recurse into it.
            Node::Shape(s) => out.push((s.id.clone(), s.locked == Some(true))),
            Node::Frame(f) => collect_text_entries(&f.children, out),
            Node::Group(g) => collect_text_entries(&g.children, out),
            Node::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_text_entries(&cell.children, out);
                    }
                }
            }
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Toc(_)
            | Node::Footnote(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Apply a literal find-and-replace across one or all text nodes in `doc`.
///
/// See [`Op::FindReplaceText`] for the full specification.
pub(super) fn apply_find_replace_text(
    find: &str,
    replace: &str,
    node: Option<&str>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Eager: empty find string is invalid.
    if find.is_empty() {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            "find string must be non-empty",
            None,
            None,
        ));
        return;
    }

    match node {
        // ── Scoped mode: one named text node ─────────────────────────────────
        Some(node_id) => match find_node_any_mut(doc, node_id) {
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("node {:?} not found in document", node_id),
                    None,
                    Some(node_id.to_owned()),
                ));
            }
            Some(Node::Text(text_node)) => {
                if replace_in_text_node(text_node, find, replace) {
                    record_affected(node_id, affected);
                } else {
                    diagnostics.push(Diagnostic::advisory(
                        "tx.noop",
                        format!(
                            "find_replace_text: {:?} not found in node {:?}; document is unchanged",
                            find, node_id
                        ),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
            Some(Node::Shape(shape_node)) => {
                if replace_in_spans(&mut shape_node.spans, find, replace) {
                    record_affected(node_id, affected);
                } else {
                    diagnostics.push(Diagnostic::advisory(
                        "tx.noop",
                        format!(
                            "find_replace_text: {:?} not found in node {:?}; document is unchanged",
                            find, node_id
                        ),
                        None,
                        Some(node_id.to_owned()),
                    ));
                }
            }
            Some(other) => {
                let kind = node_kind_str(other);
                diagnostics.push(Diagnostic::error(
                    "tx.wrong_node_type",
                    format!(
                        "find_replace_text requires a text or shape node but {:?} is a {}",
                        node_id, kind
                    ),
                    None,
                    Some(node_id.to_owned()),
                ));
            }
        },

        // ── Doc-wide mode: all text nodes ─────────────────────────────────────
        None => {
            // Phase 1: collect (id, is_locked) for every text node in the
            // document in one shared pass — avoids a second O(n) tree walk
            // per node in Phase 2.
            let mut all_text_entries: Vec<(String, bool)> = Vec::new();
            for page in &doc.body.pages {
                collect_text_entries(&page.children, &mut all_text_entries);
            }

            let mut skipped: Vec<String> = Vec::new();
            // Track whether this op changed anything (independent of prior ops
            // in the same transaction that may have already populated `affected`).
            let mut this_op_changed = false;

            // Phase 2: for each text node, skip locked ones then mutate.
            for (id, is_locked) in &all_text_entries {
                if *is_locked {
                    skipped.push(id.clone());
                    continue;
                }

                // Mutate via find_node_any_mut. Text nodes and shapes both own
                // span lists; any other node kind is not a text target here.
                let changed = match find_node_any_mut(doc, id) {
                    Some(Node::Text(text_node)) => replace_in_text_node(text_node, find, replace),
                    Some(Node::Shape(shape_node)) => {
                        replace_in_spans(&mut shape_node.spans, find, replace)
                    }
                    Some(Node::Rect(_))
                    | Some(Node::Ellipse(_))
                    | Some(Node::Line(_))
                    | Some(Node::Code(_))
                    | Some(Node::Frame(_))
                    | Some(Node::Group(_))
                    | Some(Node::Image(_))
                    | Some(Node::Polygon(_))
                    | Some(Node::Polyline(_))
                    | Some(Node::Instance(_))
                    | Some(Node::Field(_))
                    | Some(Node::Footnote(_))
                    | Some(Node::Toc(_))
                    | Some(Node::Table(_))
                    | Some(Node::Connector(_))
                    | Some(Node::Pattern(_))
                    | Some(Node::Chart(_))
                    | Some(Node::Unknown(_))
                    | None => false,
                };
                if changed {
                    record_affected(id, affected);
                    this_op_changed = true;
                }
            }

            // Sort skipped list for determinism, then emit advisory if any.
            skipped.sort();
            if !skipped.is_empty() {
                diagnostics.push(Diagnostic::warning(
                    "tx.locked_skipped",
                    format!(
                        "find_replace_text: skipped locked node(s): {}",
                        skipped.join(", ")
                    ),
                    None,
                    None,
                ));
            }

            // If this op changed nothing AND nothing was skipped, emit noop advisory.
            if !this_op_changed && skipped.is_empty() {
                diagnostics.push(Diagnostic::advisory(
                    "tx.noop",
                    format!(
                        "find_replace_text: {:?} not found in any text node or shape label; document is unchanged",
                        find
                    ),
                    None,
                    None,
                ));
            }
        }
    }
}

// ── ReplaceText ───────────────────────────────────────────────────────────────

pub(super) fn apply_replace_text(
    node_id: &str,
    spans: &[OpSpan],
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(Node::Text(text_node)) => {
            text_node.spans = op_spans_to_text_spans(spans);
            record_affected(node_id, affected);
        }
        Some(Node::Shape(shape_node)) => {
            shape_node.spans = op_spans_to_text_spans(spans);
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.unsupported_property",
                format!("replace_text is not supported on a {} node", kind),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}
