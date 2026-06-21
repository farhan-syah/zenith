//! Zenith library subsystem: pack format, registry, and resolver.
//!
//! A library "pack" is a `.zen` file whose IDENTITY is declared by a single
//! `library` SELF-entry in its own `libraries` block, for example:
//!
//! ```kdl
//! libraries { library id="@zenith/flowchart" version="1.0.0" }
//! ```
//!
//! That entry's `id` is the package id and `version` is the pack version. A
//! pack's ITEMS are its `components`, filter/mask `tokens`, and `actions`:
//! item `decision` in pack `@zenith/flowchart` is addressed
//! `@zenith/flowchart#decision`.
//!
//! PRESET packs are embedded in the binary via [`include_str!`] (see
//! [`EMBEDDED_PACKS`]); PROJECT packs live in `<project_dir>/libraries/*.zen`
//! and are scanned at runtime. Resolution order is project packs first, then
//! embedded presets (a project pack shadows an embedded pack of the same id).
//!
//! This module contains pure pack-loading/registry logic only; the CLI command
//! that consumes it lives in [`crate::commands::library`].

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use zenith_core::{
    AssetDecl, ComponentDef, Dimension, Document, InstanceNode, KdlAdapter, KdlSource, LibraryDef,
    Node, ProvenanceDef, Style, Token, TokenLiteral, TokenType, TokenValue, Unit,
};

/// Embedded preset packs, as `(pack_id, pack_source)` pairs.
///
/// Each `pack_source` is the verbatim `.zen` text of a shipped preset library,
/// bundled into the binary via [`include_str!`] (mirroring how the default
/// fonts are bundled in `zenith-core`). The `pack_id` is the expected package
/// id and is used only for diagnostics/lookup convenience; the authoritative id
/// is parsed from the pack's own `library` self-entry.
pub const EMBEDDED_PACKS: &[(&str, &str)] = &[
    (
        "@zenith/flowchart",
        include_str!("../../assets/libraries/zenith-flowchart.zen"),
    ),
    (
        "@zenith/filters",
        include_str!("../../assets/libraries/zenith-filters.zen"),
    ),
    (
        "@zenith/masks",
        include_str!("../../assets/libraries/zenith-masks.zen"),
    ),
];

/// Where a [`LibraryPack`] was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackSource {
    /// A preset pack embedded in the binary.
    Preset,
    /// A project pack read from the given `.zen` file path.
    Project(PathBuf),
}

impl PackSource {
    /// A short, stable label for human/JSON output: `"preset"` or `"project"`.
    pub fn label(&self) -> &'static str {
        match self {
            PackSource::Preset => "preset",
            PackSource::Project(_) => "project",
        }
    }
}

/// What kind of thing a pack item is.
///
/// A pack exports COMPONENT items (materialized as an instance on a page),
/// TOKEN items (filter tokens, copied into the target's tokens block with their
/// color-token dependencies — no instance, no page required), and ACTION items
/// (addressed as `<pkg>#<action-id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A component item, addressed `<pkg>#<component-id>`.
    Component,
    /// A filter-token item, addressed `<pkg>#<token-id>`.
    Token,
    /// An action item, addressed `<pkg>#<action-id>`.
    Action,
}

impl ItemKind {
    /// A short, stable label for human/JSON output: `"component"`, `"token"`, or `"action"`.
    pub fn label(&self) -> &'static str {
        match self {
            ItemKind::Component => "component",
            ItemKind::Token => "token",
            ItemKind::Action => "action",
        }
    }
}

/// A single exported item of a [`LibraryPack`]: its id plus its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackItem {
    /// The item id (a component id, a filter-token id, or an action id).
    pub id: String,
    /// Whether the item is a component, a filter token, or an action.
    pub kind: ItemKind,
}

/// A loaded library pack: its identity plus the items it provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPack {
    /// The package id, parsed from the pack's `library` self-entry.
    pub id: String,
    /// The pack version, parsed from the pack's `library` self-entry.
    pub version: Option<String>,
    /// Where the pack came from.
    pub source: PackSource,
    /// The items the pack provides: component ids first (in source order),
    /// then exportable token ids (in source order), then action ids (in source
    /// order).
    pub items: Vec<PackItem>,
}

