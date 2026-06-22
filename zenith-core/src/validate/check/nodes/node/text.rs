//! Per-kind checks for `text` and `image` nodes.

use std::collections::HashSet;

use crate::ast::node::{ImageNode, TextNode};
use crate::ast::value::dim_to_px;
use crate::diagnostics::Diagnostic;

use super::shared::{
    check_anchor, check_optional_dim, check_spans, check_style_ref, is_valid_blend_mode,
};
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

pub(in crate::validate::check) fn check_text(
    t: &TextNode,
    ctx: WalkCtx,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    geom_required: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_style_ids,
        all_node_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&t.id, seen_ids, diagnostics);
    if let Some(bm) = t.blend_mode.as_deref()
        && !is_valid_blend_mode(bm)
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "text '{}': blend-mode '{bm}' is not a recognized value",
                t.id
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }
    check_style_ref(
        &t.id,
        t.style.as_deref(),
        declared_style_ids,
        t.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y.
    let anchor_active = check_anchor(
        &t.id,
        t.anchor.as_deref(),
        t.anchor_zone.as_deref(),
        zone_ids,
        t.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    // Required geometry.
    check_optional_dim(
        &t.id,
        "x",
        t.x.as_ref(),
        xy_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "y",
        t.y.as_ref(),
        xy_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "w",
        t.w.as_ref(),
        geom_required,
        t.source_span,
        diagnostics,
    );
    check_optional_dim(
        &t.id,
        "h",
        t.h.as_ref(),
        geom_required,
        t.source_span,
        diagnostics,
    );

    // Visual properties.
    check_visual_prop(
        &t.id,
        "fill",
        t.fill.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "stroke",
        t.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "stroke-width",
        t.stroke_width.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "contrast-bg",
        t.contrast_bg.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "font-family",
        t.font_family.as_ref(),
        VisualExpect::FontFamily,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "font-size",
        t.font_size.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "font-size-min",
        t.font_size_min.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "font-weight",
        t.font_weight.as_ref(),
        VisualExpect::FontWeight,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "shadow",
        t.shadow.as_ref(),
        VisualExpect::Shadow,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "filter",
        t.filter.as_ref(),
        VisualExpect::Filter,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &t.id,
        "mask",
        t.mask.as_ref(),
        VisualExpect::Mask,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    if let Some(d) = t.blur.as_ref()
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("text '{}': blur must be >= 0", t.id),
            t.source_span,
            Some(t.id.clone()),
        ));
    }

    // Per-span visual properties. Spans inherit the node id as their
    // subject so token refs in `span ... fill=(token)".." font-weight=..`
    // are registered (otherwise the token is falsely flagged unused) and
    // get the same existence/type/raw-literal validation as node props.
    check_spans(
        &t.id,
        &t.spans,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );

    // Text-runaround exclusion: an `text-exclusion` naming an id that
    // does not exist among the document's node ids is advisory (the
    // render proceeds with no exclusion, byte-identical to a node
    // without the attribute). Mirrors `field.unresolved_ref`.
    if let Some(target) = &t.text_exclusion
        && !all_node_ids.contains(target)
    {
        diagnostics.push(Diagnostic::warning(
            "text-exclusion.unresolved_ref",
            format!(
                "text '{}': text-exclusion '{}' matches no node id in the document",
                t.id, target
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in t.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "text '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                t.id, prop_name
            ),
            t.source_span,
            Some(t.id.clone()),
        ));
    }
}

