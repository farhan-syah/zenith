use std::path::Path;

use zenith_core::{KdlAdapter, KdlSource, Severity, validate};
use zenith_tx::TxStatus;

use crate::library::{ItemKind, parse_spec, resolve_packs};

/// Error produced by the `library add` command.
#[derive(Debug)]
pub struct AddCmdErr {
    /// Human-readable message.
    pub message: String,
    /// Recommended exit code.
    pub exit_code: u8,
}

impl AddCmdErr {
    fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }
}

/// The successful outcome of `library add`: the canonical formatted source to
/// write back (or print on `--dry-run`) plus a human-readable summary.
#[derive(Debug)]
pub struct AddResult {
    /// The canonical formatted bytes of the mutated document.
    pub formatted: Vec<u8>,
    /// A multi-line human-readable summary of what was added.
    pub summary: String,
}

/// Materialize the library item named by `spec` into the document `target_src`,
/// returning the formatted result + a summary.
///
/// `project_dir` is the directory whose `libraries/*.zen` packs are resolved
/// alongside the embedded presets (the `--into` file's parent). `at` is the
/// instance origin in pixels; `id_base` overrides the generated instance id base.
///
/// This is pure: it parses, mutates an in-memory [`zenith_core::Document`],
/// VALIDATES the result (hard errors abort with no write), and formats — it never
/// touches the filesystem itself (the dispatcher reads/writes files). Steps mirror
/// [`crate::library::materialize`]: resolve pack → copy component (dedup) → copy
/// dep tokens/styles/assets (dedup) → unique instance id → insert instance →
/// record libraries + provenance → validate → format.
///
/// `page` is required only for COMPONENT items (which materialize as an instance
/// on a page); TOKEN items (filter tokens) ignore it.
///
/// # Errors
///
/// Returns [`AddCmdErr`] on a malformed spec, parse/format failure, unknown
/// package/item, a missing page (for a component item), or a post-mutation
/// validation that has hard errors.
pub fn add(
    target_src: &str,
    spec: &str,
    project_dir: Option<&Path>,
    page: Option<&str>,
    at: (f64, f64),
    id_override: Option<&str>,
) -> Result<AddResult, AddCmdErr> {
    let (pkg_id, item) = parse_spec(spec).map_err(|e| AddCmdErr::new(e.message, 2))?;

    let mut target = KdlAdapter
        .parse(target_src.as_bytes())
        .map_err(|e| AddCmdErr::new(format!("parse error: {}", e.message), 2))?;

    let packs = resolve_packs(project_dir);
    let id_base = id_override.unwrap_or(item.as_str());

    // Determine the item kind from the resolved pack's exported items. An unknown
    // pkg/item falls through to a `materialize*` call, which yields a precise
    // "unknown package/item" diagnostic.
    let item_kind = packs
        .iter()
        .find(|p| p.id == pkg_id)
        .and_then(|p| p.items.iter().find(|it| it.id == item))
        .map(|it| it.kind);

    let summary = match item_kind {
        Some(ItemKind::Action) => {
            let outcome = crate::library::materialize_action(target_src, &packs, &pkg_id, &item)
                .map_err(|e| AddCmdErr::new(e.message, 2))?;

            // Rejected → early-return with the rejection diagnostics; the two
            // accepted variants yield the status label used in the summary.
            let status_label = match outcome.tx_result.status {
                TxStatus::Rejected => {
                    let diag_lines: Vec<String> = outcome
                        .tx_result
                        .diagnostics
                        .iter()
                        .map(crate::commands::format_diagnostic_line)
                        .collect();
                    return Err(AddCmdErr::new(
                        format!(
                            "action '{}#{}' was rejected:\n{}",
                            pkg_id,
                            item,
                            diag_lines.join("\n")
                        ),
                        1,
                    ));
                }
                TxStatus::Accepted => "accepted",
                TxStatus::AcceptedWithWarnings => "accepted-with-warnings",
            };

            let final_source = outcome.final_source.ok_or_else(|| {
                AddCmdErr::new("internal error: accepted action produced no source", 2)
            })?;

            let result_doc = KdlAdapter.parse(final_source.as_bytes()).map_err(|e| {
                AddCmdErr::new(
                    format!(
                        "internal error: could not re-parse action result: {}",
                        e.message
                    ),
                    2,
                )
            })?;

            let formatted = validate_and_format(&result_doc)?;

            let affected = if outcome.tx_result.affected_node_ids.is_empty() {
                "none".to_owned()
            } else {
                outcome.tx_result.affected_node_ids.join(", ")
            };
            let provenance_id = outcome.provenance_id.unwrap_or_default();
            let mut summary = String::new();
            summary.push_str(&format!(
                "applied {}#{} ({})\n",
                outcome.pkg_id, outcome.item, status_label
            ));
            summary.push_str(&format!("  affected: {}\n", affected));
            summary.push_str(&format!("  provenance: {}", provenance_id));
            for w in &outcome.warnings {
                summary.push_str(&format!("\n  warning: {}", w));
            }
            return Ok(AddResult { formatted, summary });
        }
        Some(ItemKind::Token) => {
            // TOKEN item: copy the filter token + color deps; no instance, no page.
            let outcome =
                crate::library::materialize_token(&mut target, &packs, &pkg_id, &item, id_base)
                    .map_err(|e| AddCmdErr::new(e.message, 2))?;
            let deps = if outcome.dep_token_ids.is_empty() {
                "none".to_owned()
            } else {
                outcome.dep_token_ids.join(", ")
            };
            let mut summary = String::new();
            summary.push_str(&format!(
                "added {}#{} as {} token '{}'\n",
                outcome.pkg_id, outcome.item, outcome.apply_property, outcome.token_id
            ));
            summary.push_str(&format!(
                "  apply with: {}=(token)\"{}\"\n",
                outcome.apply_property, outcome.token_id
            ));
            summary.push_str(&format!("  dependencies: {}\n", deps));
            summary.push_str(&format!("  provenance: {}", outcome.provenance_id));
            for w in &outcome.warnings {
                summary.push_str(&format!("\n  warning: {}", w));
            }
            summary
        }
        // COMPONENT item (or unknown). A real component requires `--page`. For an
        // unknown pkg/item (`None`), skip the page requirement and let
        // `materialize` emit the precise "unknown package/item" diagnostic — it
        // checks pkg/item BEFORE page, so an empty page never masks that error.
        Some(ItemKind::Component) | None => {
            let page = match item_kind {
                Some(ItemKind::Component) => page.ok_or_else(|| {
                    AddCmdErr::new(
                        "page is required to add a component item (use --page <id>)",
                        2,
                    )
                })?,
                Some(ItemKind::Token) | Some(ItemKind::Action) | None => page.unwrap_or(""),
            };
            let outcome =
                crate::library::materialize(&mut target, &packs, &pkg_id, &item, page, id_base, at)
                    .map_err(|e| AddCmdErr::new(e.message, 2))?;
            let mut summary = String::new();
            summary.push_str(&format!(
                "added {}#{} as instance '{}' on page '{}'\n",
                outcome.pkg_id, outcome.item, outcome.instance_id, page
            ));
            summary.push_str(&format!("  component: {}\n", outcome.target_component_id));
            summary.push_str(&format!("  provenance: {}", outcome.provenance_id));
            for w in &outcome.warnings {
                summary.push_str(&format!("\n  warning: {}", w));
            }
            summary
        }
    };

    let formatted = validate_and_format(&target)?;
    Ok(AddResult { formatted, summary })
}