/// Whether a token type is an EXPORTABLE library item (addressable as
/// `<pkg>#<token-id>` and copied by `materialize_token`).
///
/// Effect tokens — `filter` and `mask` — are self-contained, applyable units
/// that other documents reference by id, so they are exported items. Color /
/// dimension / gradient / shadow tokens are dependencies pulled in transitively
/// when an exported token (or component) needs them, not standalone items.
fn is_exportable_token(ty: &TokenType) -> bool {
    matches!(ty, TokenType::Filter | TokenType::Mask)
}

/// An error produced while parsing a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackError {
    /// Human-readable message describing the failure.
    pub message: String,
}

impl PackError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PackError {}

/// Parse a `.zen` pack `source` into a [`LibraryPack`] tagged with `source_kind`.
///
/// Pack identity is derived from the document's `libraries` block: the library
/// entry whose `id` matches the document's `project` id is the SELF-entry; if no
/// entry matches the project id but there is exactly one library entry, that
/// sole entry is used. A pack with no identifying library self-entry is an error
/// (a pack MUST declare its identity).
///
/// Items are the document's component ids in source order, followed by its
/// FILTER token ids in source order. (Only filter tokens are exported items;
/// color/dimension tokens are dependencies, not items.)
///
/// # Errors
///
/// Returns [`PackError`] when the source fails to parse, or when no library
/// self-entry can be determined.
pub fn parse_pack(source: &str, source_kind: PackSource) -> Result<LibraryPack, PackError> {
    let doc = KdlAdapter
        .parse(source.as_bytes())
        .map_err(|e| PackError::new(format!("parse error: {}", e)))?;

    let project_id = doc.project.as_ref().map(|p| p.id.as_str());

    // Prefer the library entry whose id matches the project id; otherwise fall
    // back to the sole library entry when there is exactly one.
    let self_entry = project_id
        .and_then(|pid| doc.libraries.iter().find(|lib| lib.id == pid))
        .or(match doc.libraries.as_slice() {
            [only] => Some(only),
            _ => None,
        });

    let self_entry = self_entry.ok_or_else(|| {
        PackError::new(
            "pack has no identifying library self-entry (declare \
             `libraries { library id=\"…\" version=\"…\" }`)",
        )
    })?;

    // Component items first (source order), then filter-token items (source
    // order). A token is an exported item only when it is a filter token.
    let mut items: Vec<PackItem> = doc
        .components
        .iter()
        .map(|c| PackItem {
            id: c.id.clone(),
            kind: ItemKind::Component,
        })
        .collect();
    items.extend(
        doc.tokens
            .tokens
            .iter()
            .filter(|t| is_exportable_token(&t.token_type))
            .map(|t| PackItem {
                id: t.id.clone(),
                kind: ItemKind::Token,
            }),
    );
    items.extend(doc.actions.iter().map(|a| PackItem {
        id: a.id.clone(),
        kind: ItemKind::Action,
    }));

    Ok(LibraryPack {
        id: self_entry.id.clone(),
        version: self_entry.version.clone(),
        source: source_kind,
        items,
    })
}

/// Parse every entry in [`EMBEDDED_PACKS`] into a [`LibraryPack`].
///
/// An embedded pack that fails to parse is skipped (embedded content is shipped
/// and tested, so this should not happen in practice); the returned vector
/// contains only the packs that parsed successfully.
pub fn load_embedded_packs() -> Vec<LibraryPack> {
    EMBEDDED_PACKS
        .iter()
        .filter_map(|(_, src)| parse_pack(src, PackSource::Preset).ok())
        .collect()
}

/// Scan `project_dir/libraries/*.zen` and parse each file into a [`LibraryPack`].
///
/// A missing `libraries/` directory (or a `project_dir` without one) yields an
/// empty vector. Files that fail to read or parse are skipped — a note is
/// written to stderr — so one bad pack never aborts the whole listing.
pub fn load_project_packs(project_dir: &Path) -> Vec<LibraryPack> {
    let libraries_dir = project_dir.join("libraries");
    let entries = match std::fs::read_dir(&libraries_dir) {
        Ok(entries) => entries,
        // Missing directory (or any read error) → no project packs.
        Err(_) => return Vec::new(),
    };

    let mut packs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("zen") {
            continue;
        }
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("note: skipping '{}': {}", path.display(), e);
                continue;
            }
        };
        match parse_pack(&source, PackSource::Project(path.clone())) {
            Ok(pack) => packs.push(pack),
            Err(e) => eprintln!("note: skipping '{}': {}", path.display(), e),
        }
    }
    packs
}

