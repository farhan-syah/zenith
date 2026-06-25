//! Determinism guard: a document's `doc-id` root attribute is RENDER-IGNORED.
//!
//! The history subsystem stamps an identity (`doc-id`) into the document's
//! root node. These tests prove that stamping never perturbs the deterministic
//! render pipeline: rendering a `.zen` *with* a `doc-id` produces byte-identical
//! PNG output to rendering the same `.zen` *without* one.

use zenith_cli::commands::render::to_png_with_dir;
use zenith_cli::config::CliPolicyFlags;

/// A minimal valid `.zen` document with NO `doc-id` attribute.
///
/// Mirrors the `MINIMAL_NO_ID` fixture in `history_pipeline.rs`. The `"#hex"`
/// color literal forces the `r##` delimiter.
const WITHOUT_ID: &str = r##"zenith version=1 {
  project id="proj.hist" name="History Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
  }
  styles {
  }
  document id="doc.hist" title="History Test" {
    page id="page.one" w=(px)480 h=(px)160 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)480 h=(px)160 fill=(token)"color.bg"
    }
  }
}
"##;

/// The same document, byte-for-byte, EXCEPT the root node carries a `doc-id`.
fn with_id() -> String {
    let out = WITHOUT_ID.replace(
        "zenith version=1 {",
        r#"zenith version=1 doc-id="01ARZ3NDEKTSV4RRFFQ69G5FAV" {"#,
    );
    assert_ne!(
        out, WITHOUT_ID,
        "the doc-id substitution must actually change the source"
    );
    out
}

/// Core proof: a `doc-id` on the root node does not affect rendered pixels.
/// Rendering WITH and WITHOUT a `doc-id` under identical settings must produce
/// byte-identical PNG output.
#[test]
fn doc_id_is_render_ignored() {
    let with = with_id();

    let png_without = to_png_with_dir(WITHOUT_ID, None, 1, false, &CliPolicyFlags::default(), None)
        .unwrap_or_else(|e| {
            panic!(
                "render WITHOUT doc-id failed (exit {}): {}",
                e.exit_code, e.message
            )
        })
        .png;

    let png_with = to_png_with_dir(&with, None, 1, false, &CliPolicyFlags::default(), None)
        .unwrap_or_else(|e| {
            panic!(
                "render WITH doc-id failed (exit {}): {}",
                e.exit_code, e.message
            )
        })
        .png;

    assert!(!png_without.is_empty(), "render output must not be empty");
    assert_eq!(
        png_with, png_without,
        "doc-id must be render-ignored: WITH and WITHOUT doc-id must render byte-identical PNGs"
    );
}

/// The general determinism gate still holds with a `doc-id` present: rendering
/// the WITH_ID fixture twice must produce byte-identical output.
#[test]
fn render_with_doc_id_is_deterministic() {
    let with = with_id();

    let first = to_png_with_dir(&with, None, 1, false, &CliPolicyFlags::default(), None)
        .unwrap_or_else(|e| panic!("first render failed (exit {}): {}", e.exit_code, e.message))
        .png;

    let second = to_png_with_dir(&with, None, 1, false, &CliPolicyFlags::default(), None)
        .unwrap_or_else(|e| panic!("second render failed (exit {}): {}", e.exit_code, e.message))
        .png;

    assert_eq!(
        first, second,
        "two renders of the WITH-doc-id fixture must produce identical bytes"
    );
}
