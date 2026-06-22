//! Token materialization: `library add` of a filter/mask TOKEN item.

use std::collections::{BTreeMap, BTreeSet};

use zenith_core::{LibraryDef, ProvenanceDef, Token, TokenLiteral, TokenType, TokenValue};

use super::add::{
    AddError, collect_all_ids, copy_tokens, load_pack_document, unique_id, unknown_package_error,
};
use super::registry::{LibraryPack, is_exportable_token};

/// The outcome of a successful [`materialize_token`] call.
///
/// All ids are the FINAL ids written into the target document. The filter-token
/// id is kept VERBATIM (e.g. `noir`), so the user can apply it via
/// `filter=(token)"noir"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAddOutcome {
    /// The package id the item came from (e.g. `@zenith/filters`).
    pub pkg_id: String,
    /// The item name within the pack (e.g. `noir`).
    pub item: String,
    /// The copied token id (kept as-is, e.g. `noir` or `vignette`).
    pub token_id: String,
    /// The property the copied token is applied through: `"filter"` or `"mask"`.
    pub apply_property: &'static str,
    /// Dependency tokens copied alongside the item token (sorted, deduped).
    pub dep_token_ids: Vec<String>,
    /// The unique id of the recorded provenance entry.
    pub provenance_id: String,
    /// Non-fatal dependency-conflict warnings (see [`super::AddOutcome::warnings`]).
    pub warnings: Vec<String>,
}

/// Collect the transitive token DEPS of `filter_token` among `pack_tokens`.
///
/// A filter token references color tokens only through its `Duotone` ops'
/// `shadow`/`highlight` ids. Each referenced id is followed through any
/// `TokenValue::Reference` alias chain to a fixpoint (cycle-guarded by a visited
/// set), so the full closure of dependency token ids is returned. The filter
/// token itself is NOT included. For non-duotone filters the result is empty.
///
/// Deterministic: returns a [`BTreeSet`] (sorted, deduped).
pub(crate) fn collect_filter_dep_ids(
    filter_token: &Token,
    pack_tokens: &[Token],
) -> BTreeSet<String> {
    // Seed: the direct color-token ids referenced by duotone ops.
    let mut seeds: Vec<String> = Vec::new();
    if let TokenValue::Literal(TokenLiteral::Filter(lit)) = &filter_token.value {
        for op in &lit.ops {
            if op.kind == zenith_core::FilterKind::Duotone {
                if let Some(s) = &op.shadow {
                    seeds.push(s.clone());
                }
                if let Some(h) = &op.highlight {
                    seeds.push(h.clone());
                }
            }
        }
    }

    let mut deps: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = seeds;
    while let Some(id) = stack.pop() {
        // `insert` returns false if already present → fixpoint / cycle guard.
        if !deps.insert(id.clone()) {
            continue;
        }
        // Follow an alias chain: if the dep token is itself a reference, the
        // referenced target is also a dependency.
        if let Some(tok) = pack_tokens.iter().find(|t| t.id == id)
            && let TokenValue::Reference { token_id } = &tok.value
        {
            stack.push(token_id.clone());
        }
    }
    deps
}