/// Resolve all available packs: project packs first, then embedded presets.
///
/// Project packs take precedence over embedded packs of the same id (a project
/// pack SHADOWS an embedded preset). Both are returned, each tagged with its
/// [`PackSource`], so callers that LIST can show every pack; callers that
/// MATERIALIZE should prefer the first pack for a given id. The result is sorted
/// by id for deterministic output (project before embedded on ties).
pub fn resolve_packs(project_dir: Option<&Path>) -> Vec<LibraryPack> {
    let mut packs = Vec::new();
    if let Some(dir) = project_dir {
        packs.extend(load_project_packs(dir));
    }
    packs.extend(load_embedded_packs());

    // Stable, deterministic order: by id, with project packs ahead of embedded
    // on ties (so the shadowing winner sorts first).
    packs.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then_with(|| source_rank(&a.source).cmp(&source_rank(&b.source)))
    });
    packs
}

/// Sort rank giving project packs precedence over embedded presets.
fn source_rank(source: &PackSource) -> u8 {
    match source {
        PackSource::Project(_) => 0,
        PackSource::Preset => 1,
    }
}

// ── Materialization (`library add`) ───────────────────────────────────────────

/// An error produced while materializing a library item into a target document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddError {
    /// Human-readable message describing the failure.
    pub message: String,
}

