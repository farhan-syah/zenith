//! Container-node compilation: `frame` (clip-only) and `group` (translate +
//! opacity cascade), plus the bounding-box helpers used to determine a group's
//! rotation pivot.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, FontProvider, FrameNode, GroupNode, InstanceNode, Node, Override, Point,
    PropertyValue, ResolvedToken, Style, Unit, dim_to_px,
};
use zenith_layout::RustybuzzEngine;

use crate::ir::SceneCommand;

use super::chain::ChainAssignments;
use super::field::FieldCtx;
use super::util::{resolve_property_dimension_px, rotation_degrees, unsupported_unit_diag};
use super::{ComponentMap, RenderCtx, compile_node, node_role, style_prop};

// NOTE: compile_frame → compile_node → compile_frame recursion has no depth
// guard, consistent with the compile_group limitation in v0.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_frame(
    frame: &FrameNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false (no PushClip emitted).
    if frame.visible == Some(false) {
        return;
    }

    // All four geometry dimensions are required for a frame clip rectangle.
    // Resolve them BEFORE pushing any PushClip to keep push/pop balanced.
    let (Some(x_dim), Some(y_dim), Some(w_dim), Some(h_dim)) =
        (&frame.x, &frame.y, &frame.w, &frame.h)
    else {
        diagnostics.push(Diagnostic::advisory(
            "scene.missing_geometry",
            format!(
                "frame '{}' is missing one or more geometry properties (x, y, w, h); \
                 skipped",
                frame.id
            ),
            frame.source_span,
            Some(frame.id.clone()),
        ));
        return;
    };

    let Some(frame_x) = dim_to_px(x_dim.value, &x_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "x",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_y) = dim_to_px(y_dim.value, &y_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "y",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_w) = dim_to_px(w_dim.value, &w_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "w",
            frame.source_span,
        ));
        return;
    };
    let Some(frame_h) = dim_to_px(h_dim.value, &h_dim.unit) else {
        diagnostics.push(unsupported_unit_diag(
            "frame",
            &frame.id,
            "h",
            frame.source_span,
        ));
        return;
    };

    // Rotation bracket — outermost, wrapping PushClip + children + PopClip.
    // v0 limitation: the clip rectangle below stays axis-aligned even when the
    // frame is rotated; rotated children may extend past the axis-aligned clip.
    let frame_rot = rotation_degrees(frame.rotate.as_ref());
    if let Some(angle) = frame_rot {
        let cx = ctx.dx + frame_x + frame_w / 2.0;
        let cy = ctx.dy + frame_y + frame_h / 2.0;
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Clip rectangle is the frame's own bbox.
    commands.push(SceneCommand::PushClip {
        x: frame_x,
        y: frame_y,
        w: frame_w,
        h: frame_h,
    });

    // Frame clips only — it does NOT translate children (dx/dy unchanged).
    // Opacity cascades into all descendant alphas exactly as group does.
    let child_ctx = RenderCtx {
        opacity: ctx.opacity * frame.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
        dx: ctx.dx, // clip-only: no translation
        dy: ctx.dy, // clip-only: no translation
        // Page baseline grid cascades unchanged so all text shares one grid.
        baseline_grid: ctx.baseline_grid,
    };

    match frame.layout.as_deref() {
        Some("flow") => {
            compile_frame_flow(
                frame,
                frame_x,
                frame_y,
                frame_w,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                field_ctx,
                child_ctx,
            );
        }
        Some("grid") => {
            compile_frame_grid(
                frame,
                frame_x,
                frame_y,
                frame_w,
                frame_h,
                resolved,
                style_map,
                components,
                fonts,
                engine,
                commands,
                diagnostics,
                chains,
                field_ctx,
                child_ctx,
            );
        }
        _ => {
            // Absolute (clip-only) model: children render at their own page coords.
            for child in &frame.children {
                compile_node(
                    child,
                    resolved,
                    style_map,
                    components,
                    fonts,
                    engine,
                    commands,
                    diagnostics,
                    chains,
                    field_ctx,
                    child_ctx,
                );
            }
        }
    }

    commands.push(SceneCommand::PopClip);

    if frame_rot.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
    // Frame emits no fill of its own in v0.
}

