//! Agent-runs block rendering for `zenith inspect`.
//!
//! The public surface is two pure functions:
//! - [`build_agent_run_entries`] — converts `&[AgentRun]` to
//!   `Vec<AgentRunInspectJson>` for the `--json` path.
//! - [`render_agent_runs_human`] — formats the same data as a human-readable
//!   section string, mirroring the style used for the `recipes` output.

use zenith_core::AgentRun;

use crate::json_types::{AgentRunInspectJson, AgentStepInspectJson};

// ── JSON builder ──────────────────────────────────────────────────────────────

/// Convert a slice of [`AgentRun`] to [`AgentRunInspectJson`] entries (source
/// order is preserved).
pub fn build_agent_run_entries(runs: &[AgentRun]) -> Vec<AgentRunInspectJson> {
    runs.iter().map(run_to_json).collect()
}

fn run_to_json(r: &AgentRun) -> AgentRunInspectJson {
    AgentRunInspectJson {
        id: r.id.clone(),
        brief: r.brief.clone(),
        step_count: r.steps.len(),
        steps: r.steps.iter().map(step_to_json).collect(),
    }
}

fn step_to_json(s: &zenith_core::AgentStep) -> AgentStepInspectJson {
    AgentStepInspectJson {
        id: s.id.clone(),
        parent: s.parent.clone(),
        action: s.action.clone(),
        action_version: s.action_version.clone(),
        affected_node_count: s.affected_nodes.len(),
        diagnostic_count: s.diagnostics.len(),
    }
}

// ── Human renderer ────────────────────────────────────────────────────────────

/// Render the `agent-runs` section for human output.
///
/// Returns an empty string when `runs` is empty (consistent with how the
/// recipes section simply emits nothing when absent).
pub fn render_agent_runs_human(runs: &[AgentRun]) -> String {
    if runs.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for r in runs {
        // Header line: `agent-run <id>  steps=<N>`
        out.push_str(&format!("agent-run {}  steps={}\n", r.id, r.steps.len()));

        // Optional brief
        if let Some(ref brief) = r.brief {
            out.push_str(&format!("  brief={}\n", brief));
        }

        // Per-step summary lines
        for step in &r.steps {
            out.push_str(&format!("  step {}  action={}", step.id, step.action));
            if let Some(ref ver) = step.action_version {
                out.push_str(&format!("@{}", ver));
            }
            if let Some(ref parent) = step.parent {
                out.push_str(&format!("  parent={}", parent));
            }
            if !step.affected_nodes.is_empty() {
                out.push_str(&format!("  affected={} node(s)", step.affected_nodes.len()));
            }
            if !step.diagnostics.is_empty() {
                out.push_str(&format!("  diagnostics={}", step.diagnostics.len()));
            }
            out.push('\n');
        }
    }
    out.trim_end().to_owned()
}
