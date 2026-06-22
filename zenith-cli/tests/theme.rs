//! Integration tests for `zenith theme new` — the synthesized theme packs must
//! parse, validate without hard diagnostics, and be canonical, in both schemes.

use zenith_cli::commands::theme::{Shape, ThemeInput, new};
use zenith_core::theme::Scheme;
use zenith_core::{KdlAdapter, KdlSource, Severity, validate};

fn input(name: &'static str, scheme: Scheme, primary: &'static str) -> ThemeInput<'static> {
    ThemeInput {
        name,
        scheme,
        primary,
        secondary: None,
        accent: None,
        neutral: None,
        info: None,
        success: None,
        warning: None,
        error: None,
        shape: Shape::default(),
    }
}

fn hard_errors(src: &str) -> usize {
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .expect("synthesized theme must parse");
    validate(&doc)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

#[test]
fn light_theme_validates_clean() {
    let src = new(&input("acme", Scheme::Light, "#7c3aed")).expect("synth light");
    assert_eq!(
        hard_errors(&src),
        0,
        "light theme had hard diagnostics:\n{src}"
    );
}

#[test]
fn dark_theme_validates_clean() {
    let src = new(&input("acme", Scheme::Dark, "#22c55e")).expect("synth dark");
    assert_eq!(
        hard_errors(&src),
        0,
        "dark theme had hard diagnostics:\n{src}"
    );
}

#[test]
fn depth_theme_validates_clean() {
    // A raised theme adds the shadow.depth token + node shadow; must still pass.
    let mut i = input("raised", Scheme::Light, "#605dff");
    i.shape.depth = true;
    let src = new(&i).expect("synth depth");
    assert_eq!(
        hard_errors(&src),
        0,
        "depth theme had hard diagnostics:\n{src}"
    );
}

#[test]
fn invalid_primary_is_error() {
    assert!(new(&input("x", Scheme::Light, "not-hex")).is_err());
}

#[test]
fn output_is_canonical() {
    let src = new(&input("acme", Scheme::Dark, "#22c55e")).expect("synth");
    let doc = KdlAdapter.parse(src.as_bytes()).expect("parse");
    let formatted = KdlAdapter.format(&doc).expect("format");
    assert_eq!(
        formatted,
        src.as_bytes(),
        "theme output must already be canonical"
    );
}
