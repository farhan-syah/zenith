//! Integration tests: agent-runs block validation.
//!
//! Covers all six agent-run-check diagnostics:
//!   - `agent_run.duplicate_run_id`
//!   - `agent_run.duplicate_step_id`
//!   - `agent_run.empty_action`
//!   - `agent_run.unresolved_parent_step`
//!   - `agent_run.unknown_affected_node`
//!   - `agent_run.invalid_diagnostic_severity`
//!
//! Plus a clean-doc regression guard (fully-valid block → no agent_run.* codes).

mod common;

use common::*;

fn parse_and_validate(src: &str) -> ValidationReport {
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    validate(&doc)
}

// ── Clean agent-runs block → no agent_run.* diagnostics ──────────────────────

/// A fully-valid `agent-runs` block must produce no `agent_run.*` diagnostics.
/// Affected-node references real nodes; parent references a real step;
/// inline diagnostics use valid severities.
#[test]
fn valid_agent_runs_block_is_clean() {
    let src = r##"zenith version=1 {
  project id="proj.ar.clean" name="Clean"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.alpha" brief="Layout pass" {
      step id="step.1" action="read_file" {
        affected-node "node.header"
        diagnostic severity="warning" code="agent.overlap" message="2px overlap"
      }
      step id="step.2" action="write_node" parent="step.1" {
        affected-node "node.body"
      }
    }
  }
  document id="doc.ar.clean" title="Clean" {
    page id="page.main" w=(px)1280 h=(px)720 {
      rect id="node.header" x=(px)0 y=(px)0 w=(px)1280 h=(px)80
      rect id="node.body" x=(px)0 y=(px)80 w=(px)1280 h=(px)640
    }
  }
}
"##;
    let report = parse_and_validate(src);
    let agent_codes: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with("agent_run."))
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        agent_codes.is_empty(),
        "clean agent-runs block must produce no agent_run.* diagnostics; got {:?}",
        agent_codes
    );
}

// ── agent_run.duplicate_run_id ────────────────────────────────────────────────

/// Two `run` entries with the same `id` → `agent_run.duplicate_run_id`.
#[test]
fn duplicate_run_id_is_error() {
    let src = r##"zenith version=1 {
  project id="proj.dup.run" name="DUP"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.a" {
      step id="step.1" action="read_file" {
      }
    }
    run id="run.a" {
      step id="step.1" action="write_node" {
      }
    }
  }
  document id="doc.dup.run" title="DUP" {
    page id="page.main" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.duplicate_run_id"),
        "duplicate run id must produce agent_run.duplicate_run_id; got {:?}",
        codes(&report)
    );
}

// ── agent_run.duplicate_step_id ───────────────────────────────────────────────

/// Two `step` entries within the same run sharing an id →
/// `agent_run.duplicate_step_id`.
#[test]
fn duplicate_step_id_is_error() {
    let src = r##"zenith version=1 {
  project id="proj.dup.step" name="DUP"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.beta" {
      step id="step.1" action="read_file" {
      }
      step id="step.1" action="write_node" {
      }
    }
  }
  document id="doc.dup.step" title="DUP" {
    page id="page.main" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.duplicate_step_id"),
        "duplicate step id must produce agent_run.duplicate_step_id; got {:?}",
        codes(&report)
    );
}

// ── agent_run.empty_action ────────────────────────────────────────────────────

/// A step with an empty `action` field → `agent_run.empty_action` (warning).
#[test]
fn empty_step_action_is_warning() {
    let src = r##"zenith version=1 {
  project id="proj.empty.action" name="EA"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.gamma" {
      step id="step.1" action="" {
      }
    }
  }
  document id="doc.empty.action" title="EA" {
    page id="page.main" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.empty_action"),
        "empty step action must produce agent_run.empty_action; got {:?}",
        codes(&report)
    );
}

// ── agent_run.unresolved_parent_step ─────────────────────────────────────────

/// A step whose `parent` names a step id not present in the same run →
/// `agent_run.unresolved_parent_step` (advisory).
#[test]
fn unresolved_parent_step_is_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.parent" name="PA"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.delta" {
      step id="step.1" action="read_file" parent="step.ghost" {
      }
    }
  }
  document id="doc.parent" title="PA" {
    page id="page.main" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.unresolved_parent_step"),
        "unresolved parent step must produce agent_run.unresolved_parent_step; got {:?}",
        codes(&report)
    );
    // Confirm it is advisory severity
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "agent_run.unresolved_parent_step")
        .unwrap();
    assert_eq!(
        diag.severity,
        Severity::Advisory,
        "unresolved_parent_step must be Advisory severity"
    );
}

// ── agent_run.unknown_affected_node ──────────────────────────────────────────

/// A step's `affected-node` names a node id not present in the document →
/// `agent_run.unknown_affected_node` (advisory).
#[test]
fn unknown_affected_node_is_advisory() {
    let src = r##"zenith version=1 {
  project id="proj.aff" name="AFF"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.epsilon" {
      step id="step.1" action="read_file" {
        affected-node "node.deleted"
      }
    }
  }
  document id="doc.aff" title="AFF" {
    page id="page.main" w=(px)1280 h=(px)720 {
      rect id="node.real" x=(px)0 y=(px)0 w=(px)100 h=(px)100
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.unknown_affected_node"),
        "unknown affected node must produce agent_run.unknown_affected_node; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "agent_run.unknown_affected_node")
        .unwrap();
    assert_eq!(
        diag.severity,
        Severity::Advisory,
        "unknown_affected_node must be Advisory severity"
    );
}

// ── agent_run.invalid_diagnostic_severity ────────────────────────────────────

/// An inline step diagnostic with an unrecognized severity →
/// `agent_run.invalid_diagnostic_severity` (warning).
#[test]
fn invalid_diagnostic_severity_is_warning() {
    let src = r##"zenith version=1 {
  project id="proj.sev" name="SEV"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  agent-runs {
    run id="run.zeta" {
      step id="step.1" action="read_file" {
        diagnostic severity="warn" code="x.y" message="bad severity value"
      }
    }
  }
  document id="doc.sev" title="SEV" {
    page id="page.main" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "agent_run.invalid_diagnostic_severity"),
        "invalid severity must produce agent_run.invalid_diagnostic_severity; got {:?}",
        codes(&report)
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.code == "agent_run.invalid_diagnostic_severity")
        .unwrap();
    assert_eq!(
        diag.severity,
        Severity::Warning,
        "invalid_diagnostic_severity must be Warning severity"
    );
}