/// Lay a flow-frame's children out as a vertical stack inside its padded
/// content box, compiling each at the injected absolute coordinates.
///
/// Triggered only when `frame.layout == Some("flow")`. `frame_x`/`frame_y`/
/// `frame_w` are the already-resolved frame box in page coordinates (the same
/// values used for the surrounding `PushClip`). Children stack in source order
/// with `gap` between them; `padding` insets the content box uniformly. Both
/// `padding` and `gap` are token-only dimension style props on the frame's
/// style, defaulting to `0.0` when absent.
#[allow(clippy::too_many_arguments)]
fn compile_frame_flow(
    frame: &FrameNode,
    frame_x: f64,
    frame_y: f64,
    frame_w: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    child_ctx: RenderCtx,
) {
    // Resolve padding / gap from the frame's style (token → px); 0.0 default.
    let pad = resolve_property_dimension_px(
        &style_prop(&frame.style, style_map, "padding").cloned(),
        resolved,
        0.0,
    );
    let gap = resolve_property_dimension_px(
        &style_prop(&frame.style, style_map, "gap").cloned(),
        resolved,
        0.0,
    );

    // Content box: uniform padding on all four sides.
    let content_left = frame_x + pad;
    let content_top = frame_y + pad;
    let content_w = (frame_w - 2.0 * pad).max(0.0);

    // Lay out children that participate (skip invisible and guide nodes) so a
    // trailing gap is only suppressed relative to the LAST laid-out child.
    let laid_out: Vec<&Node> = frame
        .children
        .iter()
        .filter(|c| !node_skipped_in_flow(c))
        .collect();
    let last_idx = laid_out.len().saturating_sub(1);

    let mut cursor_y = content_top;
    for (i, child) in laid_out.iter().enumerate() {
        // Cross-axis = start; child width = own declared `w` or the content
        // width. (A text child's own `align` still centers WITHIN its width.)
        let child_w = node_declared_w(child).unwrap_or(content_w);

        // Vertical extent: own declared `h` when present, else the MEASURED
        // intrinsic height returned by compiling the child (text/code wrapped
        // height; 0.0 for leaves with no declared height).
        let declared_h = node_declared_h(child);

        // Inject the absolute box onto a clone; compile with the SAME ctx
        // (dx/dy unchanged — injected coords are absolute page coords).
        let positioned = with_flow_box(child, content_left, cursor_y, child_w, declared_h);
        let measured_h = compile_node(
            &positioned,
            resolved,
            style_map,
            components,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            field_ctx,
            child_ctx,
        );

        // Advance by the declared height when present, otherwise the measured
        // intrinsic height read back from the compile.
        let advance = declared_h.unwrap_or(measured_h);
        cursor_y += advance;
        if i != last_idx {
            cursor_y += gap;
        }
    }
}

