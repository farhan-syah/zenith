//! Pure logic for `zenith schema`.
//!
//! The public entry points operate entirely on static schema data — no
//! filesystem I/O.  The caller (dispatch) is responsible for printing the
//! returned string and mapping the exit code.

use zenith_core::schema as core_schema;
use zenith_tx::schema as tx_schema;

use crate::commands::serialize_pretty;
use crate::json_types::{
    SchemaAttr, SchemaBrandChildNode, SchemaBrandDiagCode, SchemaBrandOutput, SchemaDiagnosticCode,
    SchemaDiagnosticsOutput, SchemaNodeContent, SchemaNodeDetail, SchemaNodeEntry,
    SchemaNodeOutput, SchemaNodesOutput, SchemaOpDetail, SchemaOpEntry, SchemaOpFieldEntry,
    SchemaOpOutput, SchemaOpsOutput, SchemaOverridePropEntry, SchemaOverviewOutput,
    SchemaSurfaceOutput, SchemaTokenDetail, SchemaTokenEntry, SchemaTokenOutput,
    SchemaTokensOutput, SchemaVariantOutput,
};

/// Precedence note shown on the `schema diagnostics` surface.
const DIAGNOSTICS_PRECEDENCE: &str = "policy resolution is last-wins across global config, local config, in-file diagnostics, then CLI flags";

