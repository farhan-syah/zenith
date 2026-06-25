//! Word shaping: the shared per-word re-shaping pipeline used by both the
//! single-box wrap path and the chain distributor, plus the resolved-span
//! carriers, shared metrics, glyph conversion, and the font weight/family/
//! vertical-align resolvers the whole module depends on.

use std::collections::{BTreeMap, BTreeSet};

use zenith_core::{
    Diagnostic, FontProvider, FontSource, FontStyle, PropertyValue, ResolvedToken, ResolvedValue,
};
use zenith_layout::{ShapeRequest, TextDirection, TextLayoutEngine, ZenithGlyphRun};

use crate::ir::{Color, SceneGlyph};

use super::ctx::{NodeShape, ShapeEnv};

/// The bundled monospace family used for `code` spans and the `code` node.
pub(in crate::compile) const CODE_MONO_FAMILY: &str = "Noto Sans Mono";

/// Default background color for inline `code` spans: a light neutral gray
/// (#F0F0F0). Internal only — no author token required.
pub(in crate::compile) const CODE_BG: Color = Color::srgb(240, 240, 240, 255);

/// Default color for `link` spans: conventional link blue (#0066CC). Applied
/// only when the span has no explicit `fill`. Internal only.
pub(in crate::compile) const LINK_COLOR: Color = Color::srgb(0, 102, 204, 255);

/// A re-shaped word plus the visual attributes inherited from its source span.
///
/// A word may shape to multiple font-runs (per-glyph fallback), so `runs` is a
/// Vec laid out left-to-right; `advance` is their summed width.
pub(in crate::compile) struct WordToken {
    pub(in crate::compile) runs: Vec<ZenithGlyphRun>,
    pub(in crate::compile) advance: f64,
    pub(in crate::compile) color: Color,
    pub(in crate::compile) underline: bool,
    pub(in crate::compile) strikethrough: bool,
    /// Per-span highlight background color. `None` = no highlight (byte-identical
    /// to a span without it); `Some(c)` causes a background FillRect to be
    /// emitted behind the glyph run by the emit path.
    pub(in crate::compile) highlight: Option<Color>,
    /// `true` when this word was shaped from a `code=#true` span. Causes a
    /// `CODE_BG` background FillRect to be emitted behind the glyph run.
    pub(in crate::compile) code: bool,
    /// Retained hyperlink URL from a `link="…"` span. `None` = no link.
    /// Carried for future PDF annotation / GUI use; not rendered beyond the
    /// color + underline treatment applied during resolution.
    pub(in crate::compile) link: Option<String>,
    /// Super/subscript baseline shift in pixels (negative = up; 0 = baseline).
    /// Applied per-glyph-run by [`super::emit::emit_lines`] on top of the line
    /// baseline.
    pub(in crate::compile) baseline_dy: f64,
    /// The exact source text this word was shaped from, plus the weight/style/
    /// size needed to RE-shape a hyphenated fragment of it. Used ONLY by the
    /// optional hyphenation path in [`super::pack::pack_lines`]; the non-hyphenate
    /// path never reads it, so default-off packing is byte-identical. `paragraph`
    /// is the 0-based paragraph index this word belongs to (newline-separated
    /// source), consumed by widow/orphan control in the chain distributor.
    pub(in crate::compile) src: WordSource,
}

/// The source text + shaping attributes a [`WordToken`] was produced from, so a
/// hyphenated fragment can be deterministically re-shaped with identical style.
#[derive(Clone)]
pub(in crate::compile) struct WordSource {
    pub(in crate::compile) text: String,
    pub(in crate::compile) weight: u16,
    pub(in crate::compile) style: FontStyle,
    pub(in crate::compile) font_size: f32,
    /// 0-based paragraph index (each `\n` in the source starts a new paragraph).
    pub(in crate::compile) paragraph: usize,
    /// When this token is a hyphenation fragment, the ORIGINAL unsplit word it
    /// came from, with `true` for the head (`fragment-`) and `false` for the
    /// tail. The chain distributor uses this to MERGE an adjacent head+tail back
    /// into the original word before re-wrapping it in the next box, so a
    /// fragment is never hyphenated twice. `None` for an ordinary word.
    pub(in crate::compile) hyphen_part: Option<(String, bool)>,
}

/// Shared font metrics captured from the first successfully shaped word.
#[derive(Clone, Copy, Default)]
pub(in crate::compile) struct WordMetrics {
    pub(in crate::compile) ascent: f64,
    pub(in crate::compile) line_height: f64,
    pub(in crate::compile) space_advance: f64,
}

