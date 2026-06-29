//! Serialisable DTO types for JSON output.
//!
//! These types are defined in the CLI crate — we do NOT add serde to
//! zenith-core.  Each type maps from zenith-core/zenith-scene types to a
//! schema-versioned JSON shape.

use serde::Serialize;

/// JSON representation of a [`zenith_core::Diagnostic`].
#[derive(Debug, Serialize)]
pub struct DiagnosticJson {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
}

impl From<&zenith_core::Diagnostic> for DiagnosticJson {
    fn from(d: &zenith_core::Diagnostic) -> Self {
        Self {
            code: d.code.clone(),
            severity: severity_str(&d.severity).to_owned(),
            message: d.message.clone(),
            subject_id: d.subject_id.clone(),
        }
    }
}

pub(crate) fn severity_str(s: &zenith_core::Severity) -> &'static str {
    match s {
        zenith_core::Severity::Error => "error",
        zenith_core::Severity::Warning => "warning",
        zenith_core::Severity::Advisory => "advisory",
    }
}

/// Top-level JSON envelope for `validate`.
#[derive(Debug, Serialize)]
pub struct ValidateOutput {
    pub schema: &'static str,
    pub valid: bool,
    pub diagnostics: Vec<DiagnosticJson>,
}

/// Top-level JSON envelope for `fmt`.
#[derive(Debug, Serialize)]
pub struct FmtOutput {
    pub schema: &'static str,
    pub changed: bool,
    pub hash: String,
}

/// A single token entry for `tokens` output.
#[derive(Debug, Serialize)]
pub struct TokenEntry {
    pub id: String,
    pub token_type: String,
    pub resolved_value: String,
}

/// Top-level JSON envelope for `tokens`.
#[derive(Debug, Serialize)]
pub struct TokensOutput {
    pub schema: &'static str,
    pub tokens: Vec<TokenEntry>,
    pub diagnostics: Vec<DiagnosticJson>,
}

/// Top-level JSON envelope for `render`.
#[derive(Debug, Serialize)]
pub struct RenderOutput {
    pub schema: &'static str,
    pub diagnostics: Vec<DiagnosticJson>,
}

/// Top-level JSON envelope for `tx`.
#[derive(Debug, Serialize)]
pub struct TxOutputJson {
    pub schema: &'static str,
    pub status: String,
    pub affected: Vec<String>,
    pub diagnostics: Vec<DiagnosticJson>,
    pub changed: bool,
}

/// Per-row result in the `merge --json` batch report.
#[derive(Debug, Serialize)]
pub struct MergeRowResult {
    pub row: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub status: &'static str,
    pub outputs: Vec<String>,
    pub diagnostics: Vec<DiagnosticJson>,
}

/// Top-level JSON envelope for `merge --json`.
#[derive(Debug, Serialize)]
pub struct MergeOutput {
    pub schema: &'static str,
    pub total_rows: usize,
    pub written: usize,
    pub failed: usize,
    pub rows: Vec<MergeRowResult>,
}

/// One row entry in the generation manifest (successful rows only).
#[derive(Debug, Serialize)]
pub struct ManifestRow {
    pub row: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub outputs: Vec<String>,
}

/// Deterministic generation manifest for `zenith merge --manifest`.
#[derive(Debug, Serialize)]
pub struct MergeManifest {
    pub schema: &'static str,
    /// Manifest format version. Bumped only when the manifest structure changes
    /// (never on a routine crate release), so identical inputs stay byte-identical.
    pub generator: &'static str,
    pub source_sha256: String,
    pub data_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_by: Option<String>,
    pub rows: Vec<ManifestRow>,
}

// ── Variant JSON types ────────────────────────────────────────────────────────

/// Per-variant result in the `variant --json` envelope.
#[derive(Debug, Serialize)]
pub struct VariantResultJson {
    pub id: String,
    pub source: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_zen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_png: Option<String>,
    pub diagnostics: Vec<DiagnosticJson>,
}

/// Top-level JSON envelope for `variant --json`.
#[derive(Debug, Serialize)]
pub struct VariantOutput {
    pub schema: &'static str,
    pub total_variants: usize,
    pub generated: usize,
    pub failed: usize,
    pub variants: Vec<VariantResultJson>,
}

