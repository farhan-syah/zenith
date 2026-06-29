//! Per-kind checks for effect-producing nodes.

use std::collections::BTreeSet;

use crate::ast::node::LightNode;
use crate::diagnostics::Diagnostic;
use crate::validate::check::nodes::WalkCtx;
use crate::validate::check::register_id;
use crate::validate::check::visual::{VisualExpect, check_visual_prop};

use super::shared::{TokenEnv, check_optional_dim};
use super::suggest::check_unknown_props;

pub(in crate::validate::check) fn check_light(
    l: &LightNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    geom_required: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&l.id, seen_ids, diagnostics);

    {
        let mut tokens = TokenEnv {
            referenced: referenced_token_ids,
            resolved: ctx.resolved_tokens,
        };
        check_optional_dim(
            &l.id,
            "x",
            l.x.as_ref(),
            geom_required,
            l.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &l.id,
            "y",
            l.y.as_ref(),
            geom_required,
            l.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &l.id,
            "radius",
            l.radius.as_ref(),
            geom_required,
            l.source_span,
            &mut tokens,
            diagnostics,
        );
    }

    check_visual_prop(
        &l.id,
        "color",
        l.color.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        ctx.resolved_tokens,
        diagnostics,
    );
    if let Some(kind) = &l.kind
        && !matches!(kind.as_str(), "ambient" | "glow" | "key" | "rim")
    {
        diagnostics.push(Diagnostic::warning(
            "light.unknown_kind",
            format!(
                "light '{}': kind '{}' is not recognized; expected ambient, glow, key, or rim",
                l.id, kind
            ),
            l.source_span,
            Some(l.id.clone()),
        ));
    }
    if let Some(opacity) = l.opacity
        && !(0.0..=1.0).contains(&opacity)
    {
        diagnostics.push(Diagnostic::warning(
            "node.opacity_out_of_range",
            format!(
                "light '{}': opacity {} is outside the valid range 0.0..=1.0",
                l.id, opacity
            ),
            l.source_span,
            Some(l.id.clone()),
        ));
    }
    if let Some(angle) = &l.angle
        && !matches!(angle.unit, crate::ast::value::Unit::Deg)
    {
        diagnostics.push(Diagnostic::warning(
            "light.angle_ignored",
            format!(
                "light '{}': angle uses unit '{}'; only deg is recognized",
                l.id,
                angle.unit.as_annotation()
            ),
            l.source_span,
            Some(l.id.clone()),
        ));
    }
    check_unknown_props("light", &l.id, &l.unknown_props, l.source_span, diagnostics);
}