/// Validate the mutated `target` (hard errors abort with no write) then format it
/// to canonical bytes. Shared by the component and token `add` branches.
fn validate_and_format(target: &zenith_core::Document) -> Result<Vec<u8>, AddCmdErr> {
    let report = validate(target);
    let errors: Vec<String> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(crate::commands::format_diagnostic_line)
        .collect();
    if !errors.is_empty() {
        return Err(AddCmdErr::new(
            format!(
                "materialized document has {} validation error(s):\n{}",
                errors.len(),
                errors.join("\n")
            ),
            1,
        ));
    }
    KdlAdapter
        .format(target)
        .map_err(|e| AddCmdErr::new(format!("format error: {}", e.message), 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── `add` command tests ────────────────────────────────────────────────────

    const TARGET_SRC: &str = r#"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {}
  }
}
"#;

    #[test]
    fn add_produces_formatted_doc_that_round_trips_and_compiles() {
        let result = add(
            TARGET_SRC,
            "@zenith/flowchart#decision",
            None,
            Some("pg"),
            (120.0, 80.0),
            None,
        )
        .expect("add ok");

        // Result is valid UTF-8 KDL that reparses + validates clean.
        let src = String::from_utf8(result.formatted).expect("utf8");
        let doc = KdlAdapter.parse(src.as_bytes()).expect("reparse");
        let errors: Vec<_> = validate(&doc)
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "errors: {:?}", errors);

        // Summary mentions the instance + component + provenance.
        assert!(
            result.summary.contains("decision"),
            "summary: {}",
            result.summary
        );
        assert!(
            result.summary.contains("lib.zenith.flowchart.decision"),
            "summary: {}",
            result.summary
        );

        // Smoke: the document compiles to a non-empty scene (instance expands to
        // the shape) when rendered to a scene JSON.
        let artifact = crate::commands::render::to_scene_json(
            &src,
            None,
            1,
            &crate::config::CliPolicyFlags::default(),
        )
        .expect("compile ok");
        let scene: serde_json::Value =
            serde_json::from_str(&artifact.json).expect("scene json parses");
        let commands = scene["commands"].as_array().expect("commands array");
        assert!(
            !commands.is_empty(),
            "instance must expand to at least one scene command"
        );
    }

    #[test]
    fn add_malformed_spec_errors() {
        let err = add(TARGET_SRC, "no-hash", None, Some("pg"), (0.0, 0.0), None)
            .expect_err("malformed spec errors");
        assert_eq!(err.exit_code, 2);
    }

    #[test]
    fn add_unknown_page_errors() {
        let err = add(
            TARGET_SRC,
            "@zenith/flowchart#decision",
            None,
            Some("nope"),
            (0.0, 0.0),
            None,
        )
        .expect_err("unknown page errors");
        assert!(
            err.message.contains("page 'nope' not found"),
            "msg: {}",
            err.message
        );
    }

    #[test]
    fn add_unknown_pkg_and_item_error() {
        let e1 = add(
            TARGET_SRC,
            "@no/such#decision",
            None,
            Some("pg"),
            (0.0, 0.0),
            None,
        )
        .expect_err("unknown pkg");
        assert!(e1.message.contains("@zenith/flowchart"), "{}", e1.message);
        let e2 = add(
            TARGET_SRC,
            "@zenith/flowchart#nope",
            None,
            Some("pg"),
            (0.0, 0.0),
            None,
        )
        .expect_err("unknown item");
        assert!(e2.message.contains("process"), "{}", e2.message);
    }

    #[test]
    fn add_is_pure_on_input_string() {
        // `add` never mutates its input; writing happens only in the dispatcher.
        // Two calls on the same input yield byte-identical output (deterministic).
        let a = add(
            TARGET_SRC,
            "@zenith/flowchart#process",
            None,
            Some("pg"),
            (0.0, 0.0),
            None,
        )
        .expect("a");
        let b = add(
            TARGET_SRC,
            "@zenith/flowchart#process",
            None,
            Some("pg"),
            (0.0, 0.0),
            None,
        )
        .expect("b");
        assert_eq!(a.formatted, b.formatted, "add is deterministic + pure");
    }

    #[test]
    fn add_filter_token_then_apply_compiles() {
        let result = add(
            TARGET_SRC,
            "@zenith/filters#noir",
            None,
            None,
            (0.0, 0.0),
            None,
        )
        .expect("add filter token ok");

        // Result reparses + validates clean.
        let src = String::from_utf8(result.formatted).expect("utf8");
        let doc = KdlAdapter.parse(src.as_bytes()).expect("reparse");
        let errors: Vec<_> = validate(&doc)
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "errors: {:?}", errors);

        // Summary mentions how to apply the token.
        assert!(
            result.summary.contains("filter=(token)\"noir\""),
            "summary: {}",
            result.summary
        );

        // The added token can be applied to a rect: add it into a target that
        // already carries a rect referencing `filter=(token)"noir"`, then assert
        // the result validates clean and compiles to scene commands.
        const TARGET_WITH_RECT: &str = r#"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {
      rect id="r" x=(px)10 y=(px)10 w=(px)100 h=(px)100 filter=(token)"noir"
    }
  }
}
"#;
        let applied = add(
            TARGET_WITH_RECT,
            "@zenith/filters#noir",
            None,
            None,
            (0.0, 0.0),
            None,
        )
        .expect("add into rect target ok");
        let applied_src = String::from_utf8(applied.formatted).expect("utf8");
        let applied_doc = KdlAdapter
            .parse(applied_src.as_bytes())
            .expect("reparse applied");
        let applied_errors: Vec<_> = validate(&applied_doc)
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            applied_errors.is_empty(),
            "applied errors: {:?}",
            applied_errors
        );
        let artifact = crate::commands::render::to_scene_json(
            &applied_src,
            None,
            1,
            &crate::config::CliPolicyFlags::default(),
        )
        .expect("compile ok");
        let scene: serde_json::Value =
            serde_json::from_str(&artifact.json).expect("scene json parses");
        let commands = scene["commands"].as_array().expect("commands array");
        assert!(!commands.is_empty(), "applied filter compiles to commands");
    }

    #[test]
    fn add_action_accepted_applies_tx_and_writes_provenance() {
        // Pack source with an action that updates token color.brand to #e11d48.
        // Raw string uses r##"..."## to avoid early termination on "#e11d48".
        const ACTION_PACK_SRC: &str = r##"zenith version=1 {
  project id="@test/actions" name="Test Actions"
  libraries { library id="@test/actions" version="1.0.0" }
  actions {
    action id="apply-brand-kit" {
      tx "{\"ops\":[{\"op\":\"update_token_value\",\"id\":\"color.brand\",\"value\":\"#e11d48\"}]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {}
  }
}
"##;
        // Target document that declares the token the action will update.
        const TARGET_WITH_TOKEN: &str = r##"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {
    token id="color.brand" type="color" value="#111111"
  }
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {}
  }
}
"##;

        let dir = tempfile::tempdir().expect("tempdir");
        let lib_dir = dir.path().join("libraries");
        std::fs::create_dir_all(&lib_dir).expect("create libraries dir");
        std::fs::write(lib_dir.join("actions.zen"), ACTION_PACK_SRC).expect("write pack");

        let result = add(
            TARGET_WITH_TOKEN,
            "@test/actions#apply-brand-kit",
            Some(dir.path()),
            None,
            (0.0, 0.0),
            None,
        )
        .expect("action add ok");

        let src = String::from_utf8(result.formatted).expect("utf8");
        assert!(src.contains("#e11d48"), "updated value in output: {}", src);
        assert!(
            result.summary.contains("apply-brand-kit"),
            "summary mentions action id: {}",
            result.summary
        );
        assert!(
            result.summary.contains("provenance"),
            "summary mentions provenance: {}",
            result.summary
        );
    }

    #[test]
    fn add_action_rejected_returns_error_exit_1() {
        // Action targets a non-existent token — tx will be rejected.
        const ACTION_PACK_SRC: &str = r##"zenith version=1 {
  project id="@test/actions" name="Test Actions"
  libraries { library id="@test/actions" version="1.0.0" }
  actions {
    action id="bad-action" {
      tx "{\"ops\":[{\"op\":\"update_token_value\",\"id\":\"no.such.token\",\"value\":\"#fff\"}]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {}
  }
}
"##;

        let dir = tempfile::tempdir().expect("tempdir");
        let lib_dir = dir.path().join("libraries");
        std::fs::create_dir_all(&lib_dir).expect("create libraries dir");
        std::fs::write(lib_dir.join("actions.zen"), ACTION_PACK_SRC).expect("write pack");

        let err = add(
            TARGET_SRC,
            "@test/actions#bad-action",
            Some(dir.path()),
            None,
            (0.0, 0.0),
            None,
        )
        .expect_err("rejected action must return an error");

        assert_eq!(err.exit_code, 1, "exit_code must be 1 for rejected tx");
        assert!(
            err.message.contains("rejected"),
            "msg must mention rejected: {}",
            err.message
        );
    }

    #[test]
    fn add_component_without_page_errors() {
        let err = add(
            TARGET_SRC,
            "@zenith/flowchart#decision",
            None,
            None,
            (0.0, 0.0),
            None,
        )
        .expect_err("component without page errors");
        assert_eq!(err.exit_code, 2);
        assert!(
            err.message.contains("--page"),
            "msg should ask for --page: {}",
            err.message
        );
    }
}
