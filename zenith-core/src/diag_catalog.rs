//! The single source of truth for every diagnostic code the engine can emit.
//!
//! This catalog is hand-maintained and drift-guarded (mirroring [`crate::schema`]'s
//! node-kind tables). It drives BOTH:
//! - the diagnostic-policy validation in [`crate::validate`] (which codes are
//!   *governable* by a `diagnostics { … }` block, and which are always Errors), and
//! - the `zenith schema diagnostics` surface (re-exposed through [`crate::schema`]).
//!
//! ## Adding a new diagnostic code
//!
//! Any new diagnostic code emitted anywhere in the workspace **MUST** be added to
//! [`DIAGNOSTIC_CODES`] below, with its real [`Severity`] and a one-line summary.
//! A code that is not in this catalog is treated as *unknown* by the policy
//! validator: a `diagnostics { … }` entry naming it produces `policy.unknown_code`.
//!
//! ## Governable vs. always-Error
//!
//! A code is **governable** when its catalog severity is `Warning` or `Advisory`:
//! an `allow`/`deny`/`warn` entry can adjust how it is reported. A code whose
//! catalog severity is `Error` is **always-Error** and immutable — `allow`/`warn`
//! cannot weaken it (the validator emits `policy.ineffective_on_error`); a `deny`
//! on it is a silent no-op (it is already an Error).

use crate::diagnostics::Severity;

/// One catalog entry: a stable diagnostic `code`, the [`Severity`] the engine
/// emits it at, and a one-line human summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticCodeInfo {
    /// The stable dot-separated code, e.g. `"layout.off_canvas"`.
    pub code: &'static str,
    /// The severity the engine emits this diagnostic at.
    pub severity: Severity,
    /// One-line description of what the diagnostic means.
    pub summary: &'static str,
}

impl DiagnosticCodeInfo {
    /// True when a `diagnostics { … }` policy entry can adjust this code — i.e.
    /// its severity is `Warning` or `Advisory`. Error-severity codes are
    /// immutable.
    pub fn is_governable(&self) -> bool {
        match self.severity {
            Severity::Error => false,
            Severity::Warning | Severity::Advisory => true,
        }
    }
}

/// The three policy verbs accepted inside a `diagnostics { … }` block, in
/// canonical order.
pub const DIAGNOSTIC_VERBS: &[&str] = &["allow", "deny", "warn"];

