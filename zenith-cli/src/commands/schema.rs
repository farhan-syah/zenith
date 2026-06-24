//! Pure logic for `zenith schema`.
//!
//! The public entry points operate entirely on static schema data — no
//! filesystem I/O.  The caller (dispatch) is responsible for printing the
//! returned string and mapping the exit code.

use zenith_core::schema as core_schema;
use zenith_tx::schema as tx_schema;

use crate::commands::serialize_pretty;
use crate::json_types::{
    SchemaAttr, SchemaDiagnosticCode, SchemaDiagnosticsOutput, SchemaNodeDetail, SchemaNodeEntry,
    SchemaNodeOutput, SchemaNodesOutput, SchemaOpDetail, SchemaOpEntry, SchemaOpFieldEntry,
    SchemaOpOutput, SchemaOpsOutput, SchemaOverviewOutput, SchemaSurfaceOutput, SchemaTokenDetail,
    SchemaTokenEntry, SchemaTokenOutput, SchemaTokensOutput,
};

/// Precedence note shown on the `schema diagnostics` surface.
const DIAGNOSTICS_PRECEDENCE: &str = "Resolution today is the in-file `diagnostics { … }` \
block (last-wins per code); CLI flags and config-file overrides resolve in a later unit.";

// ── Public entry points ───────────────────────────────────────────────────────

/// Bare `zenith schema`: short overview with counts and drill-in hints.
///
/// Returns `(stdout, exit_code)`.
pub fn overview(json: bool) -> (String, u8) {
    let node_count = core_schema::node_kinds().len();
    let op_count = tx_schema::op_names().len();
    let token_type_count = core_schema::token_types().len();

    if json {
        let out = SchemaOverviewOutput {
            schema: "zenith-schema-v1",
            node_kinds: node_count,
            tx_ops: op_count,
            token_types: token_type_count,
        };
        (serialize_pretty(&out), 0)
    } else {
        let diag_count = core_schema::diagnostic_codes().len();
        let text = format!(
            "Zenith schema — {node_count} node kinds, {op_count} tx ops, \
             {token_type_count} token types, 3 non-node surfaces, \
             {diag_count} diagnostic codes\n\n\
             Drill in:\n  \
             zenith schema nodes              # list all node kinds\n  \
             zenith schema node <kind>        # attributes for one kind\n  \
             zenith schema ops                # list all tx ops\n  \
             zenith schema op <name>          # fields + example for one op\n  \
             zenith schema tokens             # list all token types\n  \
             zenith schema token <type>       # value form + children + example for one type\n  \
             zenith schema page               # page declaration attributes\n  \
             zenith schema asset              # asset declaration attributes\n  \
             zenith schema document           # document root attributes\n  \
             zenith schema diagnostics        # diagnostic-policy verbs + codes\n\n\
             Attribute types, required-ness, and valid values are enforced by \
             `zenith validate`."
        );
        (text, 0)
    }
}