/// A span already resolved to color/decoration/weight/style, ready for the
/// per-word re-shaping the wrap + chain paths perform. Mirrors the private
/// `ShapedSpan` fields the wrap path consumes.
pub(in crate::compile) struct ResolvedSpan {
    pub(in crate::compile) text: String,
    pub(in crate::compile) color: Color,
    pub(in crate::compile) underline: bool,
    pub(in crate::compile) strikethrough: bool,
    /// Per-span highlight background color (`None` = no highlight).
    pub(in crate::compile) highlight: Option<Color>,
    /// `true` when this span was authored with `code=#true`. The shaper
    /// switches the family to [`CODE_MONO_FAMILY`] and the emit path emits
    /// a [`CODE_BG`] background rect (like highlight but fixed color).
    pub(in crate::compile) code: bool,
    /// Retained hyperlink URL (`None` = no link). Carried through to
    /// [`WordToken`] for future PDF annotation use.
    pub(in crate::compile) link: Option<String>,
    pub(in crate::compile) weight: u16,
    pub(in crate::compile) style: FontStyle,
    /// The span's OWN font size (reduced for super/subscript). When equal to the
    /// shared node size, shaping is byte-identical to the size-less form.
    pub(in crate::compile) font_size: f32,
    /// Super/subscript baseline shift in pixels (negative = up; 0 = baseline).
    pub(in crate::compile) baseline_dy: f64,
}

/// Emit a `font.glyph_missing` warning for `node_id` when `missing` is
/// non-empty. Shared by the NOWRAP pass-1 loop and [`shape_words`] so the
/// format string and `Diagnostic` construction live in exactly one place.
pub(in crate::compile) fn emit_glyph_missing(
    diagnostics: &mut Vec<Diagnostic>,
    node_id: &str,
    span: Option<zenith_core::Span>,
    missing: &BTreeSet<char>,
) {
    if missing.is_empty() {
        return;
    }
    let chars_desc = missing
        .iter()
        .map(|&c| format!("'{}' (U+{:04X})", c, c as u32))
        .collect::<Vec<_>>()
        .join(", ");
    diagnostics.push(Diagnostic::warning(
        "font.glyph_missing",
        format!(
            "text node '{node_id}' contains character(s) with no glyph in any registered font: \
             {chars_desc}"
        ),
        span,
        Some(node_id.to_owned()),
    ));
}

