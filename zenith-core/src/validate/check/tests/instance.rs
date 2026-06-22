//! `instance` validation tests (moved verbatim from the former single-file
//! `validate/check/tests.rs`; test bodies unchanged).

// ── Component / instance validation ───────────────────────────────────────────

mod component_validation {
    use crate::parse::{KdlAdapter, KdlSource};
    use crate::validate::validate;

    fn parse_doc(src: &str) -> crate::ast::Document {
        KdlAdapter.parse(src.as_bytes()).expect("must parse")
    }

    fn has_code(report: &crate::validate::ValidationReport, code: &str) -> bool {
        report.diagnostics.iter().any(|d| d.code == code)
    }

    const BASE_TOKENS: &str = r##"  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#101010"
    token id="color.fg" type="color" value="#fafafa"
  }
  styles {}"##;

    #[test]
    fn unknown_component_reference_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="real.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="missing" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_reference"),
            "expected component.unknown_reference: {:?}",
            report.diagnostics
        );
        assert!(report.has_errors());
    }

    #[test]
    fn unknown_override_target_is_warning() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      instance id="inst.1" component="c.one" x=(px)0 y=(px)0 {{
        override ref="does.not.exist" {{ span "X" }}
      }}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "component.unknown_override_target"),
            "expected component.unknown_override_target: {:?}",
            report.diagnostics
        );
        // It is a Warning, not a hard error.
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "component.unknown_override_target"
                    && d.severity == crate::diagnostics::Severity::Error)
        );
    }

    #[test]
    fn duplicate_component_id_is_error() {
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="dup" {{
      rect id="a" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
    component id="dup" {{
      rect id="b" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "duplicate component id must be id.duplicate: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn local_child_ids_do_not_collide_across_components() {
        // Two components both declare a child id "bg" and "label" — this must
        // NOT trigger id.duplicate because component child ids are local.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.a" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "A" }}
    }}
    component id="c.b" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      text id="label" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.fg" {{ span "B" }}
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{}}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            !has_code(&report, "id.duplicate"),
            "component-local ids must not collide across components: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn instance_id_participates_in_global_uniqueness() {
        // An instance id that collides with a page node id → id.duplicate.
        let src = format!(
            r##"zenith version=1 {{
  project id="p" name="P"
{BASE_TOKENS}
  components {{
    component id="c.one" {{
      rect id="bg" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
    }}
  }}
  document id="d" {{
    page id="pg" w=(px)100 h=(px)100 {{
      rect id="dup.id" x=(px)0 y=(px)0 w=(px)10 h=(px)10 fill=(token)"color.bg"
      instance id="dup.id" component="c.one" x=(px)0 y=(px)0 {{}}
    }}
  }}
}}
"##
        );
        let report = validate(&parse_doc(&src));
        assert!(
            has_code(&report, "id.duplicate"),
            "instance id must participate in global uniqueness: {:?}",
            report.diagnostics
        );
    }
}