pub(in crate::validate::check) fn check_image(
    img: &ImageNode,
    ctx: WalkCtx,
    seen_ids: &mut HashSet<String>,
    referenced_token_ids: &mut HashSet<String>,
    geom_required: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WalkCtx {
        resolved_tokens,
        declared_asset_ids,
        declared_style_ids,
        zone_ids,
        ..
    } = ctx;
    register_id(&img.id, seen_ids, diagnostics);
    if let Some(bm) = img.blend_mode.as_deref()
        && !is_valid_blend_mode(bm)
    {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "image '{}': blend-mode '{bm}' is not a recognized value",
                img.id
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }
    check_style_ref(
        &img.id,
        img.style.as_deref(),
        declared_style_ids,
        img.source_span,
        diagnostics,
    );

    // A recognized anchor supplies both x and y.
    let anchor_active = check_anchor(
        &img.id,
        img.anchor.as_deref(),
        img.anchor_zone.as_deref(),
        zone_ids,
        img.source_span,
        diagnostics,
    );
    let xy_required = geom_required && !anchor_active;

    // Required geometry: x, y, w, h must all be present (mirror rect).
    check_optional_dim(
        &img.id,
        "x",
        img.x.as_ref(),
        xy_required,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "y",
        img.y.as_ref(),
        xy_required,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "w",
        img.w.as_ref(),
        geom_required,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "h",
        img.h.as_ref(),
        geom_required,
        img.source_span,
        diagnostics,
    );

    // src-rect: all-four-or-none rule.
    let src_present_count = [
        img.src_x.as_ref(),
        img.src_y.as_ref(),
        img.src_w.as_ref(),
        img.src_h.as_ref(),
    ]
    .iter()
    .filter(|d| d.is_some())
    .count();
    if src_present_count > 0 && src_present_count < 4 {
        diagnostics.push(Diagnostic::error(
            "image.partial_src_rect",
            format!(
                "image '{}': src-x/src-y/src-w/src-h must all be present together; \
                 found {src_present_count} of 4",
                img.id
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // src-w/src-h must be > 0 when present (and unit is resolvable to px).
    if let Some(sw) = &img.src_w
        && let Some(sw_px) = dim_to_px(sw.value, &sw.unit)
        && sw_px <= 0.0
    {
        diagnostics.push(Diagnostic::error(
            "image.invalid_src_rect",
            format!("image '{}': src-w must be > 0 (got {})", img.id, sw.value,),
            img.source_span,
            Some(img.id.clone()),
        ));
    }
    if let Some(sh) = &img.src_h
        && let Some(sh_px) = dim_to_px(sh.value, &sh.unit)
        && sh_px <= 0.0
    {
        diagnostics.push(Diagnostic::error(
            "image.invalid_src_rect",
            format!("image '{}': src-h must be > 0 (got {})", img.id, sh.value,),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // Unit validation for each src-* field (required=false: partial is already caught above).
    check_optional_dim(
        &img.id,
        "src-x",
        img.src_x.as_ref(),
        false,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "src-y",
        img.src_y.as_ref(),
        false,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "src-w",
        img.src_w.as_ref(),
        false,
        img.source_span,
        diagnostics,
    );
    check_optional_dim(
        &img.id,
        "src-h",
        img.src_h.as_ref(),
        false,
        img.source_span,
        diagnostics,
    );

    // The referenced asset must exist in the document's assets block.
    if !declared_asset_ids.contains(&img.asset) {
        diagnostics.push(Diagnostic::error(
            "asset.unknown_reference",
            format!(
                "image '{}': references asset '{}' which is not declared in the \
                 assets block",
                img.id, img.asset
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // Validate fit (version-relative; forward-compat warning).
    if let Some(fit) = &img.fit
        && !matches!(fit.as_str(), "contain" | "cover" | "stretch" | "none")
    {
        diagnostics.push(Diagnostic::warning(
            "image.invalid_fit",
            format!(
                "image '{}': unrecognized fit '{}' (version-relative; allowed \
                 values are contain, cover, stretch, none)",
                img.id, fit
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // Visual properties.
    // clip-radius is a dimension token (mirror rect `radius`); only
    // meaningful for clip="rounded" but type-checked whenever present.
    check_visual_prop(
        &img.id,
        "clip-radius",
        img.clip_radius.as_ref(),
        VisualExpect::Dimension,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &img.id,
        "shadow",
        img.shadow.as_ref(),
        VisualExpect::Shadow,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &img.id,
        "filter",
        img.filter.as_ref(),
        VisualExpect::Filter,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    check_visual_prop(
        &img.id,
        "mask",
        img.mask.as_ref(),
        VisualExpect::Mask,
        referenced_token_ids,
        resolved_tokens,
        diagnostics,
    );
    if let Some(d) = img.blur.as_ref()
        && d.value < 0.0
    {
        diagnostics.push(Diagnostic::error(
            "node.invalid_geometry",
            format!("image '{}': blur must be >= 0", img.id),
            img.source_span,
            Some(img.id.clone()),
        ));
    }

    // Unknown properties.
    for prop_name in img.unknown_props.keys() {
        diagnostics.push(Diagnostic::warning(
            "node.unknown_property",
            format!(
                "image '{}': unknown property '{}' (version-relative; \
                 may be valid in a later schema version)",
                img.id, prop_name
            ),
            img.source_span,
            Some(img.id.clone()),
        ));
    }
    // Image is a leaf — no child recursion.
}