/// The complete catalog of diagnostic codes the engine can emit.
///
/// Sorted by code for deterministic output. The `#[cfg(test)]` drift guard below
/// asserts the list is non-empty and that every code is unique.
///
/// NOTE: any new diagnostic code MUST be added here (see the module docs).
pub const DIAGNOSTIC_CODES: &[DiagnosticCodeInfo] = &[
    info(
        "anchor.cycle",
        Severity::Error,
        "Anchor parent chain forms a cycle.",
    ),
    info(
        "anchor.edge_without_sibling",
        Severity::Warning,
        "`anchor-edge` set without an `anchor-sibling`.",
    ),
    info(
        "anchor.gap_invalid_unit",
        Severity::Warning,
        "`anchor-gap` uses a non-pixel unit.",
    ),
    info(
        "anchor.parent_without_anchor",
        Severity::Warning,
        "`anchor-parent` set without an `anchor`.",
    ),
    info(
        "anchor.sibling_without_anchor",
        Severity::Warning,
        "`anchor-sibling` set without an `anchor`.",
    ),
    info(
        "anchor.unknown_edge",
        Severity::Error,
        "`anchor-edge` value is not a recognized edge.",
    ),
    info(
        "anchor.unknown_value",
        Severity::Error,
        "`anchor` value is not a recognized anchor point.",
    ),
    info(
        "anchor.unresolvable_parent",
        Severity::Error,
        "`anchor-parent` references an unknown node.",
    ),
    info(
        "anchor.unresolved_sibling",
        Severity::Error,
        "`anchor-sibling` references an unknown node.",
    ),
    info(
        "anchor.unresolved_zone",
        Severity::Error,
        "`anchor-zone` references an unknown zone.",
    ),
    info(
        "anchor.zone_without_anchor",
        Severity::Warning,
        "`anchor-zone` set without an `anchor`.",
    ),
    info(
        "asset.invalid_kind",
        Severity::Error,
        "`asset` kind is not a recognized asset kind.",
    ),
    info(
        "asset.invalid_src",
        Severity::Error,
        "`asset` src is empty or malformed.",
    ),
    info(
        "asset.unknown_property",
        Severity::Warning,
        "Unrecognized property on an `asset` declaration.",
    ),
    info(
        "asset.unknown_reference",
        Severity::Error,
        "Image references an undeclared asset id.",
    ),
    info(
        "component.unknown_override_target",
        Severity::Warning,
        "Instance override targets an unknown component child.",
    ),
    info(
        "component.unknown_reference",
        Severity::Error,
        "Instance references an undeclared component id.",
    ),
    info(
        "connector.invalid_anchor",
        Severity::Warning,
        "Connector anchor value is not recognized.",
    ),
    info(
        "connector.invalid_marker",
        Severity::Warning,
        "Connector marker value is not recognized.",
    ),
    info(
        "connector.invalid_route",
        Severity::Warning,
        "Connector route value is not recognized.",
    ),
    info(
        "connector.missing_target",
        Severity::Warning,
        "Connector is missing a `from` or `to` target.",
    ),
    info(
        "connector.unknown_target",
        Severity::Warning,
        "Connector `from`/`to` references an unknown node.",
    ),
    info(
        "contrast.low",
        Severity::Warning,
        "Text/background contrast is below the WCAG 2.2 threshold.",
    ),
    info(
        "document.invalid_colorspace",
        Severity::Warning,
        "Document `colorspace` value is unrecognized.",
    ),
    info(
        "document.invalid_page_parity_start",
        Severity::Warning,
        "Document `page-parity-start` value is unrecognized.",
    ),
    info(
        "document.invalid_page_progression",
        Severity::Warning,
        "Document `page-progression` value is unrecognized.",
    ),
    info(
        "document.invalid_spread_gutter",
        Severity::Warning,
        "Document `spread-gutter` value is invalid.",
    ),
    info(
        "document.no_pages",
        Severity::Error,
        "Document body contains no pages.",
    ),
    info(
        "field.unknown_type",
        Severity::Warning,
        "Field `type` is not a recognized field type.",
    ),
    info(
        "field.unresolved_ref",
        Severity::Warning,
        "Field references an unknown target.",
    ),
    info(
        "filter.duotone_missing_color",
        Severity::Error,
        "Duotone filter op is missing a shadow/highlight color.",
    ),
    info(
        "filter.invalid_amount",
        Severity::Error,
        "Filter op `amount` is out of range.",
    ),
    info(
        "filter.invalid_scale",
        Severity::Error,
        "Filter op `scale` is out of range.",
    ),
    info(
        "filter.no_ops",
        Severity::Error,
        "Filter token declares no ops.",
    ),
    info(
        "fold.content_crossing",
        Severity::Advisory,
        "Content crosses a declared fold line.",
    ),
    info(
        "footnote.unresolved_ref",
        Severity::Warning,
        "Text span footnote-ref names an unknown footnote.",
    ),
    info(
        "frame.child_overflow",
        Severity::Advisory,
        "A frame child extends outside the frame box.",
    ),
    info(
        "gradient.invalid_radius",
        Severity::Error,
        "Radial gradient radius is invalid.",
    ),
    info(
        "gradient.stop_unresolved",
        Severity::Error,
        "Gradient stop color references an unknown token.",
    ),
    info(
        "gradient.stop_wrong_type",
        Severity::Error,
        "Gradient stop color token is not a color.",
    ),
    info(
        "gradient.too_few_stops",
        Severity::Error,
        "Gradient declares fewer than two stops.",
    ),
    info(
        "grid.missing_columns",
        Severity::Advisory,
        "Table grid layout has no column definitions.",
    ),
    info(
        "group.invalid_intensity",
        Severity::Warning,
        "Group `intensity` is out of the 0.0–1.0 range.",
    ),
    info(
        "id.duplicate",
        Severity::Error,
        "An id is used more than once in the document.",
    ),
    info(
        "image.invalid_fit",
        Severity::Warning,
        "Image `fit` value is not recognized.",
    ),
    info(
        "image.invalid_src_rect",
        Severity::Error,
        "Image source-crop rectangle is invalid.",
    ),
    info(
        "image.partial_src_rect",
        Severity::Error,
        "Image source-crop rectangle is only partially specified.",
    ),
    info(
        "layout.off_canvas",
        Severity::Advisory,
        "A node extends outside the page bounds.",
    ),
    info(
        "library.unknown_property",
        Severity::Warning,
        "Unrecognized property on a `library` declaration.",
    ),
    info(
        "margin.violation",
        Severity::Advisory,
        "Content intrudes into a declared book live-area margin.",
    ),
    info(
        "mask.invalid_feather",
        Severity::Error,
        "Mask `feather` value is invalid.",
    ),
    info(
        "mask.invalid_radius",
        Severity::Error,
        "Mask `radius` value is invalid.",
    ),
    info(
        "master.unknown_reference",
        Severity::Error,
        "A page references an undeclared master id.",
    ),
    info(
        "node.invalid_geometry",
        Severity::Error,
        "A node geometry attribute uses an unknown unit.",
    ),
    info(
        "node.missing_geometry",
        Severity::Error,
        "A node is missing a required geometry attribute.",
    ),
    info(
        "node.unknown_kind",
        Severity::Warning,
        "A node kind is not recognized by this engine version.",
    ),
    info(
        "node.unknown_property",
        Severity::Warning,
        "A node carries a property this engine does not recognize.",
    ),
    info(
        "page.invalid_bleed",
        Severity::Warning,
        "Page `bleed` value is invalid.",
    ),
    info(
        "page.invalid_line_jumps",
        Severity::Warning,
        "Page `line-jumps` value is not recognized.",
    ),
    info(
        "page.invalid_parity",
        Severity::Warning,
        "Page `parity` value is not recognized.",
    ),
    info(
        "pattern.grid_missing_spacing",
        Severity::Error,
        "Grid pattern is missing required `spacing`.",
    ),
    info(
        "pattern.invalid_count",
        Severity::Error,
        "Pattern `count` is out of range.",
    ),
    info(
        "pattern.invalid_spacing",
        Severity::Error,
        "Pattern `spacing` is invalid.",
    ),
    info(
        "pattern.jitter_out_of_range",
        Severity::Warning,
        "Pattern `jitter` is out of the 0.0–1.0 range.",
    ),
    info(
        "pattern.scatter_missing_count",
        Severity::Error,
        "Scatter pattern is missing required `count`.",
    ),
    info(
        "pattern.unknown_kind",
        Severity::Error,
        "Pattern `kind` is not `grid` or `scatter`.",
    ),
    info(
        "provenance.unknown_library",
        Severity::Error,
        "A provenance origin names an undeclared library.",
    ),
    info(
        "provenance.unknown_node",
        Severity::Error,
        "A provenance origin names an unknown node.",
    ),
    info(
        "provenance.unknown_property",
        Severity::Warning,
        "Unrecognized property on a provenance `origin`.",
    ),
    info(
        "recipe.duplicate_id",
        Severity::Error,
        "Two recipes declare the same id.",
    ),
    info(
        "recipe.unknown_bounds",
        Severity::Error,
        "Recipe `bounds` references an unknown node.",
    ),
    info(
        "recipe.unknown_expanded_node",
        Severity::Error,
        "Recipe `expanded` names an unknown node.",
    ),
    info(
        "recipe.unknown_palette_token",
        Severity::Error,
        "Recipe palette references an unknown or non-color token.",
    ),
    info(
        "safe_zone.violation",
        Severity::Advisory,
        "Content violates a declared safe/dead zone.",
    ),
    info(
        "section.duplicate_start_page",
        Severity::Error,
        "Two sections start on the same page.",
    ),
    info(
        "section.invalid_folio_style",
        Severity::Warning,
        "Section `folio-style` value is not recognized.",
    ),
    info(
        "section.unknown_start_page",
        Severity::Error,
        "Section `start-page` references an unknown page.",
    ),
    info(
        "shadow.layer_unresolved",
        Severity::Error,
        "Shadow layer color references an unknown token.",
    ),
    info(
        "shadow.layer_wrong_type",
        Severity::Error,
        "Shadow layer color token is not a color.",
    ),
    info(
        "shadow.no_layers",
        Severity::Error,
        "Shadow token declares no layers.",
    ),
    info(
        "shape.insufficient_points",
        Severity::Error,
        "A polygon/polyline has too few points.",
    ),
    info(
        "shape.invalid_h_align",
        Severity::Warning,
        "Shape `h-align` value is not recognized.",
    ),
    info(
        "shape.invalid_stroke_alignment",
        Severity::Warning,
        "Shape `stroke-alignment` value is not recognized.",
    ),
    info(
        "shape.invalid_v_align",
        Severity::Warning,
        "Shape `v-align` value is not recognized.",
    ),
    info(
        "shape.unknown_kind",
        Severity::Warning,
        "Shape `kind` is not a recognized preset shape.",
    ),
    info(
        "style.unknown_property",
        Severity::Warning,
        "A style block carries an unrecognized property.",
    ),
    info(
        "style.unknown_reference",
        Severity::Error,
        "A node references an undeclared style id.",
    ),
    info(
        "table.cell_overflow",
        Severity::Error,
        "A table cell's content overflows its cell box.",
    ),
    info(
        "table.invalid_border_collapse",
        Severity::Warning,
        "Table `border-collapse` value is not recognized.",
    ),
    info(
        "table.invalid_h_align",
        Severity::Warning,
        "Table `h-align` value is not recognized.",
    ),
    info(
        "table.invalid_v_align",
        Severity::Warning,
        "Table `v-align` value is not recognized.",
    ),
    info(
        "text-exclusion.unresolved_ref",
        Severity::Warning,
        "Text `text-exclusion` references an unknown node.",
    ),
    info(
        "toc.no_selector",
        Severity::Warning,
        "A `toc` node declares no selector.",
    ),
    info(
        "token.cyclic_reference",
        Severity::Error,
        "A token reference chain forms a cycle.",
    ),
    info(
        "token.duplicate_id",
        Severity::Error,
        "Two tokens declare the same id.",
    ),
    info(
        "token.incompatible_property",
        Severity::Error,
        "A token is referenced by an incompatible property.",
    ),
    info(
        "token.invalid_value",
        Severity::Error,
        "A token has an invalid value for its type.",
    ),
    info(
        "token.raw_visual_literal",
        Severity::Error,
        "A visual property uses a raw literal instead of a token.",
    ),
    info(
        "token.type_mismatch",
        Severity::Error,
        "A token value does not match its declared type.",
    ),
    info(
        "token.unknown_reference",
        Severity::Error,
        "A property references an undeclared token id.",
    ),
    info(
        "token.unknown_type",
        Severity::Warning,
        "A token declares an unrecognized type.",
    ),
    info(
        "token.unused",
        Severity::Advisory,
        "A token is declared but never referenced.",
    ),
    info(
        "value.out_of_range",
        Severity::Error,
        "A numeric value is outside its allowed range.",
    ),
    info(
        "variant.duplicate_id",
        Severity::Error,
        "Two variants declare the same id.",
    ),
    info(
        "variant.invalid_dimension",
        Severity::Error,
        "A variant `w`/`h` value is invalid.",
    ),
    info(
        "variant.override_unknown_node",
        Severity::Error,
        "A variant override targets an unknown node.",
    ),
    info(
        "variant.unknown_source",
        Severity::Error,
        "A variant `source` references an unknown page.",
    ),
    // ── Diagnostic-policy self-validation (emitted by the policy checker) ──────
    info(
        "policy.unknown_code",
        Severity::Warning,
        "A `diagnostics { … }` entry names a code the engine does not emit.",
    ),
    info(
        "policy.ineffective_on_error",
        Severity::Warning,
        "`allow`/`warn` cannot weaken an always-Error diagnostic code.",
    ),
];