/// Tokenise resolved spans into per-word [`WordToken`]s (one re-shape per word,
/// with per-glyph fallback) and capture the shared font metrics.
///
/// This is the SINGLE shaping routine used by both the single-box wrap path and
/// the chain distributor, so a chain member and a standalone wrapped node shape
/// identical word geometry. `node_id` is used only for diagnostics wording.
pub(in crate::compile) fn shape_words(
    spans: &[ResolvedSpan],
    families: &[String],
    shape: NodeShape,
    env: ShapeEnv,
    diagnostics: &mut Vec<Diagnostic>,
    node_id: &str,
    span: Option<zenith_core::Span>,
) -> (Vec<WordToken>, WordMetrics) {
    let font_size = shape.font_size;
    let node_base_weight = shape.base_weight;
    let direction = shape.direction;
    let engine = env.engine;
    let fonts = env.fonts;

    let mut tokens: Vec<WordToken> = Vec::new();
    let mut metrics = WordMetrics::default();
    let mut have_metrics = false;
    // Running paragraph index. Each `\n` in the source (across spans) starts a
    // new paragraph; consecutive spans without a newline keep the same index, so
    // a multi-span paragraph stays one paragraph. Widow/orphan control reads this
    // per-line; the default-off path never inspects it.
    let mut paragraph: usize = 0;
    // Accumulate chars with no glyph in any registered face across ALL words of
    // this node. Emitted as a single diagnostic after the word loop.
    let mut node_missing: BTreeSet<char> = BTreeSet::new();

    for shaped in spans {
        // A super/subscript span carries its own reduced size; a baseline span
        // uses the shared node `font_size`. Metrics (ascent/line_height) are
        // captured ONLY from a full-size word so the line grid stays uniform.
        let is_vertical_align = shaped.baseline_dy != 0.0;
        let word_font_size = shaped.font_size;
        // `code` spans override the node family with the bundled mono face.
        // All other spans use the node-level `families` slice unchanged.
        let code_families_buf: Vec<String>;
        let span_families: &[String] = if shaped.code {
            code_families_buf = vec![CODE_MONO_FAMILY.to_owned()];
            &code_families_buf
        } else {
            families
        };
        // Split the span text into paragraphs on `\n`; each segment after the
        // first increments the running paragraph index. `split('\n')` always
        // yields ≥1 segment, so a span without a newline keeps `paragraph`.
        for (seg_idx, segment) in shaped.text.split('\n').enumerate() {
            if seg_idx > 0 {
                paragraph += 1;
            }
            for word in segment.split_whitespace() {
                let req = ShapeRequest {
                    text: word,
                    families: span_families,
                    weight: shaped.weight,
                    style: shaped.style,
                    font_size: word_font_size,
                    direction,
                };
                match engine.shape_with_fallback(&req, fonts) {
                    Err(e) => {
                        diagnostics.push(Diagnostic::advisory(
                            "scene.text_unshaped",
                            format!("text node '{}' could not be shaped: {}", node_id, e.message),
                            span,
                            Some(node_id.to_owned()),
                        ));
                    }
                    Ok(result) => {
                        node_missing.extend(result.missing_chars);
                        if !have_metrics
                            && !is_vertical_align
                            && let Some(first) = result.runs.first()
                        {
                            metrics.ascent = first.ascent as f64;
                            metrics.line_height = first.line_height as f64;
                            have_metrics = true;
                        }
                        let advance: f64 = result.runs.iter().map(|r| r.advance_width as f64).sum();
                        tokens.push(WordToken {
                            advance,
                            color: shaped.color,
                            underline: shaped.underline,
                            strikethrough: shaped.strikethrough,
                            highlight: shaped.highlight,
                            code: shaped.code,
                            link: shaped.link.clone(),
                            baseline_dy: shaped.baseline_dy,
                            runs: result.runs,
                            src: WordSource {
                                text: word.to_owned(),
                                weight: shaped.weight,
                                style: shaped.style,
                                font_size: word_font_size,
                                paragraph,
                                hyphen_part: None,
                            },
                        });
                    }
                }
            }
        }
    }

    // Emit one warning per node listing every character that had no glyph in
    // any registered face (would silently render as .notdef / tofu).
    emit_glyph_missing(diagnostics, node_id, span, &node_missing);

    // Shape a single space once (node base weight/style) for inter-word gaps
    // and packing measurement.
    metrics.space_advance = {
        let req = ShapeRequest {
            text: " ",
            families,
            weight: node_base_weight,
            style: FontStyle::Normal,
            font_size,
            // A single space's advance is direction-independent; keep LTR so the
            // inter-word gap measurement is identical for LTR and RTL.
            direction: TextDirection::Ltr,
        };
        match engine.shape(&req, fonts) {
            Ok(run) => run.advance_width as f64,
            Err(_) => 0.0,
        }
    };

    (tokens, metrics)
}

/// Map a [`ZenithGlyphRun`]'s positioned glyphs into [`SceneGlyph`] records.
///
/// Used by every shaped-run emit site (Text, highlighted Code, plain Code) so
/// that the field mapping is defined in exactly one place.
pub(in crate::compile) fn run_to_scene_glyphs(run: &ZenithGlyphRun) -> Vec<SceneGlyph> {
    run.glyphs
        .iter()
        .map(|g| SceneGlyph {
            glyph_id: g.glyph_id,
            dx: g.x,
            dy: g.y,
        })
        .collect()
}

/// Resolve an optional font-weight property to a numeric weight (100–900).
///
/// Returns `default` when the property is absent, references a non-fontWeight
/// (or unresolved) token, or carries a dimension. The idiomatic path is a token
/// ref resolving to a `FontWeight`. A bare numeric literal (e.g. `font-weight=700`)
/// is parsed directly; an unparsable literal falls back to `default`. Mirrors
/// `resolve_property_dimension_px`.
pub(in crate::compile) fn resolve_font_weight(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: u16,
) -> u16 {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontWeight(w) => *w as u16,
                ResolvedValue::Color(_)
                | ResolvedValue::CmykColor { .. }
                | ResolvedValue::Dimension(_)
                | ResolvedValue::Number(_)
                | ResolvedValue::FontFamily(_)
                | ResolvedValue::Gradient(_)
                | ResolvedValue::Shadow(_)
                | ResolvedValue::Filter(_)
                | ResolvedValue::Mask(_) => default,
            },
            None => default,
        },
        Some(PropertyValue::Literal(s)) => s.parse::<u16>().unwrap_or(default),
        // A dimension or data ref is not a weight → fall back to the default.
        Some(PropertyValue::Dimension(_)) | Some(PropertyValue::DataRef(_)) => default,
        None => default,
    }
}

