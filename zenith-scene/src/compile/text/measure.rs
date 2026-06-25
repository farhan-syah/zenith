//! Text measurement helpers: resolve a node's font size/family/spans and reuse
//! the production [`super::shape::shape_words`] + [`super::pack::pack_lines`]
//! pipeline to report a node's natural width or wrapped block height. Shared with
//! the table content-measurer so geometry resolution lives in one place.

use std::collections::BTreeMap;

use zenith_core::{
    Diagnostic, FontProvider, FontStyle, PropertyValue, ResolvedToken, Style, TextNode,
};
use zenith_layout::{RustybuzzEngine, TextDirection};

use crate::ir::Color;

use super::super::style_prop;
use super::super::util::resolve_property_dimension_px;
use super::ctx::{NodeShape, ShapeEnv};
use super::pack::pack_lines;
use super::shape::{
    ResolvedSpan, resolve_font_family_name, resolve_font_weight, resolve_vertical_align,
    shape_words,
};

/// Resolve a text node's font size in pixels with style cascade (default 16.0).
/// Shared by the chain-member render path and mirrors `compile_text`'s inline
/// resolution.
pub(in crate::compile) fn font_size_px(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
) -> f32 {
    let font_size_prop = text
        .font_size
        .clone()
        .or_else(|| style_prop(&text.style, style_map, "font-size").cloned());
    resolve_property_dimension_px(font_size_prop.as_ref(), resolved, 16.0) as f32
}

