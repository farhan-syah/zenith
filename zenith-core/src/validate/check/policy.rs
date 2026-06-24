//! Diagnostic-policy application and self-validation.
//!
//! The document-level [`DiagnosticPolicy`] (parsed from a root `diagnostics { … }`
//! block) adjusts how diagnostic codes are *reported*. This module holds the two
//! steps the [driver](super::driver) runs at the end of validation:
//!
//! 1. [`apply_policy`] rewrites the assembled diagnostic list per the policy.
//! 2. [`check_policy_entries`] appends diagnostics ABOUT the policy itself
//!    (unknown codes, entries that try to weaken an Error).
//!
//! These run in that order so a policy can never suppress the warnings about
//! itself: self-validation is appended *after* `apply_policy` has already run.
//!
//! ## Bright lines
//!
//! - Policy applies to **Warning**- and **Advisory**-severity diagnostics only.
//!   **Error** severity is IMMUTABLE.
//!   - `allow`: severity != Error → drop it; Error → keep unchanged.
//!   - `deny` : severity != Error → set Error, keep; already-Error → keep.
//!   - `warn` : severity != Error → set Warning, keep; Error → keep unchanged.
//! - The policy NEVER changes rendered output — it is consulted only here, in
//!   validation. With an empty policy, `apply_policy` is the identity function.

use crate::ast::policy::{DiagnosticPolicy, PolicyVerb};
use crate::diag_catalog;
use crate::diagnostics::{Diagnostic, Severity};

/// Apply `policy` to an assembled diagnostic list, returning the adjusted list.
///
/// Each diagnostic is matched against the policy's effective verb for its code
/// (last-wins). Error-severity diagnostics are never dropped or weakened. With an
/// empty policy this returns the input unchanged (identity pass), preserving the
/// default-off byte-identical guarantee.
pub(super) fn apply_policy(
    diagnostics: Vec<Diagnostic>,
    policy: &DiagnosticPolicy,
) -> Vec<Diagnostic> {
    // Fast path: an empty policy is an exact identity pass.
    if policy.entries.is_empty() {
        return diagnostics;
    }

    let mut out: Vec<Diagnostic> = Vec::with_capacity(diagnostics.len());
    for mut diag in diagnostics {
        match policy.verb_for(&diag.code) {
            None => out.push(diag),
            Some(verb) => match verb {
                PolicyVerb::Allow => {
                    // Drop Warning/Advisory; keep Error untouched (immutable).
                    match diag.severity {
                        Severity::Error => out.push(diag),
                        Severity::Warning | Severity::Advisory => {
                            // Suppressed: do not push.
                        }
                    }
                }
                PolicyVerb::Deny => {
                    // Elevate Warning/Advisory to Error; already-Error stays.
                    match diag.severity {
                        Severity::Error => {}
                        Severity::Warning | Severity::Advisory => {
                            diag.severity = Severity::Error;
                        }
                    }
                    out.push(diag);
                }
                PolicyVerb::Warn => {
                    // Force Warning/Advisory to Warning; Error is immutable.
                    match diag.severity {
                        Severity::Error => {}
                        Severity::Warning | Severity::Advisory => {
                            diag.severity = Severity::Warning;
                        }
                    }
                    out.push(diag);
                }
            },
        }
    }
    out
}

