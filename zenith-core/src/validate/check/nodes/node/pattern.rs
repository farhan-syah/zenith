//! Per-kind check for the `pattern` node.

use std::collections::BTreeSet;

use crate::ast::node::PatternNode;
use crate::ast::value::dim_to_px;
use crate::diagnostics::Diagnostic;

use super::shared::{
    AnchorParentCtx, AnchorProps, VisualProps, check_anchor, check_optional_dim, check_style_ref,
    check_visual_props,
};
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;

pub(in crate::validate::check) fn check_pattern(
    p: &PatternNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    geom_required: bool,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    // The pattern's own id participates in id-uniqueness. The motif is a
    // TEMPLATE and is intentionally NOT visited here, so its id (if any) is
    // never collected.
    register_id(&p.id, seen_ids, diagnostics);
    check_style_ref(
        &p.id,
        p.style.as_deref(),
        declared_style_ids,
        p.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y; the pattern IS anchor-bearing.
    let anchor_active = check_anchor(
        &p.id,
        AnchorProps {
            anchor: p.anchor.as_deref(),
            anchor_zone: p.anchor_zone.as_deref(),
            anchor_sibling: p.anchor_sibling.as_deref(),
            anchor_parent: p.anchor_parent == Some(true),
            anchor_edge: p.anchor_edge.as_deref(),
            anchor_gap: p.anchor_gap.as_ref(),
        },
        parent_ctx,
        zone_ids,
        p.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    check_optional_dim(
        &p.id,
        "x",
        p.x.as_ref(),
        xy_required,
        p.source_span,
        diagnostics,
    );
    check_optional_dim(
        &p.id,
        "y",
        p.y.as_ref(),
        xy_required,
        p.source_span,
        diagnostics,
    );
    check_optional_dim(
        &p.id,
        "w",
        p.w.as_ref(),
        geom_required,
        p.source_span,
        diagnostics,
    );
    check_optional_dim(
        &p.id,
        "h",
        p.h.as_ref(),
        geom_required,
        p.source_span,
        diagnostics,
    );

    // Visual properties — token refs collected for token-usage checks, and the
    // shared per-corner-radius / stroke-dash guards. This mirrors the complete
    // set that check_rect collects so a token used only on a pattern's
    // border/stroke-outer/radius/blur props is counted as used; the pattern's
    // fill/stroke/radius paint its bounds background panel.
    let props = VisualProps {
        fill: p.fill.as_ref(),
        stroke: p.stroke.as_ref(),
        stroke_width: p.stroke_width.as_ref(),
        stroke_dash: p.stroke_dash.as_ref(),
        stroke_gap: p.stroke_gap.as_ref(),
        stroke_linecap: p.stroke_linecap.as_deref(),
        border_top: p.border_top.as_ref(),
        border_bottom: p.border_bottom.as_ref(),
        border_left: p.border_left.as_ref(),
        border_right: p.border_right.as_ref(),
        stroke_outer: p.stroke_outer.as_ref(),
        border_width: p.border_width.as_ref(),
        stroke_outer_width: p.stroke_outer_width.as_ref(),
        blend_mode: p.blend_mode.as_deref(),
        radius: p.radius.as_ref(),
        radius_tl: p.radius_tl.as_ref(),
        radius_tr: p.radius_tr.as_ref(),
        radius_br: p.radius_br.as_ref(),
        radius_bl: p.radius_bl.as_ref(),
        shadow: p.shadow.as_ref(),
        filter: p.filter.as_ref(),
        mask: p.mask.as_ref(),
        blur: p.blur.as_ref(),
    };
    check_visual_props(
        "pattern",
        &p.id,
        p.source_span,
        props,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Pattern-specific semantic checks.
    //
    // The expansion engine recognizes exactly "grid" and "scatter"; any other
    // kind string cannot render and is reported immediately. When the kind is
    // unknown we skip the kind-specific requirement checks to avoid noise.
    let kind_known = matches!(p.kind.as_str(), "grid" | "scatter");
    if !kind_known {
        diagnostics.push(Diagnostic::error(
            "pattern.unknown_kind",
            format!(
                "pattern '{}': kind '{}' is not recognized; expected \"grid\" or \"scatter\"",
                p.id, p.kind
            ),
            p.source_span,
            Some(p.id.clone()),
        ));
    }

    if kind_known {
        // Grid layout steps by spacing; scatter places `count` instances.
        if p.kind == "grid" && p.spacing.is_none() {
            diagnostics.push(Diagnostic::error(
                "pattern.grid_missing_spacing",
                format!("pattern '{}': kind \"grid\" requires a spacing value", p.id),
                p.source_span,
                Some(p.id.clone()),
            ));
        }
        if p.kind == "scatter" && p.count.is_none() {
            diagnostics.push(Diagnostic::error(
                "pattern.scatter_missing_count",
                format!(
                    "pattern '{}': kind \"scatter\" requires a count value",
                    p.id
                ),
                p.source_span,
                Some(p.id.clone()),
            ));
        }
    }

    // count <= 0 is invalid regardless of kind.
    if let Some(count) = p.count {
        if count <= 0 {
            diagnostics.push(Diagnostic::error(
                "pattern.invalid_count",
                format!("pattern '{}': count must be > 0, got {}", p.id, count),
                p.source_span,
                Some(p.id.clone()),
            ));
        }
    }

    // spacing <= 0 or not px-convertible is invalid regardless of kind.
    if let Some(spacing) = p.spacing.as_ref() {
        match dim_to_px(spacing.value, &spacing.unit) {
            None => {
                diagnostics.push(Diagnostic::error(
                    "pattern.invalid_spacing",
                    format!(
                        "pattern '{}': spacing has an unresolvable unit and cannot be used for layout",
                        p.id
                    ),
                    p.source_span,
                    Some(p.id.clone()),
                ));
            }
            Some(px) if px <= 0.0 => {
                diagnostics.push(Diagnostic::error(
                    "pattern.invalid_spacing",
                    format!("pattern '{}': spacing must be > 0, got {px}px", p.id),
                    p.source_span,
                    Some(p.id.clone()),
                ));
            }
            Some(_) => {}
        }
    }

    // jitter outside 0.0..=1.0 is a warning (the engine clamps, but the author
    // almost certainly made an error).
    if let Some(jitter) = p.jitter {
        if !(0.0..=1.0).contains(&jitter) {
            diagnostics.push(Diagnostic::warning(
                "pattern.jitter_out_of_range",
                format!(
                    "pattern '{}': jitter {jitter} is outside the valid range 0.0..=1.0",
                    p.id
                ),
                p.source_span,
                Some(p.id.clone()),
            ));
        }
    }

    // Unknown properties.
    for prop_name in p.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "pattern '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                p.id, prop_name
            ),
            p.source_span,
            Some(p.id.clone()),
        ));
    }
}
