//! Action materialization: `library add` of an ACTION item.

use std::collections::BTreeMap;

use zenith_core::{ActionDef, KdlAdapter, KdlSource, LibraryDef, ProvenanceDef};
use zenith_tx::{Transaction, TxResult, TxStatus, run_transaction};

use super::add::{
    AddError, collect_all_ids, dependency_conflict, load_pack_document, unique_id,
    unknown_package_error,
};
use super::registry::LibraryPack;

/// The outcome of a successful [`materialize_action`] call.
///
/// All ids are the FINAL ids written into the target document. When
/// `tx_result.status == Rejected` the function still returns `Ok` but
/// `final_source` and `provenance_id` are `None` and no document mutation was
/// recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionAddOutcome {
    /// The package id the item came from (e.g. `@test/actions`).
    pub pkg_id: String,
    /// The item name within the pack (e.g. `apply-brand-kit`).
    pub item: String,
    /// The transaction result (status, diagnostics, source_before/after,
    /// affected_node_ids).
    pub tx_result: TxResult,
    /// The fully-formatted new document source: tx applied, action copied in,
    /// library import added, and provenance recorded. `None` when the tx was
    /// Rejected.
    pub final_source: Option<String>,
    /// The recorded provenance entry id. `None` when the tx was Rejected.
    pub provenance_id: Option<String>,
    /// Non-fatal warnings (e.g. action id already present with different
    /// metadata).
    pub warnings: Vec<String>,
}

/// Materialize the action item `pkg_id#action_id` against `target_src`,
/// returning the [`ActionAddOutcome`] describing what happened.
///
/// This is the PURE core of a `library add` for an ACTION item: it produces a
/// new document source string and performs NO filesystem or process I/O.
/// Steps:
///
/// 1. Resolve the FIRST pack in `packs` whose id == `pkg_id`; load its full
///    [`zenith_core::Document`] and find the [`ActionDef`] whose id ==
///    `action_id`.
/// 2. Parse `target_src` into a [`zenith_core::Document`].
/// 3. Parse the action's `tx_json` into a [`Transaction`].
/// 4. Run the transaction against the target.  If the result is Rejected,
///    return immediately with `final_source: None` and `provenance_id: None`.
/// 5. Re-parse `tx_result.source_after` and:
///    - Copy the [`ActionDef`] into `result_doc.actions` (dedup by id;
///      same-id-different-content keeps the existing one and records a
///      warning).
///    - Add a `libraries` import for `pkg_id` (dedup by id; conflict warning
///      on mismatch, using the pack's version — mirror `materialize_token`).
///    - Generate a unique provenance id (via [`unique_id`]/[`collect_all_ids`])
///      and push a [`ProvenanceDef`] whose `node` is the action id.
/// 6. Format the result document and return the full outcome.
///
/// # Errors
///
/// Returns [`AddError`] when the package or item is unknown (the message lists
/// the available options), the target or the tx envelope cannot be parsed, or
/// the formatted output cannot be produced.
pub fn materialize_action(
    target_src: &str,
    packs: &[LibraryPack],
    pkg_id: &str,
    action_id: &str,
) -> Result<ActionAddOutcome, AddError> {
    // 1. Resolve the pack + load its document. ────────────────────────────────
    let pack = packs
        .iter()
        .find(|p| p.id == pkg_id)
        .ok_or_else(|| unknown_package_error(pkg_id, packs))?;

    let pack_doc = load_pack_document(pack)?;

    let pack_action = pack_doc
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| {
            let available: Vec<&str> = pack_doc.actions.iter().map(|a| a.id.as_str()).collect();
            AddError::new(format!(
                "unknown action item '{}' in package '{}' (available: {})",
                action_id,
                pkg_id,
                if available.is_empty() {
                    "none".to_owned()
                } else {
                    available.join(", ")
                }
            ))
        })?;

    // 2. Parse the target document. ────────────────────────────────────────────
    let target_doc = KdlAdapter
        .parse(target_src.as_bytes())
        .map_err(|e| AddError::new(format!("error parsing target document: {}", e)))?;

    // 3. Parse the action's tx envelope. ──────────────────────────────────────
    let tx = Transaction::from_json(&pack_action.tx_json).map_err(|e| {
        AddError::new(format!(
            "malformed tx-script in action '{}': {}",
            action_id, e
        ))
    })?;

    // 4. Run the transaction. ──────────────────────────────────────────────────
    let tx_result = run_transaction(&target_doc, &tx)
        .map_err(|e| AddError::new(format!("transaction error: {}", e)))?;

    // Rejected → return immediately; the CLI layer decides what to show the user.
    match &tx_result.status {
        TxStatus::Rejected => {
            return Ok(ActionAddOutcome {
                pkg_id: pkg_id.to_owned(),
                item: action_id.to_owned(),
                tx_result,
                final_source: None,
                provenance_id: None,
                warnings: vec![],
            });
        }
        TxStatus::Accepted | TxStatus::AcceptedWithWarnings => {}
    }

    // 5. Re-parse the post-tx source and record the action + library + provenance.
    let mut result_doc = KdlAdapter
        .parse(tx_result.source_after.as_bytes())
        .map_err(|e| {
            AddError::new(format!(
                "internal error: could not re-parse transaction output: {}",
                e
            ))
        })?;

    let mut warnings: Vec<String> = Vec::new();

    // Copy the ActionDef (dedup by id; conflict on same-id different content).
    match result_doc.actions.iter().find(|a| a.id == action_id) {
        Some(existing) if existing.tx_json != pack_action.tx_json => {
            warnings.push(dependency_conflict("action", action_id));
        }
        Some(_) => {}
        None => result_doc.actions.push(ActionDef {
            id: pack_action.id.clone(),
            label: pack_action.label.clone(),
            version: pack_action.version.clone(),
            tx_json: pack_action.tx_json.clone(),
            source_span: None,
            unknown_props: BTreeMap::new(),
        }),
    }

    // Add the libraries import (mirror materialize_token exactly). ─────────────
    if !result_doc.libraries.iter().any(|l| l.id == pkg_id) {
        result_doc.libraries.push(LibraryDef {
            id: pkg_id.to_owned(),
            version: pack.version.clone(),
            hash: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        });
    }

    // Generate a unique provenance id and push the record. ────────────────────
    let all_ids = collect_all_ids(&result_doc);
    let provenance_id = unique_id(&format!("prov.{}", action_id), &all_ids);
    result_doc.provenance.push(ProvenanceDef {
        id: provenance_id.clone(),
        node: action_id.to_owned(),
        library: pkg_id.to_owned(),
        item: Some(action_id.to_owned()),
        linked: Some(true),
        source_span: None,
        unknown_props: BTreeMap::new(),
    });

    // 6. Format the final document. ────────────────────────────────────────────
    let final_bytes = KdlAdapter
        .format(&result_doc)
        .map_err(|e| AddError::new(format!("error formatting result document: {}", e)))?;
    let final_source = String::from_utf8(final_bytes)
        .map_err(|e| AddError::new(format!("error encoding result document: {}", e)))?;

    Ok(ActionAddOutcome {
        pkg_id: pkg_id.to_owned(),
        item: action_id.to_owned(),
        tx_result,
        final_source: Some(final_source),
        provenance_id: Some(provenance_id),
        warnings,
    })
}
