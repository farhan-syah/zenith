//! Transforms for the `agent-runs` block: run definitions, steps, step params,
//! and step diagnostics.
//!
//! Each `run id="…" …` inside `agent-runs { … }` is parsed into an
//! [`AgentRun`] with zero or more [`AgentStep`] children, each of which may
//! carry [`AgentStepParam`] and [`AgentStepDiagnostic`] records.

use kdl::{KdlNode, KdlValue};

use crate::ast::agent_run::{AgentRun, AgentStep, AgentStepDiagnostic, AgentStepParam};
use crate::error::{ParseError, ParseErrorCode};

use super::helpers::{
    collect_unknown_props, entry_to_property_value, node_span, optional_string_prop,
    optional_string_prop_aliased, required_string_prop,
};

const AGENT_RUN_KNOWN_PROPS: &[&str] = &["id", "brief"];
const AGENT_STEP_KNOWN_PROPS: &[&str] = &[
    "id",
    "parent",
    "action",
    "action-version",
    "action_version",
    "action-hash",
    "action_hash",
];
const AGENT_STEP_PARAM_KNOWN_PROPS: &[&str] = &["name", "value"];

/// Transform the document-level `agent-runs { … }` block into a list of
/// [`AgentRun`]. Each `run id="…" …` is a block node; non-`run` children
/// inside the block are silently ignored (forward-compat). Mirrors
/// [`transform_recipes`](super::document::transform_recipes).
pub(super) fn transform_agent_runs(node: &KdlNode) -> Result<Vec<AgentRun>, ParseError> {
    let mut defs: Vec<AgentRun> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "run" {
                defs.push(transform_agent_run_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_agent_run_def(node: &KdlNode) -> Result<AgentRun, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let brief = optional_string_prop(node, "brief").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, AGENT_RUN_KNOWN_PROPS);
    let source_span = node_span(node);

    let mut constraints: Option<String> = None;
    let mut plan: Option<String> = None;
    let mut steps: Vec<AgentStep> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "constraints" => {
                    // Positional string arg: `constraints "…"` — same pattern
                    // as `tx "…"` in the actions block.
                    if let Some(s) = child.get(0).and_then(|v| match v {
                        KdlValue::String(s) => Some(s.clone()),
                        _ => None,
                    }) {
                        constraints = Some(s);
                    }
                }
                "plan" => {
                    if let Some(s) = child.get(0).and_then(|v| match v {
                        KdlValue::String(s) => Some(s.clone()),
                        _ => None,
                    }) {
                        plan = Some(s);
                    }
                }
                "step" => {
                    steps.push(transform_agent_step(child)?);
                }
                _ => {}
            }
        }
    }

    Ok(AgentRun {
        id,
        brief,
        constraints,
        plan,
        steps,
        source_span,
        unknown_props,
    })
}

fn transform_agent_step(node: &KdlNode) -> Result<AgentStep, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let action = required_string_prop(node, "action")?.to_owned();
    let parent = optional_string_prop(node, "parent").map(str::to_owned);
    let action_version =
        optional_string_prop_aliased(node, "action-version", "action_version").map(str::to_owned);
    let action_hash =
        optional_string_prop_aliased(node, "action-hash", "action_hash").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, AGENT_STEP_KNOWN_PROPS);
    let source_span = node_span(node);

    let mut params: Vec<AgentStepParam> = Vec::new();
    let mut affected_nodes: Vec<String> = Vec::new();
    let mut diagnostics: Vec<AgentStepDiagnostic> = Vec::new();
    let mut source_hash: Option<String> = None;

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "param" => {
                    params.push(transform_agent_step_param(child)?);
                }
                "affected-node" => {
                    if let Some(s) = child.get(0).and_then(|v| match v {
                        KdlValue::String(s) => Some(s.clone()),
                        _ => None,
                    }) {
                        affected_nodes.push(s);
                    }
                }
                "diagnostic" => {
                    diagnostics.push(transform_agent_step_diagnostic(child)?);
                }
                "source-hash" => {
                    if let Some(s) = child.get(0).and_then(|v| match v {
                        KdlValue::String(s) => Some(s.clone()),
                        _ => None,
                    }) {
                        source_hash = Some(s);
                    }
                }
                _ => {}
            }
        }
    }

    Ok(AgentStep {
        id,
        parent,
        action,
        action_version,
        action_hash,
        params,
        affected_nodes,
        diagnostics,
        source_hash,
        source_span,
        unknown_props,
    })
}

fn transform_agent_step_param(node: &KdlNode) -> Result<AgentStepParam, ParseError> {
    let name = required_string_prop(node, "name")?.to_owned();
    let value = node
        .entry("value")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("agent-run `param` `{name}` is missing required property `value`"),
            )
        })
        .and_then(entry_to_property_value)?;
    let unknown_props = collect_unknown_props(node, AGENT_STEP_PARAM_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(AgentStepParam {
        name,
        value,
        source_span,
        unknown_props,
    })
}

fn transform_agent_step_diagnostic(node: &KdlNode) -> Result<AgentStepDiagnostic, ParseError> {
    let severity = required_string_prop(node, "severity")?.to_owned();
    let code = required_string_prop(node, "code")?.to_owned();
    let message = required_string_prop(node, "message")?.to_owned();
    let source_span = node_span(node);

    Ok(AgentStepDiagnostic {
        severity,
        code,
        message,
        source_span,
    })
}
