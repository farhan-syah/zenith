//! Per-kind checks for `shape`, `connector`, and `unknown` nodes.
//!
//! `check_unknown` emits the forward-compat warning and registers the optional
//! id; the dispatcher in [`super::super::nodes::walk_node`] performs the child
//! recursion so traversal order is unchanged.

use std::collections::HashSet;

use crate::ast::node::{ConnectorNode, ShapeNode, UnknownNode};
use crate::diagnostics::Diagnostic;

use super::shared::{check_anchor, check_optional_dim, check_spans, check_style_ref};
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

pub(in crate::validate::check) fn check_shape(
    s: &ShapeNode,
    ctx: WalkCtx,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    geom_required: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&s.id, seen_ids, diagnostics);
    check_style_ref(
        &s.id,
        s.style.as_deref(),
        declared_style_ids,
        s.source_span,
        diagnostics,
    );
    check_style_ref(
        &s.id,
        s.text_style.as_deref(),
        declared_style_ids,
        s.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y.
    let anchor_active = check_anchor(
        &s.id,
        s.anchor.as_deref(),
        s.anchor_zone.as_deref(),
        zone_ids,
        s.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    // Required geometry: x, y, w, h must all be present.
    check_optional_dim(
        &s.id,
        "x",
        s.x.as_ref(),
        xy_required,
        s.source_span,
        diagnostics,
    );
    check_optional_dim(
        &s.id,
        "y",
        s.y.as_ref(),
        xy_required,
        s.source_span,
        diagnostics,
    );
    check_optional_dim(
        &s.id,
        "w",
        s.w.as_ref(),
        geom_required,
        s.source_span,
        diagnostics,
    );
    check_optional_dim(
        &s.id,
        "h",
        s.h.as_ref(),
        geom_required,
        s.source_span,
        diagnostics,
    );

    // Visual properties — all token-required.
    check_visual_prop(
        &s.id,
        "fill",
        s.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &s.id,
        "stroke",
        s.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &s.id,
        "stroke-width",
        s.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &s.id,
        "radius",
        s.radius.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &s.id,
        "padding",
        s.padding.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Enum-value checks (Warnings on unrecognized values, not errors).
    if let Some(k) = s.kind.as_deref()
        && !matches!(k, "process" | "decision" | "terminator" | "ellipse")
    {
        diagnostics.push(Diagnostic::warning(
            "shape.unknown_kind",
            format!(
                "shape '{}': kind '{k}' is not one of \
                 process/decision/terminator/ellipse",
                s.id
            ),
            s.source_span,
            Some(s.id.clone()),
        ));
    }
    if let Some(sa) = s.stroke_alignment.as_deref()
        && !matches!(sa, "inside" | "center" | "outside")
    {
        diagnostics.push(Diagnostic::warning(
            "shape.invalid_stroke_alignment",
            format!(
                "shape '{}': stroke-alignment '{sa}' is not one of \
                 inside/center/outside",
                s.id
            ),
            s.source_span,
            Some(s.id.clone()),
        ));
    }
    if let Some(ha) = s.h_align.as_deref()
        && !matches!(ha, "start" | "center" | "end")
    {
        diagnostics.push(Diagnostic::warning(
            "shape.invalid_h_align",
            format!(
                "shape '{}': h-align '{ha}' is not one of start/center/end",
                s.id
            ),
            s.source_span,
            Some(s.id.clone()),
        ));
    }
    if let Some(va) = s.v_align.as_deref()
        && !matches!(va, "top" | "middle" | "bottom")
    {
        diagnostics.push(Diagnostic::warning(
            "shape.invalid_v_align",
            format!(
                "shape '{}': v-align '{va}' is not one of top/middle/bottom",
                s.id
            ),
            s.source_span,
            Some(s.id.clone()),
        ));
    }

    // Per-span visual properties (mirrors Node::Text). Registers token
    // refs so they are not falsely flagged as unused, and type-checks
    // fill/font-weight on each span.
    check_spans(
        &s.id,
        &s.spans,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Unknown properties.
    for prop_name in s.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "shape '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                s.id, prop_name
            ),
            s.source_span,
            Some(s.id.clone()),
        ));
    }
}

