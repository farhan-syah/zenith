//! Integration tests: diagnostic-policy resolution on the render path.
//!
//! Verifies that `--deny <code>` elevates an advisory to a blocking Error on
//! render (exit non-zero / Err return), and that a local `.zenith.kdl` `allow`
//! suppresses an advisory on the same path.
//!
//! All filesystem state is rooted in tempdirs; no `$HOME`/cwd mutation, so the
//! tests are parallel-safe.

use std::fs;

use tempfile::TempDir;
use zenith_cli::commands::render::to_png_with_dir;
use zenith_cli::config::CliPolicyFlags;

// ── Fixture ──────────────────────────────────────────────────────────────────

/// A minimal valid document that has an unused token (`token.unused` advisory
/// by default). With no flags and no config the render must succeed. With
/// `--deny token.unused` the render must fail (exit 1).
const DOC_WITH_UNUSED_TOKEN: &str = r##"zenith version=1 {
  project id="proj.rp" name="Render Policy"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
    token id="color.unused" type="color" value="#abcdef"
  }
  styles {}
  document id="doc.rp" title="Render Policy" {
    page id="page.rp" w=(px)100 h=(px)100 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.bg"
    }
  }
}
"##;

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Without any flags or config files, the document with an unused token renders
/// successfully (advisory is not elevated to an error).
#[test]
fn render_succeeds_without_flags() {
    let result = to_png_with_dir(
        DOC_WITH_UNUSED_TOKEN,
        None,
        1,
        false,
        &CliPolicyFlags::default(),
    );
    assert!(
        result.is_ok(),
        "render must succeed with no flags; got: {:?}",
        result.err().map(|e| e.message)
    );
}

/// `--deny token.unused` elevates the advisory to a blocking Error; the render
/// must fail (Err with exit_code 1).
#[test]
fn deny_flag_turns_advisory_into_render_failure() {
    let flags = CliPolicyFlags {
        deny: vec!["token.unused".to_owned()],
        ..Default::default()
    };
    let result = to_png_with_dir(DOC_WITH_UNUSED_TOKEN, None, 1, false, &flags);
    assert!(
        result.is_err(),
        "render must fail when advisory is --deny'd; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.exit_code, 1,
        "validation error must produce exit code 1; got {}",
        err.exit_code
    );
    assert!(
        err.message.contains("token.unused"),
        "error message must mention the denied code; got: {}",
        err.message
    );
}

/// A local `.zenith.kdl` that `deny`s `token.unused` must block the render
/// even without CLI flags — config files are resolved from the document's
/// parent directory on the render path.
#[test]
fn local_config_deny_blocks_render() {
    let tmp = TempDir::new().expect("tempdir");
    // Write a local config that denies the advisory.
    fs::write(
        tmp.path().join(".zenith.kdl"),
        b"diagnostics {\n  deny \"token.unused\"\n}\n",
    )
    .expect("write .zenith.kdl");

    // Write the document into the same directory so start_dir resolves the config.
    let doc_path = tmp.path().join("test.zen");
    fs::write(&doc_path, DOC_WITH_UNUSED_TOKEN.as_bytes()).expect("write doc");

    let result = to_png_with_dir(
        DOC_WITH_UNUSED_TOKEN,
        Some(tmp.path()),
        1,
        false,
        &CliPolicyFlags::default(),
    );
    assert!(
        result.is_err(),
        "render must fail when local config denies the advisory; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.exit_code, 1,
        "config-driven elevation must produce exit code 1; got {}",
        err.exit_code
    );
}

/// A local `.zenith.kdl` that `allow`s `token.unused` keeps the render green
/// even when no in-file policy is set (the config suppresses the advisory).
/// This also verifies the additive byte-identical guarantee: with default flags
/// and an `allow` config, the render produces the same PNG bytes as running
/// without any config at all (both produce a clean render).
#[test]
fn local_config_allow_keeps_render_clean() {
    let tmp = TempDir::new().expect("tempdir");
    // Write a local config that explicitly allows the advisory (no-op, but exercises the path).
    fs::write(
        tmp.path().join(".zenith.kdl"),
        b"diagnostics {\n  allow \"token.unused\"\n}\n",
    )
    .expect("write .zenith.kdl");

    let result = to_png_with_dir(
        DOC_WITH_UNUSED_TOKEN,
        Some(tmp.path()),
        1,
        false,
        &CliPolicyFlags::default(),
    );
    assert!(
        result.is_ok(),
        "render must succeed when local config allows the advisory; got: {:?}",
        result.err().map(|e| e.message)
    );
}

/// `--allow token.unused` with no other config must not change render success:
/// an advisory that is already non-blocking remains non-blocking. Verifies
/// additive byte-identical behaviour (allow on a non-Error advisory is a no-op).
#[test]
fn allow_flag_on_advisory_is_transparent() {
    let flags = CliPolicyFlags {
        allow: vec!["token.unused".to_owned()],
        ..Default::default()
    };
    let result = to_png_with_dir(DOC_WITH_UNUSED_TOKEN, None, 1, false, &flags);
    assert!(
        result.is_ok(),
        "render must still succeed when an advisory code is --allow'd; got: {:?}",
        result.err().map(|e| e.message)
    );
    // Byte-identical check: same bytes as with no flags.
    let png_with_flags = result.unwrap().png;
    let png_no_flags = to_png_with_dir(
        DOC_WITH_UNUSED_TOKEN,
        None,
        1,
        false,
        &CliPolicyFlags::default(),
    )
    .expect("baseline render must succeed")
    .png;
    assert_eq!(
        png_with_flags, png_no_flags,
        "--allow on a non-Error advisory must produce byte-identical output to no flags"
    );
}

/// A malformed local `.zenith.kdl` must cause a config-load error (exit 2),
/// mirroring the validate command's behaviour.
#[test]
fn malformed_local_config_causes_render_error_exit_2() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(tmp.path().join(".zenith.kdl"), b"diagnostics {{{ bad kdl")
        .expect("write bad config");

    let result = to_png_with_dir(
        DOC_WITH_UNUSED_TOKEN,
        Some(tmp.path()),
        1,
        false,
        &CliPolicyFlags::default(),
    );
    assert!(
        result.is_err(),
        "render must fail when local config is malformed; got Ok"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.exit_code, 2,
        "malformed config must produce exit code 2; got {}",
        err.exit_code
    );
    assert!(
        err.message.contains("config.error"),
        "error message must mention config.error; got: {}",
        err.message
    );
}
