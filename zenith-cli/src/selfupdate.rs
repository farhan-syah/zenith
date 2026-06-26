//! `zenith update` — self-update by piping the published install script to `sh`.
//!
//! This is the system-facing edge of the CLI: it spawns processes and mutates
//! the install directory. It therefore lives outside `commands/`, which is pure
//! and never touches the environment. `lib.rs` owns the stdout/stderr edge; this
//! module owns the process plumbing and returns a plain `Result`.
//!
//! Zenith has no project domain, so the install script is fetched straight from
//! GitHub's raw endpoint on the default branch — the same script the
//! `curl … | sh` one-liner uses.

use std::process::{Command, Stdio};

/// Raw URL of the install script on the default branch.
const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/zenitheditor/zenith/main/scripts/install.sh";

/// Download and run the install script, forwarding the channel/version flags.
///
/// Mirrors the flags accepted by `scripts/install.sh`: `--pre` for the latest
/// prerelease, `--version <TAG>` for an exact version. With neither, the latest
/// stable release is installed.
pub fn run(pre: bool, version: Option<&str>) -> Result<(), String> {
    let mut sh_args: Vec<String> = vec!["-s".to_string(), "--".to_string()];
    if pre {
        sh_args.push("--pre".to_string());
    }
    if let Some(v) = version {
        let tag = if v.starts_with('v') {
            v.to_string()
        } else {
            format!("v{v}")
        };
        sh_args.push("--version".to_string());
        sh_args.push(tag);
    }

    let (dl_cmd, dl_args): (&str, &[&str]) = if which("curl") {
        ("curl", &["-fsSL", INSTALL_SCRIPT_URL])
    } else if which("wget") {
        ("wget", &["-qO-", INSTALL_SCRIPT_URL])
    } else {
        return Err("curl or wget is required for self-update".to_string());
    };

    let mut downloader = Command::new(dl_cmd)
        .args(dl_args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {dl_cmd}: {e}"))?;

    let pipe = downloader
        .stdout
        .take()
        .ok_or_else(|| "failed to capture the download stream".to_string())?;

    let status = Command::new("sh")
        .args(&sh_args)
        .stdin(pipe)
        .status()
        .map_err(|e| format!("failed to run the install script: {e}"))?;

    // Reap the downloader so it does not linger as a zombie.
    let _ = downloader.wait();

    if status.success() {
        Ok(())
    } else {
        Err("the install script failed".to_string())
    }
}

/// Return `true` if `cmd` is resolvable on `PATH` (via the POSIX `command -v`).
fn which(cmd: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
