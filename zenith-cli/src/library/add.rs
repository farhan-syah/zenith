//! Shared materialization machinery for `library add`.
//!
//! Holds the pieces every `materialize*` flavor needs: the [`AddError`] /
//! [`AddOutcome`] contract, spec parsing ([`parse_spec`]), loading a resolved
//! pack's full [`Document`] ([`load_pack_document`]), id collection / uniquing
//! ([`collect_all_ids`], [`unique_id`]), and the dedup-with-conflict-warning
//! copiers for tokens / styles / assets.

use std::collections::BTreeSet;

use zenith_core::{
    AssetDecl, Dimension, Document, KdlAdapter, KdlSource, Node, Style, Token, Unit,
};

use super::registry::{EMBEDDED_PACKS, LibraryPack, PackSource};

/// An error produced while materializing a library item into a target document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddError {
    /// Human-readable message describing the failure.
    pub message: String,
}

impl AddError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AddError {}

/// The outcome of a successful [`super::materialize`] call.
///
/// All ids are the FINAL ids written into the target document, so the caller can
/// build a deterministic human/JSON summary without re-deriving them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOutcome {
    /// The package id the item came from (e.g. `@zenith/flowchart`).
    pub pkg_id: String,
    /// The item name within the pack (e.g. `decision`).
    pub item: String,
    /// The namespaced target component id the item was copied to.
    pub target_component_id: String,
    /// The unique id of the inserted instance node.
    pub instance_id: String,
    /// The unique id of the recorded provenance entry.
    pub provenance_id: String,
    /// Non-fatal dependency-conflict warnings (a pack token/style/asset id that
    /// already existed in the target with a DIFFERENT definition; the target's
    /// existing definition was kept). Each entry is a `library.dependency_conflict`
    /// human-readable line.
    pub warnings: Vec<String>,
}

/// Parse a `pkg#item` spec into `(pkg_id, item)`.
///
/// # Errors
///
/// Returns [`AddError`] when the spec has no `#`, or either side is empty.
pub fn parse_spec(spec: &str) -> Result<(String, String), AddError> {
    let (pkg, item) = spec.split_once('#').ok_or_else(|| {
        AddError::new(format!(
            "malformed item spec {:?} (expected `<package>#<item>`, e.g. \
             `@zenith/flowchart#decision`)",
            spec
        ))
    })?;
    if pkg.is_empty() || item.is_empty() {
        return Err(AddError::new(format!(
            "malformed item spec {:?} (both package and item must be non-empty, \
             e.g. `@zenith/flowchart#decision`)",
            spec
        )));
    }
    Ok((pkg.to_owned(), item.to_owned()))
}

/// Load the FULL [`Document`] of a resolved pack.
///
/// [`super::resolve_packs`] only yields pack METADATA; materialization needs the
/// pack's component/token/style/asset subtrees, so this re-reads and parses the
/// pack's source: embedded presets from [`EMBEDDED_PACKS`], project packs from
/// disk.
///
/// # Errors
///
/// Returns [`AddError`] when the embedded source for `pack.id` cannot be located,
/// or when a project pack file cannot be read or parsed.
pub fn load_pack_document(pack: &LibraryPack) -> Result<Document, AddError> {
    let source = match &pack.source {
        PackSource::Preset => EMBEDDED_PACKS
            .iter()
            .find(|(id, _)| *id == pack.id)
            .map(|(_, src)| (*src).to_owned())
            .ok_or_else(|| {
                AddError::new(format!("embedded pack '{}' source not found", pack.id))
            })?,
        PackSource::Project(path) => std::fs::read_to_string(path).map_err(|e| {
            AddError::new(format!("error reading pack '{}': {}", path.display(), e))
        })?,
    };
    KdlAdapter
        .parse(source.as_bytes())
        .map_err(|e| AddError::new(format!("error parsing pack '{}': {}", pack.id, e)))
}

