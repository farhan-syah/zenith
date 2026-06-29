//! Document-level diagnostic-policy AST types.
//!
//! A `.zen` document may carry a root `diagnostics { … }` block that adjusts how
//! specific diagnostic codes are *reported*. This is a lint-level model in the
//! spirit of rustc lint levels:
//!
//! ```text
//! diagnostics {
//!     allow "layout.off_canvas"     // suppress this advisory
//!     allow "layout.off_canvas" "bg.glow" "bg.rim" // suppress only these nodes
//!     deny  "font.local"            // elevate to a blocking Error (CI gate)
//!     warn  "node.unknown_property" // force to Warning
//! }
//! ```
//!
//! The policy affects ONLY which diagnostics are surfaced by validation — it is
//! consulted in [`crate::validate()`] and nowhere else. The scene compiler and the
//! render path never see it, so a policy can never change rendered output. A
//! document with no `diagnostics` block parses to an empty [`DiagnosticPolicy`],
//! which is an identity pass (no entries → no effect), so the default-off path is
//! byte-identical to before this feature existed.
//!
//! Bright lines (see [`crate::validate()`] for the application logic):
//! - Policy applies to **Warning**- and **Advisory**-severity diagnostics only.
//!   **Error** severity is IMMUTABLE: an `allow` never drops an Error and a
//!   `warn` never weakens an Error.
//! - **Last-wins** for duplicate codes: a later entry for the same code overrides
//!   any earlier one (exactly like rustc lint levels on the command line).

use super::Span;

/// The verb of a single [`PolicyEntry`] — how a diagnostic code's reporting is
/// adjusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVerb {
    /// Suppress the diagnostic when its severity is Warning or Advisory. An
    /// Error-severity diagnostic is left unchanged (Errors are immutable).
    Allow,
    /// Elevate the diagnostic to Error severity (turning a Warning/Advisory into
    /// a blocking Error). An already-Error diagnostic stays Error.
    Deny,
    /// Force the diagnostic to Warning severity when it is currently Warning or
    /// Advisory. An Error-severity diagnostic is left unchanged.
    Warn,
}

/// A single entry inside a document's `diagnostics { … }` block: one verb
/// applied to one diagnostic code, optionally scoped to diagnostic subject ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEntry {
    /// What this entry does to the named code.
    pub verb: PolicyVerb,
    /// The diagnostic code this entry governs, e.g. `"layout.off_canvas"`.
    pub code: String,
    /// Optional diagnostic subject ids this entry governs, e.g. `"bg.glow"`.
    ///
    /// When empty, the entry governs every diagnostic with the matching code.
    /// When non-empty, the entry governs only diagnostics whose `subject_id`
    /// exactly matches one of these values.
    pub subjects: Vec<String>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The complete document-level diagnostic policy: an ordered list of
/// [`PolicyEntry`] records as written in the `diagnostics { … }` block.
///
/// The default value is empty, which is an identity pass: with no entries the
/// policy has no effect on validation output. Resolution is **last-wins** — see
/// [`DiagnosticPolicy::verb_for`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticPolicy {
    /// Policy entries in source order. Declaration order is preserved so the
    /// formatter can round-trip the block verbatim; resolution applies last-wins.
    pub entries: Vec<PolicyEntry>,
}

impl DiagnosticPolicy {
    /// The effective verb for `code` and `subject_id`, or `None` if no entry
    /// governs that diagnostic.
    ///
    /// Resolution is **last-wins** among entries that match both code and
    /// subjects. A code-only entry matches every subject for that code; a scoped
    /// entry matches only the listed subject ids.
    pub fn verb_for(&self, code: &str, subject_id: Option<&str>) -> Option<&PolicyVerb> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.matches(code, subject_id))
            .map(|e| &e.verb)
    }
}

impl PolicyEntry {
    fn matches(&self, code: &str, subject_id: Option<&str>) -> bool {
        if self.code != code {
            return false;
        }
        if self.subjects.is_empty() {
            return true;
        }
        match subject_id {
            Some(actual) => self.subjects.iter().any(|expected| expected == actual),
            None => false,
        }
    }
}

// `Eq` is derivable on `DiagnosticPolicy`/`PolicyEntry` only because `PolicyVerb`
// is `Eq` and `Span`/`String` are `Eq`; if a future field breaks `Eq`, drop it
// from the derive rather than suppressing.

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(verb: PolicyVerb, code: &str) -> PolicyEntry {
        PolicyEntry {
            verb,
            code: code.to_owned(),
            subjects: Vec::new(),
            source_span: None,
        }
    }

    #[test]
    fn default_policy_is_empty_and_inert() {
        let p = DiagnosticPolicy::default();
        assert!(p.entries.is_empty());
        assert_eq!(p.verb_for("anything", None), None);
    }

    #[test]
    fn verb_for_returns_the_governing_verb() {
        let p = DiagnosticPolicy {
            entries: vec![entry(PolicyVerb::Allow, "layout.off_canvas")],
        };
        assert_eq!(
            p.verb_for("layout.off_canvas", Some("r.off")),
            Some(&PolicyVerb::Allow)
        );
        assert_eq!(p.verb_for("token.unused", None), None);
    }

    #[test]
    fn verb_for_is_last_wins() {
        let p = DiagnosticPolicy {
            entries: vec![
                entry(PolicyVerb::Deny, "node.unknown_property"),
                entry(PolicyVerb::Warn, "node.unknown_property"),
            ],
        };
        // The later `warn` overrides the earlier `deny`.
        assert_eq!(
            p.verb_for("node.unknown_property", None),
            Some(&PolicyVerb::Warn)
        );
    }

    #[test]
    fn scoped_entry_matches_only_its_subjects() {
        let p = DiagnosticPolicy {
            entries: vec![PolicyEntry {
                verb: PolicyVerb::Allow,
                code: "layout.off_canvas".to_owned(),
                subjects: vec!["bg.glow".to_owned(), "bg.rim".to_owned()],
                source_span: None,
            }],
        };
        assert_eq!(
            p.verb_for("layout.off_canvas", Some("bg.glow")),
            Some(&PolicyVerb::Allow)
        );
        assert_eq!(
            p.verb_for("layout.off_canvas", Some("bg.rim")),
            Some(&PolicyVerb::Allow)
        );
        assert_eq!(p.verb_for("layout.off_canvas", Some("shape.1")), None);
        assert_eq!(p.verb_for("layout.off_canvas", None), None);
    }
}
