//! Per-kind checks for the "special" leaf nodes that were already extracted as
//! helpers: `polygon`, `polyline`, `instance`, `field`, `toc`, and `footnote`.
//! None of these recurse into laid-out children at this site.

use std::collections::BTreeSet;

use crate::ast::node::{FieldNode, FootnoteNode, InstanceNode, PolygonNode, PolylineNode, TocNode};
use crate::diagnostics::Diagnostic;

use super::shared::{
    AnchorParentCtx, AnchorProps, check_anchor, check_optional_dim, check_spans, check_style_ref,
};
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

// ── polygon / polyline validation ─────────────────────────────────────────────

pub(in crate::validate::check) fn check_polygon(
    poly: &PolygonNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        ..
    } = ctx;
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y (both must be present with a known unit).
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
    }

    // polygon requires at least 3 points.
    if poly.points.len() < 3 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polygon '{}': requires at least 3 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // stroke-alignment: only "inside", "center", "outside" are valid.
    if let Some(sa) = &poly.stroke_alignment
        && !matches!(sa.as_str(), "inside" | "center" | "outside")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unrecognized stroke-alignment '{}' (version-relative; \
                 allowed values are inside, center, outside)",
                poly.id, sa
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polygon '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polygon is a LEAF: no child-node recursion (points are sub-data).
}

pub(in crate::validate::check) fn check_polyline(
    poly: &PolylineNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        ..
    } = ctx;
    register_id(&poly.id, seen_ids, diagnostics);
    check_style_ref(
        &poly.id,
        poly.style.as_deref(),
        declared_style_ids,
        poly.source_span,
        diagnostics,
    );

    // Validate each point's x and y.
    for (idx, pt) in poly.points.iter().enumerate() {
        let x_label = format!("point[{idx}].x");
        let y_label = format!("point[{idx}].y");
        check_optional_dim(
            &poly.id,
            &x_label,
            pt.x.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
        check_optional_dim(
            &poly.id,
            &y_label,
            pt.y.as_ref(),
            true,
            poly.source_span,
            diagnostics,
        );
    }

    // polyline requires at least 2 points.
    if poly.points.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "shape.insufficient_points",
            format!(
                "polyline '{}': requires at least 2 points, got {}",
                poly.id,
                poly.points.len()
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Visual properties.
    check_visual_prop(
        &poly.id,
        "fill",
        poly.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke",
        poly.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &poly.id,
        "stroke-width",
        poly.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // fill-rule: only "nonzero" and "evenodd" are valid.
    if let Some(fr) = &poly.fill_rule
        && !matches!(fr.as_str(), "nonzero" | "evenodd")
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unrecognized fill-rule '{}' (version-relative; \
                 allowed values are nonzero, evenodd)",
                poly.id, fr
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in poly.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "polyline '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                poly.id, prop_name
            ),
            poly.source_span,
            Some(poly.id.clone()),
        ));
    }
    // polyline is a LEAF: no child-node recursion (points are sub-data).
}

// ── instance validation ───────────────────────────────────────────────────────