/// `zenith schema nodes`: list all node kinds with their summaries.
///
/// Returns `(stdout, exit_code)`.
pub fn nodes(json: bool) -> (String, u8) {
    let kinds = core_schema::node_kinds();

    if json {
        let entries: Vec<SchemaNodeEntry> = kinds
            .iter()
            .map(|&kind| SchemaNodeEntry {
                kind: kind.to_owned(),
                // node_summary is always Some for every kind in node_kinds().
                summary: core_schema::node_summary(kind).unwrap_or("").to_owned(),
            })
            .collect();
        let out = SchemaNodesOutput {
            schema: "zenith-schema-v1",
            nodes: entries,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = String::from("node kinds:\n");
        for &kind in kinds {
            let summary = core_schema::node_summary(kind).unwrap_or("");
            text.push_str(&format!("  {kind:<12}  {summary}\n"));
        }
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema node <kind>`: detail for one node kind.
///
/// Returns `(stdout, exit_code)`. On unknown kind, exit_code is 1 and stdout
/// contains the error message (suitable for printing via the normal `println!`
/// path so the caller need not special-case stderr).
pub fn node_detail(kind: &str, json: bool) -> (String, u8) {
    let summary = match core_schema::node_summary(kind) {
        Some(s) => s,
        None => {
            let valid = core_schema::node_kinds().join(", ");
            let msg = format!("error: unknown node kind '{kind}'\nvalid kinds: {valid}");
            return (msg, 1);
        }
    };

    let attrs: Vec<SchemaAttr> = core_schema::node_attributes(kind)
        .iter()
        .map(|&a| SchemaAttr {
            name: a.to_owned(),
            ty: core_schema::attribute_type(a).to_owned(),
        })
        .collect();

    if json {
        let out = SchemaNodeOutput {
            schema: "zenith-schema-v1",
            node: SchemaNodeDetail {
                kind: kind.to_owned(),
                summary: summary.to_owned(),
                attributes: attrs,
            },
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("{kind}: {summary}\n");
        if attrs.is_empty() {
            text.push_str("  (no fixed attribute list)\n");
        } else {
            text.push_str("Attributes:\n");
            text.push_str(&format_attr_table(&attrs));
        }
        text.push_str(
            "\nNote: required-ness and full valid values are enforced by\n\
             `zenith validate` (the authoritative diagnostic loop).",
        );
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema ops`: list all tx ops with their summaries.
///
/// Returns `(stdout, exit_code)`.
pub fn ops(json: bool) -> (String, u8) {
    let names = tx_schema::op_names();

    if json {
        let entries: Vec<SchemaOpEntry> = names
            .iter()
            .map(|&name| SchemaOpEntry {
                op: name.to_owned(),
                summary: tx_schema::op_summary(name).unwrap_or("").to_owned(),
            })
            .collect();
        let out = SchemaOpsOutput {
            schema: "zenith-schema-v1",
            ops: entries,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = String::from("tx ops:\n");
        for &name in names {
            let summary = tx_schema::op_summary(name).unwrap_or("");
            text.push_str(&format!("  {name:<24}  {summary}\n"));
        }
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema op <name>`: full detail for one tx op (summary + fields + example).
///
/// Returns `(stdout, exit_code)`. On unknown name, exit_code is 1.
pub fn op_detail(name: &str, json: bool) -> (String, u8) {
    let summary = match tx_schema::op_summary(name) {
        Some(s) => s,
        None => {
            let valid = tx_schema::op_names().join(", ");
            let msg = format!("error: unknown op '{name}'\nvalid ops: {valid}");
            return (msg, 1);
        }
    };

    // op_fields and op_example are always Some when op_summary is Some
    // (enforced by the drift-guard tests in zenith-tx).
    let fields = tx_schema::op_fields(name).unwrap_or(&[]);
    let example = tx_schema::op_example(name).unwrap_or("");

    if json {
        let field_entries: Vec<SchemaOpFieldEntry> = fields
            .iter()
            .map(|f| SchemaOpFieldEntry {
                name: f.name.to_owned(),
                ty: f.ty.to_owned(),
                required: f.required,
            })
            .collect();
        let out = SchemaOpOutput {
            schema: "zenith-schema-v1",
            op: SchemaOpDetail {
                op: name.to_owned(),
                summary: summary.to_owned(),
                fields: field_entries,
                example: example.to_owned(),
            },
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("{name}: {summary}\n");
        if fields.is_empty() {
            text.push_str("\nFields: (none — this op carries no fields beyond the \"op\" tag)\n");
        } else {
            text.push_str("\nFields:\n");
            for f in fields {
                let req = if f.required { ", required" } else { "" };
                text.push_str(&format!("  {:<20}  ({}{req})\n", f.name, f.ty));
            }
        }
        text.push_str(&format!("\nExample:\n  {example}"));
        (text, 0)
    }
}

// ── Token type formatters ─────────────────────────────────────────────────────

/// `zenith schema tokens`: list all token types with their summaries.
///
/// Returns `(stdout, exit_code)`.
pub fn tokens(json: bool) -> (String, u8) {
    let types = core_schema::token_types();

    if json {
        let entries: Vec<SchemaTokenEntry> = types
            .iter()
            .map(|&ty| SchemaTokenEntry {
                ty: ty.to_owned(),
                // token_type_summary is always Some for every type in token_types().
                summary: core_schema::token_type_summary(ty).unwrap_or("").to_owned(),
            })
            .collect();
        let out = SchemaTokensOutput {
            schema: "zenith-schema-v1",
            token_types: entries,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = String::from("token types:\n");
        for &ty in types {
            let summary = core_schema::token_type_summary(ty).unwrap_or("");
            text.push_str(&format!("  {ty:<12}  {summary}\n"));
        }
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema token <type>`: full detail for one token type.
///
/// Returns `(stdout, exit_code)`. On unknown type, exit_code is 1 and stdout
/// contains the error message.
pub fn token_detail(ty: &str, json: bool) -> (String, u8) {
    let desc = match core_schema::token_type_descriptor(ty) {
        Some(d) => d,
        None => {
            let valid = core_schema::token_types().join(", ");
            let msg = format!("error: unknown token type '{ty}'\nvalid types: {valid}");
            return (msg, 1);
        }
    };

    if json {
        let out = SchemaTokenOutput {
            schema: "zenith-schema-v1",
            token: SchemaTokenDetail {
                ty: desc.type_name.to_owned(),
                summary: desc.summary.to_owned(),
                value_form: desc.value_form.to_owned(),
                child_nodes: desc.child_nodes.to_owned(),
                example: desc.example.to_owned(),
            },
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("{}: {}\n", desc.type_name, desc.summary);
        if !desc.value_form.is_empty() {
            text.push_str(&format!("\nValue form:\n  {}\n", desc.value_form));
        }
        if !desc.child_nodes.is_empty() {
            text.push_str(&format!("\nChild nodes:\n  {}\n", desc.child_nodes));
        }
        text.push_str(&format!(
            "\nExample:\n  {}",
            desc.example.replace('\n', "\n  ")
        ));
        (text, 0)
    }
}

// ── Non-node surface formatters ───────────────────────────────────────────────

/// `zenith schema page`: summary + recognized attributes for a page declaration.
///
/// Returns `(stdout, exit_code)`.
pub fn page(json: bool) -> (String, u8) {
    surface_detail(
        "page",
        core_schema::page_summary(),
        core_schema::page_attributes(),
        json,
    )
}

/// `zenith schema asset`: summary + recognized attributes for an asset declaration.
///
/// Returns `(stdout, exit_code)`.
pub fn asset(json: bool) -> (String, u8) {
    surface_detail(
        "asset",
        core_schema::asset_summary(),
        core_schema::asset_attributes(),
        json,
    )
}

/// `zenith schema document`: summary + recognized attributes for the document root.
///
/// Returns `(stdout, exit_code)`.
pub fn document(json: bool) -> (String, u8) {
    surface_detail(
        "document",
        core_schema::document_summary(),
        core_schema::document_attributes(),
        json,
    )
}

/// Shared formatter for non-node surfaces (page / asset / document).
fn surface_detail(
    surface: &'static str,
    summary: &'static str,
    raw_attrs: Vec<&'static str>,
    json: bool,
) -> (String, u8) {
    let attrs: Vec<SchemaAttr> = raw_attrs
        .iter()
        .map(|&a| SchemaAttr {
            name: a.to_owned(),
            ty: core_schema::attribute_type(a).to_owned(),
        })
        .collect();

    if json {
        let out = SchemaSurfaceOutput {
            schema: "zenith-schema-v1",
            surface,
            summary: summary.to_owned(),
            attributes: attrs,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("{surface}: {summary}\n");
        if attrs.is_empty() {
            text.push_str("  (no fixed attribute list)\n");
        } else {
            text.push_str("Attributes:\n");
            text.push_str(&format_attr_table(&attrs));
        }
        text.push_str(
            "\nNote: required-ness and full valid values are enforced by\n\
             `zenith validate` (the authoritative diagnostic loop).",
        );
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema diagnostics`: the in-file diagnostic-policy verbs and the
/// governable diagnostic codes.
///
/// Returns `(stdout, exit_code)`.
pub fn diagnostics(json: bool) -> (String, u8) {
    let summary = core_schema::diagnostics_summary();
    let verbs = core_schema::diagnostics_verbs();
    let catalog = core_schema::diagnostic_codes();

    if json {
        let codes: Vec<SchemaDiagnosticCode> = catalog
            .iter()
            .map(|info| SchemaDiagnosticCode {
                code: info.code.to_owned(),
                severity: crate::json_types::severity_str(&info.severity).to_owned(),
                summary: info.summary.to_owned(),
                governable: info.is_governable(),
            })
            .collect();
        let out = SchemaDiagnosticsOutput {
            schema: "zenith-schema-v1",
            summary: summary.to_owned(),
            verbs: verbs.iter().map(|&v| v.to_owned()).collect(),
            precedence: DIAGNOSTICS_PRECEDENCE,
            codes,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("diagnostics: {summary}\n\n");

        text.push_str("Policy verbs (in a root `diagnostics { … }` block):\n");
        text.push_str("  allow \"<code>\"  —  suppress this advisory/warning\n");
        text.push_str("  deny  \"<code>\"  —  elevate to a blocking Error (CI gate)\n");
        text.push_str("  warn  \"<code>\"  —  force to a Warning\n\n");

        text.push_str("Precedence: ");
        text.push_str(DIAGNOSTICS_PRECEDENCE);
        text.push_str("\n\n");

        // Governable codes (Warning/Advisory) — what a policy can actually adjust.
        text.push_str("Governable codes (code · severity · summary):\n");
        let governable: Vec<&_> = catalog.iter().filter(|i| i.is_governable()).collect();
        let col_width = governable.iter().map(|i| i.code.len()).max().unwrap_or(0);
        for info in &governable {
            text.push_str(&format!(
                "  {:<col_width$}  {:<9}  {}\n",
                info.code,
                crate::json_types::severity_str(&info.severity),
                info.summary,
                col_width = col_width,
            ));
        }

        text.push_str(
            "\nNote: integrity Errors cannot be suppressed or weakened — `allow`/`warn` on an \
             Error code is reported as `policy.ineffective_on_error`.",
        );
        (text.trim_end().to_owned(), 0)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Format a list of attributes as a two-column table: `name  —  type`.
///
/// The name column is left-padded by 2 spaces and right-padded to the longest
/// name in the list so the `—` separators align vertically.
fn format_attr_table(attrs: &[SchemaAttr]) -> String {
    let col_width = attrs.iter().map(|a| a.name.len()).max().unwrap_or(0);

    let mut out = String::new();
    for attr in attrs {
        out.push_str(&format!(
            "  {:<col_width$}  —  {}\n",
            attr.name,
            attr.ty,
            col_width = col_width,
        ));
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_human_contains_counts() {
        let (text, code) = overview(false);
        assert_eq!(code, 0);
        assert!(text.contains("node kinds"), "must mention node kinds");
        assert!(text.contains("tx ops"), "must mention tx ops");
    }

    #[test]
    fn overview_json_schema_field() {
        let (text, code) = overview(true);
        assert_eq!(code, 0);
        assert!(
            text.contains("zenith-schema-v1"),
            "JSON must carry schema field"
        );
        assert!(
            text.contains("node_kinds"),
            "JSON must carry node_kinds count"
        );
    }

    #[test]
    fn nodes_human_contains_rect() {
        let (text, code) = nodes(false);
        assert_eq!(code, 0);
        assert!(text.contains("rect"), "must list rect kind");
        assert!(text.contains("Rectangle"), "must include rect summary");
    }

    #[test]
    fn nodes_json_schema_field() {
        let (text, code) = nodes(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"kind\""));
    }

    #[test]
    fn node_detail_known_kind() {
        let (text, code) = node_detail("rect", false);
        assert_eq!(code, 0);
        assert!(text.contains("rect"), "must name the kind");
        assert!(text.contains("Attributes:"), "must list attributes");
        assert!(text.contains("fill"), "rect must have a fill attribute");
        assert!(
            text.contains("token ref"),
            "fill must show its type hint (token ref)"
        );
        assert!(text.contains("—"), "attributes must use — separator");
        assert!(
            text.contains("zenith validate"),
            "must mention zenith validate for types"
        );
    }

    #[test]
    fn node_detail_human_shows_name_and_type() {
        // Human output: each attribute line is "  <name>  —  <type>"
        let (text, code) = node_detail("rect", false);
        assert_eq!(code, 0);
        // x is a px literal, fill is a token ref.
        assert!(text.contains("x  "), "must list x attribute; got:\n{text}");
        assert!(
            text.contains("px literal"),
            "x must show px literal type; got:\n{text}"
        );
        assert!(
            text.contains("token ref: color/gradient"),
            "fill must show token ref type; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_json_known_kind() {
        let (text, code) = node_detail("pattern", true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"kind\""));
        assert!(text.contains("\"attributes\""));
        // New shape: attributes is an array of {name, ty} objects.
        assert!(
            text.contains("\"name\""),
            "attribute objects must have name field"
        );
        assert!(
            text.contains("\"ty\""),
            "attribute objects must have ty field"
        );
    }

    #[test]
    fn node_detail_json_attr_has_type_hint() {
        let (text, code) = node_detail("rect", true);
        assert_eq!(code, 0);
        // fill must appear with its type.
        assert!(
            text.contains("\"fill\""),
            "fill attribute must appear; got:\n{text}"
        );
        assert!(
            text.contains("token ref"),
            "fill type must be a token ref; got:\n{text}"
        );
        // x must appear with px literal type.
        assert!(
            text.contains("px literal"),
            "x must have px literal type; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_unknown_kind_returns_error() {
        let (text, code) = node_detail("not-a-kind", false);
        assert_eq!(code, 1);
        assert!(
            text.contains("unknown node kind"),
            "must report unknown kind"
        );
        assert!(text.contains("valid kinds"), "must list valid kinds");
    }

    #[test]
    fn ops_human_contains_set_fill() {
        let (text, code) = ops(false);
        assert_eq!(code, 0);
        assert!(text.contains("set_fill"), "must list set_fill op");
    }

    #[test]
    fn ops_json_schema_field() {
        let (text, code) = ops(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"op\""));
    }

    #[test]
    fn op_detail_known_op() {
        let (text, code) = op_detail("set_fill", false);
        assert_eq!(code, 0);
        assert!(text.contains("set_fill"), "must name the op");
        assert!(text.contains("fill"), "must mention the fill field");
        assert!(text.contains("Fields:"), "must include Fields section");
        assert!(text.contains("Example:"), "must include Example section");
    }

    #[test]
    fn op_detail_json_known_op() {
        let (text, code) = op_detail("add_node", true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"op\""));
        assert!(
            text.contains("\"fields\""),
            "JSON must include fields array"
        );
        assert!(
            text.contains("\"example\""),
            "JSON must include example string"
        );
    }

    #[test]
    fn op_detail_detach_pattern_human() {
        let (text, code) = op_detail("detach_pattern", false);
        assert_eq!(code, 0);
        assert!(text.contains("detach_pattern"));
        assert!(text.contains("Fields:"));
        assert!(text.contains("node"));
        assert!(text.contains("Example:"));
    }

    #[test]
    fn op_detail_set_fill_json_has_node_and_fill_fields() {
        let (text, code) = op_detail("set_fill", true);
        assert_eq!(code, 0);
        assert!(text.contains("\"node\""), "fields must include node");
        assert!(text.contains("\"fill\""), "fields must include fill");
        assert!(text.contains("token ref"), "fill type must be token ref");
        assert!(
            text.contains("color.brand"),
            "example must use realistic value"
        );
    }

    #[test]
    fn op_detail_unknown_op_returns_error() {
        let (text, code) = op_detail("not_an_op", false);
        assert_eq!(code, 1);
        assert!(text.contains("unknown op"), "must report unknown op");
        assert!(text.contains("valid ops"), "must list valid ops");
    }

    #[test]
    fn overview_mentions_new_surfaces() {
        let (text, code) = overview(false);
        assert_eq!(code, 0);
        assert!(text.contains("page"), "overview must mention page surface");
        assert!(
            text.contains("asset"),
            "overview must mention asset surface"
        );
        assert!(
            text.contains("document"),
            "overview must mention document surface"
        );
    }

    #[test]
    fn page_human_contains_geometry_attrs() {
        let (text, code) = page(false);
        assert_eq!(code, 0);
        assert!(text.contains("page"), "must name the surface");
        assert!(text.contains("Attributes:"), "must list attributes");
        assert!(text.contains("w"), "page must have w attribute");
        assert!(text.contains("h"), "page must have h attribute");
        assert!(text.contains("—"), "attributes must use — separator");
        assert!(
            text.contains("px literal"),
            "w/h must show px literal type hint"
        );
        assert!(
            text.contains("zenith validate"),
            "must mention zenith validate"
        );
    }

    #[test]
    fn page_json_schema_field() {
        let (text, code) = page(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"surface\""));
        assert!(text.contains("\"attributes\""));
        assert!(text.contains("\"page\""));
        // New shape: attributes is an array of {name, ty} objects.
        assert!(
            text.contains("\"name\""),
            "attribute objects must have name field"
        );
        assert!(
            text.contains("\"ty\""),
            "attribute objects must have ty field"
        );
    }

    #[test]
    fn asset_human_contains_provenance_attrs() {
        let (text, code) = asset(false);
        assert_eq!(code, 0);
        assert!(text.contains("asset"), "must name the surface");
        assert!(text.contains("sha256"), "asset must include sha256");
        assert!(text.contains("ai-prompt"), "asset must include ai-prompt");
    }

    #[test]
    fn asset_json_schema_field() {
        let (text, code) = asset(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"asset\""));
    }

    #[test]
    fn document_human_contains_root_attrs() {
        let (text, code) = document(false);
        assert_eq!(code, 0);
        assert!(text.contains("document"), "must name the surface");
        assert!(
            text.contains("colorspace"),
            "document must include colorspace"
        );
        assert!(text.contains("doc-id"), "document must include doc-id");
    }

    #[test]
    fn document_json_schema_field() {
        let (text, code) = document(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"document\""));
    }

    #[test]
    fn overview_mentions_token_types() {
        let (text, code) = overview(false);
        assert_eq!(code, 0);
        assert!(
            text.contains("token types"),
            "overview must mention token types; got:\n{text}"
        );
        assert!(
            text.contains("zenith schema tokens"),
            "overview must mention 'zenith schema tokens'; got:\n{text}"
        );
        assert!(
            text.contains("zenith schema token"),
            "overview must mention 'zenith schema token <type>'; got:\n{text}"
        );
    }

    #[test]
    fn overview_json_has_token_types_count() {
        let (text, code) = overview(true);
        assert_eq!(code, 0);
        assert!(
            text.contains("token_types"),
            "JSON overview must carry token_types count; got:\n{text}"
        );
    }

    #[test]
    fn tokens_human_lists_all_types() {
        let (text, code) = tokens(false);
        assert_eq!(code, 0);
        assert!(text.contains("color"), "must list color type");
        assert!(text.contains("gradient"), "must list gradient type");
        assert!(text.contains("shadow"), "must list shadow type");
        assert!(text.contains("filter"), "must list filter type");
        assert!(text.contains("mask"), "must list mask type");
        assert!(text.contains("dimension"), "must list dimension type");
        assert!(text.contains("number"), "must list number type");
        assert!(text.contains("fontFamily"), "must list fontFamily type");
        assert!(text.contains("fontWeight"), "must list fontWeight type");
    }

    #[test]
    fn tokens_json_schema_field() {
        let (text, code) = tokens(true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"token_types\""));
        assert!(text.contains("\"ty\""));
        assert!(text.contains("\"summary\""));
    }

    #[test]
    fn token_detail_color_human() {
        let (text, code) = token_detail("color", false);
        assert_eq!(code, 0);
        assert!(text.contains("color"), "must name the type");
        assert!(
            text.contains("Value form:"),
            "must include Value form section"
        );
        assert!(text.contains("#rrggbb"), "must describe hex color form");
        assert!(text.contains("Example:"), "must include Example section");
    }

    #[test]
    fn token_detail_gradient_human() {
        let (text, code) = token_detail("gradient", false);
        assert_eq!(code, 0);
        assert!(text.contains("gradient"), "must name the type");
        assert!(
            text.contains("Child nodes:"),
            "gradient must include Child nodes section"
        );
        assert!(text.contains("stop"), "gradient must describe stop child");
        assert!(text.contains("Example:"), "must include Example section");
    }

    #[test]
    fn token_detail_shadow_human() {
        let (text, code) = token_detail("shadow", false);
        assert_eq!(code, 0);
        assert!(text.contains("shadow"), "must name the type");
        assert!(
            text.contains("Child nodes:"),
            "shadow must include Child nodes section"
        );
        assert!(text.contains("layer"), "shadow must describe layer child");
    }

    #[test]
    fn token_detail_json_has_all_fields() {
        let (text, code) = token_detail("gradient", true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"token\""));
        assert!(text.contains("\"ty\""));
        assert!(text.contains("\"summary\""));
        assert!(text.contains("\"value_form\""));
        assert!(text.contains("\"child_nodes\""));
        assert!(text.contains("\"example\""));
    }

    #[test]
    fn token_detail_unknown_type_returns_error() {
        let (text, code) = token_detail("bogus", false);
        assert_eq!(code, 1);
        assert!(
            text.contains("unknown token type"),
            "must report unknown type"
        );
        assert!(text.contains("valid types"), "must list valid types");
    }

    #[test]
    fn token_detail_fontweight_no_value_form_confusion() {
        // fontWeight must explicitly say bare integer, NOT a string or dimension.
        let (text, code) = token_detail("fontWeight", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("700"),
            "fontWeight example must use a bare integer"
        );
        // The value form must not suggest string or dimension syntax.
        assert!(
            !text.contains("\"700\""),
            "fontWeight must not suggest string form"
        );
    }
}
