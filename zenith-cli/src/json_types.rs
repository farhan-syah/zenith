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

/// A single node-kind detail entry in the `schema node <kind>` JSON output.
#[derive(Debug, Serialize)]
pub struct SchemaNodeDetail {
    pub kind: String,
    pub summary: String,
    pub attributes: Vec<String>,
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
}

/// Top-level JSON envelope for `schema page`, `schema asset`, `schema document`.
#[derive(Debug, Serialize)]
pub struct SchemaSurfaceOutput {
    pub schema: &'static str,
    /// Which non-node surface this describes: `"page"`, `"asset"`, or `"document"`.
    pub surface: &'static str,
    pub summary: String,
    pub attributes: Vec<String>,
}

// ── Agent-run inspect JSON types ──────────────────────────────────────────────

/// A single step entry within an [`AgentRunInspectJson`].
#[derive(Debug, Serialize)]
pub struct AgentStepInspectJson {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_version: Option<String>,
    pub affected_node_count: usize,
    pub diagnostic_count: usize,
}

/// A single agent-run entry in the `agent_runs` array of [`crate::commands::inspect::InspectOutput`].
#[derive(Debug, Serialize)]
pub struct AgentRunInspectJson {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    pub step_count: usize,
    pub steps: Vec<AgentStepInspectJson>,
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
