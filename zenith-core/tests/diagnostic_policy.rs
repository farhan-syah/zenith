//! Integration tests for the document-level diagnostic policy (`diagnostics { … }`).
//!
//! Exercises the public surface only (parse → validate → format):
//! - `allow` suppresses an advisory; `deny` elevates to Error; `warn` forces Warning.
//! - `allow` does NOT suppress a real integrity Error (Error severity is immutable).
//! - last-wins for duplicate codes.
//! - an unknown code → `policy.unknown_code`.
//! - `allow` on an always-Error code → `policy.ineffective_on_error`.
//! - a document with no `diagnostics` block validates and round-trips identically.
//! - the `diagnostics` block round-trips through the formatter and is idempotent.

mod common;

use common::*;
use zenith_core::format::format_document;

fn parse(src: &str) -> Document {
    let adapter = KdlAdapter;
    adapter.parse(src.as_bytes()).expect("parse")
}

/// A document whose single rect sits off-canvas → a `layout.off_canvas` advisory,
/// optionally adjusted by an injected `diagnostics { … }` block.
fn doc_with_off_canvas(policy_block: &str) -> Document {
    let src = format!(
        r##"zenith version=1 {{
{policy_block}  document id="doc.policy" title="P" {{
    page id="page.1" w=(px)100 h=(px)100 {{
      rect id="r.off" x=(px)-20 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.bg"
    }}
  }}
  tokens format="zenith-token-v1" {{
    token id="color.bg" type="color" value="#ffffff"
  }}
}}
"##
    );
    parse(&src)
}

fn doc_with_two_off_canvas(policy_block: &str) -> Document {
    let src = format!(
        r##"zenith version=1 {{
{policy_block}  document id="doc.policy.subject" title="P" {{
    page id="page.1" w=(px)100 h=(px)100 {{
      rect id="bg.glow" x=(px)-20 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.bg"
      rect id="shape.accidental" x=(px)90 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.bg"
    }}
  }}
  tokens format="zenith-token-v1" {{
    token id="color.bg" type="color" value="#ffffff"
  }}
}}
"##
    );
    parse(&src)
}

fn doc_with_three_off_canvas(policy_block: &str) -> Document {
    let src = format!(
        r##"zenith version=1 {{
{policy_block}  document id="doc.policy.subjects" title="P" {{
    page id="page.1" w=(px)100 h=(px)100 {{
      rect id="bg.glow" x=(px)-20 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.bg"
      rect id="bg.rim" x=(px)0 y=(px)-20 w=(px)40 h=(px)40 fill=(token)"color.bg"
      rect id="shape.accidental" x=(px)90 y=(px)0 w=(px)40 h=(px)40 fill=(token)"color.bg"
    }}
  }}
  tokens format="zenith-token-v1" {{
    token id="color.bg" type="color" value="#ffffff"
  }}
}}
"##
    );
    parse(&src)
}

fn severity_of<'a>(report: &'a ValidationReport, code: &str) -> Option<&'a Severity> {
    report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .map(|d| &d.severity)
}

fn subjects_for<'a>(report: &'a ValidationReport, code: &str) -> Vec<&'a str> {
    report
        .diagnostics
        .iter()
        .filter(|d| d.code == code)
        .filter_map(|d| d.subject_id.as_deref())
        .collect()
}

#[test]
fn no_block_is_unchanged() {
    // Baseline: off-canvas advisory present, no policy diagnostics.
    let doc = doc_with_off_canvas("");
    assert!(doc.diagnostic_policy.entries.is_empty());
    let report = validate(&doc);
    assert!(
        has_code(&report, "layout.off_canvas"),
        "baseline must emit the advisory; codes: {:?}",
        codes(&report)
    );
    assert!(!has_code(&report, "policy.unknown_code"));
    assert!(!has_code(&report, "policy.ineffective_on_error"));
}

