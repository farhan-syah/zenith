//! Render the per-agent contents of a rule-format skill file.
//!
//! Folder-format agents receive the embedded tree verbatim (its `SKILL.md`
//! frontmatter already follows the Agent Skills standard), so only the
//! single-file rule agents need rendering here.

use super::agent::Agent;
use super::assets::{skill_description, skill_md_body};

/// A note prepended to single-file installs: the reference packs are a
/// folder-skill feature, so here the agent should lean on the CLI's own help.
const RULE_NOTE: &str = "> **Single-file install.** The `references/` packs, `templates/`, and \
`themes/` ship with the full folder skill (Claude Code, Codex, OpenCode) and live in the \
[repo](https://github.com/zenitheditor/zenith). In this agent, drive the self-documenting \
`zenith` CLI directly — run `zenith --help` and `zenith <command> --help` for exact flags, \
and read the repo's `examples/*.zen` for syntax.";

/// Render the file contents for a rule-format `agent`.
pub fn render_rule(agent: Agent) -> String {
    let body = skill_md_body();
    let desc = skill_description().unwrap_or_default();
    match agent {
        // Cursor `.mdc` rules: frontmatter with `description` + `alwaysApply`.
        Agent::Cursor => format!(
            "---\nalwaysApply: false\ndescription: {desc}\n---\n\n{RULE_NOTE}\n\n{body}",
            desc = yaml_scalar(&desc),
        ),
        // Windsurf rules: bare markdown.
        Agent::Windsurf => format!("{RULE_NOTE}\n\n{body}"),
        // Everything else: plain markdown with an identifying H1.
        Agent::Aider
        | Agent::Zed
        | Agent::Gemini
        | Agent::Copilot
        | Agent::Continue
        | Agent::Kiro
        | Agent::Antigravity => format!("# Zenith\n\n{RULE_NOTE}\n\n{body}"),
        // Folder agents never reach here.
        Agent::ClaudeCode | Agent::Codex | Agent::OpenCode => body.to_owned(),
    }
}

/// Quote a YAML scalar only when it contains characters that would otherwise
/// break parsing.
fn yaml_scalar(s: &str) -> String {
    let needs = s.contains(':')
        || s.contains('#')
        || s.contains('"')
        || s.contains('\'')
        || s.starts_with(char::is_whitespace)
        || s.ends_with(char::is_whitespace);
    if needs {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}