pub(in crate::validate::check) fn check_connector(
    c: &ConnectorNode,
    ctx: WalkCtx,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        all_node_ids,
        ..
    } = ctx;
    register_id(&c.id, seen_ids, diagnostics);
    check_style_ref(
        &c.id,
        c.style.as_deref(),
        declared_style_ids,
        c.source_span,
        diagnostics,
    );

    // Stroke visual properties — token-required (connector has no fill).
    check_visual_prop(
        &c.id,
        "stroke",
        c.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &c.id,
        "stroke-width",
        c.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Target existence: `from`/`to`, when present, must name an id in the
    // document. A reference to a missing id is advisory (the connector
    // simply renders nothing at compile time). Mirrors
    // `text-exclusion.unresolved_ref`.
    if let Some(target) = &c.from
        && !all_node_ids.contains(target)
    {
        diagnostics.push(Diagnostic::warning(
            "connector.unknown_target",
            format!(
                "connector '{}': from '{}' matches no node id in the document",
                c.id, target
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }
    if let Some(target) = &c.to
        && !all_node_ids.contains(target)
    {
        diagnostics.push(Diagnostic::warning(
            "connector.unknown_target",
            format!(
                "connector '{}': to '{}' matches no node id in the document",
                c.id, target
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }

    // Missing endpoints: a connector with no `from`/`to` can't route.
    if c.from.is_none() || c.to.is_none() {
        diagnostics.push(Diagnostic::warning(
            "connector.missing_target",
            format!(
                "connector '{}': both 'from' and 'to' are required to route",
                c.id
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }

    // Enum-value checks (Warnings on unrecognized values, not errors).
    if let Some(r) = c.route.as_deref()
        && !matches!(r, "straight" | "orthogonal")
    {
        diagnostics.push(Diagnostic::warning(
            "connector.invalid_route",
            format!(
                "connector '{}': route '{r}' is not one of straight/orthogonal",
                c.id
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }
    for (label, marker) in [
        ("marker-start", c.marker_start.as_deref()),
        ("marker-end", c.marker_end.as_deref()),
    ] {
        if let Some(m) = marker
            && !matches!(m, "none" | "arrow")
        {
            diagnostics.push(Diagnostic::warning(
                "connector.invalid_marker",
                format!(
                    "connector '{}': {label} '{m}' is not one of none/arrow",
                    c.id
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }
    for (label, anchor) in [
        ("from-anchor", c.from_anchor.as_deref()),
        ("to-anchor", c.to_anchor.as_deref()),
    ] {
        if let Some(a) = anchor
            && !matches!(a, "top" | "bottom" | "left" | "right" | "center" | "auto")
        {
            diagnostics.push(Diagnostic::warning(
                "connector.invalid_anchor",
                format!(
                    "connector '{}': {label} '{a}' is not one of \
                     top/bottom/left/right/center/auto",
                    c.id
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }

    // Unknown properties.
    for prop_name in c.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "connector '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                c.id, prop_name
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }
}

/// Emit the forward-compat `node.unknown_kind` warning and register the
/// optional id. The child recursion stays in the dispatcher so the unknown
/// node's children are walked in the same position as before.
pub(in crate::validate::check) fn check_unknown(
    u: &UnknownNode,
    seen_ids: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic::warning(
        "node.unknown_kind",
        format!(
            "unknown node kind '{}' (forward-compatibility; \
             this kind may be valid in a later schema version)",
            u.kind
        ),
        u.source_span,
        u.id.clone(),
    ));
    // Register the id (if any) so the unknown node is addressable and
    // participates in duplicate-id detection alongside known nodes.
    if let Some(id) = &u.id {
        register_id(id, seen_ids, diagnostics);
    }
}
