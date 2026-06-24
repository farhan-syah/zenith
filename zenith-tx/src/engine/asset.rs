//! Asset op application: [`apply_add_asset`] and [`apply_set_asset`].

use zenith_core::{AssetDecl, AssetKind, Diagnostic, Document, Node};

use super::{find_node_any_mut, node_kind_str, record_affected};

// ── AddAsset ──────────────────────────────────────────────────────────────────

/// Add a new [`AssetDecl`] to `doc.assets.assets`.
///
/// Eagerly rejects with `tx.duplicate_id` if an asset with `id` already
/// exists. Post-validation handles `asset.invalid_src` (unsafe paths / URLs)
/// and `asset.invalid_kind` (unknown kind strings) — those are not re-checked
/// here.
pub(super) fn apply_add_asset(
    id: &str,
    kind: &str,
    src: &str,
    sha256: Option<&str>,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Eager duplicate-id check: the validator would also catch this via
    // `id.duplicate`, but we surface it as `tx.duplicate_id` immediately so
    // the caller sees an actionable engine-level error (matching add_page's
    // pattern).
    if doc.assets.assets.iter().any(|a| a.id == id) {
        diagnostics.push(Diagnostic::error(
            "tx.duplicate_id",
            format!("add_asset: an asset with id {:?} already exists", id),
            None,
            Some(id.to_owned()),
        ));
        return;
    }

    doc.assets.assets.push(AssetDecl {
        id: id.to_owned(),
        kind: AssetKind::from_kind_str(kind),
        src: src.to_owned(),
        sha256: sha256.map(str::to_owned),
        ai_prompt: None,
        ai_model: None,
        ai_provider: None,
        ai_seed: None,
        ai_generation_date: None,
        ai_license: None,
        ai_source_rights: None,
        ai_safety_status: None,
        ai_reuse_policy: None,
        source_span: None,
        unknown_props: Default::default(),
    });

    record_affected(id, affected);
}

// ── SetAsset ──────────────────────────────────────────────────────────────────

/// Set the `asset` field on an `image` node to `asset_id`.
///
/// Eagerly rejects with `tx.invalid_value` if the referenced asset exists and
/// its kind is `Font` — image nodes require an `image` or `svg` asset. An
/// unknown `asset_id` is allowed through; post-validation catches it via
/// `asset.unknown_reference`.
///
/// Rejects with `tx.unknown_node` if `node_id` is not found in the document.
/// Rejects with `tx.wrong_node_type` if `node_id` does not name an image node.
pub(super) fn apply_set_asset(
    node_id: &str,
    asset_id: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Eager kind guard: if the asset exists AND is a Font, reject immediately.
    // The validator only checks existence (asset.unknown_reference), not kind
    // fitness, so this guard is the sole enforcement point.
    let is_font = doc
        .assets
        .assets
        .iter()
        .find(|a| a.id == asset_id)
        .is_some_and(|a| matches!(a.kind, AssetKind::Font));

    if is_font {
        diagnostics.push(Diagnostic::error(
            "tx.invalid_value",
            format!(
                "set_asset: asset {:?} has kind font; image nodes require kind image or svg",
                asset_id
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(Node::Image(img)) => {
            img.asset = asset_id.to_owned();
            record_affected(node_id, affected);
        }
        Some(other) => {
            let kind = node_kind_str(other);
            diagnostics.push(Diagnostic::error(
                "tx.wrong_node_type",
                format!(
                    "set_asset requires an image node but {:?} is a {}",
                    node_id, kind
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
    }
}
