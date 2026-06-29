//! Shared helpers for the vector leaf compilers (dashed-stroke resolution).

use std::collections::BTreeMap;

use zenith_core::{PropertyValue, ResolvedToken};

use crate::ir::LineCap;

use super::super::util::resolve_property_dimension_px;

/// Resolve dashed-stroke parameters from raw node fields.
///
/// Returns `(stroke_dash, stroke_gap, stroke_linecap)`:
/// - `stroke_dash`/`stroke_gap` are `None` when dash is absent or `<= 0`
///   (solid stroke, byte-identical to prior behavior).
/// - `stroke_linecap` is `None` (Butt default) when dash is absent.
pub(in crate::compile) fn resolve_dash_params(
    dash_prop: Option<&PropertyValue>,
    gap_prop: Option<&PropertyValue>,
    linecap_str: Option<&str>,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> (Option<f64>, Option<f64>, Option<LineCap>) {
    let dash_px = resolve_property_dimension_px(dash_prop, resolved, -1.0);
    let gap_px = resolve_property_dimension_px(gap_prop, resolved, -1.0);
    let (stroke_dash, stroke_gap) = if dash_px > 0.0 {
        let g = if gap_px >= 0.0 { gap_px } else { dash_px };
        (Some(dash_px), Some(g))
    } else {
        (None, None)
    };
    let stroke_linecap = linecap_str.map(|s| match s {
        "round" => LineCap::Round,
        "square" => LineCap::Square,
        _ => LineCap::Butt,
    });
    // Only emit linecap when dash is active (solid strokes ignore it).
    let stroke_linecap = stroke_dash.and(stroke_linecap);
    (stroke_dash, stroke_gap, stroke_linecap)
}
