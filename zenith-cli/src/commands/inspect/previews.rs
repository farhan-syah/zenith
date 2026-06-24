//! Previews block rendering for `zenith inspect`.
//!
//! The public surface is two pure functions:
//! - [`build_preview_entries`] — converts `&[PreviewArtifact]` to
//!   `Vec<PreviewInspectJson>` for the `--json` path.
//! - [`render_previews_human`] — formats the same data as a human-readable
//!   section string.

use zenith_core::PreviewArtifact;

use crate::json_types::{PreviewCritiqueInspectJson, PreviewInspectJson};

// ── JSON builder ──────────────────────────────────────────────────────────────

/// Convert a slice of [`PreviewArtifact`] to [`PreviewInspectJson`] entries
/// (source order is preserved).
pub fn build_preview_entries(previews: &[PreviewArtifact]) -> Vec<PreviewInspectJson> {
    previews.iter().map(preview_to_json).collect()
}

fn preview_to_json(p: &PreviewArtifact) -> PreviewInspectJson {
    PreviewInspectJson {
        candidate: p.candidate.clone(),
        source_hash: p.source_hash.clone(),
        output: p.output.clone(),
        output_hash: p.output_hash.clone(),
        parent_revision: p.parent_revision.clone(),
        critique_count: p.critiques.len(),
        critiques: p.critiques.iter().map(critique_to_json).collect(),
    }
}

fn critique_to_json(c: &zenith_core::PreviewCritique) -> PreviewCritiqueInspectJson {
    PreviewCritiqueInspectJson {
        severity: c.severity.clone(),
        code: c.code.clone(),
    }
}

// ── Human renderer ────────────────────────────────────────────────────────────

/// Render the `previews` section for human output.
///
/// Returns an empty string when `previews` is empty.
pub fn render_previews_human(previews: &[PreviewArtifact]) -> String {
    if previews.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for p in previews {
        // Header line: `preview <candidate>  critiques=<N>`
        out.push_str(&format!(
            "preview {}  critiques={}\n",
            p.candidate,
            p.critiques.len()
        ));

        // Optional metadata fields
        if let Some(ref sh) = p.source_hash {
            out.push_str(&format!("  source-hash={}\n", sh));
        }
        if let Some(ref o) = p.output {
            out.push_str(&format!("  output={}\n", o));
        }
        if let Some(ref oh) = p.output_hash {
            out.push_str(&format!("  output-hash={}\n", oh));
        }
        if let Some(ref pr) = p.parent_revision {
            out.push_str(&format!("  parent-revision={}\n", pr));
        }

        // Per-critique summary lines
        for critique in &p.critiques {
            out.push_str(&format!(
                "  critique {}  code={}\n",
                critique.severity, critique.code
            ));
        }
    }
    out.trim_end().to_owned()
}
