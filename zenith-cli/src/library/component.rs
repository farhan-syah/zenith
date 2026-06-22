//! Component materialization: `library add` of a COMPONENT item.

use std::collections::BTreeMap;

use zenith_core::{ComponentDef, Document, InstanceNode, LibraryDef, Node, ProvenanceDef};

use super::add::{
    AddError, AddOutcome, collect_all_ids, copy_assets, copy_styles, copy_tokens,
    load_pack_document, px, target_component_id, unique_id, unknown_package_error,
};
use super::registry::LibraryPack;

/// Materialize the pack item `pkg_id#item` into `target` at `(at_x, at_y)` on the
/// page `page_id`, returning the [`AddOutcome`] describing what was added.
///
/// This is the PURE core of `library add`: it mutates the parsed `target`
/// [`Document`] in place and performs NO filesystem or process I/O (the caller
/// resolves the pack set, reads files, formats, and writes). Steps:
///
/// 1. Resolve the FIRST pack in `packs` whose id == `pkg_id` (project shadows
///    preset); load its full [`Document`] and find the `ComponentDef` == `item`.
/// 2. Copy that component into `target` under a namespaced id
///    (`lib.<sanitized-pkg>.<item>`), REUSING an existing copy if present (dedup).
///    Child ids are left untouched (instance expansion prefixes them at compile).
/// 3. Copy ALL of the pack's tokens/styles/assets into `target`, deduping by id;
///    a same-id-but-different-definition collision keeps the target's existing
///    one and records a `library.dependency_conflict` warning.
/// 4. Generate a unique instance id (base = `id_base`) against ALL target ids,
///    insert an [`InstanceNode`] referencing the copied component onto the page.
/// 5. Record a `libraries` entry for `pkg_id` (if absent) and a unique
///    `provenance` record linking the instance to the item.
///
/// # Errors
///
/// Returns [`AddError`] when the package or item is unknown (the message lists
/// the available options), or the page id is not found.
pub fn materialize(
    target: &mut Document,
    packs: &[LibraryPack],
    pkg_id: &str,
    item: &str,
    page_id: &str,
    id_base: &str,
    at: (f64, f64),
) -> Result<AddOutcome, AddError> {
    let (at_x, at_y) = at;
    // 1. Resolve the pack + load its document. ────────────────────────────────
    let pack = packs
        .iter()
        .find(|p| p.id == pkg_id)
        .ok_or_else(|| unknown_package_error(pkg_id, packs))?;

    let pack_doc = load_pack_document(pack)?;

    let comp = pack_doc
        .components
        .iter()
        .find(|c| c.id == item)
        .ok_or_else(|| {
            let available: Vec<&str> = pack_doc.components.iter().map(|c| c.id.as_str()).collect();
            AddError::new(format!(
                "unknown item '{}' in package '{}' (available: {})",
                item,
                pkg_id,
                if available.is_empty() {
                    "none".to_owned()
                } else {
                    available.join(", ")
                }
            ))
        })?;

    // Verify the target page exists BEFORE mutating anything, so an unknown page
    // leaves the target document untouched.
    if !target.body.pages.iter().any(|p| p.id == page_id) {
        let available: Vec<&str> = target.body.pages.iter().map(|p| p.id.as_str()).collect();
        return Err(AddError::new(format!(
            "page '{}' not found in target document (available: {})",
            page_id,
            if available.is_empty() {
                "none".to_owned()
            } else {
                available.join(", ")
            }
        )));
    }

    let mut warnings: Vec<String> = Vec::new();

    // 2. Copy the component (dedup by namespaced id). ──────────────────────────
    let comp_id = target_component_id(pkg_id, item);
    if !target.components.iter().any(|c| c.id == comp_id) {
        target.components.push(ComponentDef {
            id: comp_id.clone(),
            children: comp.children.clone(),
            source_span: None,
        });
    }

    // 3. Copy dependency tokens/styles/assets (dedup by id). ───────────────────
    // Ensure the target's tokens block has a format (adopt the pack's when empty).
    if target.tokens.format.is_empty() {
        target.tokens.format = pack_doc.tokens.format.clone();
    }
    copy_tokens(
        &pack_doc.tokens.tokens,
        &mut target.tokens.tokens,
        &mut warnings,
    );
    copy_styles(
        &pack_doc.styles.styles,
        &mut target.styles.styles,
        &mut warnings,
    );
    copy_assets(
        &pack_doc.assets.assets,
        &mut target.assets.assets,
        &mut warnings,
    );

    // 4. Generate a unique instance id + insert the instance on the page. ──────
    let mut all_ids = collect_all_ids(target);
    let instance_id = unique_id(id_base, &all_ids);
    all_ids.insert(instance_id.clone());

    let instance = InstanceNode {
        id: instance_id.clone(),
        name: None,
        role: None,
        component: comp_id.clone(),
        x: Some(px(at_x)),
        y: Some(px(at_y)),
        opacity: None,
        visible: None,
        locked: None,
        overrides: Vec::new(),
        source_span: None,
        unknown_props: BTreeMap::new(),
    };

    // The page is guaranteed to exist (checked above); push the instance at the
    // end of its children = top of z-order.
    if let Some(page) = target.body.pages.iter_mut().find(|p| p.id == page_id) {
        page.children.push(Node::Instance(instance));
    }

    // 5. Record libraries + provenance. ────────────────────────────────────────
    let provenance_id = unique_id(&format!("prov.{}", instance_id), &all_ids);

    if !target.libraries.iter().any(|l| l.id == pkg_id) {
        target.libraries.push(LibraryDef {
            id: pkg_id.to_owned(),
            version: pack.version.clone(),
            hash: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        });
    }

    target.provenance.push(ProvenanceDef {
        id: provenance_id.clone(),
        node: instance_id.clone(),
        library: pkg_id.to_owned(),
        item: Some(item.to_owned()),
        linked: Some(true),
        source_span: None,
        unknown_props: BTreeMap::new(),
    });

    Ok(AddOutcome {
        pkg_id: pkg_id.to_owned(),
        item: item.to_owned(),
        target_component_id: comp_id,
        instance_id,
        provenance_id,
        warnings,
    })
}
