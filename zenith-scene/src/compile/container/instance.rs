//! `instance` expansion: clone the referenced component subtree, apply
//! overrides, prefix descendant ids, and delegate to [`compile_group`] for the
//! translation + opacity cascade. Also hosts the id-prefix walk reused by the
//! parent module's master-page projection.

use std::collections::BTreeMap;

use zenith_core::{Diagnostic, GroupNode, InstanceNode, Node, Override, PropertyValue};

use crate::ir::SceneCommand;

use super::super::{NodeCtx, RenderCtx};
use super::group::compile_group;

/// Compile an `instance` node by expanding its referenced component subtree.
///
/// Expansion strategy (per the component/symbol design):
/// 1. Look the component up in `components`; a missing component emits an
///    advisory `scene.unknown_component` and the instance is skipped.
/// 2. CLONE the component's children (never mutate the stored definition),
///    apply each override to the matching LOCAL-id descendant, then PREFIX every
///    descendant id with the instance id (`<inst-id>/<local-id>`) so multiple
///    instances of the same component never produce duplicate ids in the scene.
/// 3. Wrap the prepared children in a synthetic [`GroupNode`] carrying the
///    instance's `x`/`y` origin (as the group translation) and its
///    `opacity`/`visible` cascade, then delegate to [`compile_group`]. This
///    reuses the group translation + opacity-cascade machinery verbatim rather
///    than duplicating it; the instance itself emits no command.
pub(in crate::compile) fn compile_instance(
    instance: &InstanceNode,
    cx: NodeCtx,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: RenderCtx,
) {
    // Entire expansion excluded when visible=false (mirror group/frame).
    if instance.visible == Some(false) {
        return;
    }

    let Some(component) = cx.components.get(instance.component.as_str()) else {
        diagnostics.push(Diagnostic::advisory(
            "scene.unknown_component",
            format!(
                "instance '{}' references component '{}' which is not declared; \
                 the instance is skipped",
                instance.id, instance.component
            ),
            instance.source_span,
            Some(instance.id.clone()),
        ));
        return;
    };

    // Clone the component subtree (the stored definition is never mutated),
    // apply overrides against LOCAL ids, then prefix ids with the instance id.
    let mut children = component.children.clone();
    for ov in &instance.overrides {
        apply_override(&mut children, ov);
    }
    let prefix = format!("{}/", instance.id);
    prefix_ids_in_children(&mut children, &prefix);

    // Build a synthetic group carrying the instance origin + cascade and reuse
    // compile_group's translation/opacity logic. The group's own id is the
    // instance id (it emits no command, so the id is only for self-consistency).
    let synthetic = GroupNode {
        id: instance.id.clone(),
        name: instance.name.clone(),
        role: instance.role.clone(),
        x: instance.x.clone(),
        y: instance.y.clone(),
        w: None,
        h: None,
        opacity: instance.opacity,
        visible: instance.visible,
        locked: instance.locked,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_parent: None,
        children,
        source_span: instance.source_span,
        unknown_props: BTreeMap::new(),
    };

    compile_group(&synthetic, cx, commands, diagnostics, ctx);
}

