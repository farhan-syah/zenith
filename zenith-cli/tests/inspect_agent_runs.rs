//! Integration tests for `zenith inspect` — agent-runs block surfacing.
//!
//! Exercises the public [`zenith_cli::commands::inspect::run`] function directly
//! (same pattern as `inspect_recipes.rs`).

use zenith_cli::commands::inspect::run;

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Document with a fully-specified `agent-runs` block (two runs).
const DOC_WITH_AGENT_RUNS: &str = r##"zenith version=1 {
  project id="proj.ai" name="Agent Runs Inspect Integration"
  tokens format="zenith-token-v1" {
  }
  styles {}
  agent-runs {
    run id="run.alpha" brief="Layout pass" {
      step id="step.1" action="read_file" {
        affected-node "node.header"
        affected-node "node.body"
        diagnostic severity="warning" code="agent.overlap" message="2px overlap"
      }
      step id="step.2" action="write_node" parent="step.1" action-version="write_node@2" {
        affected-node "node.header"
      }
    }
    run id="run.beta" {
      step id="step.a" action="validate_doc" {
      }
    }
  }
  document id="doc.ai" title="Agent Runs Inspect Integration" {
    page id="page.ai" w=(px)1280 h=(px)720 {
      rect id="node.header" x=(px)0 y=(px)0 w=(px)1280 h=(px)80
      rect id="node.body" x=(px)0 y=(px)80 w=(px)1280 h=(px)640
    }
  }
}
"##;

/// Document with no `agent-runs` block at all.
const DOC_NO_AGENT_RUNS: &str = r##"zenith version=1 {
  project id="proj.nar" name="No Agent Runs Integration"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.nar" title="No Agent Runs Integration" {
    page id="page.nar" w=(px)400 h=(px)300 {
      rect id="rect.nar" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;

// ── Human output: document with agent-runs ────────────────────────────────────

#[test]
fn human_output_includes_run_ids() {
    let out = run(DOC_WITH_AGENT_RUNS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("run.alpha"),
        "human output must include first run id; got:\n{out}"
    );
    assert!(
        out.contains("run.beta"),
        "human output must include second run id; got:\n{out}"
    );
}

#[test]
fn human_output_includes_step_ids_and_actions() {
    let out = run(DOC_WITH_AGENT_RUNS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("step.1"),
        "human output must include step id step.1; got:\n{out}"
    );
    assert!(
        out.contains("read_file"),
        "human output must include action read_file; got:\n{out}"
    );
    assert!(
        out.contains("step.2"),
        "human output must include step id step.2; got:\n{out}"
    );
    assert!(
        out.contains("write_node"),
        "human output must include action write_node; got:\n{out}"
    );
}

#[test]
fn human_output_includes_brief() {
    let out = run(DOC_WITH_AGENT_RUNS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("Layout pass"),
        "human output must include the run brief; got:\n{out}"
    );
}

#[test]
fn human_output_also_contains_pages() {
    let out = run(DOC_WITH_AGENT_RUNS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("page page.ai"),
        "human output must still include the pages section; got:\n{out}"
    );
}

// ── Human output: document without agent-runs ─────────────────────────────────

#[test]
fn human_output_no_agent_runs_section_when_empty() {
    let out = run(DOC_NO_AGENT_RUNS, None, false).expect("inspect must succeed");
    assert!(
        !out.contains("agent-run"),
        "human output must not contain 'agent-run' when doc has no agent-runs block; got:\n{out}"
    );
    // Pages must still appear.
    assert!(
        out.contains("page page.nar"),
        "pages section must still appear; got:\n{out}"
    );
}

// ── JSON output: document with agent-runs ─────────────────────────────────────

#[test]
fn json_output_includes_agent_runs_array() {
    let out = run(DOC_WITH_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
    let arr = v["agent_runs"]
        .as_array()
        .expect("agent_runs must be a JSON array");
    assert_eq!(arr.len(), 2, "must have 2 run entries");
}

#[test]
fn json_output_run_ids_in_source_order() {
    let out = run(DOC_WITH_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["agent_runs"].as_array().unwrap();
    assert_eq!(arr[0]["id"], "run.alpha");
    assert_eq!(arr[1]["id"], "run.beta");
}

#[test]
fn json_output_run_brief_and_step_count() {
    let out = run(DOC_WITH_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let alpha = &v["agent_runs"][0];
    assert_eq!(alpha["brief"], "Layout pass");
    assert_eq!(alpha["step_count"], 2);
    let beta = &v["agent_runs"][1];
    assert_eq!(beta["step_count"], 1);
    assert!(
        beta.get("brief").map(|v| v.is_null()).unwrap_or(true),
        "brief must be absent for run with no brief"
    );
}

#[test]
fn json_output_step_fields() {
    let out = run(DOC_WITH_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let steps = v["agent_runs"][0]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);

    let s1 = &steps[0];
    assert_eq!(s1["id"], "step.1");
    assert_eq!(s1["action"], "read_file");
    assert_eq!(s1["affected_node_count"], 2);
    assert_eq!(s1["diagnostic_count"], 1);
    // No parent on step.1 — must be absent when None (skip_serializing_if)
    assert!(
        s1.get("parent").map(|v| v.is_null()).unwrap_or(true),
        "parent must be absent for step with no parent"
    );

    let s2 = &steps[1];
    assert_eq!(s2["id"], "step.2");
    assert_eq!(s2["action"], "write_node");
    assert_eq!(s2["parent"], "step.1");
    assert_eq!(s2["action_version"], "write_node@2");
    assert_eq!(s2["affected_node_count"], 1);
    assert_eq!(s2["diagnostic_count"], 0);
}

#[test]
fn json_output_schema_and_pages_unaffected() {
    let out = run(DOC_WITH_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "zenith-inspect-v1");
    let pages = v["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0]["id"], "page.ai");
}

// ── JSON output: document without agent-runs ──────────────────────────────────

#[test]
fn json_output_empty_agent_runs_array_when_no_block() {
    let out = run(DOC_NO_AGENT_RUNS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["agent_runs"]
        .as_array()
        .expect("agent_runs must be present as empty array");
    assert!(
        arr.is_empty(),
        "agent_runs array must be empty; got: {arr:?}"
    );
}