/// Build the "unknown library package" [`AddError`], listing the available pack
/// ids (sorted, de-duplicated) so the caller sees what they could have meant.
pub(super) fn unknown_package_error(pkg_id: &str, packs: &[LibraryPack]) -> AddError {
    let mut available: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
    available.sort_unstable();
    available.dedup();
    AddError::new(format!(
        "unknown library package '{}' (available: {})",
        pkg_id,
        if available.is_empty() {
            "none".to_owned()
        } else {
            available.join(", ")
        }
    ))
}

/// Sanitize a package id into a safe component-id fragment.
///
/// Replaces `@` and `/` (and any other non-`[A-Za-z0-9._-]` byte) with `.`,
/// collapsing the result so `@zenith/flowchart` → `zenith.flowchart`.
pub(crate) fn sanitize_pkg(pkg_id: &str) -> String {
    let mut out = String::with_capacity(pkg_id.len());
    let mut prev_dot = false;
    for ch in pkg_id.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
            prev_dot = ch == '.';
        } else {
            // Collapse runs of separators (e.g. a leading '@') into a single '.'.
            if !prev_dot && !out.is_empty() {
                out.push('.');
                prev_dot = true;
            }
        }
    }
    // Trim a trailing separator.
    while out.ends_with('.') {
        out.pop();
    }
    out
}

/// The namespaced target component id for a pack item, e.g.
/// `lib.zenith.flowchart.decision`.
pub(crate) fn target_component_id(pkg_id: &str, item: &str) -> String {
    format!("lib.{}.{}", sanitize_pkg(pkg_id), item)
}

/// Recursively insert every id-bearing node id under `children` into `out`,
/// descending into every container (group/frame/instance has no children, table
/// cells do). Mirrors the validator's `collect_local_ids` but ALSO captures the
/// `Unknown` node id when present (forward-compat: an unknown node may still be
/// addressable), so dedup never accidentally reuses a taken id.
fn collect_node_ids(children: &[Node], out: &mut BTreeSet<String>) {
    for child in children {
        match child {
            Node::Rect(n) => {
                out.insert(n.id.clone());
            }
            Node::Ellipse(n) => {
                out.insert(n.id.clone());
            }
            Node::Line(n) => {
                out.insert(n.id.clone());
            }
            Node::Text(n) => {
                out.insert(n.id.clone());
            }
            Node::Code(n) => {
                out.insert(n.id.clone());
            }
            Node::Image(n) => {
                out.insert(n.id.clone());
            }
            Node::Polygon(n) => {
                out.insert(n.id.clone());
            }
            Node::Polyline(n) => {
                out.insert(n.id.clone());
            }
            Node::Frame(n) => {
                out.insert(n.id.clone());
                collect_node_ids(&n.children, out);
            }
            Node::Group(n) => {
                out.insert(n.id.clone());
                collect_node_ids(&n.children, out);
            }
            Node::Instance(n) => {
                out.insert(n.id.clone());
            }
            Node::Field(n) => {
                out.insert(n.id.clone());
            }
            Node::Toc(n) => {
                out.insert(n.id.clone());
            }
            Node::Footnote(n) => {
                out.insert(n.id.clone());
            }
            Node::Table(n) => {
                out.insert(n.id.clone());
                for row in &n.rows {
                    for cell in &row.cells {
                        collect_node_ids(&cell.children, out);
                    }
                }
            }
            Node::Shape(n) => {
                out.insert(n.id.clone());
            }
            Node::Connector(n) => {
                out.insert(n.id.clone());
            }
            Node::Pattern(n) => {
                out.insert(n.id.clone());
            }
            Node::Chart(n) => {
                out.insert(n.id.clone());
            }
            Node::Light(n) => {
                out.insert(n.id.clone());
            }
            Node::Mesh(n) => {
                out.insert(n.id.clone());
            }
            Node::Unknown(n) => {
                if let Some(id) = &n.id {
                    out.insert(id.clone());
                }
                collect_node_ids(&n.children, out);
            }
        }
    }
}

