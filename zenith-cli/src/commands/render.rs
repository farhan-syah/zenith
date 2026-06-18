//! Pure logic for `zenith render`.
//!
//! Two public entry points:
//! - [`to_scene_json`] — parse → validate → compile → scene JSON string.
//! - [`to_png`]        — parse → validate → compile → PNG bytes.
//!
//! Both operate entirely on in-memory source text; the caller is responsible
//! for all filesystem I/O.

use zenith_core::{KdlAdapter, KdlSource, default_provider, validate};
use zenith_render::render_png;
use zenith_scene::compile;

// ── Error type ────────────────────────────────────────────────────────────────

/// Error produced by the render command.
#[derive(Debug)]
pub struct RenderCmdErr {
    /// Human-readable message.
    pub message: String,
    /// Recommended exit code.
    pub exit_code: u8,
}

impl RenderCmdErr {
    fn new(msg: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: msg.into(),
            exit_code,
        }
    }
}

// ── Entry points ──────────────────────────────────────────────────────────────

/// Parse `src`, validate it, compile the scene, and return the scene JSON.
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - Scene JSON serialisation fails (exit code 2).
pub fn to_scene_json(src: &str) -> Result<String, RenderCmdErr> {
    let provider = default_provider();
    let compile_result = parse_validate_compile(src, &provider)?;
    compile_result
        .scene
        .to_json()
        .map_err(|e| RenderCmdErr::new(format!("scene serialisation error: {e}"), 2))
}

/// Parse `src`, validate it, compile the scene, and return PNG bytes.
///
/// Returns `Err` when:
/// - The source fails to parse (exit code 2).
/// - The document has validation errors (exit code 1).
/// - Rendering fails (exit code 2).
pub fn to_png(src: &str) -> Result<Vec<u8>, RenderCmdErr> {
    let provider = default_provider();
    let compile_result = parse_validate_compile(src, &provider)?;
    render_png(&compile_result.scene, &provider)
        .map_err(|e| RenderCmdErr::new(format!("render error: {e}"), 2))
}

// ── Shared pipeline helper ────────────────────────────────────────────────────

/// Parse → validate → compile, returning `CompileResult`.
///
/// Returns early with an error if parse fails or if validation has errors.
/// The `provider` is the same instance used for both compile and render so
/// the two steps see the same font registry.
fn parse_validate_compile(
    src: &str,
    provider: &dyn zenith_core::FontProvider,
) -> Result<zenith_scene::CompileResult, RenderCmdErr> {
    // Parse ─────────────────────────────────────────────────────────────────
    let doc = KdlAdapter
        .parse(src.as_bytes())
        .map_err(|e| RenderCmdErr::new(format!("error[parse.error]: {}", e.message), 2))?;

    // Validate ───────────────────────────────────────────────────────────────
    let report = validate(&doc);
    if report.has_errors() {
        let msgs: Vec<String> = report
            .diagnostics
            .iter()
            .filter(|d| d.severity == zenith_core::Severity::Error)
            .map(|d| format!("error[{}]: {}", d.code, d.message))
            .collect();
        return Err(RenderCmdErr::new(msgs.join("\n"), 1));
    }

    // Compile ────────────────────────────────────────────────────────────────
    Ok(compile(&doc, provider))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_DOC: &str = r##"zenith version=1 {
  project id="proj.r" name="Render Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.accent" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.r" title="Render Test" {
    page id="page.r" w=(px)320 h=(px)200 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)320 h=(px)200 fill=(token)"color.bg"
      rect id="rect.accent" x=(px)40 y=(px)40 w=(px)240 h=(px)120 fill=(token)"color.accent"
    }
  }
}
"##;

    const INVALID_DOC: &str = r##"zenith version=1 {
  project id="proj.inv" name="Invalid"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.bg" type="color" value="#000000"
  }
  styles {}
  document id="doc.inv" title="Invalid" {
    page id="page.inv" w=(px)100 h=(px)100 {
      rect id="rect.inv" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.bg"
    }
  }
}
"##;

    #[test]
    fn to_png_returns_png_magic_bytes() {
        let png = to_png(VALID_DOC).expect("render must succeed");
        assert!(
            png.len() >= 4,
            "PNG must have at least 4 bytes; got {}",
            png.len()
        );
        assert_eq!(
            &png[0..4],
            &[0x89, 0x50, 0x4E, 0x47],
            "PNG must start with magic bytes 89 50 4E 47"
        );
    }

    #[test]
    fn to_png_is_non_empty() {
        let png = to_png(VALID_DOC).expect("render must succeed");
        assert!(!png.is_empty(), "PNG output must not be empty");
    }

    #[test]
    fn to_png_with_validation_error_returns_err() {
        let result = to_png(INVALID_DOC);
        assert!(
            result.is_err(),
            "document with validation errors must not render"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.exit_code, 1,
            "validation errors must produce exit code 1"
        );
    }

    #[test]
    fn to_scene_json_contains_schema_field() {
        let json = to_scene_json(VALID_DOC).expect("scene JSON must succeed");
        assert!(
            json.contains("zenith-scene-v1"),
            "scene JSON must contain schema field; got snippet: {}",
            &json[..json.len().min(200)]
        );
    }

    #[test]
    fn to_scene_json_with_validation_error_returns_err() {
        let result = to_scene_json(INVALID_DOC);
        assert!(result.is_err(), "invalid doc must not produce scene JSON");
    }

    #[test]
    fn to_png_deterministic_two_runs_equal() {
        let png1 = to_png(VALID_DOC).expect("run 1");
        let png2 = to_png(VALID_DOC).expect("run 2");
        assert_eq!(png1, png2, "two renders of the same doc must be identical");
    }
}
