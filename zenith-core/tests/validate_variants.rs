//! Integration tests: variants block validation.
//!
//! Covers all four variant-check diagnostics:
//!   - `variant.duplicate_id`
//!   - `variant.unknown_source`
//!   - `variant.invalid_dimension`
//!   - `variant.override_unknown_node`
//!
//! Plus a clean-variants regression guard.

mod common;

use common::*;

// ── Helper: parse a raw .zen source and run validate ─────────────────────────

fn parse_and_validate(src: &str) -> ValidationReport {
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    validate(&doc)
}

// ── Clean variants → no variant.* diagnostics ────────────────────────────────

/// A document with a well-formed `variants` block must produce no variant.*
/// diagnostics. All four fields valid; override targets nodes that exist on the
/// source page.
#[test]
fn valid_variants_block_is_clean() {
    let src = r##"zenith version=1 {
  project id="proj.v" name="V"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="square" source="page.main" w=(px)1080 h=(px)1080 {
      override node="hero" visible=#false
    }
    variant id="story" source="page.main" w=(px)1080 h=(px)1920 {
    }
  }
  document id="doc.v" title="V" {
    page id="page.main" w=(px)1920 h=(px)1080 {
      rect id="hero" x=(px)0 y=(px)0 w=(px)400 h=(px)300
    }
  }
}
"##;
    let report = parse_and_validate(src);
    let variant_codes: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with("variant."))
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        variant_codes.is_empty(),
        "clean variants block must produce no variant.* diagnostics; got {:?}",
        variant_codes
    );
}

// ── variant.duplicate_id ─────────────────────────────────────────────────────

/// Two `variant` entries with the same `id` → `variant.duplicate_id`.
#[test]
fn duplicate_variant_id_is_error() {
    let src = r##"zenith version=1 {
  project id="proj.dup" name="DUP"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="square" source="page.main" w=(px)1080 h=(px)1080 {
    }
    variant id="square" source="page.main" w=(px)800 h=(px)800 {
    }
  }
  document id="doc.dup" title="DUP" {
    page id="page.main" w=(px)1920 h=(px)1080 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "variant.duplicate_id"),
        "duplicate variant id must produce variant.duplicate_id; got {:?}",
        codes(&report)
    );
}

// ── variant.unknown_source ────────────────────────────────────────────────────

/// A variant whose `source` names a non-existent page → `variant.unknown_source`.
/// Additionally, override-node checks must be SUPPRESSED for that variant (no
/// `variant.override_unknown_node` emitted).
#[test]
fn unknown_source_page_is_error_and_suppresses_override_check() {
    let src = r##"zenith version=1 {
  project id="proj.src" name="SRC"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="square" source="page.missing" w=(px)1080 h=(px)1080 {
      override node="totally-absent" visible=#false
    }
  }
  document id="doc.src" title="SRC" {
    page id="page.main" w=(px)1920 h=(px)1080 {
      rect id="hero" x=(px)0 y=(px)0 w=(px)400 h=(px)300
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "variant.unknown_source"),
        "unknown source page must produce variant.unknown_source; got {:?}",
        codes(&report)
    );
    assert!(
        !has_code(&report, "variant.override_unknown_node"),
        "override-node check must be suppressed when source is unknown; got {:?}",
        codes(&report)
    );
}

// ── variant.invalid_dimension ─────────────────────────────────────────────────

/// A variant with `w=(pct)50` (non-px-convertible unit) → `variant.invalid_dimension`.
#[test]
fn non_px_convertible_width_is_invalid_dimension() {
    let src = r##"zenith version=1 {
  project id="proj.dim" name="DIM"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="pct-w" source="page.main" w=(pct)50 h=(px)1080 {
    }
  }
  document id="doc.dim" title="DIM" {
    page id="page.main" w=(px)1920 h=(px)1080 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "variant.invalid_dimension"),
        "non-px-convertible width must produce variant.invalid_dimension; got {:?}",
        codes(&report)
    );
}

/// A variant with `h=(px)0` (non-positive height) → `variant.invalid_dimension`.
#[test]
fn zero_height_is_invalid_dimension() {
    let src = r##"zenith version=1 {
  project id="proj.zero" name="ZERO"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="zero-h" source="page.main" w=(px)1080 h=(px)0 {
    }
  }
  document id="doc.zero" title="ZERO" {
    page id="page.main" w=(px)1920 h=(px)1080 {
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "variant.invalid_dimension"),
        "zero height must produce variant.invalid_dimension; got {:?}",
        codes(&report)
    );
}

// ── variant.override_unknown_node ─────────────────────────────────────────────

/// An override targeting a node id absent from the source page →
/// `variant.override_unknown_node`. The source page exists (so the check runs).
#[test]
fn override_targeting_absent_node_is_error() {
    let src = r##"zenith version=1 {
  project id="proj.ov" name="OV"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  variants {
    variant id="square" source="page.main" w=(px)1080 h=(px)1080 {
      override node="ghost" visible=#false
    }
  }
  document id="doc.ov" title="OV" {
    page id="page.main" w=(px)1920 h=(px)1080 {
      rect id="hero" x=(px)0 y=(px)0 w=(px)400 h=(px)300
    }
  }
}
"##;
    let report = parse_and_validate(src);
    assert!(
        has_code(&report, "variant.override_unknown_node"),
        "override targeting absent node must produce variant.override_unknown_node; got {:?}",
        codes(&report)
    );
}