/// Collect EVERY id declared anywhere in `doc` into one set, used to generate
/// unique instance/provenance ids that cannot collide with anything in the
/// target: every node id (recursively, across pages, masters, and components),
/// plus all block-level ids (tokens, styles, assets, libraries, components,
/// masters, sections, provenance, pages, the document id, and the project id).
///
/// Deterministic and side-effect-free.
pub fn collect_all_ids(doc: &Document) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();

    if let Some(project) = &doc.project {
        ids.insert(project.id.clone());
    }
    ids.insert(doc.body.id.clone());

    for t in &doc.tokens.tokens {
        ids.insert(t.id.clone());
    }
    for s in &doc.styles.styles {
        ids.insert(s.id.clone());
    }
    for a in &doc.assets.assets {
        ids.insert(a.id.clone());
    }
    for l in &doc.libraries {
        ids.insert(l.id.clone());
    }
    for p in &doc.provenance {
        ids.insert(p.id.clone());
    }
    for s in &doc.sections {
        ids.insert(s.id.clone());
    }

    for comp in &doc.components {
        ids.insert(comp.id.clone());
        collect_node_ids(&comp.children, &mut ids);
    }
    for master in &doc.masters {
        ids.insert(master.id.clone());
        collect_node_ids(&master.children, &mut ids);
    }
    for page in &doc.body.pages {
        ids.insert(page.id.clone());
        collect_node_ids(&page.children, &mut ids);
    }

    ids
}

/// Deterministically pick `base`, or `base.1`, `base.2`, … — the first variant
/// not present in `taken`.
pub(super) fn unique_id(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    let mut n = 1u64;
    loop {
        let candidate = format!("{}.{}", base, n);
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// A pixel [`Dimension`].
pub(crate) fn px(value: f64) -> Dimension {
    Dimension {
        value,
        unit: Unit::Px,
    }
}

/// Copy pack tokens into `target` tokens, deduping by id; a same-id-different-
/// value collision keeps the existing token and records a conflict warning.
pub(super) fn copy_tokens(pack: &[Token], target: &mut Vec<Token>, warnings: &mut Vec<String>) {
    for tok in pack {
        match target.iter().find(|t| t.id == tok.id) {
            // Compare by semantic fields only (type + value); `source_span`
            // differs between parses and is not a real conflict.
            Some(existing)
                if existing.token_type != tok.token_type || existing.value != tok.value =>
            {
                warnings.push(dependency_conflict("token", &tok.id));
            }
            Some(_) => {}
            None => target.push(tok.clone()),
        }
    }
}

/// Copy pack styles into `target` styles, deduping by id (see [`copy_tokens`]).
pub(super) fn copy_styles(pack: &[Style], target: &mut Vec<Style>, warnings: &mut Vec<String>) {
    for st in pack {
        match target.iter().find(|t| t.id == st.id) {
            Some(existing) if existing.properties != st.properties => {
                warnings.push(dependency_conflict("style", &st.id));
            }
            Some(_) => {}
            None => target.push(st.clone()),
        }
    }
}

/// Copy pack assets into `target` assets, deduping by id (see [`copy_tokens`]).
///
/// Conflict detection compares `kind`, `src`, AND `sha256`: a same-id asset
/// with a different SHA-256 digest is a real semantic conflict (same path,
/// different content integrity assertion) and warrants a dependency warning.
pub(super) fn copy_assets(
    pack: &[AssetDecl],
    target: &mut Vec<AssetDecl>,
    warnings: &mut Vec<String>,
) {
    for asset in pack {
        match target.iter().find(|a| a.id == asset.id) {
            Some(existing)
                if existing.kind != asset.kind
                    || existing.src != asset.src
                    || existing.sha256 != asset.sha256 =>
            {
                warnings.push(dependency_conflict("asset", &asset.id));
            }
            Some(_) => {}
            None => target.push(asset.clone()),
        }
    }
}

/// A `library.dependency_conflict` warning line for a kept-existing dependency.
pub(super) fn dependency_conflict(kind: &str, id: &str) -> String {
    format!(
        "library.dependency_conflict: {} '{}' already exists in the target with a \
         different definition; kept the existing one",
        kind, id
    )
}