/// Lay a grid-frame's children out into a `columns × rows` grid inside its
/// padded content box, compiling each at the injected absolute coordinates.
///
/// Triggered only when `frame.layout == Some("grid")`. `frame_x`/`frame_y`/
/// `frame_w`/`frame_h` are the already-resolved frame box in page coordinates
/// (the same values used for the surrounding `PushClip`). Participating children
/// (the same set the flow layout lays out: visible, non-guide) auto-place
/// row-major into the grid. Both `padding` and `gap` are token-only dimension
/// style props on the frame's style, defaulting to `0.0` when absent.
///
/// Cell sizing (uniform gutters of `gap`):
/// - `cols = frame.columns.unwrap_or(1).max(1)`
/// - `effective_rows = frame.rows.max(1)` or, when absent,
///   `ceil(n / cols).max(1)` so the grid grows to fit its children.
/// - `col_w = ((content_w - (cols-1)*gap) / cols).max(0.0)`
/// - `row_h = ((content_h - (effective_rows-1)*gap) / effective_rows).max(0.0)`
///
/// Unlike flow, every cell's height is FIXED (`Some(row_h)`) so an image child
/// with `fit="cover"` fills its cell.
#[allow(clippy::too_many_arguments)]
fn compile_frame_grid(
    frame: &FrameNode,
    frame_x: f64,
    frame_y: f64,
    frame_w: f64,
    frame_h: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    child_ctx: RenderCtx,
) {
    // Resolve padding / gap from the frame's style (token → px); 0.0 default.
    let pad = resolve_property_dimension_px(
        &style_prop(&frame.style, style_map, "padding").cloned(),
        resolved,
        0.0,
    );
    let gap = resolve_property_dimension_px(
        &style_prop(&frame.style, style_map, "gap").cloned(),
        resolved,
        0.0,
    );

    // Content box: uniform padding on all four sides.
    let content_left = frame_x + pad;
    let content_top = frame_y + pad;
    let content_w = (frame_w - 2.0 * pad).max(0.0);
    let content_h = (frame_h - 2.0 * pad).max(0.0);

    // Participating children: skip invisible and guide nodes (reuse flow helper).
    let participating: Vec<&Node> = frame
        .children
        .iter()
        .filter(|c| !node_skipped_in_flow(c))
        .collect();
    let n = participating.len();

    // Column / row counts (both guaranteed >= 1 so divisions are safe).
    let cols = frame.columns.unwrap_or(1).max(1) as usize;
    let effective_rows = frame
        .rows
        .map(|r| r.max(1) as usize)
        .unwrap_or_else(|| n.div_ceil(cols).max(1));

    // Uniform cell sizing with `gap` gutters between cells.
    let col_w = ((content_w - (cols - 1) as f64 * gap) / cols as f64).max(0.0);
    let row_h = ((content_h - (effective_rows - 1) as f64 * gap) / effective_rows as f64).max(0.0);

    for (i, child) in participating.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cell_x = content_left + col as f64 * (col_w + gap);
        let cell_y = content_top + row as f64 * (row_h + gap);

        // Inject the FULL fixed cell box (always Some(row_h)) so e.g. an image
        // with fit="cover" fills its cell. Compile at absolute page coords
        // (dx/dy unchanged). The measured height is ignored — cells are fixed.
        let positioned = with_flow_box(child, cell_x, cell_y, col_w, Some(row_h));
        let _ = compile_node(
            &positioned,
            resolved,
            style_map,
            components,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            field_ctx,
            child_ctx,
        );
    }
}

/// Whether a child is excluded from flow layout entirely (consumes no space):
/// `visible == Some(false)` or `role == "guide"`.
fn node_skipped_in_flow(node: &Node) -> bool {
    node_role(node) == Some("guide") || node_visible(node) == Some(false)
}

/// The `visible` flag of any node kind, if set (kinds without the property
/// — `Unknown` — yield `None`).
fn node_visible(node: &Node) -> Option<bool> {
    match node {
        Node::Rect(n) => n.visible,
        Node::Ellipse(n) => n.visible,
        Node::Line(n) => n.visible,
        Node::Text(n) => n.visible,
        Node::Code(n) => n.visible,
        Node::Frame(n) => n.visible,
        Node::Group(n) => n.visible,
        Node::Image(n) => n.visible,
        Node::Polygon(n) => n.visible,
        Node::Polyline(n) => n.visible,
        Node::Instance(n) => n.visible,
        Node::Field(n) => n.visible,
        // A footnote has no `visible` flag.
        Node::Footnote(_) => None,
        Node::Unknown(_) => None,
    }
}