/// Validate an `instance` node:
/// - its own `id` participates in GLOBAL uniqueness;
/// - `component` must reference a declared component → else
///   `component.unknown_reference` (Error);
/// - each override `ref` must match a LOCAL descendant id of the referenced
///   component → else `component.unknown_override_target` (Warning).
///
/// The instance is a container-ish node but it does NOT recurse here: its
/// expanded subtree (and the component definition's own ids) are validated at
/// the component definition site, not per-instance, so token/asset refs are
/// checked once.
pub(in crate::validate::check) fn check_instance(
    inst: &InstanceNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_component_ids,
        component_local_ids,
        ..
    } = ctx;
    register_id(&inst.id, seen_ids, diagnostics);

    let component_known = declared_component_ids.contains(&inst.component);
    if !component_known {
        diagnostics.push(Diagnostic::error(
            "component.unknown_reference",
            format!(
                "instance '{}': references component '{}' which is not declared in the \
                 components block",
                inst.id, inst.component
            ),
            inst.source_span,
            Some(inst.id.clone()),
        ));
    }

    // Override targets are only checkable when the component is known. Look up the
    // referenced component's local-id set; an override `ref` that matches no local
    // descendant id → warning.
    let local_ids = component_local_ids.get(&inst.component);
    for ov in &inst.overrides {
        // Validate (and register as referenced) any token refs the override
        // carries, so an override-only token is not falsely flagged unused and
        // a bad override fill/span fill is type-checked like a node fill.
        check_visual_prop(
            &inst.id,
            "fill",
            ov.fill.as_ref(),
            VisualExpect::Color,
            referenced_token_ids,
            resolved_tokens,
            diagnostics,
        );
        if let Some(spans) = &ov.spans {
            for span in spans {
                check_visual_prop(
                    &inst.id,
                    "fill",
                    span.fill.as_ref(),
                    VisualExpect::Color,
                    referenced_token_ids,
                    resolved_tokens,
                    diagnostics,
                );
            }
        }

        let target_known = local_ids
            .map(|ids| ids.contains(&ov.ref_id))
            .unwrap_or(false);
        if component_known && !target_known {
            diagnostics.push(Diagnostic::warning(
                "component.unknown_override_target",
                format!(
                    "instance '{}': override ref '{}' matches no descendant id in component '{}'",
                    inst.id, ov.ref_id, inst.component
                ),
                ov.source_span.or(inst.source_span),
                Some(inst.id.clone()),
            ));
        }
    }

    // Unknown properties on the instance node.
    for prop_name in inst.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "instance '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                inst.id, prop_name
            ),
            inst.source_span,
            Some(inst.id.clone()),
        ));
    }
}

// ── field validation ──────────────────────────────────────────────────────────

/// The known v0 field types.
const KNOWN_FIELD_TYPES: &[&str] = &[
    "running-head",
    "page-number",
    "page-ref",
    "page-count",
    "section-page-number",
    "section-page-count",
    "section-name",
];

/// Validate a `field` node:
/// - its own `id` participates in GLOBAL uniqueness;
/// - `type` must be one of the known field types → else `field.unknown_type`
///   (Warning);
/// - a `page-ref` field whose `target` matches no node id anywhere in the
///   document → `field.unresolved_ref` (Warning);
/// - `style`/`fill`/`font-family`/`font-size` are validated like a text node's,
///   and any token refs are registered so they are not flagged unused.
///
/// A field is a leaf — it does not recurse. Geometry is optional (an absent
/// x/w defaults to the page live area at compile time), so no missing-geometry
/// error is raised here.
pub(in crate::validate::check) fn check_field(
    field: &FieldNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        all_node_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&field.id, seen_ids, diagnostics);
    check_style_ref(
        &field.id,
        field.style.as_deref(),
        declared_style_ids,
        field.source_span,
        diagnostics,
    );

    // Validate the anchor value (geometry is all-optional for fields anyway).
    check_anchor(
        &field.id,
        AnchorProps {
            anchor: field.anchor.as_deref(),
            anchor_zone: field.anchor_zone.as_deref(),
            anchor_sibling: field.anchor_sibling.as_deref(),
            anchor_parent: field.anchor_parent == Some(true),
        },
        parent_ctx,
        zone_ids,
        field.source_span,
        diagnostics,
    );

    // Unknown field type → Warning (never a hard error; the field simply renders
    // nothing at compile time).
    if !KNOWN_FIELD_TYPES.contains(&field.field_type.as_str()) {
        diagnostics.push(Diagnostic::warning(
            "field.unknown_type",
            format!(
                "field '{}': unknown type '{}'; expected one of {}",
                field.id,
                field.field_type,
                KNOWN_FIELD_TYPES.join(", ")
            ),
            field.source_span,
            Some(field.id.clone()),
        ));
    }

    // A page-ref field with an unresolvable target → Warning. A page-ref with no
    // target at all is also unresolved (nothing to point at).
    if field.field_type == "page-ref" {
        let resolved = field
            .target
            .as_ref()
            .map(|t| all_node_ids.contains(t))
            .unwrap_or(false);
        if !resolved {
            diagnostics.push(Diagnostic::warning(
                "field.unresolved_ref",
                format!(
                    "field '{}': page-ref target {} matches no node id in the document",
                    field.id,
                    field
                        .target
                        .as_deref()
                        .map(|t| format!("'{t}'"))
                        .unwrap_or_else(|| "(absent)".to_owned())
                ),
                field.source_span,
                Some(field.id.clone()),
            ));
        }
    }

    // Visual properties (mirror the text-node checks).
    check_visual_prop(
        &field.id,
        "fill",
        field.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &field.id,
        "font-family",
        field.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &field.id,
        "font-size",
        field.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Unknown properties on the field node.
    for prop_name in field.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "field '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                field.id, prop_name
            ),
            field.source_span,
            Some(field.id.clone()),
        ));
    }
}

