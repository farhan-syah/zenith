//! Integration tests for the `previews` block: parse, serialize, and
//! round-trip.
//!
//! Exercises:
//! - Full parse → field access → format → re-parse → AST equality (spans stripped).
//! - Absent `previews` block → empty vec, no output, byte-identical to before.
//! - Free-form string fields containing `"`, `\`, and newlines escape correctly.
//! - Unknown-prop capture on a `preview` node survives round-trip.

mod common;

use common::*;
use zenith_core::format::format_document;

// ── previews: parse, serialize, and round-trip ────────────────────────────────

/// **Round-trip**: parse a doc with a `previews` block (two preview entries:
/// one full entry with all optional fields + two critiques, one minimal entry
/// with only `candidate`) → format → re-parse → AST equality (spans stripped).
/// Also asserts canonical position (before `document`) and that all fields emit
/// correctly.
#[test]
fn test_previews_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.pv" name="PV"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  previews {
    preview candidate="page.hero" source-hash="abc123" output="out/hero.png" output-hash="def456" parent-revision="rev.1" {
      critique severity="warn" code="preview.contrast" message="Contrast ratio too low"
      critique severity="error" code="preview.bleed" message="Content bleeds off edge"
    }
    preview candidate="page.back"
  }
  document id="doc.pv" title="PV" {
    page id="page.hero" w=(px)1280 h=(px)720 {
    }
    page id="page.back" w=(px)1280 h=(px)720 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.previews.len(), 2, "expected 2 preview entries");

    let pv1 = &doc.previews[0];
    assert_eq!(pv1.candidate, "page.hero");
    assert_eq!(pv1.source_hash.as_deref(), Some("abc123"));
    assert_eq!(pv1.output.as_deref(), Some("out/hero.png"));
    assert_eq!(pv1.output_hash.as_deref(), Some("def456"));
    assert_eq!(pv1.parent_revision.as_deref(), Some("rev.1"));
    assert_eq!(pv1.critiques.len(), 2);
    assert_eq!(pv1.critiques[0].severity, "warn");
    assert_eq!(pv1.critiques[0].code, "preview.contrast");
    assert_eq!(pv1.critiques[0].message, "Contrast ratio too low");
    assert_eq!(pv1.critiques[1].severity, "error");
    assert_eq!(pv1.critiques[1].code, "preview.bleed");
    assert_eq!(pv1.critiques[1].message, "Content bleeds off edge");

    let pv2 = &doc.previews[1];
    assert_eq!(pv2.candidate, "page.back");
    assert_eq!(pv2.source_hash, None);
    assert_eq!(pv2.output, None);
    assert_eq!(pv2.output_hash, None);
    assert_eq!(pv2.parent_revision, None);
    assert!(pv2.critiques.is_empty());

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    // Key fields must appear in the output.
    assert!(
        formatted_str
            .contains(r#"preview candidate="page.hero" source-hash="abc123" output="out/hero.png" output-hash="def456" parent-revision="rev.1""#),
        "full preview header must emit; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(
            r#"critique severity="warn" code="preview.contrast" message="Contrast ratio too low""#
        ),
        "first critique must emit; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(
            r#"critique severity="error" code="preview.bleed" message="Content bleeds off edge""#
        ),
        "second critique must emit; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains(r#"preview candidate="page.back""#),
        "minimal preview must emit; got:\n{formatted_str}"
    );

    // Canonical order: previews before document.
    let previews_at = formatted_str.find("previews {").expect("previews block");
    let doc_at = formatted_str.find("document ").expect("document block");
    assert!(
        previews_at < doc_at,
        "previews must be emitted before document; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).previews,
        strip_spans(reparsed).previews,
        "previews must survive a parse → format → parse round-trip (idempotent)"
    );
}

/// **Absent `previews` block is an empty vec**: a document with no `previews`
/// block must parse with `doc.previews` empty, produce no `previews { … }`
/// output, and be byte-identical across two format passes.
#[test]
fn test_absent_previews_is_empty_and_byte_identical() {
    let src = r##"zenith version=1 {
  project id="proj.nopv" name="NoPV"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.nopv" title="NoPV" {
    page id="p" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert!(
        doc.previews.is_empty(),
        "absent previews block must yield an empty vec"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        !formatted_str.contains("previews"),
        "no previews block must be emitted for an empty vec; got:\n{formatted_str}"
    );

    // Idempotency: output is byte-identical on a second pass.
    let reparsed = adapter.parse(&formatted).expect("re-parse");
    let formatted2 = format_document(&reparsed).expect("format 2");
    assert_eq!(
        formatted, formatted2,
        "absent previews must be byte-identical across two format passes"
    );
}

/// **Free-form escaping round-trip**: `output` (a path containing a backslash
/// and a double-quote) and a critique `message` (containing `"` and `\n`)
/// survive parse → format → parse with the exact same string value.
#[test]
fn test_preview_free_form_escaping_round_trip() {
    let tricky_path = r#"out/dir\"sub"/file.png"#;
    let tricky_message = "first line\nsecond \"quoted\" line\\backslash";
    let src = format!(
        r##"zenith version=1 {{
  project id="proj.esc3" name="ESC3"
  tokens format="zenith-token-v1" {{
  }}
  styles {{
  }}
  previews {{
    preview candidate="page.x" output={output:?} {{
      critique severity="warn" code="c.esc" message={message:?}
    }}
  }}
  document id="doc.esc3" title="ESC3" {{
    page id="page.x" w=(px)640 h=(px)360 {{
    }}
  }}
}}
"##,
        output = tricky_path,
        message = tricky_message,
    );

    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    let pv = &doc.previews[0];
    assert_eq!(
        pv.output.as_deref(),
        Some(tricky_path),
        "output path must parse back exactly"
    );
    assert_eq!(
        pv.critiques[0].message, tricky_message,
        "critique message must parse back exactly"
    );

    let formatted = format_document(&doc).expect("format");
    let reparsed = adapter.parse(&formatted).expect("re-parse escaped output");

    assert_eq!(
        reparsed.previews[0].output.as_deref(),
        Some(tricky_path),
        "output path with special chars must survive round-trip"
    );
    assert_eq!(
        strip_spans(doc).previews,
        strip_spans(reparsed).previews,
        "escaped previews must be round-trip identical"
    );
}

/// **Unknown-prop round-trip**: an unrecognized annotated prop on a `preview`
/// is captured in `unknown_props` and survives parse → format → parse
/// byte-identically.
#[test]
fn test_preview_unknown_props_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.uknpv" name="UKNPV"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  previews {
    preview candidate="page.y" confidence=(px)9
  }
  document id="doc.uknpv" title="UKNPV" {
    page id="page.y" w=(px)640 h=(px)360 {
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    assert_eq!(doc.previews.len(), 1);
    let pv = &doc.previews[0];

    let confidence_prop = pv
        .unknown_props
        .get("confidence")
        .expect("annotated unknown prop `confidence` must be captured on preview");
    assert_eq!(
        confidence_prop.ty.as_deref(),
        Some("px"),
        "annotation on preview unknown prop must survive"
    );

    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");

    assert!(
        formatted_str.contains("confidence=(px)9"),
        "annotated unknown prop on preview must round-trip; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse");
    assert_eq!(
        strip_spans(doc).previews,
        strip_spans(reparsed).previews,
        "previews with unknown props must survive full round-trip"
    );
}
