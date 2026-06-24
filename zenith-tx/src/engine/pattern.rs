//! Pattern op application: [`apply_detach_pattern`].
//!
//! Materializes a `pattern` node into an editable `group` of native shapes by
//! replacing the pattern in place with a group that carries the pattern's
//! bounds and one motif clone per instance position. The instance positions are
//! computed by [`pattern_positions`] — the SAME function the scene uses to place
//! live pattern instances — so the detached group renders identically.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, Dimension, Document, GroupNode, Node, PatternLayout, dim_to_px, pattern_positions,
};

use super::geometry::node_geometry_mut;
use super::structure::node_set_id_any;
use super::{find_node_any_mut, px, record_affected};

/// Resolve an optional [`Dimension`] to a positive pixel magnitude.
///
/// Returns `None` when the dimension is absent, does not resolve to px (e.g. a
/// percentage or degree unit), or is not finite and `> 0`.
fn resolve_positive_px(dim: &Option<Dimension>) -> Option<f64> {
    dim.as_ref()
        .and_then(|d| dim_to_px(d.value, &d.unit))
        .filter(|&v| v.is_finite() && v > 0.0)
}

/// Replace the pattern node `node_id` with an editable group of native shapes.
///
/// Steps:
/// 1. Locate the node; reject `tx.unknown_node` if absent.
/// 2. Require it to be a pattern; reject `tx.not_a_pattern` otherwise.
/// 3. Resolve the pattern's `w`/`h` bounds to positive px; reject
///    `tx.pattern_unresolved_bounds` if either is missing or non-positive.
/// 4. Compute instance positions via [`pattern_positions`]; reject
///    `tx.pattern_not_expandable` if the layout yields no instances.
/// 5. Clone the motif once per position, assigning each clone an id of
///    `<node_id>.<index>` and setting its `x`/`y` to the instance offset.
/// 6. Build a group with the pattern's id and bounds carrying those clones and
///    overwrite the pattern node with it.
pub(super) fn apply_detach_pattern(
    node_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let Some(slot) = find_node_any_mut(doc, node_id) else {
        diagnostics.push(Diagnostic::error(
            "tx.unknown_node",
            format!("detach_pattern: node {node_id:?} not found in document"),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    };

    let Node::Pattern(p) = &*slot else {
        diagnostics.push(Diagnostic::error(
            "tx.not_a_pattern",
            format!("detach_pattern: node {node_id:?} is not a pattern"),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    };

    // Resolve bounds; both width and height are required and must be positive.
    let (Some(bw), Some(bh)) = (resolve_positive_px(&p.w), resolve_positive_px(&p.h)) else {
        diagnostics.push(Diagnostic::error(
            "tx.pattern_unresolved_bounds",
            format!(
                "detach_pattern: pattern {node_id:?} has unresolved or non-positive \
                 bounds; both w and h must resolve to a positive pixel size"
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    };

    let spacing = resolve_positive_px(&p.spacing);
    let seed = p.seed.unwrap_or(0);
    let jitter = p.jitter.unwrap_or(0.0);
    let count = p.count;

    let positions = pattern_positions(PatternLayout {
        kind: &p.kind,
        bounds_w: bw,
        bounds_h: bh,
        spacing,
        count,
        seed,
        jitter,
    });

    if positions.is_empty() {
        diagnostics.push(Diagnostic::error(
            "tx.pattern_not_expandable",
            format!(
                "detach_pattern: pattern {node_id:?} expands to no instances; its \
                 kind may be unknown or a required parameter is missing"
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    // Build one motif clone per instance position, in render order.
    let mut children: Vec<Node> = Vec::with_capacity(positions.len());
    for (i, (ox, oy)) in positions.iter().enumerate() {
        let mut child = (*p.motif).clone();
        node_set_id_any(&mut child, format!("{node_id}.{i}"));
        if let Some((cx, cy, _, _)) = node_geometry_mut(&mut child) {
            *cx = Some(px(*ox));
            *cy = Some(px(*oy));
        }
        children.push(child);
    }

    // The group carries the pattern's id and bounds; its x/y translation places
    // each child at `bounds_origin + offset`, the same place the scene renders
    // the corresponding live pattern instance. Visual props default to absent.
    let group = GroupNode {
        id: p.id.clone(),
        name: p.name.clone(),
        role: p.role.clone(),
        x: p.x.clone(),
        y: p.y.clone(),
        w: p.w.clone(),
        h: p.h.clone(),
        opacity: None,
        visible: None,
        locked: None,
        rotate: None,
        blend_mode: None,
        blur: None,
        style: None,
        semantic_role: None,
        intensity: None,
        layer_priority: None,
        children,
        anchor: None,
        anchor_zone: None,
        anchor_sibling: None,
        anchor_edge: None,
        anchor_gap: None,
        anchor_parent: None,
        source_span: None,
        unknown_props: BTreeMap::new(),
    };

    *slot = Node::Group(group);

    record_affected(node_id, affected);
}
