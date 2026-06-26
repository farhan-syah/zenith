//! Integration tests for `zenith plugin` — the multi-agent skill installer.

use std::path::Path;

use zenith_cli::commands::plugin::agent::{Agent, Scope, SkillFormat};
use zenith_cli::commands::plugin::assets::{
    COMMAND_FILES, SKILL_FILES, skill_description, skill_md_body, skill_md_raw,
};
use zenith_cli::commands::plugin::detect::is_installed;
use zenith_cli::commands::plugin::install::{WriteOutcome, install_agent};
use zenith_cli::commands::plugin::paths::{SkillTarget, command_dir, skill_target};
use zenith_cli::commands::plugin::uninstall::{RemoveOutcome, uninstall_agent};

// ── Embedded assets ─────────────────────────────────────────────────────────

#[test]
fn skill_tree_is_embedded() {
    assert!(!SKILL_FILES.is_empty(), "skill tree must be embedded");
    assert!(
        SKILL_FILES.iter().any(|(name, _)| *name == "SKILL.md"),
        "SKILL.md must be present"
    );
    assert!(
        SKILL_FILES
            .iter()
            .any(|(n, _)| n.starts_with("references/")),
        "reference packs must be present"
    );
    assert!(!COMMAND_FILES.is_empty(), "commands must be embedded");
}

#[test]
fn frontmatter_is_stripped_for_rule_body() {
    assert!(skill_md_raw().contains("# Zenith"));
    assert!(
        !skill_md_body().starts_with("---"),
        "body has frontmatter removed"
    );
    assert!(skill_md_body().contains("# Zenith"));
    let desc = skill_description().expect("description parsed from frontmatter");
    assert!(desc.contains(".zen"));
}

// ── Path table ──────────────────────────────────────────────────────────────

#[test]
fn folder_agents_get_a_skill_directory() {
    let root = Path::new("/proj");
    for agent in [Agent::ClaudeCode, Agent::Codex, Agent::OpenCode] {
        assert_eq!(agent.format(), SkillFormat::Folder);
        match skill_target(agent, Scope::Project, root) {
            Some(SkillTarget::Folder(dir)) => {
                assert!(dir.ends_with("skills/zenith"), "{dir:?}");
                assert!(dir.starts_with("/proj"));
            }
            other => panic!("{agent:?} should be a folder target, got {other:?}"),
        }
    }
}

#[test]
fn rule_agents_get_a_single_file() {
    let root = Path::new("/proj");
    match skill_target(Agent::Cursor, Scope::Project, root) {
        Some(SkillTarget::Rule(p)) => assert!(p.ends_with(".cursor/rules/zenith.mdc"), "{p:?}"),
        other => panic!("cursor should be a rule file, got {other:?}"),
    }
}

#[test]
fn cursor_and_windsurf_have_no_user_scope() {
    let root = Path::new("/proj");
    assert!(skill_target(Agent::Cursor, Scope::User, root).is_none());
    assert!(skill_target(Agent::Windsurf, Scope::User, root).is_none());
}

#[test]
fn only_claude_and_opencode_have_command_dirs() {
    let root = Path::new("/proj");
    assert!(command_dir(Agent::ClaudeCode, Scope::Project, root).is_some());
    assert!(command_dir(Agent::OpenCode, Scope::Project, root).is_some());
    assert!(command_dir(Agent::Codex, Scope::Project, root).is_none());
    assert!(command_dir(Agent::Cursor, Scope::Project, root).is_none());
}

#[test]
fn agent_parse_accepts_aliases() {
    assert_eq!(Agent::parse("claude"), Some(Agent::ClaudeCode));
    assert_eq!(Agent::parse("claude-code"), Some(Agent::ClaudeCode));
    assert_eq!(Agent::parse("open-code"), Some(Agent::OpenCode));
    assert_eq!(Agent::parse("nope"), None);
}

// ── Install / idempotency / force ───────────────────────────────────────────

#[test]
fn folder_install_writes_whole_tree_and_commands() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false, false);
    assert!(report.unsupported.is_none());
    // Every file installed; none errored.
    assert_eq!(report.files.len(), SKILL_FILES.len() + COMMAND_FILES.len());
    assert!(
        report
            .files
            .iter()
            .all(|f| matches!(f.outcome, Ok(WriteOutcome::Installed)))
    );
    // Entry file and a reference pack exist on disk.
    let skill = tmp.path().join(".claude/skills/zenith/SKILL.md");
    let refs = tmp
        .path()
        .join(".claude/skills/zenith/references/layout.md");
    let cmd = tmp.path().join(".claude/commands/zenith-new.md");
    assert!(skill.is_file() && refs.is_file() && cmd.is_file());
    assert!(is_installed(Agent::ClaudeCode, Scope::Project, tmp.path()));
}

#[test]
fn second_install_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    install_agent(Agent::OpenCode, Scope::Project, tmp.path(), false, false);
    let again = install_agent(Agent::OpenCode, Scope::Project, tmp.path(), false, false);
    assert!(
        again
            .files
            .iter()
            .all(|f| matches!(f.outcome, Ok(WriteOutcome::AlreadyCurrent)))
    );
}

#[test]
fn changed_file_needs_force() {
    let tmp = tempfile::tempdir().expect("tempdir");
    install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false, false);
    let skill = tmp.path().join(".claude/skills/zenith/SKILL.md");
    std::fs::write(&skill, b"tampered").expect("write");

    // Without force: reported as WouldOverwrite, file left untouched.
    let no_force = install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false, false);
    assert!(
        no_force
            .files
            .iter()
            .any(|f| matches!(f.outcome, Ok(WriteOutcome::WouldOverwrite)))
    );
    assert_eq!(std::fs::read(&skill).unwrap(), b"tampered");

    // With force: overwritten back to the embedded content.
    install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), true, false);
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), skill_md_raw());
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false, true);
    assert!(
        report
            .files
            .iter()
            .all(|f| matches!(f.outcome, Ok(WriteOutcome::Installed)))
    );
    assert!(
        !tmp.path().join(".claude").exists(),
        "dry-run must not write"
    );
}

#[test]
fn rule_install_writes_one_rendered_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = install_agent(Agent::Cursor, Scope::Project, tmp.path(), false, false);
    assert_eq!(report.files.len(), 1);
    let mdc = tmp.path().join(".cursor/rules/zenith.mdc");
    let body = std::fs::read_to_string(&mdc).expect("read mdc");
    assert!(body.starts_with("---\nalwaysApply: false"));
    assert!(body.contains("Single-file install"));
}

// ── Uninstall ───────────────────────────────────────────────────────────────

#[test]
fn uninstall_removes_tree_and_commands() {
    let tmp = tempfile::tempdir().expect("tempdir");
    install_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false, false);
    let report = uninstall_agent(Agent::ClaudeCode, Scope::Project, tmp.path(), false);
    assert!(
        report
            .items
            .iter()
            .all(|i| matches!(i.outcome, Ok(RemoveOutcome::Removed)))
    );
    assert!(!tmp.path().join(".claude/skills/zenith").exists());
    assert!(!is_installed(Agent::ClaudeCode, Scope::Project, tmp.path()));
}

#[test]
fn uninstall_absent_is_clean() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = uninstall_agent(Agent::Zed, Scope::Project, tmp.path(), false);
    assert!(
        report
            .items
            .iter()
            .all(|i| matches!(i.outcome, Ok(RemoveOutcome::Absent)))
    );
}