const DIAGNOSTICS_SYNTAX: &[&str] = &[
    "allow \"<code>\"",
    "allow \"<code>\" \"<subject-id>\"",
    "allow \"<code>\" \"<subject-id>\" \"<subject-id>\"",
    "deny \"<code>\"",
    "warn \"<code>\"",
];

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
             {token_type_count} token types, 7 non-node surfaces, \
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
             zenith schema variant            # variants block + override entry structure\n  \
             zenith schema diagnostics        # diagnostic-policy verbs + codes\n  \
             zenith schema brand              # brand-contract block (allowed colors/fonts/weights)\n  \
             zenith schema block              # block role declaration: vocab, props, scopes\n\n\
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
            // Provide a targeted hint for the two most-common near-misses.
            let hint = match kind {
                "override" | "variant" => {
                    "\nhint: 'override' and 'variant' are not node kinds — \
                     see `zenith schema variant` for the variants block and override entry."
                }
                _ => "",
            };
            let msg = format!("error: unknown node kind '{kind}'\nvalid kinds: {valid}{hint}");
            return (msg, 1);
        }
    };

    let attrs: Vec<SchemaAttr> = core_schema::node_attributes(kind)
        .iter()
        .map(|&a| SchemaAttr {
            name: a.to_owned(),
            ty: core_schema::attribute_type_for_kind(kind, a).to_owned(),
        })
        .collect();

    let content_desc = core_schema::node_content(kind);
    let example = core_schema::node_example(kind);

    if json {
        let content = content_desc.map(|d| SchemaNodeContent {
            description: d.description.to_owned(),
            example: d.example.to_owned(),
        });
        let out = SchemaNodeOutput {
            schema: "zenith-schema-v1",
            node: SchemaNodeDetail {
                kind: kind.to_owned(),
                summary: summary.to_owned(),
                attributes: attrs,
                example: example.map(str::to_owned),
                content,
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
        if let Some(d) = content_desc {
            text.push_str("\nContent:\n");
            text.push_str(&format!("  {}\n", d.description));
            text.push_str("  Example:\n");
            for line in d.example.lines() {
                text.push_str(&format!("    {line}\n"));
            }
        }
        if let Some(ex) = example {
            text.push_str("\nExample:\n");
            for line in ex.lines() {
                text.push_str(&format!("  {line}\n"));
            }
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

/// `zenith schema variant`: descriptor for the `variants` block and `override` entry.
///
/// Returns `(stdout, exit_code)`.
pub fn variant(json: bool) -> (String, u8) {
    let desc = core_schema::variant_descriptor();

    if json {
        let props: Vec<SchemaOverridePropEntry> = desc
            .override_props
            .iter()
            .map(|&(name, ty, required)| SchemaOverridePropEntry {
                name: name.to_owned(),
                ty: ty.to_owned(),
                required,
            })
            .collect();
        let out = SchemaVariantOutput {
            schema: "zenith-schema-v1",
            summary: desc.summary.to_owned(),
            block_structure: desc.block_structure.to_owned(),
            variant_node: desc.variant_node.to_owned(),
            override_entry: desc.override_entry.to_owned(),
            override_props: props,
            example: desc.example.to_owned(),
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("variant: {}\n", desc.summary);

        text.push_str(&format!("\nBlock structure:\n  {}\n", desc.block_structure));
        text.push_str(&format!(
            "\nvariant node:\n  {}\n",
            desc.variant_node.replace('\n', "\n  ")
        ));
        text.push_str(&format!(
            "\noverride entry:\n  {}\n",
            desc.override_entry.replace('\n', "\n  ")
        ));

        text.push_str("\nOverride properties:\n");
        let col_width = desc
            .override_props
            .iter()
            .map(|(n, _, _)| n.len())
            .max()
            .unwrap_or(0);
        for &(name, ty, required) in desc.override_props {
            let req = if required { ", required" } else { "" };
            text.push_str(&format!(
                "  {:<col_width$}  —  ({ty}{req})\n",
                name,
                col_width = col_width,
            ));
        }

        text.push_str(&format!(
            "\nExample:\n  {}",
            desc.example.replace('\n', "\n  ")
        ));
        (text, 0)
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
            syntax: DIAGNOSTICS_SYNTAX.iter().map(|&s| s.to_owned()).collect(),
            precedence: DIAGNOSTICS_PRECEDENCE,
            codes,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("diagnostics: {summary}\n\n");

        text.push_str("Policy verbs (in a root `diagnostics { … }` block):\n");
        text.push_str("  allow \"<code>\"  —  suppress this advisory/warning\n");
        text.push_str(
            "  allow \"<code>\" \"<subject-id>\" [\"<subject-id>\" …]  —  suppress only listed subjects\n",
        );
        text.push_str("  deny  \"<code>\"  —  elevate to a blocking Error (CI gate)\n");
        text.push_str("  warn  \"<code>\"  —  force to a Warning\n\n");

        text.push_str("Examples:\n");
        text.push_str("  allow \"layout.off_canvas\"\n");
        text.push_str("  allow \"layout.off_canvas\" \"bg.glow\" \"bg.rim\"\n\n");

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
             Error code is reported as `policy.ineffective_on_error`. Only governable \
             (Warning/Advisory) codes are listed above; for the COMPLETE catalog including the \
             always-enforced Error codes (e.g. `token.raw_visual_literal`), run \
             `zenith schema diagnostics --json`.",
        );
        (text.trim_end().to_owned(), 0)
    }
}

/// `zenith schema brand`: structure and semantics of the top-level `brand { … }` block.
///
/// Returns `(stdout, exit_code)`.
pub fn brand(json: bool) -> (String, u8) {
    const SUMMARY: &str = "Declare the allowed palette, fonts, and weights for this document; \
        resolved token values outside the contract emit Warnings that can be elevated to \
        blocking Errors for a CI gate.";

    const PLACEMENT: &str = "Top-level child of the root `zenith version=1 { … }` node, \
        sibling of `tokens`, `assets`, and `document`. At most one `brand { … }` block \
        per document.";

    const ABSENT_MEANS: &str = "An absent child node means that category is UNCONSTRAINED — \
        omitting `colors` allows any color; omitting `fonts` allows any font family; \
        omitting `weights` allows any weight. A completely empty `brand {}` block constrains \
        nothing.";

    const CHILD_NODES: &[SchemaBrandChildNode] = &[
        SchemaBrandChildNode {
            node: "colors",
            syntax: r##"colors "#rrggbb" "#rrggbb" …"##,
            description: "Allowed sRGB hex colors (case-insensitive). Color tokens and the \
                sRGB-equivalent of CMYK tokens are compared against this list. Any resolved \
                color token whose value is absent from this set emits `brand.color_off_palette`.",
        },
        SchemaBrandChildNode {
            node: "fonts",
            syntax: r#"fonts "Family Name" "Another Family" …"#,
            description: "Allowed font family names. Any resolved fontFamily token whose value \
                is not in this set emits `brand.font_not_allowed`.",
        },
        SchemaBrandChildNode {
            node: "weights",
            syntax: "weights 400 700 …",
            description: "Allowed font weights as bare integers (100–900 in multiples of 100). \
                Any resolved fontWeight token whose value is not in this set emits \
                `brand.weight_not_allowed`.",
        },
    ];

    const DIAG_CODES: &[SchemaBrandDiagCode] = &[
        SchemaBrandDiagCode {
            code: "brand.color_off_palette",
            severity: "warning",
            summary: "Resolved color token value is not in the declared brand palette.",
        },
        SchemaBrandDiagCode {
            code: "brand.font_not_allowed",
            severity: "warning",
            summary: "Resolved fontFamily token value is not in the declared brand font list.",
        },
        SchemaBrandDiagCode {
            code: "brand.weight_not_allowed",
            severity: "warning",
            summary: "Resolved fontWeight token value is not in the declared brand weight list.",
        },
    ];

    const EXAMPLE: &str = concat!(
        "zenith version=1 {\n",
        "  brand {\n",
        "    colors \"#0b1f33\" \"#1b6cf0\" \"#ffffff\"\n",
        "    fonts \"Noto Sans\"\n",
        "    weights 400 700\n",
        "  }\n",
        "  tokens format=\"zenith-token-v1\" {\n",
        "    token id=\"color.primary\" type=\"color\" value=\"#1b6cf0\"\n",
        "    token id=\"color.bg\"      type=\"color\" value=\"#ffffff\"\n",
        "    token id=\"font.body\"     type=\"fontFamily\" value=\"Noto Sans\"\n",
        "    token id=\"weight.bold\"   type=\"fontWeight\" value=700\n",
        "  }\n",
        "  document id=\"doc\" title=\"Brand demo\" {}\n",
        "}\n",
        "\n",
        "# CI gate — make off-contract values block the build:\n",
        "#   zenith validate doc.zen --deny brand.color_off_palette\n",
        "#\n",
        "# Or declare the policy in-file:\n",
        "#   diagnostics { deny \"brand.color_off_palette\" }"
    );

    if json {
        let child_nodes: Vec<SchemaBrandChildNode> = CHILD_NODES
            .iter()
            .map(|n| SchemaBrandChildNode {
                node: n.node,
                syntax: n.syntax,
                description: n.description,
            })
            .collect();
        let diag_codes: Vec<SchemaBrandDiagCode> = DIAG_CODES
            .iter()
            .map(|d| SchemaBrandDiagCode {
                code: d.code,
                severity: d.severity,
                summary: d.summary,
            })
            .collect();
        let out = SchemaBrandOutput {
            schema: "zenith-schema-v1",
            summary: SUMMARY.to_owned(),
            placement: PLACEMENT,
            child_nodes,
            absent_means: ABSENT_MEANS,
            diagnostic_codes: diag_codes,
            example: EXAMPLE,
        };
        (serialize_pretty(&out), 0)
    } else {
        let mut text = format!("brand: {SUMMARY}\n");

        text.push_str(&format!("\nPlacement:\n  {PLACEMENT}\n"));

        text.push_str("\nChild nodes (all optional):\n");
        for node in CHILD_NODES {
            text.push_str(&format!(
                "  {:<8}  syntax:  {}\n           {}\n",
                node.node, node.syntax, node.description
            ));
        }

        text.push_str(&format!("\nAbsent-child rule:\n  {ABSENT_MEANS}\n"));

        text.push_str("\nDiagnostic codes (Warning by default):\n");
        let col = DIAG_CODES.iter().map(|d| d.code.len()).max().unwrap_or(0);
        for d in DIAG_CODES {
            text.push_str(&format!(
                "  {:<col$}  —  {}\n",
                d.code,
                d.summary,
                col = col,
            ));
        }

        text.push_str(
            "\nCI gate:\n  \
            Elevate to blocking Errors with `--deny <code>` on the CLI:\n    \
            zenith validate doc.zen --deny brand.color_off_palette\n  \
            Or declare the policy in-file (cross-reference `zenith schema diagnostics`):\n    \
            diagnostics { deny \"brand.color_off_palette\" }\n",
        );

        text.push_str(&format!("\nExample:\n  {}", EXAMPLE.replace('\n', "\n  ")));
        (text, 0)
    }
}

/// `zenith schema block`: role vocabulary, properties, scopes, and cascade for
/// the `block role="…"` declaration.
///
/// Returns `(stdout, exit_code)`.
pub fn block(json: bool) -> (String, u8) {
    // Single source of truth lives in zenith-core; no need to duplicate it here.
    let role_vocab = zenith_core::BLOCK_ROLE_VOCAB;

    const PROPS: &[(&str, &str)] = &[
        (
            "role",
            "string — required; the markdown block role to target (see vocab above)",
        ),
        (
            "font-family",
            "token ref or literal string — override font family for this role",
        ),
        (
            "font-size",
            "token ref, (px) literal, or dimension — override font size",
        ),
        (
            "font-weight",
            "token ref or literal — override font weight (100–900)",
        ),
        (
            "fill",
            "token ref or color literal — override text fill color",
        ),
        (
            "align",
            r#"string — text alignment: "left", "center", "right", "justify""#,
        ),
        ("italic", "#true / #false — override italic rendering"),
        (
            "space-before",
            "(px) or other dimension — extra space above the block",
        ),
        (
            "space-after",
            "(px) or other dimension — extra space below the block",
        ),
    ];

    const SCOPES: &[(&str, &str)] = &[
        (
            "document",
            "Declared as a direct child of the `document id=… { … }` block. \
          Lowest cascade precedence — applies when neither the page nor the text node \
          declares a matching role.",
        ),
        (
            "page",
            "Declared as a child of a `page id=… { … }` block (alongside `safe-zone` and `fold`). \
          Middle cascade precedence — overrides the document scope for this page's text nodes.",
        ),
        (
            "text",
            "Declared as a child of a `text id=… { … }` block (before `span` children). \
          Highest cascade precedence — overrides both document and page scope for this node.",
        ),
    ];

    const CASCADE_NOTE: &str = "Cascade precedence: text > page > document. \
        When the same `role` is declared at multiple scopes, the most-specific scope wins \
        property-by-property (fine-grained merging is a later unit; in this unit the whole \
        `BlockStyle` struct is stored per scope and the layout engine merges at consume time). \
        Block decls are consumed ONLY on text nodes with `format=\"markdown\"`; they have no \
        effect on plain-text or non-markdown nodes.";

    // Source syntax that PRODUCES each block role (for agent discoverability).
    // Uses r##"..."## because headings contain '#'.
    const SOURCE_SYNTAX: &[(&str, &str)] = &[
        (
            "h1..h6",
            r##"# H1  ## H2  ### H3  #### H4  ##### H5  ###### H6  (ATX headings)"##,
        ),
        ("p", "blank line between paragraphs"),
        ("blockquote", "> text on its own line"),
        (
            "li",
            "- item  or  * item  or  + item  (unordered);  1. item  (ordered)",
        ),
        (
            "code-block",
            "``` (optional lang)\ncode lines\n```  (fenced; lang after opening fence is optional)",
        ),
        ("hr", "--- or *** or ___ on its own line"),
    ];
    // Inline marks (not block roles, but shown here for completeness since block decls
    // apply to the same format=\"markdown\" nodes).
    const INLINE_SYNTAX: &str =
        "**bold**  *italic*  ~~strike~~  ==highlight==  ++underline++  `code`  [label](url)";

    const V1_LIMITS: &str = "v1 limitation: in a chain flow, code-block backgrounds and --- rules are not drawn \
         and blockquote/list indent is not applied. These render fully only in a single \
         non-chained text box.";

    const EXAMPLE: &str = concat!(
        "document id=\"doc.main\" {\n",
        "  block role=\"h1\" font-size=(token)\"size.h1\" font-weight=(token)\"weight.bold\" space-after=(px)16\n",
        "  block role=\"p\"  space-after=(px)8\n",
        "  page id=\"pg.cover\" w=(px)1280 h=(px)720 {\n",
        "    block role=\"h1\" fill=(token)\"color.accent\"\n",
        "    text id=\"body\" format=\"markdown\" src=\"article.md\" x=(px)80 y=(px)80 w=(px)1120 h=(px)560 {\n",
        "      block role=\"p\" space-after=(px)4\n",
        "    }\n",
        "  }\n",
        "}",
    );

    if json {
        use serde_json::{json, to_string_pretty};
        let roles: Vec<&str> = role_vocab.to_vec();
        let props: Vec<serde_json::Value> = PROPS
            .iter()
            .map(|(name, desc)| json!({ "name": name, "description": desc }))
            .collect();
        let scopes: Vec<serde_json::Value> = SCOPES
            .iter()
            .map(|(name, desc)| json!({ "scope": name, "description": desc }))
            .collect();
        let source_syntax: Vec<serde_json::Value> = SOURCE_SYNTAX
            .iter()
            .map(|(role, syntax)| json!({ "role": role, "source_syntax": syntax }))
            .collect();
        let out = json!({
            "schema": "zenith-schema-v1",
            "surface": "block",
            "role_vocabulary": roles,
            "markdown_source_syntax": source_syntax,
            "markdown_inline_syntax": INLINE_SYNTAX,
            "v1_limitations": V1_LIMITS,
            "properties": props,
            "scopes": scopes,
            "cascade": CASCADE_NOTE,
            "example": EXAMPLE,
        });
        (to_string_pretty(&out).unwrap_or_else(|e| e.to_string()), 0)
    } else {
        let mut text = String::new();
        text.push_str("block role=\"…\" — per-role markdown block style declaration\n");
        text.push_str("\nRole vocabulary and markdown source syntax:\n");
        let col = SOURCE_SYNTAX
            .iter()
            .map(|(r, _)| r.len())
            .max()
            .unwrap_or(0);
        for (role, syntax) in SOURCE_SYNTAX {
            text.push_str(&format!("  {role:<col$}  {syntax}\n", col = col));
        }
        text.push_str(&format!(
            "\nInline marks (format=\"markdown\"):\n  {INLINE_SYNTAX}\n"
        ));
        text.push_str(&format!("\nv1 limitations:\n  {V1_LIMITS}\n"));
        text.push_str("\nProperties (on block role=\"…\" declarations):\n");
        for (name, desc) in PROPS {
            text.push_str(&format!("  {name:<16}  {desc}\n"));
        }
        text.push_str("\nScopes:\n");
        for (scope, desc) in SCOPES {
            text.push_str(&format!("  {scope:<12}  {desc}\n"));
        }
        text.push_str(&format!("\nCascade:\n  {CASCADE_NOTE}\n"));
        text.push_str(&format!("\nExample:\n  {}", EXAMPLE.replace('\n', "\n  ")));
        (text, 0)
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
    fn node_detail_override_kind_hints_variant_surface() {
        // "override" is not a node kind; the error must hint at `zenith schema variant`.
        let (text, code) = node_detail("override", false);
        assert_eq!(code, 1);
        assert!(
            text.contains("unknown node kind"),
            "must report unknown kind"
        );
        assert!(
            text.contains("zenith schema variant"),
            "error for 'override' must hint at `zenith schema variant`; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_variant_kind_hints_variant_surface() {
        // "variant" is also not a node kind; same hint applies.
        let (text, code) = node_detail("variant", false);
        assert_eq!(code, 1);
        assert!(
            text.contains("unknown node kind"),
            "must report unknown kind"
        );
        assert!(
            text.contains("zenith schema variant"),
            "error for 'variant' must hint at `zenith schema variant`; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_other_unknown_no_variant_hint() {
        // Truly unknown kinds get no variant hint.
        let (text, code) = node_detail("frobnicate", false);
        assert_eq!(code, 1);
        assert!(
            text.contains("unknown node kind"),
            "must report unknown kind"
        );
        assert!(
            !text.contains("zenith schema variant"),
            "generic unknown kind must not mention variant surface; got:\n{text}"
        );
    }

    #[test]
    fn variant_human_contains_key_sections() {
        let (text, code) = variant(false);
        assert_eq!(code, 0);
        assert!(text.contains("variant"), "must name the surface");
        assert!(
            text.contains("Override properties:"),
            "must list override properties"
        );
        assert!(
            text.contains("node"),
            "override properties must include 'node' selector"
        );
        assert!(
            text.contains("visible"),
            "override properties must include 'visible'"
        );
        assert!(
            text.contains("x") && text.contains("y") && text.contains("w") && text.contains("h"),
            "override properties must include geometry keys x/y/w/h; got:\n{text}"
        );
        assert!(
            text.contains("Example:"),
            "must include a worked example section"
        );
        assert!(
            text.contains("source="),
            "example must show the source= attribute on a variant node"
        );
    }

    #[test]
    fn variant_human_override_node_selector_note() {
        let (text, code) = variant(false);
        assert_eq!(code, 0);
        // The override entry description must emphasise that the key is `node`, not `id`.
        assert!(
            text.contains("node"),
            "override entry must describe the 'node' selector key; got:\n{text}"
        );
        // Must warn about the wrong key.
        assert!(
            text.to_lowercase().contains("not") || text.contains("NOT"),
            "override entry should warn that 'id' is the wrong key; got:\n{text}"
        );
    }

    #[test]
    fn variant_json_schema_field() {
        let (text, code) = variant(true);
        assert_eq!(code, 0);
        assert!(
            text.contains("zenith-schema-v1"),
            "JSON must carry schema field"
        );
        assert!(
            text.contains("\"summary\""),
            "JSON must carry summary field"
        );
        assert!(
            text.contains("\"override_props\""),
            "JSON must carry override_props array"
        );
        assert!(
            text.contains("\"example\""),
            "JSON must carry example field"
        );
    }

    #[test]
    fn variant_json_override_props_have_geometry() {
        let (text, code) = variant(true);
        assert_eq!(code, 0);
        // x, y, w, h must all appear as override prop names.
        for key in &["\"x\"", "\"y\"", "\"w\"", "\"h\""] {
            assert!(
                text.contains(key),
                "variant JSON override_props must include {key}; got:\n{text}"
            );
        }
        // node must be required.
        assert!(
            text.contains("\"node\""),
            "variant JSON override_props must include node; got:\n{text}"
        );
    }

    #[test]
    fn op_detail_add_node_position_describes_id_field() {
        // Regression: before/after variants use `id` (sibling id), not `sibling`.
        let (text, code) = op_detail("add_node", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("id"),
            "add_node position description must mention the 'id' field; got:\n{text}"
        );
        assert!(
            text.contains("before") && text.contains("after"),
            "add_node position description must mention before/after variants; got:\n{text}"
        );
        assert!(
            text.contains("index"),
            "add_node position description must mention index variant; got:\n{text}"
        );
    }

    #[test]
    fn op_detail_add_node_position_json_has_correct_shape() {
        let (text, code) = op_detail("add_node", true);
        assert_eq!(code, 0);
        // The ty string must contain "id" to describe the before/after sibling key.
        assert!(
            text.contains("sibling-id") || text.contains("\"id\""),
            "add_node position field ty must describe the sibling id key; got:\n{text}"
        );
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

    // ── Content section tests ─────────────────────────────────────────────────

    #[test]
    fn node_detail_shape_human_shows_content_section() {
        let (text, code) = node_detail("shape", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("Content:"),
            "shape detail must include Content section; got:\n{text}"
        );
        assert!(
            text.contains("span"),
            "shape Content section must mention span children; got:\n{text}"
        );
        assert!(
            text.contains("label") || text.contains("centered"),
            "shape Content section must describe the owned label behaviour; got:\n{text}"
        );
        assert!(
            text.contains("Example:"),
            "shape Content section must include an example; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_shape_json_has_content_field() {
        let (text, code) = node_detail("shape", true);
        assert_eq!(code, 0);
        assert!(
            text.contains("\"content\""),
            "shape JSON must carry a content field; got:\n{text}"
        );
        assert!(
            text.contains("\"description\""),
            "shape JSON content must carry a description; got:\n{text}"
        );
        assert!(
            text.contains("\"example\""),
            "shape JSON content must carry an example; got:\n{text}"
        );
        // content must be non-null
        assert!(
            !text.contains("\"content\": null"),
            "shape JSON content must be non-null; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_polygon_human_shows_content_section() {
        let (text, code) = node_detail("polygon", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("Content:"),
            "polygon detail must include Content section; got:\n{text}"
        );
        assert!(
            text.contains("point"),
            "polygon Content section must mention point children; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_text_human_shows_content_section() {
        let (text, code) = node_detail("text", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("Content:"),
            "text detail must include Content section; got:\n{text}"
        );
        assert!(
            text.contains("span"),
            "text Content section must mention span children; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_rect_human_no_content_section() {
        // rect has no child content; the Content section must be absent.
        let (text, code) = node_detail("rect", false);
        assert_eq!(code, 0);
        assert!(
            !text.contains("Content:"),
            "rect detail must NOT include a Content section; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_rect_json_content_is_absent() {
        let (text, code) = node_detail("rect", true);
        assert_eq!(code, 0);
        // For a leaf node with no child content, the content field must be absent entirely.
        assert!(
            !text.contains("\"content\""),
            "rect JSON must not carry a content field (skip_serializing_if = None); got:\n{text}"
        );
    }

    #[test]
    fn node_detail_light_human_shows_example_without_content() {
        let (text, code) = node_detail("light", false);
        assert_eq!(code, 0);
        assert!(
            text.contains("Example:"),
            "light must show authoring example; got:\n{text}"
        );
        assert!(
            text.contains("light id=\"bg.glow\""),
            "light example must be concrete; got:\n{text}"
        );
        assert!(
            !text.contains("Content:"),
            "light is a leaf node and must not show Content section; got:\n{text}"
        );
    }

    #[test]
    fn node_detail_light_json_has_example_without_content() {
        let (text, code) = node_detail("light", true);
        assert_eq!(code, 0);
        assert!(
            text.contains("\"example\""),
            "light JSON must carry example; got:\n{text}"
        );
        assert!(
            text.contains("bg.glow"),
            "light JSON example must include usable node id; got:\n{text}"
        );
        assert!(
            !text.contains("\"content\""),
            "light JSON must not carry child content; got:\n{text}"
        );
    }

    // ── Diagnostics surface tests ────────────────────────────────────────────

    #[test]
    fn diagnostics_human_mentions_scoped_policy_syntax() {
        let (text, code) = diagnostics(false);
        assert_eq!(code, 0);
        assert!(
            text.contains("allow \"<code>\" \"<subject-id>\""),
            "human output must show scoped diagnostic policy syntax; got:\n{text}"
        );
        assert!(
            text.contains("allow \"layout.off_canvas\" \"bg.glow\" \"bg.rim\""),
            "human output must include multi-subject example; got:\n{text}"
        );
    }

    #[test]
    fn diagnostics_json_carries_policy_syntax() {
        let (text, code) = diagnostics(true);
        assert_eq!(code, 0);
        assert!(
            text.contains("\"syntax\""),
            "JSON must carry syntax examples; got:\n{text}"
        );
        assert!(
            text.contains("allow \\\"<code>\\\" \\\"<subject-id>\\\""),
            "JSON must include scoped diagnostic policy syntax; got:\n{text}"
        );
    }

    // ── Brand surface tests ───────────────────────────────────────────────────

    #[test]
    fn brand_human_contains_key_sections() {
        let (text, code) = brand(false);
        assert_eq!(code, 0);
        assert!(
            text.contains("brand {"),
            "human output must include worked example with 'brand {{'; got:\n{text}"
        );
        assert!(
            text.contains("colors"),
            "human output must describe the colors child node; got:\n{text}"
        );
        assert!(
            text.contains("fonts"),
            "human output must describe the fonts child node; got:\n{text}"
        );
        assert!(
            text.contains("weights"),
            "human output must describe the weights child node; got:\n{text}"
        );
        assert!(
            text.contains("brand.color_off_palette"),
            "human output must list brand.color_off_palette diagnostic code; got:\n{text}"
        );
        assert!(
            text.contains("brand.font_not_allowed"),
            "human output must list brand.font_not_allowed diagnostic code; got:\n{text}"
        );
        assert!(
            text.contains("brand.weight_not_allowed"),
            "human output must list brand.weight_not_allowed diagnostic code; got:\n{text}"
        );
        assert!(
            text.contains("--deny"),
            "human output must mention --deny for CI gate; got:\n{text}"
        );
    }

    #[test]
    fn brand_json_schema_field() {
        let (text, code) = brand(true);
        assert_eq!(code, 0);
        assert!(
            text.contains("zenith-schema-v1"),
            "JSON must carry schema field; got:\n{text}"
        );
        assert!(
            text.contains("\"summary\""),
            "JSON must carry summary field; got:\n{text}"
        );
        assert!(
            text.contains("\"child_nodes\""),
            "JSON must carry child_nodes array; got:\n{text}"
        );
        assert!(
            text.contains("\"diagnostic_codes\""),
            "JSON must carry diagnostic_codes array; got:\n{text}"
        );
    }

    #[test]
    fn overview_mentions_brand_surface() {
        let (text, code) = overview(false);
        assert_eq!(code, 0);
        assert!(
            text.contains("zenith schema brand"),
            "overview must mention 'zenith schema brand'; got:\n{text}"
        );
        assert!(
            text.contains("zenith schema block"),
            "overview must mention 'zenith schema block'; got:\n{text}"
        );
        assert!(
            text.contains("7 non-node surfaces"),
            "overview must count 5 non-node surfaces after adding block; got:\n{text}"
        );
    }
}