/// Apply a single [`Override`] to the first descendant in `children` (descending
/// into `group`/`frame`/`instance` containers) whose LOCAL id equals
/// `ov.ref_id`. Mutates a CLONE — callers pass the cloned component subtree.
///
/// Supported v0 payload: replace `spans` (text targets), `fill`, and `visible`.
/// An override targeting a kind without the relevant field is a no-op for that
/// field (e.g. `spans` on a rect). An unmatched ref is silently ignored here;
/// the validator already warns via `component.unknown_override_target`.
fn apply_override(children: &mut [Node], ov: &Override) -> bool {
    for child in children.iter_mut() {
        if node_local_id(child) == Some(ov.ref_id.as_str()) {
            apply_override_to_node(child, ov);
            return true;
        }
        let grandchildren = match child {
            Node::Frame(f) => Some(&mut f.children),
            Node::Group(g) => Some(&mut g.children),
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Table(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Unknown(_) => None,
        };
        if let Some(gc) = grandchildren
            && apply_override(gc, ov)
        {
            return true;
        }
    }
    false
}

/// Merge an override's supported fields onto a single matched node.
fn apply_override_to_node(node: &mut Node, ov: &Override) {
    // spans → only a text node carries spans.
    if let Some(spans) = &ov.spans
        && let Node::Text(t) = node
    {
        t.spans = spans.clone();
    }
    // fill → the kinds that carry a fill property.
    if let Some(fill) = &ov.fill {
        set_node_fill(node, fill.clone());
    }
    // visible → every id-bearing renderable kind carries a visible flag.
    if let Some(v) = ov.visible {
        set_node_visible(node, v);
    }
}

/// Set the `fill` of a node kind that carries one; a no-op for kinds without
/// a fill property.
fn set_node_fill(node: &mut Node, fill: PropertyValue) {
    match node {
        Node::Rect(n) => n.fill = Some(fill),
        Node::Ellipse(n) => n.fill = Some(fill),
        Node::Text(n) => n.fill = Some(fill),
        Node::Code(n) => n.fill = Some(fill),
        Node::Polygon(n) => n.fill = Some(fill),
        Node::Polyline(n) => n.fill = Some(fill),
        Node::Field(n) => n.fill = Some(fill),
        Node::Toc(n) => n.fill = Some(fill),
        Node::Footnote(n) => n.fill = Some(fill),
        Node::Table(n) => n.fill = Some(fill),
        Node::Shape(n) => n.fill = Some(fill),
        // A connector is stroke-only; it has no fill to override.
        Node::Line(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
        | Node::Connector(_)
        | Node::Unknown(_) => {}
    }
}

/// Set the `visible` flag of a node kind that carries one.
fn set_node_visible(node: &mut Node, v: bool) {
    match node {
        Node::Rect(n) => n.visible = Some(v),
        Node::Ellipse(n) => n.visible = Some(v),
        Node::Line(n) => n.visible = Some(v),
        Node::Text(n) => n.visible = Some(v),
        Node::Code(n) => n.visible = Some(v),
        Node::Frame(n) => n.visible = Some(v),
        Node::Group(n) => n.visible = Some(v),
        Node::Image(n) => n.visible = Some(v),
        Node::Polygon(n) => n.visible = Some(v),
        Node::Polyline(n) => n.visible = Some(v),
        Node::Instance(n) => n.visible = Some(v),
        Node::Field(n) => n.visible = Some(v),
        Node::Toc(n) => n.visible = Some(v),
        Node::Table(n) => n.visible = Some(v),
        Node::Shape(n) => n.visible = Some(v),
        Node::Connector(n) => n.visible = Some(v),
        // A footnote has no `visible` flag; nothing to set.
        Node::Footnote(_) => {}
        Node::Unknown(_) => {}
    }
}

/// The LOCAL id of a node (the id as authored), or `None` for `Unknown`.
fn node_local_id(node: &Node) -> Option<&str> {
    match node {
        Node::Rect(n) => Some(&n.id),
        Node::Ellipse(n) => Some(&n.id),
        Node::Line(n) => Some(&n.id),
        Node::Text(n) => Some(&n.id),
        Node::Code(n) => Some(&n.id),
        Node::Frame(n) => Some(&n.id),
        Node::Group(n) => Some(&n.id),
        Node::Image(n) => Some(&n.id),
        Node::Polygon(n) => Some(&n.id),
        Node::Polyline(n) => Some(&n.id),
        Node::Instance(n) => Some(&n.id),
        Node::Field(n) => Some(&n.id),
        Node::Toc(n) => Some(&n.id),
        Node::Footnote(n) => Some(&n.id),
        Node::Table(n) => Some(&n.id),
        Node::Shape(n) => Some(&n.id),
        Node::Connector(n) => Some(&n.id),
        Node::Unknown(_) => None,
    }
}

/// Recursively prepend `prefix` to the id of every id-bearing node in
/// `children`, descending into `group`/`frame` containers (and prefixing nested
/// instance ids too). Mirrors the suffix walk used by `duplicate_page` in
/// zenith-tx (an in-order recursion, deterministic, no HashMap), but applied as
/// a PREFIX with the instance id so two instances of one component never collide.
pub(in crate::compile) fn prefix_ids_in_children(children: &mut [Node], prefix: &str) {
    for child in children.iter_mut() {
        prefix_node_id(child, prefix);
        match child {
            Node::Frame(f) => prefix_ids_in_children(&mut f.children, prefix),
            Node::Group(g) => prefix_ids_in_children(&mut g.children, prefix),
            Node::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        prefix_ids_in_children(&mut cell.children, prefix);
                    }
                }
            }
            Node::Rect(_)
            | Node::Ellipse(_)
            | Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Unknown(_) => {}
        }
    }
}

/// Prepend `prefix` to a single node's id (a no-op for `Unknown`).
fn prefix_node_id(node: &mut Node, prefix: &str) {
    macro_rules! pre {
        ($field:expr) => {{
            $field = format!("{prefix}{}", $field);
        }};
    }
    match node {
        Node::Rect(n) => pre!(n.id),
        Node::Ellipse(n) => pre!(n.id),
        Node::Line(n) => pre!(n.id),
        Node::Text(n) => pre!(n.id),
        Node::Code(n) => pre!(n.id),
        Node::Frame(n) => pre!(n.id),
        Node::Group(n) => pre!(n.id),
        Node::Image(n) => pre!(n.id),
        Node::Polygon(n) => pre!(n.id),
        Node::Polyline(n) => pre!(n.id),
        Node::Instance(n) => pre!(n.id),
        Node::Field(n) => pre!(n.id),
        Node::Toc(n) => pre!(n.id),
        Node::Footnote(n) => pre!(n.id),
        Node::Table(n) => pre!(n.id),
        Node::Shape(n) => pre!(n.id),
        Node::Connector(n) => pre!(n.id),
        Node::Unknown(_) => {}
    }
}
