//! Argument types for `zenith plugin` and its subcommands.

use clap::{Args, Subcommand};

/// Arguments for `zenith plugin`.
#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub command: PluginSub,
}

/// Subcommands of `zenith plugin`.
#[derive(Debug, Subcommand)]
pub enum PluginSub {
    /// Install the skill for the given agents (auto-detects when none are named).
    ///
    /// Install the Zenith agent skill so AI coding tools know how to drive the
    /// `zenith` CLI. Claude Code, Codex, and OpenCode receive the full folder skill (SKILL.md plus
    /// reference packs, templates, and themes); other agents receive a single self-contained rule
    /// file that points back at this self-documenting CLI. Writes are idempotent. With no agent flag,
    /// the present agents are auto-detected.
    Install(PluginInstallArgs),

    /// Remove a previously installed skill for the given agents.
    Uninstall(PluginUninstallArgs),

    /// Show where the Zenith skill is installed, per agent and scope.
    List,
}

/// Installation scope for `zenith plugin`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ScopeArg {
    /// Install for the current user (e.g. `~/.claude/skills/…`).
    User,
    /// Install into the current project (e.g. `./.claude/skills/…`).
    Project,
}

/// Per-agent selection flags, shared by install and uninstall.
///
/// With no flag set, the command auto-detects which agents are present. `--all`
/// selects every supported agent.
#[derive(Debug, Args)]
pub struct AgentFlags {
    /// Target Claude Code (folder skill).
    #[arg(long)]
    pub claude: bool,
    /// Target Codex (folder skill).
    #[arg(long)]
    pub codex: bool,
    /// Target OpenCode (folder skill).
    #[arg(long)]
    pub opencode: bool,
    /// Target Cursor (project rule).
    #[arg(long)]
    pub cursor: bool,
    /// Target Windsurf (project rule).
    #[arg(long)]
    pub windsurf: bool,
    /// Target Aider (rule file).
    #[arg(long)]
    pub aider: bool,
    /// Target Zed (rule file).
    #[arg(long)]
    pub zed: bool,
    /// Target Gemini CLI (rule file).
    #[arg(long)]
    pub gemini: bool,
    /// Target GitHub Copilot (rule file).
    #[arg(long)]
    pub copilot: bool,
    /// Target Continue (rule file).
    #[arg(long = "continue")]
    pub continue_dev: bool,
    /// Target Kiro (steering rule).
    #[arg(long)]
    pub kiro: bool,
    /// Target Antigravity (rule file).
    #[arg(long)]
    pub antigravity: bool,
    /// Target every supported agent.
    #[arg(long)]
    pub all: bool,
}

/// Arguments for `zenith plugin install`.
#[derive(Debug, Args)]
#[command(after_help = "EXAMPLES:\n  \
zenith plugin install                       # auto-detect and install for the user\n  \
zenith plugin install --claude --codex      # specific agents\n  \
zenith plugin install --all --scope project # everything, into ./\n  \
zenith plugin install --claude --dry-run    # preview without writing")]
pub struct PluginInstallArgs {
    #[command(flatten)]
    pub agents: AgentFlags,

    /// Install for the user (default) or the current project.
    #[arg(long, value_enum, default_value_t = ScopeArg::User)]
    pub scope: ScopeArg,

    /// Overwrite existing files whose content differs.
    #[arg(long)]
    pub force: bool,

    /// Show what would be written without touching the filesystem.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `zenith plugin uninstall`.
#[derive(Debug, Args)]
pub struct PluginUninstallArgs {
    #[command(flatten)]
    pub agents: AgentFlags,

    /// Uninstall from the user (default) or the current project.
    #[arg(long, value_enum, default_value_t = ScopeArg::User)]
    pub scope: ScopeArg,

    /// Show what would be removed without touching the filesystem.
    #[arg(long)]
    pub dry_run: bool,
}
