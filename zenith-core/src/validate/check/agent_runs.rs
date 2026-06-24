//! Validation for the top-level `agent-runs` block.
//!
//! Checks performed:
//!
//! 1. **`agent_run.duplicate_run_id`** (Error) — two `run` entries share the
//!    same `id`. Run ids live in their own namespace (not document node ids),
//!    so a dedicated local set is used rather than the global `register_id`.
//! 2. **`agent_run.duplicate_step_id`** (Error) — two `step` entries within a
//!    single run share the same `id`. Step ids are scoped per-run.
//! 3. **`agent_run.empty_action`** (Warning) — a step's `action` field is empty
//!    or whitespace-only.
//! 4. **`agent_run.unresolved_parent_step`** (Advisory) — a step's `parent`
//!    names a step id not present in the same run. Advisory because a recorded
//!    run may reference steps pruned from the source document.
//! 5. **`agent_run.unknown_affected_node`** (Advisory) — an `affected-node` id
//!    is not present in `all_node_ids`. Advisory because a run recorded against
//!    an older doc state may reference since-deleted nodes.
//! 6. **`agent_run.invalid_diagnostic_severity`** (Warning) — an inline step
//!    diagnostic has a `severity` value other than `"error"`, `"warning"`, or
//!    `"advisory"`.

use std::collections::BTreeSet;

use crate::ast::document::Document;
use crate::diagnostics::Diagnostic;

/// Validate the `agent-runs` block of `doc`.
///
/// `all_node_ids` — the set of all node ids across pages, masters, and
/// components. Built once by the driver before calling this function.
pub(in crate::validate::check) fn check_agent_runs(
    doc: &Document,
    all_node_ids: &BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // ── 1. Duplicate run id detection ─────────────────────────────────────────
    // Run ids live in the agent-runs namespace; they are NOT document node ids.
    let mut seen_run_ids: BTreeSet<&str> = BTreeSet::new();

    for run in &doc.agent_runs {
        if !seen_run_ids.insert(run.id.as_str()) {
            diagnostics.push(Diagnostic::error(
                "agent_run.duplicate_run_id",
                format!(
                    "agent-run '{}': id is declared more than once; \
                     run ids must be unique within the agent-runs block",
                    run.id
                ),
                run.source_span,
                Some(run.id.clone()),
            ));
        }

        // ── 2. Duplicate step id within the same run ──────────────────────────
        let mut seen_step_ids: BTreeSet<&str> = BTreeSet::new();

        for step in &run.steps {
            if !seen_step_ids.insert(step.id.as_str()) {
                diagnostics.push(Diagnostic::error(
                    "agent_run.duplicate_step_id",
                    format!(
                        "agent-run '{}': step id '{}' is declared more than once; \
                         step ids must be unique within a run",
                        run.id, step.id
                    ),
                    step.source_span,
                    Some(step.id.clone()),
                ));
            }

            // ── 3. Empty step action ──────────────────────────────────────────
            if step.action.trim().is_empty() {
                diagnostics.push(Diagnostic::warning(
                    "agent_run.empty_action",
                    format!(
                        "agent-run '{}': step '{}' has an empty action field; \
                         every step must name the action (tool/function) it invoked",
                        run.id, step.id
                    ),
                    step.source_span,
                    Some(step.id.clone()),
                ));
            }

            // ── 4. Unresolved parent step reference ───────────────────────────
            // Advisory: a recorded run may reference steps pruned from the source.
            // We check the full step list (not just steps seen so far) so that a
            // forward reference to a later step is also accepted.
            if let Some(parent_id) = &step.parent {
                let parent_exists = run.steps.iter().any(|s| s.id == *parent_id);
                if !parent_exists {
                    diagnostics.push(Diagnostic::advisory(
                        "agent_run.unresolved_parent_step",
                        format!(
                            "agent-run '{}': step '{}' references parent step '{}' \
                             which is not present in this run",
                            run.id, step.id, parent_id
                        ),
                        step.source_span,
                        Some(step.id.clone()),
                    ));
                }
            }

            // ── 5. Unknown affected-node ids ──────────────────────────────────
            // Advisory: a run recorded against an older doc state may reference
            // since-deleted nodes.
            for node_id in &step.affected_nodes {
                if !all_node_ids.contains(node_id.as_str()) {
                    diagnostics.push(Diagnostic::advisory(
                        "agent_run.unknown_affected_node",
                        format!(
                            "agent-run '{}': step '{}' names affected-node '{}' \
                             which does not exist in this document",
                            run.id, step.id, node_id
                        ),
                        step.source_span,
                        Some(step.id.clone()),
                    ));
                }
            }

            // ── 6. Invalid inline diagnostic severity ─────────────────────────
            for diag in &step.diagnostics {
                if diag.severity != "error"
                    && diag.severity != "warning"
                    && diag.severity != "advisory"
                {
                    diagnostics.push(Diagnostic::warning(
                        "agent_run.invalid_diagnostic_severity",
                        format!(
                            "agent-run '{}': step '{}' has an inline diagnostic with \
                             severity '{}'; expected \"error\", \"warning\", or \"advisory\"",
                            run.id, step.id, diag.severity
                        ),
                        step.source_span,
                        Some(step.id.clone()),
                    ));
                }
            }
        }
    }
}