/// The declared `w` of a node in pixels, if the node kind carries a `w`/`h`
/// box and the dimension resolves to pixels. Geometry-less kinds (line,
/// polygon, polyline, unknown) yield `None`.
fn node_declared_w(node: &Node) -> Option<f64> {
    let w = match node {
        Node::Rect(n) => n.w.as_ref(),
        Node::Ellipse(n) => n.w.as_ref(),
        Node::Text(n) => n.w.as_ref(),
        Node::Code(n) => n.w.as_ref(),
        Node::Frame(n) => n.w.as_ref(),
        Node::Group(n) => n.w.as_ref(),
        Node::Image(n) => n.w.as_ref(),
        Node::Field(n) => n.w.as_ref(),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => None,
    }?;
    dim_to_px(w.value, &w.unit)
}

/// The declared `h` of a node in pixels, if the node kind carries a `w`/`h`
/// box and the dimension resolves to pixels. Geometry-less kinds yield `None`.
fn node_declared_h(node: &Node) -> Option<f64> {
    let h = match node {
        Node::Rect(n) => n.h.as_ref(),
        Node::Ellipse(n) => n.h.as_ref(),
        Node::Text(n) => n.h.as_ref(),
        Node::Code(n) => n.h.as_ref(),
        Node::Frame(n) => n.h.as_ref(),
        Node::Group(n) => n.h.as_ref(),
        Node::Image(n) => n.h.as_ref(),
        Node::Field(n) => n.h.as_ref(),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => None,
    }?;
    dim_to_px(h.value, &h.unit)
}

