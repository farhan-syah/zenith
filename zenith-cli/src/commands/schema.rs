//! Pure logic for `zenith schema`.
//!
//! The public entry points operate entirely on static schema data — no
//! filesystem I/O.  The caller (dispatch) is responsible for printing the
//! returned string and mapping the exit code.

use zenith_core::schema as core_schema;
use zenith_tx::schema as tx_schema;

use crate::commands::serialize_pretty;
use crate::json_types::{
    SchemaNodeDetail, SchemaNodeEntry, SchemaNodeOutput, SchemaNodesOutput, SchemaOpDetail,
    SchemaOpEntry, SchemaOpFieldEntry, SchemaOpOutput, SchemaOpsOutput, SchemaOverviewOutput,
    SchemaSurfaceOutput,
};

// ── Public entry points ───────────────────────────────────────────────────────

/// Bare `zenith schema`: short overview with counts and drill-in hints.
///
/// Returns `(stdout, exit_code)`.
pub fn overview(json: bool) -> (String, u8) {
    let node_count = core_schema::node_kinds().len();
    let op_count = tx_schema::op_names().len();

    if json {
        let out = SchemaOverviewOutput {
            schema: "zenith-schema-v1",
            node_kinds: node_count,
            tx_ops: op_count,
        };
        (serialize_pretty(&out), 0)
    } else {
        let text = format!(
            "Zenith schema — {node_count} node kinds, {op_count} tx ops, 3 non-node surfaces\n\n\
             Drill in:\n  \
             zenith schema nodes              # list all node kinds\n  \
             zenith schema node <kind>        # attributes for one kind\n  \
             zenith schema ops                # list all tx ops\n  \
             zenith schema op <name>          # fields + example for one op\n  \
             zenith schema page               # page declaration attributes\n  \
             zenith schema asset              # asset declaration attributes\n  \
             zenith schema document           # document root attributes\n\n\
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

    let attrs: Vec<String> = core_schema::node_attributes(kind)
        .iter()
        .map(|&a| a.to_owned())
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
            // Wrap into lines of at most ~72 chars of attribute names.
            let line = attrs.join(", ");
            // Simple line-wrap at word boundaries (commas).
            text.push_str(&wrap_attr_line(&line, 2, 72));
            text.push('\n');
        }
        text.push_str(
            "\nNote: attribute types, required-ness, and valid values are\n\
             enforced by `zenith validate` (the authoritative diagnostic loop).",
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
    attrs: Vec<&'static str>,
    json: bool,
) -> (String, u8) {
    if json {
        let out = SchemaSurfaceOutput {
            schema: "zenith-schema-v1",
            surface,
            summary: summary.to_owned(),
            attributes: attrs.iter().map(|&a| a.to_owned()).collect(),
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("{surface}: {summary}\n");
        if attrs.is_empty() {
            text.push_str("  (no fixed attribute list)\n");
        } else {
            text.push_str("Attributes:\n");
            let line = attrs.join(", ");
            text.push_str(&wrap_attr_line(&line, 2, 72));
            text.push('\n');
        }
        text.push_str(
            "\nNote: attribute types, required-ness, and valid values are\n\
             enforced by `zenith validate` (the authoritative diagnostic loop).",
        );
        (text.trim_end().to_owned(), 0)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Indent and soft-wrap a comma-separated attribute list to `max_width` columns.
///
/// Each wrapped line is prefixed with `indent` spaces.  Words are never split;
/// breaking only happens at ", " boundaries.
fn wrap_attr_line(line: &str, indent: usize, max_width: usize) -> String {
    let prefix = " ".repeat(indent);
    let usable = if max_width > indent {
        max_width - indent
    } else {
        max_width
    };

    let mut out = String::new();
    let mut current = String::new();

    for part in line.split(", ") {
        if current.is_empty() {
            current.push_str(part);
        } else if current.len() + 2 + part.len() <= usable {
            current.push_str(", ");
            current.push_str(part);
        } else {
            out.push_str(&prefix);
            out.push_str(&current);
            out.push('\n');
            current = part.to_owned();
        }
    }
    if !current.is_empty() {
        out.push_str(&prefix);
        out.push_str(&current);
        out.push('\n');
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
            text.contains("zenith validate"),
            "must mention zenith validate for types"
        );
    }

    #[test]
    fn node_detail_json_known_kind() {
        let (text, code) = node_detail("pattern", true);
        assert_eq!(code, 0);
        assert!(text.contains("zenith-schema-v1"));
        assert!(text.contains("\"kind\""));
        assert!(text.contains("\"attributes\""));
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
}
