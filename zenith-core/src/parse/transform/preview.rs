//! Transforms for the `previews` block: preview artifact definitions and
//! critique records.
//!
//! Each `preview candidate="…" …` inside `previews { … }` is parsed into a
//! [`PreviewArtifact`] with zero or more [`PreviewCritique`] children.

use kdl::KdlNode;

use crate::ast::preview::{PreviewArtifact, PreviewCritique};
use crate::error::{ParseError, ParseErrorCode};

use super::helpers::{
    collect_unknown_props, node_span, optional_string_prop, optional_string_prop_aliased,
    required_string_prop,
};

const PREVIEW_KNOWN_PROPS: &[&str] = &[
    "candidate",
    "source-hash",
    "source_hash",
    "output",
    "output-hash",
    "output_hash",
    "parent-revision",
    "parent_revision",
];

/// Transform the document-level `previews { … }` block into a list of
/// [`PreviewArtifact`]. Each `preview candidate="…" …` is a block node;
/// non-`preview` children inside the block are silently ignored
/// (forward-compat).
pub(super) fn transform_previews(node: &KdlNode) -> Result<Vec<PreviewArtifact>, ParseError> {
    let mut defs: Vec<PreviewArtifact> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "preview" {
                defs.push(transform_preview_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_preview_def(node: &KdlNode) -> Result<PreviewArtifact, ParseError> {
    let candidate = required_string_prop(node, "candidate")?.to_owned();
    let source_hash =
        optional_string_prop_aliased(node, "source-hash", "source_hash").map(str::to_owned);
    let output = optional_string_prop(node, "output").map(str::to_owned);
    let output_hash =
        optional_string_prop_aliased(node, "output-hash", "output_hash").map(str::to_owned);
    let parent_revision =
        optional_string_prop_aliased(node, "parent-revision", "parent_revision").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, PREVIEW_KNOWN_PROPS);
    let source_span = node_span(node);

    let mut critiques: Vec<PreviewCritique> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "critique" {
                critiques.push(transform_preview_critique(child)?);
            }
        }
    }

    Ok(PreviewArtifact {
        candidate,
        source_hash,
        output,
        output_hash,
        parent_revision,
        critiques,
        source_span,
        unknown_props,
    })
}

fn transform_preview_critique(node: &KdlNode) -> Result<PreviewCritique, ParseError> {
    let severity = required_string_prop(node, "severity")
        .map_err(|_| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                "preview `critique` is missing required property `severity`",
            )
        })?
        .to_owned();
    let code = required_string_prop(node, "code")
        .map_err(|_| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                "preview `critique` is missing required property `code`",
            )
        })?
        .to_owned();
    let message = required_string_prop(node, "message")
        .map_err(|_| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                "preview `critique` is missing required property `message`",
            )
        })?
        .to_owned();
    let source_span = node_span(node);

    Ok(PreviewCritique {
        severity,
        code,
        message,
        source_span,
    })
}