/// Append diagnostics ABOUT the policy itself onto `diagnostics`.
///
/// - A code not present in the catalog → `policy.unknown_code` (Warning).
/// - An `allow`/`warn` entry naming an always-Error catalog code →
///   `policy.ineffective_on_error` (Warning), explaining that Errors cannot be
///   weakened. A `deny` on an Error code is a silent no-op (it is already an
///   Error), so it is NOT flagged.
///
/// Called AFTER [`apply_policy`] so these warnings cannot be suppressed by the
/// very policy they describe.
pub(super) fn check_policy_entries(policy: &DiagnosticPolicy, diagnostics: &mut Vec<Diagnostic>) {
    for entry in &policy.entries {
        match diag_catalog::lookup(&entry.code) {
            None => {
                diagnostics.push(Diagnostic::warning(
                    "policy.unknown_code",
                    format!(
                        "diagnostics policy names '{}', which is not a diagnostic code this \
                         engine emits; the entry has no effect",
                        entry.code
                    ),
                    entry.source_span,
                    Some(entry.code.clone()),
                ));
            }
            Some(catalog_entry) => {
                if !catalog_entry.is_governable() {
                    // An always-Error code: only `allow`/`warn` are ineffective;
                    // `deny` is a silent no-op (already Error).
                    let verb_name = match entry.verb {
                        PolicyVerb::Allow => "allow",
                        PolicyVerb::Warn => "warn",
                        // `deny` on an always-Error is a no-op (already Error) — do not flag.
                        PolicyVerb::Deny => continue,
                    };
                    diagnostics.push(Diagnostic::warning(
                        "policy.ineffective_on_error",
                        format!(
                            "diagnostics policy `{verb_name} \"{}\"` has no effect: '{}' is an \
                             integrity Error and Error severity cannot be suppressed or \
                             weakened",
                            entry.code, entry.code
                        ),
                        entry.source_span,
                        Some(entry.code.clone()),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::policy::PolicyEntry;

    fn policy(entries: Vec<(PolicyVerb, &str)>) -> DiagnosticPolicy {
        DiagnosticPolicy {
            entries: entries
                .into_iter()
                .map(|(verb, code)| PolicyEntry {
                    verb,
                    code: code.to_owned(),
                    source_span: None,
                })
                .collect(),
        }
    }

    fn diag(code: &str, severity: Severity) -> Diagnostic {
        Diagnostic::new(code, severity, "msg", None, None)
    }

    #[test]
    fn empty_policy_is_identity() {
        let input = vec![diag("layout.off_canvas", Severity::Advisory)];
        let out = apply_policy(input.clone(), &DiagnosticPolicy::default());
        assert_eq!(out, input);
    }

    #[test]
    fn allow_drops_advisory_but_not_error() {
        let input = vec![
            diag("layout.off_canvas", Severity::Advisory),
            diag("id.duplicate", Severity::Error),
        ];
        let p = policy(vec![
            (PolicyVerb::Allow, "layout.off_canvas"),
            (PolicyVerb::Allow, "id.duplicate"),
        ]);
        let out = apply_policy(input, &p);
        // Advisory dropped; Error survives unchanged.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "id.duplicate");
        assert_eq!(out[0].severity, Severity::Error);
    }

    #[test]
    fn deny_elevates_to_error() {
        let input = vec![diag("token.unused", Severity::Advisory)];
        let p = policy(vec![(PolicyVerb::Deny, "token.unused")]);
        let out = apply_policy(input, &p);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Error);
    }

    #[test]
    fn warn_forces_warning_and_last_wins_over_deny() {
        let input = vec![diag("node.unknown_property", Severity::Warning)];
        // deny then warn → warn wins (last).
        let p = policy(vec![
            (PolicyVerb::Deny, "node.unknown_property"),
            (PolicyVerb::Warn, "node.unknown_property"),
        ]);
        let out = apply_policy(input, &p);
        assert_eq!(out[0].severity, Severity::Warning);
    }

    #[test]
    fn unknown_code_is_flagged() {
        let p = policy(vec![(PolicyVerb::Allow, "not.a_real_code")]);
        let mut out = Vec::new();
        check_policy_entries(&p, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "policy.unknown_code");
    }

    #[test]
    fn allow_on_error_code_is_flagged_but_deny_is_silent() {
        let p = policy(vec![
            (PolicyVerb::Allow, "id.duplicate"),
            (PolicyVerb::Deny, "id.duplicate"),
        ]);
        let mut out = Vec::new();
        check_policy_entries(&p, &mut out);
        // Only the `allow` entry is flagged; `deny` on an Error is a no-op.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "policy.ineffective_on_error");
    }
}