/// Resolve a text node's font family list with style cascade, probing the
/// provider and emitting the `font.unresolved` advisory on fallback.
///
/// Priority: node-local `font_family` → style `font-family` → default
/// "Noto Sans". Extracted from [`super::text_node::compile_text`] so the table
/// content-measurer resolves families through the EXACT same path (single source
/// of truth).
pub(in crate::compile) fn resolve_text_families(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    fonts: &dyn FontProvider,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let font_family_prop = text
        .font_family
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "font-family"));
    let raw_family_name = resolve_font_family_name(font_family_prop, resolved, "Noto Sans");
    // Probe the provider with the node-level defaults (weight 400, Normal
    // style) — sufficient to confirm family availability. The advisory fires at
    // most once per text node, before any per-span shaping.
    let (family_name, fell_back, is_local) = super::shape::resolve_family_with_fallback(
        fonts,
        &raw_family_name,
        "Noto Sans",
        400,
        FontStyle::Normal,
    );
    if fell_back {
        diagnostics.push(Diagnostic::advisory(
            "font.unresolved",
            format!(
                "text node '{}': font family '{}' not available, falling back to 'Noto Sans'",
                text.id, raw_family_name
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
    }
    if is_local {
        diagnostics.push(Diagnostic::advisory(
            "font.local",
            format!(
                "text node '{}': font family '{}' resolved from a local/system font; rendering is \
                 NOT guaranteed deterministic across machines — bundle the font or guarantee the \
                 target OS provides it",
                text.id, raw_family_name
            ),
            text.source_span,
            Some(text.id.clone()),
        ));
    }
    vec![family_name]
}

/// Resolve a text node's effective spans into [`ResolvedSpan`] carriers ready
/// for [`shape_words`], using the SAME per-span fill/weight/style/vertical-align
/// cascade `compile_text` applies. Footnote-marker expansion is intentionally
/// NOT applied here — the measurer only needs the authored content geometry.
///
/// Returns `(resolved_spans, font_size, base_weight)`. Empty spans are skipped
/// (matching the shaping pass); colors use a fixed opaque black since the
/// measurer never emits glyphs.
fn build_resolved_spans(
    text: &TextNode,
    resolved: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
) -> (Vec<ResolvedSpan>, f32, u16) {
    let font_size = font_size_px(text, resolved, style_map);

    // Color/fill resolution is intentionally skipped — the measurer never emits
    // glyphs, so each carrier uses a fixed opaque black. Only weight/style/size
    // affect the shaped advances we measure.
    let node_weight_prop: Option<&PropertyValue> = text
        .font_weight
        .as_ref()
        .or_else(|| style_prop(&text.style, style_map, "font-weight"));
    let base_weight = resolve_font_weight(node_weight_prop, resolved, 400);

    let mut spans: Vec<ResolvedSpan> = Vec::with_capacity(text.spans.len());
    for span in &text.spans {
        if span.text.is_empty() {
            continue;
        }
        let weight_prop = span.font_weight.as_ref().or(node_weight_prop);
        let weight = resolve_font_weight(weight_prop, resolved, base_weight);
        let style = if span.italic == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        let (span_font_size, baseline_dy) =
            resolve_vertical_align(span.vertical_align.as_deref(), font_size);
        spans.push(ResolvedSpan {
            text: span.text.clone(),
            color: Color::srgb(0, 0, 0, 255),
            underline: span.underline == Some(true),
            strikethrough: span.strikethrough == Some(true),
            // Measure path does not render highlights, but the field must be
            // present for the shared `shape_words` signature; `None` is safe
            // because measurement only accumulates advances.
            highlight: None,
            // `code` IS propagated so the measure path uses the mono family
            // for code spans — ensuring the measured advance matches the
            // rendered advance (mono glyphs differ from sans glyphs).
            code: span.code == Some(true),
            link: None,
            weight,
            style,
            font_size: span_font_size,
            baseline_dy,
        });
    }
    (spans, font_size, base_weight)
}

/// Shared shaping environment for the text-measurement helpers, grouped so the
/// measurers stay under the argument-count lint. All fields are borrows held for
/// the duration of a single measure call.
///
/// Every field is a borrow, so the struct is itself `Copy` and is passed BY
/// VALUE (no `&MeasureEnv`) through the measurement helpers and their callers.
#[derive(Clone, Copy)]
pub(in crate::compile) struct MeasureEnv<'a> {
    pub(in crate::compile) resolved: &'a BTreeMap<String, ResolvedToken>,
    pub(in crate::compile) style_map: &'a BTreeMap<&'a str, &'a Style>,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    pub(in crate::compile) engine: &'a RustybuzzEngine,
}

/// Measure a text node's NATURAL (unwrapped) content width + shaping metrics,
/// reusing the production [`shape_words`] pipeline. Returns `None` when the node
/// has no shapeable content (all-empty spans / shaping failure).
///
/// The natural width is the widest single line produced by packing at an
/// effectively-infinite box width (so the only line breaks are authored `\n`s).
pub(in crate::compile) fn measure_text_natural(
    text: &TextNode,
    families: &[String],
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    let (spans, font_size, base_weight) = build_resolved_spans(text, env.resolved, env.style_map);
    if spans.is_empty() {
        return None;
    }
    let node_direction = match text.direction.as_deref() {
        Some("rtl") => TextDirection::Rtl,
        _ => TextDirection::Ltr,
    };
    let (tokens, metrics) = shape_words(
        &spans,
        families,
        NodeShape {
            font_size,
            base_weight,
            direction: node_direction,
        },
        ShapeEnv {
            engine: env.engine,
            fonts: env.fonts,
        },
        diagnostics,
        &text.id,
        text.source_span,
    );
    if tokens.is_empty() {
        return None;
    }
    // Pack at an effectively-infinite width so the only breaks are authored
    // newlines; the natural width is the widest resulting line.
    let lines = pack_lines(tokens, f64::INFINITY, metrics.space_advance, None);
    let natural_w = lines
        .iter()
        .map(|l| l.content_w)
        .fold(0.0_f64, f64::max)
        .max(0.0);
    Some(natural_w)
}

/// Measure a text node's WRAPPED block height at a given content-box width, in
/// pixels (`line count × line_height`), reusing [`shape_words`] + [`pack_lines`].
/// Returns `None` when the node has no shapeable content. `box_w` is clamped to
/// a tiny positive minimum so a degenerate (≤0) width still yields ≥1 line
/// rather than an empty/zero pack.
pub(in crate::compile) fn measure_text_wrapped_height(
    text: &TextNode,
    box_w: f64,
    families: &[String],
    env: MeasureEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    let (spans, font_size, base_weight) = build_resolved_spans(text, env.resolved, env.style_map);
    if spans.is_empty() {
        return None;
    }
    let node_direction = match text.direction.as_deref() {
        Some("rtl") => TextDirection::Rtl,
        _ => TextDirection::Ltr,
    };
    let (tokens, metrics) = shape_words(
        &spans,
        families,
        NodeShape {
            font_size,
            base_weight,
            direction: node_direction,
        },
        ShapeEnv {
            engine: env.engine,
            fonts: env.fonts,
        },
        diagnostics,
        &text.id,
        text.source_span,
    );
    if tokens.is_empty() {
        return None;
    }
    let safe_w = box_w.max(1.0);
    let lines = pack_lines(tokens, safe_w, metrics.space_advance, None);
    let line_count = lines.len().max(1);
    Some(line_count as f64 * metrics.line_height)
}
