//! Per-kind checks for effect-producing nodes.

use std::collections::BTreeSet;

use crate::ast::node::{LightNode, MeshNode};
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

pub(in crate::validate::check) fn check_mesh(
    m: &MeshNode,
    ctx: WalkCtx,
    seen_ids: &mut BTreeSet<String>,
    referenced_token_ids: &mut BTreeSet<String>,
    geom_required: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    register_id(&m.id, seen_ids, diagnostics);

    {
        let mut tokens = TokenEnv {
            referenced: referenced_token_ids,
            resolved: ctx.resolved_tokens,
        };
        for (prop, value) in [
            ("x", m.x.as_ref()),
            ("y", m.y.as_ref()),
            ("w", m.w.as_ref()),
            ("h", m.h.as_ref()),
        ] {
            check_optional_dim(
                &m.id,
                prop,
                value,
                geom_required,
                m.source_span,
                &mut tokens,
                diagnostics,
            );
        }
        check_optional_dim(
            &m.id,
            "extend",
            m.extend.as_ref(),
            false,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &m.id,
            "stroke-width",
            m.stroke_width.as_ref(),
            false,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &m.id,
            "stroke-dash",
            m.stroke_dash.as_ref(),
            false,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &m.id,
            "stroke-gap",
            m.stroke_gap.as_ref(),
            false,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
        let perspective = m.kind.as_deref() == Some("perspective");
        check_optional_dim(
            &m.id,
            "vanishing-x",
            m.vanishing_x.as_ref(),
            perspective,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
        check_optional_dim(
            &m.id,
            "vanishing-y",
            m.vanishing_y.as_ref(),
            perspective,
            m.source_span,
            &mut tokens,
            diagnostics,
        );
    }

    check_visual_prop(
        &m.id,
        "stroke",
        m.stroke.as_ref(),
        VisualExpect::Color,
        referenced_token_ids,
        ctx.resolved_tokens,
        diagnostics,
    );
    if let Some(kind) = &m.kind
        && !matches!(kind.as_str(), "orthographic" | "perspective")
    {
        diagnostics.push(Diagnostic::warning(
            "mesh.unknown_kind",
            format!(
                "mesh '{}': kind '{}' is not recognized; expected orthographic or perspective",
                m.id, kind
            ),
            m.source_span,
            Some(m.id.clone()),
        ));
    }
    if matches!(m.rows, Some(0)) {
        diagnostics.push(Diagnostic::error(
            "mesh.invalid_rows",
            format!("mesh '{}': rows must be greater than zero", m.id),
            m.source_span,
            Some(m.id.clone()),
        ));
    }
    if matches!(m.columns, Some(0)) {
        diagnostics.push(Diagnostic::error(
            "mesh.invalid_columns",
            format!("mesh '{}': columns must be greater than zero", m.id),
            m.source_span,
            Some(m.id.clone()),
        ));
    }
    if let Some(opacity) = m.opacity
        && !(0.0..=1.0).contains(&opacity)
    {
        diagnostics.push(Diagnostic::warning(
            "node.opacity_out_of_range",
            format!(
                "mesh '{}': opacity {} is outside the valid range 0.0..=1.0",
                m.id, opacity
            ),
            m.source_span,
            Some(m.id.clone()),
        ));
    }
    check_unknown_props("mesh", &m.id, &m.unknown_props, m.source_span, diagnostics);
}