/// One target entry in the variant generation manifest (successful variants only).
#[derive(Debug, Serialize)]
pub struct VariantManifestTarget {
    pub id: String,
    pub source: String,
    pub outputs_zen: String,
    pub outputs_png: String,
}

/// Deterministic generation manifest for `zenith variant --manifest`.
#[derive(Debug, Serialize)]
pub struct VariantManifest {
    pub schema: &'static str,
    /// Manifest format version. Bumped only when the manifest structure changes
    /// (never on a routine crate release), so identical inputs stays byte-identical.
    pub generator: &'static str,
    pub source_sha256: String,
    pub targets: Vec<VariantManifestTarget>,
}

// ── Schema JSON types ─────────────────────────────────────────────────────────

/// A single node-kind entry in the `schema nodes` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaNodeEntry {
    pub kind: String,
    pub summary: String,
}

/// A single attribute entry in the `schema node <kind>` and `schema page/asset/document` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaAttr {
    pub name: String,
    pub ty: String,
}

/// A single node-kind detail entry in the `schema node <kind>` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaNodeDetail {
    pub kind: String,
    pub summary: String,
    pub attributes: Vec<SchemaAttr>,
    /// Minimal full-node authoring example, when one is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Child-content descriptor for kinds that accept authorable children.
    /// Absent from JSON for kinds with no authorable child content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<SchemaNodeContent>,
}

/// Child-content descriptor embedded in [`SchemaNodeDetail`].
#[derive(Debug, Serialize)]
pub struct SchemaNodeContent {
    pub description: String,
    pub example: String,
}

/// A single op entry in the `schema ops` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaOpEntry {
    pub op: String,
    pub summary: String,
}

/// A single op field entry in the `schema op <name>` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaOpFieldEntry {
    pub name: String,
    pub ty: String,
    pub required: bool,
}

/// Full detail entry in the `schema op <name>` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaOpDetail {
    pub op: String,
    pub summary: String,
    pub fields: Vec<SchemaOpFieldEntry>,
    pub example: String,
}

/// Top-level JSON envelope for `schema nodes`.
#[derive(Debug, Serialize)]
pub struct SchemaNodesOutput {
    pub schema: &'static str,
    pub nodes: Vec<SchemaNodeEntry>,
}

/// Top-level JSON envelope for `schema node <kind>`.
#[derive(Debug, Serialize)]
pub struct SchemaNodeOutput {
    pub schema: &'static str,
    pub node: SchemaNodeDetail,
}

/// Top-level JSON envelope for `schema ops`.
#[derive(Debug, Serialize)]
pub struct SchemaOpsOutput {
    pub schema: &'static str,
    pub ops: Vec<SchemaOpEntry>,
}

/// Top-level JSON envelope for `schema op <name>`.
#[derive(Debug, Serialize)]
pub struct SchemaOpOutput {
    pub schema: &'static str,
    pub op: SchemaOpDetail,
}

/// Top-level JSON envelope for bare `zenith schema` (overview).
#[derive(Debug, Serialize)]
pub struct SchemaOverviewOutput {
    pub schema: &'static str,
    pub node_kinds: usize,
    pub tx_ops: usize,
    pub token_types: usize,
}

/// A single token-type entry in the `schema tokens` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaTokenEntry {
    pub ty: String,
    pub summary: String,
}

/// Full detail for one token type in the `schema token <type>` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaTokenDetail {
    pub ty: String,
    pub summary: String,
    pub value_form: String,
    pub child_nodes: String,
    pub example: String,
}

/// Top-level JSON envelope for `schema tokens`.
#[derive(Debug, Serialize)]
pub struct SchemaTokensOutput {
    pub schema: &'static str,
    pub token_types: Vec<SchemaTokenEntry>,
}

/// Top-level JSON envelope for `schema token <type>`.
#[derive(Debug, Serialize)]
pub struct SchemaTokenOutput {
    pub schema: &'static str,
    pub token: SchemaTokenDetail,
}

