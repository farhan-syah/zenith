//! Writer for the `previews` block: preview artifact definitions and critique
//! records.
//!
//! Serializes [`PreviewArtifact`] records into the canonical `previews { … }`
//! KDL block, with each `preview` containing optional field props and zero or
//! more `critique` children.

use crate::ast::{PreviewArtifact, PreviewCritique};

use super::{
    escape_kdl_string, fmt_unknown_property, indent, write_opt_str, write_opt_str_escaped,
};

/// Emit the `previews { … }` block.
///
/// Stable position: after `recipes`, before `actions`. Emitted ONLY when at
/// least one preview artifact is declared, so documents without previews keep
/// their existing canonical form unchanged (byte-identity gate). Each preview
/// emits:
///
/// ```text
/// preview candidate="…" source-hash="…" output="…escaped…" output-hash="…" parent-revision="…" {
///   critique severity="…" code="…" message="…escaped…"
/// }
/// ```
///
/// Optional inline props are omitted when absent. The `output` path and
/// critique `message` pass through [`escape_kdl_string`] (may contain
/// backslashes and quotes); the other string fields emit unescaped.
pub(super) fn write_previews_block(out: &mut String, previews: &[PreviewArtifact], depth: usize) {
    if previews.is_empty() {
        return;
    }
    indent(out, depth);
    out.push_str("previews {\n");
    for preview in previews {
        indent(out, depth + 1);
        out.push_str("preview candidate=\"");
        out.push_str(&preview.candidate);
        out.push('"');
        write_opt_str(out, "source-hash", &preview.source_hash);
        write_opt_str_escaped(out, "output", &preview.output);
        write_opt_str(out, "output-hash", &preview.output_hash);
        write_opt_str(out, "parent-revision", &preview.parent_revision);
        // Unknown props on the preview node in sorted key order.
        for (key, prop) in &preview.unknown_props {
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push_str(&fmt_unknown_property(prop));
        }
        if preview.critiques.is_empty() {
            out.push('\n');
        } else {
            out.push_str(" {\n");
            for critique in &preview.critiques {
                write_preview_critique(out, critique, depth + 2);
            }
            indent(out, depth + 1);
            out.push_str("}\n");
        }
    }
    indent(out, depth);
    out.push_str("}\n");
}

fn write_preview_critique(out: &mut String, critique: &PreviewCritique, depth: usize) {
    indent(out, depth);
    out.push_str("critique severity=\"");
    out.push_str(&critique.severity);
    out.push_str("\" code=\"");
    out.push_str(&critique.code);
    out.push_str("\" message=\"");
    out.push_str(&escape_kdl_string(&critique.message));
    out.push_str("\"\n");
}
