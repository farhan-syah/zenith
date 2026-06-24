//! Diagnostic-policy resolution from config files and command-line flags.
//!
//! The effective diagnostic policy for a `validate` run is assembled from four
//! sources, in increasing precedence:
//!
//! 1. **global config** — `<config_dir>/zenith/config.kdl`
//! 2. **local config** — the nearest `.zenith.kdl` found by walking up from the
//!    document's directory to the filesystem root
//! 3. **in-file policy** — the document's own `diagnostics { … }` block
//! 4. **CLI flags** — `--allow` / `--deny` / `--warn`
//!
//! Because policy resolution is **last-wins** (see
//! [`zenith_core::DiagnosticPolicy::verb_for`]), the four sources are simply
//! concatenated low→high into one policy and applied ONCE. The highest-
//! precedence entry for any given code wins, yielding
//! `CLI > in-file > local > global`.
//!
//! All loaders are path-injectable so tests can point them at temp directories
//! without mutating process-global state (`$HOME`, cwd). The production
//! [`load_global_policy`] resolves the config directory from `$HOME` exactly as
//! [`crate::commands`] resolves user-scope paths elsewhere in the CLI.

use std::path::{Path, PathBuf};

use zenith_core::{DiagnosticPolicy, PolicyEntry, PolicyVerb, parse_diagnostic_policy};

/// The file name of a local (per-project / per-directory) config block.
const LOCAL_CONFIG_NAME: &str = ".zenith.kdl";

/// CLI-supplied policy adjustments, one bucket per verb. Each `String` is a
/// diagnostic code. Order within a bucket and across buckets is allow → warn →
/// deny so a later flag of a different verb for the same code still wins via
/// last-wins resolution; entries are appended at the highest precedence.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CliPolicyFlags {
    /// Codes passed via `--allow`.
    pub allow: Vec<String>,
    /// Codes passed via `--warn`.
    pub warn: Vec<String>,
    /// Codes passed via `--deny`.
    pub deny: Vec<String>,
}

impl CliPolicyFlags {
    /// Whether any flag was supplied. An all-empty set contributes no entries,
    /// keeping the merged policy byte-identical to the no-flags case.
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.warn.is_empty() && self.deny.is_empty()
    }

    /// Convert the flag buckets into [`PolicyEntry`] records. CLI entries carry
    /// no source span. The buckets are emitted allow → warn → deny, so when the
    /// same code is passed under multiple verbs the last-wins rule makes
    /// `--deny` the firmest gate.
    fn entries(&self) -> Vec<PolicyEntry> {
        let mut entries = Vec::with_capacity(self.allow.len() + self.warn.len() + self.deny.len());
        for code in &self.allow {
            entries.push(PolicyEntry {
                verb: PolicyVerb::Allow,
                code: code.clone(),
                source_span: None,
            });
        }
        for code in &self.warn {
            entries.push(PolicyEntry {
                verb: PolicyVerb::Warn,
                code: code.clone(),
                source_span: None,
            });
        }
        for code in &self.deny {
            entries.push(PolicyEntry {
                verb: PolicyVerb::Deny,
                code: code.clone(),
                source_span: None,
            });
        }
        entries
    }
}

/// Merge the four policy tiers into one [`DiagnosticPolicy`].
///
/// The tiers are concatenated low→high — `global ++ local ++ in_file ++ cli` —
/// so that last-wins resolution yields `CLI > in-file > local > global`. The
/// result is applied exactly once at the validation choke point. When every
/// tier is empty the merged policy is empty (an identity pass), preserving the
/// additive byte-identical guarantee.
pub fn merge_policy(
    global: &DiagnosticPolicy,
    local: &DiagnosticPolicy,
    in_file: &DiagnosticPolicy,
    flags: &CliPolicyFlags,
) -> DiagnosticPolicy {
    let cli_entries = flags.entries();
    let mut entries = Vec::with_capacity(
        global.entries.len() + local.entries.len() + in_file.entries.len() + cli_entries.len(),
    );
    entries.extend(global.entries.iter().cloned());
    entries.extend(local.entries.iter().cloned());
    entries.extend(in_file.entries.iter().cloned());
    entries.extend(cli_entries);
    DiagnosticPolicy { entries }
}

/// Load a diagnostic policy from a KDL config file.
///
/// A missing file is not an error — it yields [`DiagnosticPolicy::default`]. A
/// present-but-unreadable file, or a malformed config block, returns a
/// human-facing error message.
pub fn load_policy_file(path: &Path) -> Result<DiagnosticPolicy, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DiagnosticPolicy::default());
        }
        Err(e) => return Err(format!("cannot read config '{}': {e}", path.display())),
    };
    parse_diagnostic_policy(&bytes)
        .map_err(|e| format!("invalid config '{}': {}", path.display(), e.message))
}