impl AddError {
    fn new(message: impl Into<String>) -> Self {
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

/// The outcome of a successful [`materialize`] call.
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
/// [`resolve_packs`] only yields pack METADATA; materialization needs the pack's
/// component/token/style/asset subtrees, so this re-reads and parses the pack's
/// source: embedded presets from [`EMBEDDED_PACKS`], project packs from disk.
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

/// Sanitize a package id into a safe component-id fragment.
///
/// Replaces `@` and `/` (and any other non-`[A-Za-z0-9._-]` byte) with `.`,
/// collapsing the result so `@zenith/flowchart` → `zenith.flowchart`.
fn sanitize_pkg(pkg_id: &str) -> String {
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
fn target_component_id(pkg_id: &str, item: &str) -> String {
    format!("lib.{}.{}", sanitize_pkg(pkg_id), item)
}

/// Recursively insert every id-bearing node id under `children` into `out`,
/// descending into every container (group/frame/instance has no children, table
/// cells do). Mirrors the validator's `collect_local_ids` but ALSO captures the
/// `Unknown` node id when present (forward-compat: an unknown node may still be
/// addressable), so dedup never accidentally reuses a taken id.
fn collect_node_ids(children: &[Node], out: &mut HashSet<String>) {
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
pub fn collect_all_ids(doc: &Document) -> HashSet<String> {
    let mut ids = HashSet::new();

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
fn unique_id(base: &str, taken: &HashSet<String>) -> String {
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
fn px(value: f64) -> Dimension {
    Dimension {
        value,
        unit: Unit::Px,
    }
}

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
    let pack = packs.iter().find(|p| p.id == pkg_id).ok_or_else(|| {
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
    })?;

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

// ── Token materialization (`library add` of a filter-token item) ──────────────

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
    /// Non-fatal dependency-conflict warnings (see [`AddOutcome::warnings`]).
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
fn collect_filter_dep_ids(filter_token: &Token, pack_tokens: &[Token]) -> BTreeSet<String> {
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
/// I/O. Unlike [`materialize`], it inserts NO instance and requires NO page.
/// Steps:
///
/// 1. Resolve the FIRST pack in `packs` whose id == `pkg_id` (project shadows
///    preset); load its full [`Document`].
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
    target: &mut Document,
    packs: &[LibraryPack],
    pkg_id: &str,
    item: &str,
    id_base: &str,
) -> Result<TokenAddOutcome, AddError> {
    // 1. Resolve the pack + load its document. ────────────────────────────────
    let pack = packs.iter().find(|p| p.id == pkg_id).ok_or_else(|| {
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
    })?;

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

/// Copy pack tokens into `target` tokens, deduping by id; a same-id-different-
/// value collision keeps the existing token and records a conflict warning.
fn copy_tokens(pack: &[Token], target: &mut Vec<Token>, warnings: &mut Vec<String>) {
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
fn copy_styles(pack: &[Style], target: &mut Vec<Style>, warnings: &mut Vec<String>) {
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
fn copy_assets(pack: &[AssetDecl], target: &mut Vec<AssetDecl>, warnings: &mut Vec<String>) {
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
fn dependency_conflict(kind: &str, id: &str) -> String {
    format!(
        "library.dependency_conflict: {} '{}' already exists in the target with a \
         different definition; kept the existing one",
        kind, id
    )
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_core::validate;

    const FLOWCHART_SRC: &str = include_str!("../../assets/libraries/zenith-flowchart.zen");

    /// A minimal target document with a single empty page `pg`.
    const TARGET_SRC: &str = r#"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {}
  }
}
"#;

    fn parse_target() -> Document {
        KdlAdapter
            .parse(TARGET_SRC.as_bytes())
            .expect("target parses")
    }

    fn hard_errors(doc: &Document) -> Vec<String> {
        validate(doc)
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == zenith_core::Severity::Error)
            .map(|d| format!("{}: {}", d.code, d.message))
            .collect()
    }

    fn first_page_instance_ids(doc: &Document) -> Vec<String> {
        doc.body.pages[0]
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Instance(i) => Some(i.id.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sanitize_pkg_strips_at_and_slash() {
        assert_eq!(sanitize_pkg("@zenith/flowchart"), "zenith.flowchart");
        assert_eq!(
            target_component_id("@zenith/flowchart", "decision"),
            "lib.zenith.flowchart.decision"
        );
    }

    #[test]
    fn parse_spec_splits_pkg_and_item() {
        assert_eq!(
            parse_spec("@zenith/flowchart#decision").expect("ok"),
            ("@zenith/flowchart".to_owned(), "decision".to_owned())
        );
    }

    #[test]
    fn parse_spec_rejects_malformed() {
        assert!(parse_spec("no-hash").is_err());
        assert!(parse_spec("#item").is_err());
        assert!(parse_spec("pkg#").is_err());
    }

    #[test]
    fn materialize_adds_component_tokens_style_instance_provenance() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let outcome = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "pg",
            "decision",
            (10.0, 20.0),
        )
        .expect("materialize ok");

        // Component copied under namespaced id.
        assert_eq!(outcome.target_component_id, "lib.zenith.flowchart.decision");
        assert!(
            target
                .components
                .iter()
                .any(|c| c.id == "lib.zenith.flowchart.decision"),
            "component copied"
        );
        // Child ids are NOT rewritten (still local `shape`).
        let comp = target
            .components
            .iter()
            .find(|c| c.id == "lib.zenith.flowchart.decision")
            .unwrap();
        assert!(matches!(comp.children.first(), Some(Node::Shape(s)) if s.id == "shape"));

        // Dep tokens + style copied.
        assert!(target.tokens.tokens.iter().any(|t| t.id == "lib.flow.fill"));
        assert!(
            target
                .tokens
                .tokens
                .iter()
                .any(|t| t.id == "lib.flow.dec.fill")
        );
        assert!(
            target
                .styles
                .styles
                .iter()
                .any(|s| s.id == "lib.flow.label")
        );
        assert_eq!(target.tokens.format, "zenith-token-v1");

        // Instance inserted on the page referencing the component.
        let inst = target.body.pages[0]
            .children
            .iter()
            .find_map(|n| match n {
                Node::Instance(i) => Some(i),
                _ => None,
            })
            .expect("instance inserted");
        assert_eq!(inst.id, "decision");
        assert_eq!(inst.component, "lib.zenith.flowchart.decision");
        assert_eq!(inst.x, Some(px(10.0)));
        assert_eq!(inst.y, Some(px(20.0)));

        // Library + provenance recorded.
        assert!(target.libraries.iter().any(|l| l.id == "@zenith/flowchart"));
        let prov = target
            .provenance
            .iter()
            .find(|p| p.node == "decision")
            .expect("provenance recorded");
        assert_eq!(prov.library, "@zenith/flowchart");
        assert_eq!(prov.item.as_deref(), Some("decision"));
        assert_eq!(prov.linked, Some(true));
        assert_eq!(outcome.provenance_id, prov.id);
        assert!(outcome.warnings.is_empty());

        // Validates clean.
        assert!(
            hard_errors(&target).is_empty(),
            "errors: {:?}",
            hard_errors(&target)
        );
    }

    #[test]
    fn materialize_round_trips_format_parse() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "pg",
            "decision",
            (0.0, 0.0),
        )
        .expect("materialize ok");
        let bytes = KdlAdapter.format(&target).expect("format");
        let reparsed = KdlAdapter.parse(&bytes).expect("reparse");
        let bytes2 = KdlAdapter.format(&reparsed).expect("format2");
        assert_eq!(bytes, bytes2, "format→parse→format is stable");
    }

    #[test]
    fn double_add_dedups_component_unique_instance_two_provenance() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let o1 = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "pg",
            "decision",
            (0.0, 0.0),
        )
        .expect("first add");
        let o2 = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "pg",
            "decision",
            (0.0, 0.0),
        )
        .expect("second add");

        // Component copied exactly once.
        assert_eq!(
            target
                .components
                .iter()
                .filter(|c| c.id == "lib.zenith.flowchart.decision")
                .count(),
            1
        );
        // Tokens not duplicated.
        assert_eq!(
            target
                .tokens
                .tokens
                .iter()
                .filter(|t| t.id == "lib.flow.fill")
                .count(),
            1
        );
        // Unique instance ids.
        assert_eq!(o1.instance_id, "decision");
        assert_eq!(o2.instance_id, "decision.1");
        assert_eq!(
            first_page_instance_ids(&target),
            vec!["decision", "decision.1"]
        );
        // Two provenance records.
        assert_eq!(target.provenance.len(), 2);
        assert_ne!(o1.provenance_id, o2.provenance_id);
        // One library entry only.
        assert_eq!(
            target
                .libraries
                .iter()
                .filter(|l| l.id == "@zenith/flowchart")
                .count(),
            1
        );
        assert!(hard_errors(&target).is_empty());
    }

    #[test]
    fn materialize_unknown_page_errors_and_does_not_mutate() {
        let mut target = parse_target();
        let before = target.clone();
        let packs = resolve_packs(None);
        let err = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "nope",
            "decision",
            (0.0, 0.0),
        )
        .expect_err("unknown page errors");
        assert!(
            err.message.contains("page 'nope' not found"),
            "msg: {}",
            err.message
        );
        assert_eq!(target, before, "target untouched on page error");
    }

    #[test]
    fn materialize_unknown_pkg_errors_with_available() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let err = materialize(
            &mut target,
            &packs,
            "@no/such",
            "decision",
            "pg",
            "decision",
            (0.0, 0.0),
        )
        .expect_err("unknown pkg errors");
        assert!(
            err.message.contains("@zenith/flowchart"),
            "lists available: {}",
            err.message
        );
    }