/// Validate a [`TocNode`]: id uniqueness, style ref, visual properties, and
/// the `toc.no_selector` advisory when both `match_role` and `match_style` are
/// absent (the toc would collect no entries at compile time).
pub(in crate::validate::check) fn check_toc(
    toc: &TocNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    parent_ctx: AnchorParentCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&toc.id, seen_ids, diagnostics);
    check_style_ref(
        &toc.id,
        toc.style.as_deref(),
        declared_style_ids,
        toc.source_span,
        diagnostics,
    );

    // Validate the anchor value (geometry is all-optional for toc anyway).
    check_anchor(
        &toc.id,
        AnchorProps {
            anchor: toc.anchor.as_deref(),
            anchor_zone: toc.anchor_zone.as_deref(),
            anchor_sibling: toc.anchor_sibling.as_deref(),
            anchor_parent: toc.anchor_parent == Some(true),
        },
        parent_ctx,
        zone_ids,
        toc.source_span,
        diagnostics,
    );

    // Warn when neither selector is set: the toc will collect no entries.
    if toc.match_role.is_none() && toc.match_style.is_none() {
        diagnostics.push(Diagnostic::warning(
            "toc.no_selector",
            format!(
                "toc '{}' has neither match-role nor match-style; it will collect no entries",
                toc.id
            ),
            toc.source_span,
            Some(toc.id.clone()),
        ));
    }

    // Visual properties (mirror the field-node checks).
    check_visual_prop(
        &toc.id,
        "fill",
        toc.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &toc.id,
        "font-family",
        toc.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &toc.id,
        "font-size",
        toc.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Unknown properties on the toc node.
    for prop_name in toc.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "toc '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                toc.id, prop_name
            ),
            toc.source_span,
            Some(toc.id.clone()),
        ));
    }
}

/// Validate a [`FootnoteNode`]: id uniqueness, style ref, the content-span and
/// node visual properties (fill/font-family/font-size, plus per-span fill/weight
/// so raw visual literals are surfaced like any text), and unknown properties.
///
/// The structural `footnote.unresolved_ref` check (a span `footnote-ref` that
/// names no footnote on the same page) is done at the PAGE level (it needs the
/// page's footnote-id set), not here. A footnote has no geometry, so there are
/// no geometry checks.
pub(in crate::validate::check) fn check_footnote(
    footnote: &FootnoteNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        ..
    } = ctx;
    register_id(&footnote.id, seen_ids, diagnostics);
    check_style_ref(
        &footnote.id,
        footnote.style.as_deref(),
        declared_style_ids,
        footnote.source_span,
        diagnostics,
    );

    check_visual_prop(
        &footnote.id,
        "fill",
        footnote.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &footnote.id,
        "font-family",
        footnote.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &footnote.id,
        "font-size",
        footnote.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Per-span visual props (mirror the text-node span checks) so token refs are
    // registered and raw visual literals are flagged `token.raw_visual_literal`.
    check_spans(
        &footnote.id,
        &footnote.spans,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    for prop_name in footnote.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "footnote '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                footnote.id, prop_name
            ),
            footnote.source_span,
            Some(footnote.id.clone()),
        ));
    }
}
