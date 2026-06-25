//! Per-kind check for the `chart` node.

use std::collections::BTreeSet;

use crate::ast::node::ChartNode;
use crate::diagnostics::Diagnostic;

use super::shared::{
    AnchorParentCtx, AnchorProps, VisualProps, check_anchor, check_optional_dim, check_style_ref,
    check_visual_props,
};
use super::suggest::check_unknown_props;
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;

pub(in crate::validate::check) fn check_chart(
    c: &ChartNode,
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
    // The chart's own id participates in id-uniqueness. The series children are
    // pure DATA and are intentionally NOT visited here.
    register_id(&c.id, seen_ids, diagnostics);
    check_style_ref(
        &c.id,
        c.style.as_deref(),
        declared_style_ids,
        c.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y; the chart IS anchor-bearing.
    let anchor_active = check_anchor(
        &c.id,
        AnchorProps {
            anchor: c.anchor.as_deref(),
            anchor_zone: c.anchor_zone.as_deref(),
            anchor_sibling: c.anchor_sibling.as_deref(),
            anchor_parent: c.anchor_parent == Some(true),
            anchor_edge: c.anchor_edge.as_deref(),
            anchor_gap: c.anchor_gap.as_ref(),
        },
        parent_ctx,
        zone_ids,
        c.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    check_optional_dim(
        &c.id,
        "x",
        c.x.as_ref(),
        xy_required,
        c.source_span,
        diagnostics,
    );
    check_optional_dim(
        &c.id,
        "y",
        c.y.as_ref(),
        xy_required,
        c.source_span,
        diagnostics,
    );
    check_optional_dim(
        &c.id,
        "w",
        c.w.as_ref(),
        geom_required,
        c.source_span,
        diagnostics,
    );
    check_optional_dim(
        &c.id,
        "h",
        c.h.as_ref(),
        geom_required,
        c.source_span,
        diagnostics,
    );

    // Visual properties — token refs collected for token-usage checks, and the
    // shared per-corner-radius / stroke-dash guards. This mirrors the complete
    // set that check_pattern collects.
    let props = VisualProps {
        fill: c.fill.as_ref(),
        stroke: c.stroke.as_ref(),
        stroke_width: c.stroke_width.as_ref(),
        stroke_dash: c.stroke_dash.as_ref(),
        stroke_gap: c.stroke_gap.as_ref(),
        stroke_linecap: c.stroke_linecap.as_deref(),
        border_top: c.border_top.as_ref(),
        border_bottom: c.border_bottom.as_ref(),
        border_left: c.border_left.as_ref(),
        border_right: c.border_right.as_ref(),
        stroke_outer: c.stroke_outer.as_ref(),
        border_width: c.border_width.as_ref(),
        stroke_outer_width: c.stroke_outer_width.as_ref(),
        blend_mode: c.blend_mode.as_deref(),
        radius: c.radius.as_ref(),
        radius_tl: c.radius_tl.as_ref(),
        radius_tr: c.radius_tr.as_ref(),
        radius_br: c.radius_br.as_ref(),
        radius_bl: c.radius_bl.as_ref(),
        shadow: c.shadow.as_ref(),
        filter: c.filter.as_ref(),
        mask: c.mask.as_ref(),
        blur: c.blur.as_ref(),
    };
    check_visual_props(
        "chart",
        &c.id,
        c.source_span,
        props,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Chart-specific semantic checks.
    //
    // The renderer recognizes "bar", "line", "area", "sparkline", "pie", and
    // "donut"; any other kind string cannot render and is reported immediately.
    let kind_known = matches!(
        c.kind.as_str(),
        "bar" | "line" | "area" | "sparkline" | "pie" | "donut"
    );
    if !kind_known {
        diagnostics.push(Diagnostic::error(
            "chart.invalid_kind",
            format!(
                "chart '{}': kind '{}' is not recognized; \
                 expected \"bar\", \"line\", \"area\", \"sparkline\", \"pie\", or \"donut\"",
                c.id, c.kind
            ),
            c.source_span,
            Some(c.id.clone()),
        ));
    }

    // Validate bar-mode against the recognized set {"grouped", "stacked"}.
    // Unknown values are a Warning (governable) — mirrors kind's validation style
    // but Advisory→Warning because the value is semantically meaningful at render
    // time and a typo would silently fall back to default.
    if let Some(bar_mode) = &c.bar_mode {
        let bar_mode_known = matches!(bar_mode.as_str(), "grouped" | "stacked");
        if !bar_mode_known {
            diagnostics.push(Diagnostic::warning(
                "chart.invalid_bar_mode",
                format!(
                    "chart '{}': bar-mode '{}' is not recognized; \
                     expected \"grouped\" or \"stacked\"",
                    c.id, bar_mode
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }

    // Validate point-placement against the recognized set {"edge", "center"}.
    if let Some(point_placement) = &c.point_placement {
        let point_placement_known = matches!(point_placement.as_str(), "edge" | "center");
        if !point_placement_known {
            diagnostics.push(Diagnostic::warning(
                "chart.invalid_point_placement",
                format!(
                    "chart '{}': point-placement '{}' is not recognized; \
                     expected \"edge\" or \"center\"",
                    c.id, point_placement
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }

    // Validate value-labels against the recognized set {"auto", "none", "top", "center"}.
    if let Some(value_labels) = &c.value_labels {
        let value_labels_known =
            matches!(value_labels.as_str(), "auto" | "none" | "top" | "center");
        if !value_labels_known {
            diagnostics.push(Diagnostic::warning(
                "chart.invalid_value_labels",
                format!(
                    "chart '{}': value-labels '{}' is not recognized; \
                     expected \"auto\", \"none\", \"top\", or \"center\"",
                    c.id, value_labels
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }

    // Validate categories count vs. series data length.
    // Emitted as Advisory (governable) when categories is non-empty and its count
    // does not match the maximum series value count.
    if !c.categories.is_empty() {
        let max_series_len = c.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
        if c.categories.len() != max_series_len {
            diagnostics.push(Diagnostic::advisory(
                "chart.category_count_mismatch",
                format!(
                    "chart '{}': {} category labels but {} data points per series",
                    c.id,
                    c.categories.len(),
                    max_series_len,
                ),
                c.source_span,
                Some(c.id.clone()),
            ));
        }
    }

    // Series color token refs — series are pure data but their color props are
    // PropertyValue token refs that must be counted as used.
    for s in &c.series {
        if let Some(crate::ast::value::PropertyValue::TokenRef(token_id)) = &s.color {
            referenced_token_ids.insert(token_id.clone());
        }
        // Per-series label-color token ref.
        if let Some(crate::ast::value::PropertyValue::TokenRef(token_id)) = &s.label_color {
            referenced_token_ids.insert(token_id.clone());
        }
    }
    // value-color token ref — collect even though the series children are not walked.
    if let Some(crate::ast::value::PropertyValue::TokenRef(token_id)) = &c.value_color {
        referenced_token_ids.insert(token_id.clone());
    }
    // label-colors token refs — per-slice color refs from the label-colors child node.
    for pv in &c.label_colors {
        if let crate::ast::value::PropertyValue::TokenRef(token_id) = pv {
            referenced_token_ids.insert(token_id.clone());
        }
    }

    // Unknown properties.
    check_unknown_props("chart", &c.id, &c.unknown_props, c.source_span, diagnostics);
}