/// Top-level JSON envelope for `schema page`, `schema asset`, `schema document`.
#[derive(Debug, Serialize)]
pub struct SchemaSurfaceOutput {
    pub schema: &'static str,
    /// Which non-node surface this describes: `"page"`, `"asset"`, or `"document"`.
    pub surface: &'static str,
    pub summary: String,
    pub attributes: Vec<SchemaAttr>,
}

/// A single governable diagnostic-code entry in the `schema diagnostics` JSON.
#[derive(Debug, Serialize)]
pub struct SchemaDiagnosticCode {
    pub code: String,
    /// `"error"`, `"warning"`, or `"advisory"`.
    pub severity: String,
    pub summary: String,
    /// True when an `allow`/`deny`/`warn` entry can adjust this code (Warning /
    /// Advisory). Error-severity codes are immutable and report `false`.
    pub governable: bool,
}

/// Top-level JSON envelope for `schema diagnostics`.
#[derive(Debug, Serialize)]
pub struct SchemaDiagnosticsOutput {
    pub schema: &'static str,
    pub summary: String,
    /// The policy verbs: `allow`, `deny`, `warn`.
    pub verbs: Vec<String>,
    /// Canonical KDL policy-entry forms.
    pub syntax: Vec<String>,
    /// Precedence note: in-file `diagnostics { … }` now; CLI flags/config later.
    pub precedence: &'static str,
    /// Every diagnostic code in the catalog (governable and always-Error).
    pub codes: Vec<SchemaDiagnosticCode>,
}

/// One override property entry in the `schema variant` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaOverridePropEntry {
    pub name: String,
    pub ty: String,
    pub required: bool,
}

/// Top-level JSON envelope for `schema variant`.
#[derive(Debug, Serialize)]
pub struct SchemaVariantOutput {
    pub schema: &'static str,
    pub summary: String,
    pub block_structure: String,
    pub variant_node: String,
    pub override_entry: String,
    pub override_props: Vec<SchemaOverridePropEntry>,
    pub example: String,
}

/// Top-level JSON envelope for `schema brand`.
#[derive(Debug, Serialize)]
pub struct SchemaBrandOutput {
    pub schema: &'static str,
    pub summary: String,
    pub placement: &'static str,
    pub child_nodes: Vec<SchemaBrandChildNode>,
    pub absent_means: &'static str,
    pub diagnostic_codes: Vec<SchemaBrandDiagCode>,
    pub example: &'static str,
}

/// One child-node descriptor in the `schema brand` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaBrandChildNode {
    pub node: &'static str,
    pub syntax: &'static str,
    pub description: &'static str,
}

/// One diagnostic code entry in the `schema brand` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaBrandDiagCode {
    pub code: &'static str,
    pub severity: &'static str,
    pub summary: &'static str,
}

// ── Fonts JSON types ──────────────────────────────────────────────────────────

/// Top-level JSON envelope for `zenith fonts --json`.
#[derive(Debug, Serialize)]
pub struct FontsOutput {
    pub schema: &'static str,
    /// Family names bundled in the binary (lowercase, sorted). These are portable:
    /// any machine with this Zenith binary will resolve them identically.
    pub bundled: Vec<String>,
    /// Family names found on this machine only (lowercase, sorted), after excluding
    /// any family already in `bundled`. Using these trips a `font.local` advisory
    /// and renders may differ on another machine.
    pub local: Vec<String>,
}

// ── Recipe inspect JSON types ─────────────────────────────────────────────────

/// A single `param` entry within a [`RecipeInspectJson`].
#[derive(Debug, Serialize)]
pub struct RecipeParamInspectJson {
    pub name: String,
    /// Canonical string representation of the parameter value: a token ref is
    /// rendered as `"<id>"`, a literal as its raw string, a dimension as
    /// `"(<unit>)<value>"`.
    pub value: String,
}

/// A single recipe entry in the `recipes` array of [`crate::commands::inspect::InspectOutput`].
#[derive(Debug, Serialize)]
pub struct RecipeInspectJson {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    pub params: Vec<RecipeParamInspectJson>,
    pub palette: Vec<String>,
    pub expanded: Vec<String>,
}