/// Materialize the filter-token item `pkg_id#item` into `target`, returning the
/// [`TokenAddOutcome`] describing what was added.
///
/// This is the PURE core of a `library add` for a TOKEN item: it mutates the
/// parsed `target` [`Document`] in place and performs NO filesystem or process
/// I/O. Unlike [`super::materialize`], it inserts NO instance and requires NO
/// page. Steps:
///
/// 1. Resolve the FIRST pack in `packs` whose id == `pkg_id` (project shadows
///    preset); load its full [`zenith_core::Document`].
/// 2. Find the FILTER token whose id == `item`.
/// 3. Collect the filter token's transitive color-token deps
///    ([`collect_filter_dep_ids`]).
/// 4. Ensure the target's tokens block has a format (adopt the pack's when empty).
/// 5. Copy the dep tokens THEN the filter token into the target (dedup by id +
///    conflict warnings, via the shared [`copy_tokens`]).
/// 6. Record a `libraries` entry for `pkg_id` (if absent).
/// 7. Record a unique `provenance` record whose `node` is the filter-token id —
///    skipped if an identical `(node, library, item)` provenance already exists.
///
/// # Errors
///
/// Returns [`AddError`] when the package or item is unknown (the message lists
/// the available options).
pub fn materialize_token(
    target: &mut zenith_core::Document,
    packs: &[LibraryPack],
    pkg_id: &str,
    item: &str,
    id_base: &str,
) -> Result<TokenAddOutcome, AddError> {
    // 1. Resolve the pack + load its document. ────────────────────────────────
    let pack = packs
        .iter()
        .find(|p| p.id == pkg_id)
        .ok_or_else(|| unknown_package_error(pkg_id, packs))?;

    let pack_doc = load_pack_document(pack)?;

    // 2. Find the FILTER token named `item`. ───────────────────────────────────
    let item_token = pack_doc
        .tokens
        .tokens
        .iter()
        .find(|t| t.id == item && is_exportable_token(&t.token_type))
        .ok_or_else(|| {
            let available: Vec<&str> = pack_doc
                .tokens
                .tokens
                .iter()
                .filter(|t| is_exportable_token(&t.token_type))
                .map(|t| t.id.as_str())
                .collect();
            AddError::new(format!(
                "unknown token item '{}' in package '{}' (available: {})",
                item,
                pkg_id,
                if available.is_empty() {
                    "none".to_owned()
                } else {
                    available.join(", ")
                }
            ))
        })?;

    let mut warnings: Vec<String> = Vec::new();

    // The property the token is applied through, and (for filters) its deps.
    let apply_property = match item_token.token_type {
        TokenType::Mask => "mask",
        _ => "filter",
    };

    // 3. Collect transitive color-token deps (filter duotone colors; none for
    //    masks, which are self-contained). ─────────────────────────────────────
    let dep_ids = collect_filter_dep_ids(item_token, &pack_doc.tokens.tokens);

    // 4. Ensure the target's tokens block has a format. ────────────────────────
    if target.tokens.format.is_empty() {
        target.tokens.format = pack_doc.tokens.format.clone();
    }

    // 5. Copy deps THEN the filter token (shared dedup + conflict logic). ───────
    let mut to_copy: Vec<Token> = Vec::with_capacity(dep_ids.len() + 1);
    for dep_id in &dep_ids {
        if let Some(tok) = pack_doc.tokens.tokens.iter().find(|t| &t.id == dep_id) {
            to_copy.push(tok.clone());
        }
    }
    to_copy.push(item_token.clone());
    copy_tokens(&to_copy, &mut target.tokens.tokens, &mut warnings);

    // 6. Record the libraries import entry. ────────────────────────────────────
    if !target.libraries.iter().any(|l| l.id == pkg_id) {
        target.libraries.push(LibraryDef {
            id: pkg_id.to_owned(),
            version: pack.version.clone(),
            hash: None,
            source_span: None,
            unknown_props: BTreeMap::new(),
        });
    }

    // 7. Record provenance (dedup identical node+library+item). ────────────────
    let token_id = item.to_owned();
    let provenance_id = if let Some(existing) = target
        .provenance
        .iter()
        .find(|p| p.node == token_id && p.library == pkg_id && p.item.as_deref() == Some(item))
    {
        // An identical provenance already links this token to its origin; reuse
        // its id rather than appending a redundant duplicate record.
        existing.id.clone()
    } else {
        let all_ids = collect_all_ids(target);
        let provenance_id = unique_id(&format!("prov.{}", id_base), &all_ids);
        target.provenance.push(ProvenanceDef {
            id: provenance_id.clone(),
            node: token_id.clone(),
            library: pkg_id.to_owned(),
            item: Some(item.to_owned()),
            linked: Some(true),
            source_span: None,
            unknown_props: BTreeMap::new(),
        });
        provenance_id
    };

    Ok(TokenAddOutcome {
        pkg_id: pkg_id.to_owned(),
        item: item.to_owned(),
        token_id,
        apply_property,
        dep_token_ids: dep_ids.into_iter().collect(),
        provenance_id,
        warnings,
    })
}
