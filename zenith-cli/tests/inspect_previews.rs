//! Integration tests for `zenith inspect` — previews block surfacing.
//!
//! Exercises the public [`zenith_cli::commands::inspect::run`] function directly.

use zenith_cli::commands::inspect::run;

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Document with a fully-specified `previews` block (two preview entries).
const DOC_WITH_PREVIEWS: &str = r##"zenith version=1 {
  project id="proj.pi" name="Previews Inspect Integration"
  tokens format="zenith-token-v1" {
  }
  styles {}
  previews {
    preview candidate="page.hero" source-hash="abc123" output="out/hero.png" output-hash="def456" parent-revision="rev.1" {
      critique severity="error" code="preview.bleed" message="Content bleeds off edge"
      critique severity="warning" code="preview.contrast" message="Contrast ratio too low"
    }
    preview candidate="page.back"
  }
  document id="doc.pi" title="Previews Inspect Integration" {
    page id="page.hero" w=(px)1280 h=(px)720 {
      rect id="rect.hero" x=(px)0 y=(px)0 w=(px)1280 h=(px)80
    }
    page id="page.back" w=(px)1280 h=(px)720 {
    }
  }
}
"##;

/// Document with no `previews` block at all.
const DOC_NO_PREVIEWS: &str = r##"zenith version=1 {
  project id="proj.npi" name="No Previews Integration"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.npi" title="No Previews Integration" {
    page id="page.npi" w=(px)400 h=(px)300 {
      rect id="rect.npi" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;

// ── Human output: document with previews ──────────────────────────────────────

#[test]
fn human_output_includes_candidate_ids() {
    let out = run(DOC_WITH_PREVIEWS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("page.hero"),
        "human output must include first candidate id; got:\n{out}"
    );
    assert!(
        out.contains("page.back"),
        "human output must include second candidate id; got:\n{out}"
    );
}

#[test]
fn human_output_includes_critique_severity_and_code() {
    let out = run(DOC_WITH_PREVIEWS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("preview.bleed"),
        "human output must include critique code preview.bleed; got:\n{out}"
    );
    assert!(
        out.contains("preview.contrast"),
        "human output must include critique code preview.contrast; got:\n{out}"
    );
}

#[test]
fn human_output_also_contains_pages() {
    let out = run(DOC_WITH_PREVIEWS, None, false).expect("inspect must succeed");
    assert!(
        out.contains("page page.hero"),
        "human output must still include the pages section; got:\n{out}"
    );
}

// ── Human output: document without previews ───────────────────────────────────

#[test]
fn human_output_no_previews_section_when_empty() {
    let out = run(DOC_NO_PREVIEWS, None, false).expect("inspect must succeed");
    // The word "preview" should not appear in the output for a doc without previews.
    assert!(
        !out.contains("preview"),
        "human output must not contain 'preview' when doc has no previews block; got:\n{out}"
    );
    // Pages must still appear.
    assert!(
        out.contains("page page.npi"),
        "pages section must still appear; got:\n{out}"
    );
}

// ── JSON output: document with previews ───────────────────────────────────────

#[test]
fn json_output_includes_previews_array() {
    let out = run(DOC_WITH_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
    let arr = v["previews"]
        .as_array()
        .expect("previews must be a JSON array");
    assert_eq!(arr.len(), 2, "must have 2 preview entries");
}

#[test]
fn json_output_preview_candidates_in_source_order() {
    let out = run(DOC_WITH_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["previews"].as_array().unwrap();
    assert_eq!(arr[0]["candidate"], "page.hero");
    assert_eq!(arr[1]["candidate"], "page.back");
}

#[test]
fn json_output_preview_optional_fields_and_critique_count() {
    let out = run(DOC_WITH_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let hero = &v["previews"][0];
    assert_eq!(hero["candidate"], "page.hero");
    assert_eq!(hero["source_hash"], "abc123");
    assert_eq!(hero["output"], "out/hero.png");
    assert_eq!(hero["output_hash"], "def456");
    assert_eq!(hero["parent_revision"], "rev.1");
    assert_eq!(hero["critique_count"], 2);

    let back = &v["previews"][1];
    assert_eq!(back["candidate"], "page.back");
    assert_eq!(back["critique_count"], 0);
    // Optional fields absent for the minimal entry.
    assert!(
        back.get("source_hash").map(|v| v.is_null()).unwrap_or(true),
        "source_hash must be absent for preview with no source_hash"
    );
}

#[test]
fn json_output_critique_fields() {
    let out = run(DOC_WITH_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let critiques = v["previews"][0]["critiques"].as_array().unwrap();
    assert_eq!(critiques.len(), 2);

    let c1 = &critiques[0];
    assert_eq!(c1["severity"], "error");
    assert_eq!(c1["code"], "preview.bleed");

    let c2 = &critiques[1];
    assert_eq!(c2["severity"], "warning");
    assert_eq!(c2["code"], "preview.contrast");
}

#[test]
fn json_output_schema_and_pages_unaffected() {
    let out = run(DOC_WITH_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "zenith-inspect-v1");
    let pages = v["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
}

// ── JSON output: document without previews ────────────────────────────────────

#[test]
fn json_output_empty_previews_array_when_no_block() {
    let out = run(DOC_NO_PREVIEWS, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["previews"]
        .as_array()
        .expect("previews must be present as empty array");
    assert!(arr.is_empty(), "previews array must be empty; got: {arr:?}");
}