#[test]
fn allow_suppresses_advisory() {
    let doc = doc_with_off_canvas("  diagnostics {\n    allow \"layout.off_canvas\"\n  }\n");
    let report = validate(&doc);
    assert!(
        !has_code(&report, "layout.off_canvas"),
        "allow must drop the advisory; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn scoped_allow_suppresses_only_matching_subject() {
    let doc = doc_with_two_off_canvas(
        "  diagnostics {\n    allow \"layout.off_canvas\" \"bg.glow\"\n  }\n",
    );
    let report = validate(&doc);
    assert_eq!(
        subjects_for(&report, "layout.off_canvas"),
        vec!["shape.accidental"],
        "scoped allow must leave unrelated off-canvas diagnostics visible; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn scoped_allow_accepts_multiple_subjects_on_one_entry() {
    let doc = doc_with_three_off_canvas(
        "  diagnostics {\n    allow \"layout.off_canvas\" \"bg.glow\" \"bg.rim\"\n  }\n",
    );
    let report = validate(&doc);
    assert_eq!(
        subjects_for(&report, "layout.off_canvas"),
        vec!["shape.accidental"],
        "multi-subject allow must suppress only listed subjects; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn deny_elevates_advisory_to_error() {
    let doc = doc_with_off_canvas("  diagnostics {\n    deny \"layout.off_canvas\"\n  }\n");
    let report = validate(&doc);
    assert_eq!(
        severity_of(&report, "layout.off_canvas"),
        Some(&Severity::Error),
        "deny must elevate to Error; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn warn_forces_warning() {
    // deny then warn on the same code → warn wins (last-wins) → Warning.
    let doc = doc_with_off_canvas(
        "  diagnostics {\n    deny \"layout.off_canvas\"\n    warn \"layout.off_canvas\"\n  }\n",
    );
    let report = validate(&doc);
    assert_eq!(
        severity_of(&report, "layout.off_canvas"),
        Some(&Severity::Warning),
        "warn must force Warning (and win last); codes: {:?}",
        codes(&report)
    );
}

#[test]
fn allow_does_not_suppress_a_real_error() {
    // Two nodes share an id → `id.duplicate` (Error). An `allow` must NOT drop it.
    let src = r##"zenith version=1 {
  diagnostics {
    allow "id.duplicate"
  }
  document id="doc.err" title="E" {
    page id="page.1" w=(px)100 h=(px)100 {
      rect id="dup" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      rect id="dup" x=(px)20 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }
  }
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
}
"##;
    let report = validate(&parse(src));
    assert_eq!(
        severity_of(&report, "id.duplicate"),
        Some(&Severity::Error),
        "an Error must survive `allow`; codes: {:?}",
        codes(&report)
    );
    // And the policy self-validation must flag the ineffective allow-on-Error.
    assert!(
        has_code(&report, "policy.ineffective_on_error"),
        "allow on an always-Error code must be flagged; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn unknown_code_is_flagged() {
    let doc = doc_with_off_canvas("  diagnostics {\n    allow \"not.a_real_code\"\n  }\n");
    let report = validate(&doc);
    assert!(
        has_code(&report, "policy.unknown_code"),
        "unknown code must be flagged; codes: {:?}",
        codes(&report)
    );
    // The unrelated advisory is untouched.
    assert!(has_code(&report, "layout.off_canvas"));
}

#[test]
fn self_validation_cannot_be_suppressed_by_the_policy() {
    // A policy that tries to allow its own self-warning still emits it, because
    // self-validation runs AFTER apply_policy.
    let doc = doc_with_off_canvas(
        "  diagnostics {\n    allow \"policy.unknown_code\"\n    allow \"not.a_real_code\"\n  }\n",
    );
    let report = validate(&doc);
    assert!(
        has_code(&report, "policy.unknown_code"),
        "self-validation must not be suppressible; codes: {:?}",
        codes(&report)
    );
}

#[test]
fn diagnostics_block_round_trips_and_is_idempotent() {
    let src = r##"zenith version=1 {
  diagnostics {
    allow "layout.off_canvas" "bg.glow" "bg.rim"
    deny "token.unused"
    warn "node.unknown_property"
  }
  document id="doc.rt" title="RT" {
    page id="page.1" w=(px)100 h=(px)100 {
      rect id="r.1" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }
  }
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
}
"##;
    let doc = parse(src);
    assert_eq!(doc.diagnostic_policy.entries.len(), 3);
    assert_eq!(
        doc.diagnostic_policy.entries[0].subjects,
        vec!["bg.glow".to_owned(), "bg.rim".to_owned()]
    );

    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    assert!(
        text.contains("diagnostics {"),
        "formatted output must contain the diagnostics block; got:\n{text}"
    );

    // Re-parse → policy identical (order preserved).
    let reparsed = parse(&text);
    assert_eq!(
        reparsed.diagnostic_policy, doc.diagnostic_policy,
        "policy must survive a format → re-parse round-trip"
    );

    // Idempotence: format(format(doc)) == format(doc).
    let formatted2 = format_document(&reparsed).expect("format2");
    let text2 = String::from_utf8(formatted2).expect("utf8");
    assert_eq!(text, text2, "formatter must be idempotent");
}

#[test]
fn empty_block_emits_nothing() {
    // A document parsed from source with NO diagnostics block must not emit one.
    let doc = doc_with_off_canvas("");
    let formatted = format_document(&doc).expect("format");
    let text = String::from_utf8(formatted).expect("utf8");
    assert!(
        !text.contains("diagnostics {"),
        "an empty policy must emit no diagnostics block; got:\n{text}"
    );
}