/// Resolve a requested font family against the provider, falling back to the
/// bundled default when the requested family is unregistered.
///
/// Returns `(family_to_use, fell_back, is_local)`:
/// - `fell_back` is `true` when the requested family was unavailable and
///   `default_family` was substituted, so the caller emits a `font.unresolved`
///   advisory and shaping proceeds with the bundled face instead of silently
///   dropping text.
/// - `is_local` is `true` when the face that was actually resolved came from a
///   local/system source ([`zenith_core::FontSource::Local`]), so the caller
///   emits a `font.local` advisory. A family can resolve cleanly from local
///   WITHOUT falling back, so `is_local` is independent of `fell_back`.
///
/// The probe weight/style match the shaping request. The bundled default is
/// always [`FontSource::Bundled`], so the fast path (and any fallback to the
/// default) is never flagged local.
pub(in crate::compile) fn resolve_family_with_fallback(
    fonts: &dyn FontProvider,
    requested: &str,
    default_family: &str,
    weight: u16,
    style: FontStyle,
) -> (String, bool, bool) {
    // Fast path: requested == default → always available (bundled), no check.
    if requested.eq_ignore_ascii_case(default_family) {
        return (requested.to_owned(), false, false);
    }
    match fonts.resolve(&[requested.to_owned()], weight, style) {
        Some(data) => {
            let is_local = data.source == FontSource::Local;
            (requested.to_owned(), false, is_local)
        }
        None => (default_family.to_owned(), true, false),
    }
}

/// Resolve a font-family [`PropertyValue`] to a raw family name string.
///
/// Priority: `TokenRef → FontFamily` value → else `default`; `Literal` → that
/// string; `Dimension` → `default` (not a family name); absent → `default`.
/// This extraction step is shared by [`super::text_node::compile_text`] and
/// [`super::super::chain`]'s style resolver so the two code paths stay
/// byte-identical.
pub(in crate::compile) fn resolve_font_family_name(
    prop: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
    default: &str,
) -> String {
    match prop {
        Some(PropertyValue::TokenRef(token_id)) => match resolved.get(token_id.as_str()) {
            Some(rt) => match &rt.value {
                ResolvedValue::FontFamily(name) => name.clone(),
                ResolvedValue::Color(_)
                | ResolvedValue::CmykColor { .. }
                | ResolvedValue::Dimension(_)
                | ResolvedValue::Number(_)
                | ResolvedValue::FontWeight(_)
                | ResolvedValue::Gradient(_)
                | ResolvedValue::Shadow(_)
                | ResolvedValue::Filter(_)
                | ResolvedValue::Mask(_) => default.to_owned(),
            },
            None => default.to_owned(),
        },
        Some(PropertyValue::Literal(name)) => name.clone(),
        Some(PropertyValue::Dimension(_)) | Some(PropertyValue::DataRef(_)) | None => {
            default.to_owned()
        }
    }
}

/// Superscript/subscript font-size scale factor applied to a span's resolved
/// size (deterministic). A `vertical-align="super"`/`"sub"` span is typeset at
/// `0.65 ×` the full font size.
const VALIGN_SCALE: f64 = 0.65;
/// Superscript baseline shift as a fraction of the FULL font size: the baseline
/// is raised (negative = up) by `0.34 × full_font_size`.
const VALIGN_SUPER_SHIFT: f64 = 0.34;
/// Subscript baseline shift as a fraction of the FULL font size: the baseline is
/// lowered by `0.16 × full_font_size`.
const VALIGN_SUB_SHIFT: f64 = 0.16;

/// Resolve a span's `vertical-align` into `(span_font_size, baseline_dy)`.
///
/// `node_font_size` is the full (node-resolved) size. For `"super"`/`"sub"` the
/// span size is reduced by [`VALIGN_SCALE`] and the baseline is shifted by a
/// fraction of the FULL size ([`VALIGN_SUPER_SHIFT`] up / [`VALIGN_SUB_SHIFT`]
/// down). Any other / absent value returns the full size and a zero shift, so a
/// span without vertical-align is byte-identical to before.
pub(in crate::compile) fn resolve_vertical_align(
    vertical_align: Option<&str>,
    node_font_size: f32,
) -> (f32, f64) {
    let full = node_font_size as f64;
    match vertical_align {
        Some("super") => ((full * VALIGN_SCALE) as f32, -(full * VALIGN_SUPER_SHIFT)),
        Some("sub") => ((full * VALIGN_SCALE) as f32, full * VALIGN_SUB_SHIFT),
        _ => (node_font_size, 0.0),
    }
}
