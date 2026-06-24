//! Writer for the `agent-runs` block: run definitions, steps, step params,
//! and step diagnostics.
//!
//! Serializes [`AgentRun`] records into the canonical `agent-runs { … }` KDL
//! block, with each `run` containing optional `constraints`, `plan`, and one
//! or more `step` children.

use crate::ast::{AgentRun, AgentStep, AgentStepDiagnostic, AgentStepParam};

use super::{
    escape_kdl_string, fmt_property_value, fmt_unknown_property, indent, write_opt_str_escaped,
};

/// Emit the `agent-runs { … }` block.
///
/// Stable position: after `recipes`, before `actions`. Emitted ONLY when at
/// least one run is declared, so documents without agent-runs keep their
/// existing canonical form unchanged (byte-identity gate). Each run emits:
///
/// ```text
/// run id="…" brief="…" {
///   constraints "…"
///   plan "…"
///   step id="…" action="…" … {
///     affected-node "…"
///     param name="…" value=…
///     diagnostic severity="…" code="…" message="…"
///     source-hash "…"
///   }
/// }
/// ```
///
/// Optional inline props and optional child blocks are omitted when absent.
/// Free-form strings (`brief`, `constraints`, `plan`, `source-hash`, diagnostic
/// `message`) pass through [`escape_kdl_string`]. Plain identifiers (`id`,
/// `parent`, `action`, `action-version`, `action-hash`, `severity`, `code`,
/// `affected-node` ids) emit unescaped. Mirrors [`write_recipes_block`](super::write_recipes_block).
pub(super) fn write_agent_runs_block(agent_runs: &[AgentRun], out: &mut String, depth: usize) {
    if agent_runs.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("agent-runs {\n");
    for run in agent_runs {
        indent(out, depth + 1);
        out.push_str("run id=\"");
        out.push_str(&run.id);
        out.push('"');
        write_opt_str_escaped(out, "brief", &run.brief);
        // Unknown props on the run node in sorted key order.
        for (key, prop) in &run.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        out.push_str(" {\n");
        if let Some(constraints) = &run.constraints {
            indent(out, depth + 2);
            out.push_str("constraints \"");
            out.push_str(&escape_kdl_string(constraints));
            out.push_str("\"\n");
        }
        if let Some(plan) = &run.plan {
            indent(out, depth + 2);
            out.push_str("plan \"");
            out.push_str(&escape_kdl_string(plan));
            out.push_str("\"\n");
        }
        for step in &run.steps {
            write_agent_step(step, out, depth + 2);
        }
        indent(out, depth + 1);
        out.push_str("}\n");
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_agent_step(step: &AgentStep, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("step id=\"");
    out.push_str(&step.id);
    out.push_str("\" action=\"");
    out.push_str(&step.action);
    out.push('"');
    if let Some(parent) = &step.parent {
        out.push_str(" parent=\"");
        out.push_str(parent);
        out.push('"');
    }
    if let Some(av) = &step.action_version {
        out.push_str(" action-version=\"");
        out.push_str(av);
        out.push('"');
    }
    if let Some(ah) = &step.action_hash {
        out.push_str(" action-hash=\"");
        out.push_str(ah);
        out.push('"');
    }
    // Unknown props on the step node in sorted key order.
    for (key, prop) in &step.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }
    // Emit child block only when there is something to write.
    let has_children = !step.affected_nodes.is_empty()
        || !step.params.is_empty()
        || !step.diagnostics.is_empty()
        || step.source_hash.is_some();
    if has_children {
        out.push_str(" {\n");
        for node_id in &step.affected_nodes {
            indent(out, depth + 1);
            out.push_str("affected-node \"");
            out.push_str(node_id);
            out.push_str("\"\n");
        }
        for param in &step.params {
            write_agent_step_param(param, out, depth + 1);
        }
        for diag in &step.diagnostics {
            write_agent_step_diagnostic(diag, out, depth + 1);
        }
        if let Some(sh) = &step.source_hash {
            indent(out, depth + 1);
            out.push_str("source-hash \"");
            out.push_str(&escape_kdl_string(sh));
            out.push_str("\"\n");
        }
        indent(out, depth);
        out.push_str("}\n");
    } else {
        out.push('\n');
    }
}

fn write_agent_step_param(param: &AgentStepParam, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("param name=\"");
    out.push_str(&param.name);
    out.push_str("\" value=");
    out.push_str(&fmt_property_value(&param.value));
    // Unknown props on the param node, in sorted key order.
    for (key, prop) in &param.unknown_props {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&fmt_unknown_property(prop));
    }
    out.push('\n');
}

fn write_agent_step_diagnostic(diag: &AgentStepDiagnostic, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("diagnostic severity=\"");
    out.push_str(&diag.severity);
    out.push_str("\" code=\"");
    out.push_str(&diag.code);
    out.push_str("\" message=\"");
    out.push_str(&escape_kdl_string(&diag.message));
    out.push_str("\"\n");
}