    #[test]
    fn materialize_unknown_item_errors_with_available() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let err = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "nope",
            "pg",
            "decision",
            (0.0, 0.0),
        )
        .expect_err("unknown item errors");
        assert!(
            err.message.contains("process"),
            "lists available items: {}",
            err.message
        );
    }

    #[test]
    fn materialize_id_override_used_as_base() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let o = materialize(
            &mut target,
            &packs,
            "@zenith/flowchart",
            "decision",
            "pg",
            "my.node",
            (0.0, 0.0),
        )
        .expect("ok");
        assert_eq!(o.instance_id, "my.node");
    }

    #[test]
    fn parse_embedded_flowchart_identity_and_items() {
        let pack = parse_pack(FLOWCHART_SRC, PackSource::Preset).expect("flowchart pack parses");
        assert_eq!(pack.id, "@zenith/flowchart");
        assert_eq!(pack.version.as_deref(), Some("1.0.0"));
        assert_eq!(pack.source, PackSource::Preset);
        assert_eq!(
            pack.items,
            vec![
                PackItem {
                    id: "process".to_owned(),
                    kind: ItemKind::Component
                },
                PackItem {
                    id: "decision".to_owned(),
                    kind: ItemKind::Component
                },
                PackItem {
                    id: "terminator".to_owned(),
                    kind: ItemKind::Component
                },
            ]
        );
    }

    #[test]
    fn parse_pack_with_actions_lists_action_items() {
        const ACTION_PACK_SRC: &str = r#"zenith version=1 {
  project id="@test/actions" name="Test Actions"
  libraries { library id="@test/actions" version="1.0.0" }
  actions {
    action id="apply-brand-kit" {
      tx "{\"ops\":[]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {
    }
  }
}
"#;
        let pack = parse_pack(ACTION_PACK_SRC, PackSource::Preset).expect("action pack parses");
        assert_eq!(pack.id, "@test/actions");
        assert!(
            pack.items.contains(&PackItem {
                id: "apply-brand-kit".to_owned(),
                kind: ItemKind::Action,
            }),
            "action item must be present; items: {:?}",
            pack.items
        );
    }

    const FILTERS_SRC: &str = include_str!("../../assets/libraries/zenith-filters.zen");

    #[test]
    fn parse_embedded_filters_lists_filter_token_items() {
        let pack = parse_pack(FILTERS_SRC, PackSource::Preset).expect("filters pack parses");
        assert_eq!(pack.id, "@zenith/filters");
        assert_eq!(pack.version.as_deref(), Some("1.0.0"));

        // Filter tokens are items; color dep tokens are NOT.
        assert!(pack.items.contains(&PackItem {
            id: "noir".to_owned(),
            kind: ItemKind::Token
        }));
        assert!(pack.items.contains(&PackItem {
            id: "duotone-gold".to_owned(),
            kind: ItemKind::Token
        }));
        // Color dep tokens are dependencies, not exported items.
        assert!(
            !pack
                .items
                .iter()
                .any(|i| i.id == "lib.filters.duo.gold.shadow"),
            "color dep tokens must not be items"
        );
        // The filters pack ships no components, so every item is a token.
        assert!(pack.items.iter().all(|i| i.kind == ItemKind::Token));
    }

    #[test]
    fn collect_filter_dep_ids_duotone_and_simple() {
        let pack = load_pack_document(&parse_pack(FILTERS_SRC, PackSource::Preset).expect("pack"))
            .expect("pack doc");

        let gold = pack
            .tokens
            .tokens
            .iter()
            .find(|t| t.id == "duotone-gold")
            .expect("duotone-gold present");
        let deps = collect_filter_dep_ids(gold, &pack.tokens.tokens);
        let deps: Vec<String> = deps.into_iter().collect();
        assert_eq!(
            deps,
            vec![
                "lib.filters.duo.gold.highlight".to_owned(),
                "lib.filters.duo.gold.shadow".to_owned(),
            ]
        );

        let noir = pack
            .tokens
            .tokens
            .iter()
            .find(|t| t.id == "noir")
            .expect("noir present");
        assert!(
            collect_filter_dep_ids(noir, &pack.tokens.tokens).is_empty(),
            "non-duotone filters have no token deps"
        );
    }

    #[test]
    fn materialize_token_copies_filter_and_deps_records_provenance() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let outcome = materialize_token(
            &mut target,
            &packs,
            "@zenith/filters",
            "duotone-gold",
            "duotone-gold",
        )
        .expect("materialize_token ok");

        // Filter token + its two color deps copied.
        assert!(target.tokens.tokens.iter().any(|t| t.id == "duotone-gold"));
        assert!(
            target
                .tokens
                .tokens
                .iter()
                .any(|t| t.id == "lib.filters.duo.gold.shadow")
        );
        assert!(
            target
                .tokens
                .tokens
                .iter()
                .any(|t| t.id == "lib.filters.duo.gold.highlight")
        );
        assert_eq!(
            outcome.dep_token_ids,
            vec![
                "lib.filters.duo.gold.highlight".to_owned(),
                "lib.filters.duo.gold.shadow".to_owned(),
            ]
        );
        assert_eq!(outcome.token_id, "duotone-gold");

        // Library + provenance recorded; provenance.node is the TOKEN id.
        assert!(target.libraries.iter().any(|l| l.id == "@zenith/filters"));
        let prov = target
            .provenance
            .iter()
            .find(|p| p.node == "duotone-gold")
            .expect("provenance recorded");
        assert_eq!(prov.library, "@zenith/filters");
        assert_eq!(prov.item.as_deref(), Some("duotone-gold"));
        assert_eq!(outcome.provenance_id, prov.id);

        assert!(
            hard_errors(&target).is_empty(),
            "errors: {:?}",
            hard_errors(&target)
        );
    }

    #[test]
    fn embedded_masks_pack_lists_mask_token_items() {
        let packs = resolve_packs(None);
        let masks = packs
            .iter()
            .find(|p| p.id == "@zenith/masks")
            .expect("@zenith/masks embedded");
        // Mask tokens are exported as token items.
        assert!(
            masks
                .items
                .iter()
                .any(|it| it.id == "vignette" && it.kind == ItemKind::Token),
            "vignette listed as a token item"
        );
        assert!(masks.items.iter().any(|it| it.id == "spotlight"));
    }

    #[test]
    fn materialize_token_mask_applies_via_mask_property_no_deps() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let outcome =
            materialize_token(&mut target, &packs, "@zenith/masks", "vignette", "vignette")
                .expect("materialize_token ok");
        // The mask token is copied; masks are self-contained (no deps).
        assert!(target.tokens.tokens.iter().any(|t| t.id == "vignette"));
        assert!(outcome.dep_token_ids.is_empty());
        // It applies through the `mask` property (not `filter`).
        assert_eq!(outcome.apply_property, "mask");
        // Provenance recorded against the token id.
        assert!(target.provenance.iter().any(|p| p.node == "vignette"));
        assert!(hard_errors(&target).is_empty());
    }

    #[test]
    fn materialize_token_simple_filter_no_deps() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let outcome = materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir")
            .expect("materialize_token ok");
        assert!(target.tokens.tokens.iter().any(|t| t.id == "noir"));
        assert!(outcome.dep_token_ids.is_empty());
        assert!(hard_errors(&target).is_empty());
    }

    #[test]
    fn materialize_token_double_add_dedups_token_and_provenance() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let o1 = materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir")
            .expect("first");
        let o2 = materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir")
            .expect("second");

        // Token copied exactly once.
        assert_eq!(
            target
                .tokens
                .tokens
                .iter()
                .filter(|t| t.id == "noir")
                .count(),
            1
        );
        // Identical provenance is not duplicated.
        assert_eq!(target.provenance.len(), 1);
        assert_eq!(o1.provenance_id, o2.provenance_id);
        // One library entry only.
        assert_eq!(
            target
                .libraries
                .iter()
                .filter(|l| l.id == "@zenith/filters")
                .count(),
            1
        );
        assert!(hard_errors(&target).is_empty());
    }

    #[test]
    fn materialize_token_unknown_item_errors_with_available() {
        let mut target = parse_target();
        let packs = resolve_packs(None);
        let err = materialize_token(&mut target, &packs, "@zenith/filters", "nope", "nope")
            .expect_err("unknown filter token errors");
        assert!(
            err.message.contains("noir"),
            "lists available: {}",
            err.message
        );
    }

    #[test]
    fn embedded_flowchart_parses_and_validates_clean() {
        let doc = KdlAdapter
            .parse(FLOWCHART_SRC.as_bytes())
            .expect("embedded pack must parse");
        let report = validate(&doc);
        let errors: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.severity == zenith_core::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "embedded pack must validate with no errors; got: {:?}",
            errors
        );
    }

    #[test]
    fn load_embedded_packs_includes_flowchart() {
        let packs = load_embedded_packs();
        assert!(
            packs.iter().any(|p| p.id == "@zenith/flowchart"),
            "embedded packs must include @zenith/flowchart"
        );
    }

    #[test]
    fn pack_without_self_entry_errors() {
        let src = r#"zenith version=1 {
  project id="proj.x" name="No Library"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)10 h=(px)10 {}
  }
}
"#;
        let err = parse_pack(src, PackSource::Preset).expect_err("must require a self-entry");
        assert!(err.message.contains("library self-entry"));
    }

    #[test]
    fn load_project_packs_finds_libraries_dir_pack() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lib_dir = dir.path().join("libraries");
        std::fs::create_dir_all(&lib_dir).expect("create libraries dir");
        std::fs::write(lib_dir.join("foo.zen"), FLOWCHART_SRC).expect("write pack");

        let packs = load_project_packs(dir.path());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "@zenith/flowchart");
        assert!(matches!(packs[0].source, PackSource::Project(_)));
    }

    #[test]
    fn load_project_packs_missing_dir_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(load_project_packs(dir.path()).is_empty());
    }

    #[test]
    fn resolve_packs_includes_embedded_when_no_project_dir() {
        let packs = resolve_packs(None);
        assert!(packs.iter().any(|p| p.id == "@zenith/flowchart"));
    }

    #[test]
    fn resolve_packs_is_sorted_by_id() {
        let packs = resolve_packs(None);
        let mut sorted = packs.clone();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<_> = packs.iter().map(|p| &p.id).collect();
        let sorted_ids: Vec<_> = sorted.iter().map(|p| &p.id).collect();
        assert_eq!(ids, sorted_ids);
    }
}