/// `const`-friendly constructor for a [`DiagnosticCodeInfo`] table entry.
const fn info(code: &'static str, severity: Severity, summary: &'static str) -> DiagnosticCodeInfo {
    DiagnosticCodeInfo {
        code,
        severity,
        summary,
    }
}

/// Look up a code's catalog entry, or `None` if the code is not in the catalog.
pub fn lookup(code: &str) -> Option<&'static DiagnosticCodeInfo> {
    DIAGNOSTIC_CODES.iter().find(|e| e.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_nonempty() {
        assert!(
            !DIAGNOSTIC_CODES.is_empty(),
            "the diagnostic-code catalog must not be empty"
        );
    }

    #[test]
    fn catalog_codes_are_unique() {
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        for entry in DIAGNOSTIC_CODES {
            assert!(
                !entry.code.is_empty(),
                "a catalog entry has an empty code string"
            );
            assert!(
                seen.insert(entry.code),
                "duplicate diagnostic code in the catalog: {}",
                entry.code,
            );
        }
    }

    #[test]
    fn governable_codes_are_warning_or_advisory() {
        for entry in DIAGNOSTIC_CODES {
            assert_eq!(
                entry.is_governable(),
                entry.severity != Severity::Error,
                "is_governable() must mirror (severity != Error) for {}",
                entry.code,
            );
        }
    }

    #[test]
    fn lookup_finds_known_and_misses_unknown() {
        assert!(lookup("layout.off_canvas").is_some());
        assert!(lookup("policy.unknown_code").is_some());
        assert!(lookup("not.a_real_code").is_none());
    }
}