/// Walk up from `start_dir` to the filesystem root, loading the first
/// `.zenith.kdl` found. If none is found, returns [`DiagnosticPolicy::default`].
///
/// The walk terminates cleanly at the root (where `parent()` is `None`) without
/// panicking.
pub fn find_local_policy(start_dir: &Path) -> Result<DiagnosticPolicy, String> {
    let mut dir: Option<&Path> = Some(start_dir);
    while let Some(current) = dir {
        let candidate = current.join(LOCAL_CONFIG_NAME);
        if candidate.is_file() {
            return load_policy_file(&candidate);
        }
        dir = current.parent();
    }
    Ok(DiagnosticPolicy::default())
}

/// Load the global policy from `<config_dir>/zenith/config.kdl`.
///
/// Injectable variant: the caller supplies the base config directory so tests
/// can point at a temp directory. A missing file yields the default policy.
pub fn load_global_policy_in(config_dir: &Path) -> Result<DiagnosticPolicy, String> {
    let path = config_dir.join("zenith").join("config.kdl");
    load_policy_file(&path)
}

/// Load the global policy from `$HOME/.config/zenith/config.kdl`.
///
/// Production variant: resolves the config directory from `$HOME` (matching the
/// user-scope path convention used elsewhere in the CLI). When `$HOME` is
/// absent there is no global config to read, so the default policy is returned.
pub fn load_global_policy() -> Result<DiagnosticPolicy, String> {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) => load_global_policy_in(&home.join(".config")),
        None => Ok(DiagnosticPolicy::default()),
    }
}

/// Load the global and local policies for a command run.
///
/// This is the shared resolution preamble used by both `validate` and `render`:
/// - Global policy is always loaded from `$HOME/.config/zenith/config.kdl`.
/// - Local policy is walked up from `start_dir` when `Some`; when `None` the
///   local policy is the default (empty).
///
/// Returns `Err` with a human-facing message on any config I/O or parse
/// failure. Missing files are not errors — they yield the default policy.
pub fn load_global_and_local(
    start_dir: Option<&Path>,
) -> Result<(DiagnosticPolicy, DiagnosticPolicy), String> {
    let global = load_global_policy()?;
    let local = match start_dir {
        Some(dir) => find_local_policy(dir)?,
        None => DiagnosticPolicy::default(),
    };
    Ok((global, local))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny(code: &str) -> DiagnosticPolicy {
        DiagnosticPolicy {
            entries: vec![PolicyEntry {
                verb: PolicyVerb::Deny,
                code: code.to_owned(),
                source_span: None,
            }],
        }
    }

    fn allow(code: &str) -> DiagnosticPolicy {
        DiagnosticPolicy {
            entries: vec![PolicyEntry {
                verb: PolicyVerb::Allow,
                code: code.to_owned(),
                source_span: None,
            }],
        }
    }

    #[test]
    fn empty_everything_is_identity() {
        let merged = merge_policy(
            &DiagnosticPolicy::default(),
            &DiagnosticPolicy::default(),
            &DiagnosticPolicy::default(),
            &CliPolicyFlags::default(),
        );
        assert!(merged.entries.is_empty());
    }

    #[test]
    fn cli_beats_in_file_beats_local_beats_global() {
        let global = allow("a");
        let local = deny("a");
        let in_file = allow("a");
        let flags = CliPolicyFlags {
            deny: vec!["a".to_owned()],
            ..Default::default()
        };
        let merged = merge_policy(&global, &local, &in_file, &flags);
        // Last-wins: CLI deny is final.
        assert_eq!(merged.verb_for("a"), Some(&PolicyVerb::Deny));
    }

    #[test]
    fn in_file_beats_config_when_no_flag() {
        let global = deny("a");
        let local = deny("a");
        let in_file = allow("a");
        let merged = merge_policy(&global, &local, &in_file, &CliPolicyFlags::default());
        assert_eq!(merged.verb_for("a"), Some(&PolicyVerb::Allow));
    }

    #[test]
    fn missing_file_is_default() {
        let policy = load_policy_file(Path::new("/no/such/zenith/config.kdl"))
            .expect("missing file must be ok");
        assert!(policy.entries.is_empty());
    }

    #[test]
    fn find_local_handles_root_without_panic() {
        // Root has no parent; walk must terminate cleanly.
        let policy = find_local_policy(Path::new("/")).expect("root walk must be ok");
        assert!(policy.entries.is_empty());
    }
}