/// Clone `node` and overwrite its `x`/`y`/`w`/`h` box with the injected
/// flow coordinates (all in absolute page px). `h` is set only when the flow
/// path resolved one (declared height); a `None` `h` leaves the clone's `h`
/// unset so the child auto-measures its own intrinsic height.
///
/// Kinds without an `x`/`y`/`w`/`h` box (`Line`/`Polygon`/`Polyline`/
/// `Unknown`) are returned unchanged — the flow path advances its cursor by
/// `0.0` for those.
fn with_flow_box(node: &Node, x: f64, y: f64, w: f64, h: Option<f64>) -> Node {
    let px = |v: f64| {
        Some(Dimension {
            value: v,
            unit: Unit::Px,
        })
    };
    let h_dim = h.and_then(px);

    let mut out = node.clone();
    match &mut out {
        Node::Rect(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Ellipse(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Text(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Code(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Image(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Frame(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Group(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Field(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        // Geometry-less kinds: no x/y/w/h box to inject. (An instance carries
        // only an x/y origin, no w/h box, so flow layout does not reposition it
        // — it renders at its authored origin and advances the cursor by 0.)
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => {}
    }
    out
}

// NOTE: compile_group → compile_node → compile_group recursion has no depth
// guard.  Pathologically deep group trees can overflow the stack.  This is a
// known v0 limitation; a guard will be added when nested documents are tested.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_group(
    group: &GroupNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) {
    // Entire subtree excluded when visible=false.
    if group.visible == Some(false) {
        return;
    }

    // Cascade opacity: multiply the group's own opacity into the inherited ctx.
    let group_opacity = group.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let child_opacity = ctx.opacity * group_opacity;

    // Resolve group x/y to pixels; absent or unsupported-unit → 0.0 (no diagnostic).
    let group_x_px = group
        .x
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .unwrap_or(0.0);
    let group_y_px = group
        .y
        .as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .unwrap_or(0.0);

    let child_dx = ctx.dx + group_x_px;
    let child_dy = ctx.dy + group_y_px;

    // Rotation bracket — outermost, wrapping all child commands.
    // Determine the pivot center:
    //   1. If the group has BOTH w and h declared → use the declared box center.
    //   2. Otherwise → compute the union bbox of direct children in device space.
    //   3. If neither yields a center (empty / geometry-less group) → skip
    //      the bracket entirely (v0 limitation, commented below).
    let group_rot = rotation_degrees(group.rotate.as_ref());
    let rot_center: Option<(f64, f64)> = if group_rot.is_some() {
        // Try declared box first.
        let declared = group
            .w
            .as_ref()
            .and_then(|wd| dim_to_px(wd.value, &wd.unit))
            .zip(
                group
                    .h
                    .as_ref()
                    .and_then(|hd| dim_to_px(hd.value, &hd.unit)),
            )
            .map(|(gw, gh)| (child_dx + gw / 2.0, child_dy + gh / 2.0));
        if declared.is_some() {
            declared
        } else {
            // Fall back to union bbox of direct children in device space.
            // v0 limitation: if the group is empty or contains only
            // geometry-less nodes no center is computable → rotation is
            // silently skipped.
            group_children_center(&group.children, child_dx, child_dy)
        }
    } else {
        None
    };

    if let (Some(angle), Some((cx, cy))) = (group_rot, rot_center) {
        commands.push(SceneCommand::PushTransform {
            angle_deg: angle,
            cx,
            cy,
        });
    }

    // Emit children in source order; the group itself produces no command.
    let child_ctx = RenderCtx {
        opacity: child_opacity,
        dx: child_dx,
        dy: child_dy,
        // Page baseline grid cascades unchanged so all text shares one grid.
        baseline_grid: ctx.baseline_grid,
    };
    for child in &group.children {
        compile_node(
            child,
            resolved,
            style_map,
            components,
            fonts,
            engine,
            commands,
            diagnostics,
            chains,
            field_ctx,
            child_ctx,
        );
    }

    if group_rot.is_some() && rot_center.is_some() {
        commands.push(SceneCommand::PopTransform);
    }
}

// ── Instance ────────────────────────────────────────────────────────────────────

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
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_instance(
    instance: &InstanceNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    components: &ComponentMap,
    fonts: &dyn FontProvider,
    engine: &RustybuzzEngine,
    commands: &mut Vec<SceneCommand>,
    diagnostics: &mut Vec<Diagnostic>,
    chains: &ChainAssignments,
    field_ctx: &FieldCtx,
    ctx: RenderCtx,
) {
    // Entire expansion excluded when visible=false (mirror group/frame).
    if instance.visible == Some(false) {
        return;
    }

    let Some(component) = components.get(instance.component.as_str()) else {
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
        style: None,
        children,
        source_span: instance.source_span,
        unknown_props: BTreeMap::new(),
    };

    compile_group(
        &synthetic,
        resolved,
        style_map,
        components,
        fonts,
        engine,
        commands,
        diagnostics,
        chains,
        field_ctx,
        ctx,
    );
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
            _ => None,
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
        Node::Footnote(n) => n.fill = Some(fill),
        Node::Line(_)
        | Node::Frame(_)
        | Node::Group(_)
        | Node::Image(_)
        | Node::Instance(_)
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
        Node::Footnote(n) => Some(&n.id),
        Node::Unknown(_) => None,
    }
}

/// Recursively prepend `prefix` to the id of every id-bearing node in
/// `children`, descending into `group`/`frame` containers (and prefixing nested
/// instance ids too). Mirrors the suffix walk used by `duplicate_page` in
/// zenith-tx (an in-order recursion, deterministic, no HashMap), but applied as
/// a PREFIX with the instance id so two instances of one component never collide.
pub(super) fn prefix_ids_in_children(children: &mut [Node], prefix: &str) {
    for child in children.iter_mut() {
        prefix_node_id(child, prefix);
        match child {
            Node::Frame(f) => prefix_ids_in_children(&mut f.children, prefix),
            Node::Group(g) => prefix_ids_in_children(&mut g.children, prefix),
            _ => {}
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
        Node::Footnote(n) => pre!(n.id),
        Node::Unknown(_) => {}
    }
}

/// Compute the axis-aligned bounding box of a `Point` list in authored coords.
///
/// Returns `(x_min, y_min, w, h)` in authored (pre-`base_dx`/`base_dy`) space,
/// or `None` when the list is empty or every point has a missing / unsupported-
/// unit coordinate.  Used by both the `Polygon` and `Polyline` arms of
/// [`group_children_center`] to avoid duplicating the accumulation loop.
fn points_bbox(pts: &[Point]) -> Option<(f64, f64, f64, f64)> {
    let mut px_min = f64::INFINITY;
    let mut py_min = f64::INFINITY;
    let mut px_max = f64::NEG_INFINITY;
    let mut py_max = f64::NEG_INFINITY;
    for pt in pts {
        let (Some(xd), Some(yd)) = (&pt.x, &pt.y) else {
            continue;
        };
        let (Some(px), Some(py)) = (dim_to_px(xd.value, &xd.unit), dim_to_px(yd.value, &yd.unit))
        else {
            continue;
        };
        px_min = px_min.min(px);
        py_min = py_min.min(py);
        px_max = px_max.max(px);
        py_max = py_max.max(py);
    }
    if px_min.is_finite() {
        Some((px_min, py_min, px_max - px_min, py_max - py_min))
    } else {
        None
    }
}

/// Compute the device-space center of a group's direct-child union bounding box.
///
/// `base_dx`/`base_dy` are the device-space origin of the group (i.e.
/// `ctx.dx + group_x_px` and `ctx.dy + group_y_px`).  Children are positioned
/// relative to those origins.
///
/// Returns `None` when no child yields a computable bbox (e.g. an empty group
/// or one containing only unknown/geometry-less nodes).
fn group_children_center(children: &[Node], base_dx: f64, base_dy: f64) -> Option<(f64, f64)> {
    // Accumulate min/max in device space.
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for child in children {
        // Helper: expand accumulated bounds by a device-space rect.
        macro_rules! expand {
            ($x:expr, $y:expr, $w:expr, $h:expr) => {
                if $w > 0.0 || $h > 0.0 {
                    min_x = min_x.min($x);
                    min_y = min_y.min($y);
                    max_x = max_x.max($x + $w);
                    max_y = max_y.max($y + $h);
                }
            };
        }

        match child {
            Node::Rect(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Ellipse(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Text(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Code(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Image(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Frame(n) => {
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            Node::Line(n) => {
                // Line bbox is the degenerate rect spanning (x1,y1)-(x2,y2).
                let (Some(x1d), Some(y1d), Some(x2d), Some(y2d)) = (&n.x1, &n.y1, &n.x2, &n.y2)
                else {
                    continue;
                };
                let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    dim_to_px(x1d.value, &x1d.unit),
                    dim_to_px(y1d.value, &y1d.unit),
                    dim_to_px(x2d.value, &x2d.unit),
                    dim_to_px(y2d.value, &y2d.unit),
                ) else {
                    continue;
                };
                let lx = x1.min(x2);
                let ly = y1.min(y2);
                let lw = (x2 - x1).abs();
                let lh = (y2 - y1).abs();
                expand!(base_dx + lx, base_dy + ly, lw, lh);
            }
            Node::Polygon(n) => {
                if let Some((x, y, w, h)) = points_bbox(&n.points) {
                    expand!(base_dx + x, base_dy + y, w, h);
                }
            }
            Node::Polyline(n) => {
                if let Some((x, y, w, h)) = points_bbox(&n.points) {
                    expand!(base_dx + x, base_dy + y, w, h);
                }
            }
            Node::Group(n) => {
                // Nested group: use its declared w/h if available, else skip.
                let (Some(xd), Some(yd), Some(wd), Some(hd)) = (&n.x, &n.y, &n.w, &n.h) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (
                    dim_to_px(xd.value, &xd.unit),
                    dim_to_px(yd.value, &yd.unit),
                    dim_to_px(wd.value, &wd.unit),
                    dim_to_px(hd.value, &hd.unit),
                ) else {
                    continue;
                };
                expand!(base_dx + x, base_dy + y, w, h);
            }
            // Instances have no authoritative bbox (their expanded subtree is
            // the geometry); a field's box is resolved at projection time, not
            // here; unknown nodes have no geometry — skip all three.
            // A footnote has no authored bbox (it renders in the footnote zone).
            Node::Instance(_) | Node::Field(_) | Node::Footnote(_) | Node::Unknown(_) => {}
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(((min_x + max_x) / 2.0, (min_y + max_y) / 2.0))
    } else {
        None
    }
}
