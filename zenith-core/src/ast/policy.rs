//! Document-level diagnostic-policy AST types.
//!
//! A `.zen` document may carry a root `diagnostics { … }` block that adjusts how
//! specific diagnostic codes are *reported*. This is a lint-level model in the
//! spirit of rustc lint levels:
//!
//! ```text
//! diagnostics {
//!     allow "layout.off_canvas"     // suppress this advisory
//!     deny  "font.local"            // elevate to a blocking Error (CI gate)
//!     warn  "node.unknown_property" // force to Warning
//! }
//! ```
//!
//! The policy affects ONLY which diagnostics are surfaced by validation — it is
//! consulted in [`crate::validate`] and nowhere else. The scene compiler and the
//! render path never see it, so a policy can never change rendered output. A
//! document with no `diagnostics` block parses to an empty [`DiagnosticPolicy`],
//! which is an identity pass (no entries → no effect), so the default-off path is
//! byte-identical to before this feature existed.
//!
//! Bright lines (see [`crate::validate`] for the application logic):
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
/// applied to one diagnostic code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEntry {
    /// What this entry does to the named code.
    pub verb: PolicyVerb,
    /// The diagnostic code this entry governs, e.g. `"layout.off_canvas"`.
    pub code: String,
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
    /// The effective verb for `code`, or `None` if no entry governs it.
    ///
    /// Resolution is **last-wins**: when several entries name the same code, the
    /// last one written takes effect, so we scan in reverse and return the first
    /// match.
    pub fn verb_for(&self, code: &str) -> Option<&PolicyVerb> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.code == code)
            .map(|e| &e.verb)
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
            source_span: None,
        }
    }

    #[test]
    fn default_policy_is_empty_and_inert() {
        let p = DiagnosticPolicy::default();
        assert!(p.entries.is_empty());
        assert_eq!(p.verb_for("anything"), None);
    }

    #[test]
    fn verb_for_returns_the_governing_verb() {
        let p = DiagnosticPolicy {
            entries: vec![entry(PolicyVerb::Allow, "layout.off_canvas")],
        };
        assert_eq!(p.verb_for("layout.off_canvas"), Some(&PolicyVerb::Allow));
        assert_eq!(p.verb_for("token.unused"), None);
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
        assert_eq!(p.verb_for("node.unknown_property"), Some(&PolicyVerb::Warn));
    }
}
