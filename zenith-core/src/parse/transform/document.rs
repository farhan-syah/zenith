//! Top-level `transform` entry point and the document-level structural blocks
//! (project, assets, libraries, actions, masters, sections, provenance,
//! components, document body, pages, folds, safe-zones).

use kdl::{KdlDocument, KdlNode, KdlValue};

use crate::ast::{
    action::ActionDef,
    asset::{AssetBlock, AssetDecl, AssetKind},
    block_style::BlockStyle,
    brand::BrandContract,
    document::{ComponentDef, Document, DocumentBody, MasterDef, Page, Project, SectionDef},
    library::LibraryDef,
    node::Node,
    policy::{DiagnosticPolicy, PolicyEntry, PolicyVerb},
    provenance::ProvenanceDef,
    recipe::{RecipeDef, RecipeParam},
    style::StyleBlock,
    token::TokenBlock,
    variant::{VariantDef, VariantOverride},
};
use crate::error::{ParseError, ParseErrorCode};

use super::block_style::transform_block_style;
use super::helpers::{
    collect_unknown_props, entry_to_dimension, entry_to_property_value, node_span,
    optional_bool_prop, optional_dimension_prop, optional_i64_prop, optional_string_prop,
    optional_string_prop_aliased, optional_u32_prop, required_string_prop,
    required_string_prop_aliased, required_u32_prop,
};
use super::node::transform_node;
use super::page::transform_page;
use super::tokens::{transform_styles, transform_tokens};

/// Canonical set of property names recognised on the document-level surface.
///
/// Covers both the root `zenith` node attributes (version, colorspace,
/// doc-id, mirror-margins, page-progression, page-parity-start,
/// facing-pages, spread-gutter, margin-*) and the required `document`
/// child-block attributes (id, title).
///
/// Both the hyphenated spelling (canonical) and the underscored alias are
/// included for each attribute that accepts either form, matching the lenient
/// parser behaviour. Used by `zenith-core::schema` to surface the authorable
/// attribute list for the `zenith schema document` subcommand.
pub(crate) const DOCUMENT_KNOWN_PROPS: &[&str] = &[
    // root `zenith` node
    "version",
    "colorspace",
    "doc-id",
    "doc_id",
    "mirror-margins",
    "mirror_margins",
    "page-progression",
    "page_progression",
    "page-parity-start",
    "page_parity_start",
    "facing-pages",
    "facing_pages",
    "spread-gutter",
    "spread_gutter",
    "margin-inner",
    "margin_inner",
    "margin-outer",
    "margin_outer",
    "margin-top",
    "margin_top",
    "margin-bottom",
    "margin_bottom",
    // `document { … }` child block
    "id",
    "title",
];

/// Transform a parsed `KdlDocument` into a Zenith `Document` AST.
pub fn transform(doc: &KdlDocument) -> Result<Document, ParseError> {
    // Find the single top-level `zenith` node.
    let zenith_node = doc
        .nodes()
        .iter()
        .find(|n| n.name().value() == "zenith")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::MissingZenithRoot,
                "no top-level `zenith` node found",
            )
        })?;

    let version = required_u32_prop(zenith_node, "version")?;
    // Optional export color space attribute on the root `zenith` node. Value
    // validity ("srgb"|"cmyk") is checked by the validator, not the parser, so
    // an unrecognized value is preserved verbatim for a precise warning.
    let colorspace = optional_string_prop(zenith_node, "colorspace").map(str::to_owned);

    // Optional stable document identity (`doc-id="01ARZ3NDEKTSV4RRFFQ69G5FAV"`).
    // The value is a ULID (Crockford base-32) minted at document creation; it
    // is preserved verbatim without validation — the parser accepts whatever
    // string the author wrote and lets the validator decide. Both the hyphenated
    // and underscored spellings are accepted for forward-compat.
    let doc_id = optional_string_prop(zenith_node, "doc-id")
        .or_else(|| optional_string_prop(zenith_node, "doc_id"))
        .map(str::to_owned);

    // Optional mirrored-margins toggle (`mirror-margins=#true`). Forward-compat:
    // both the hyphenated and underscored spellings are accepted.
    let mirror_margins = optional_bool_prop(zenith_node, "mirror-margins")
        .or_else(|| optional_bool_prop(zenith_node, "mirror_margins"));

    // Optional page-progression attribute (`page-progression="rtl"`). Value
    // validity ("ltr"|"rtl") is checked by the validator, not the parser, so an
    // unrecognized value is preserved verbatim for a precise warning.
    let page_progression = optional_string_prop(zenith_node, "page-progression")
        .or_else(|| optional_string_prop(zenith_node, "page_progression"))
        .map(str::to_owned);

    // Optional starting-parity attribute (`page-parity-start="verso"`). Value
    // validity ("recto"|"verso") is checked by the validator, not the parser, so
    // an unrecognized value is preserved verbatim for a precise warning. Both the
    // hyphenated and underscored spellings are accepted for forward-compat.
    let page_parity_start = optional_string_prop(zenith_node, "page-parity-start")
        .or_else(|| optional_string_prop(zenith_node, "page_parity_start"))
        .map(str::to_owned);

    // Optional facing-pages toggle (`facing-pages=#true`). Forward-compat:
    // both the hyphenated and underscored spellings are accepted. This is
    // informational metadata only; pages still render independently.
    let facing_pages = optional_bool_prop(zenith_node, "facing-pages")
        .or_else(|| optional_bool_prop(zenith_node, "facing_pages"));

    // Optional spread-gutter dimension (`spread-gutter=(px)40`). Drives the
    // transparent gap between the two pages of a `--spread` composite.
    // Both hyphenated and underscored spellings are accepted for forward-compat.
    let spread_gutter = optional_dimension_prop(zenith_node, "spread-gutter")
        .or_else(|| optional_dimension_prop(zenith_node, "spread_gutter"));

    // Optional DOCUMENT-LEVEL default book live-area margins. Same KDL syntax as
    // on a page (`margin-inner=(px)225`); a page that omits its own margin
    // inherits these via `Document::effective_margins`. Both hyphenated and
    // underscored spellings are accepted for forward-compat.
    let margin_inner = optional_dimension_prop(zenith_node, "margin-inner")
        .or_else(|| optional_dimension_prop(zenith_node, "margin_inner"));
    let margin_outer = optional_dimension_prop(zenith_node, "margin-outer")
        .or_else(|| optional_dimension_prop(zenith_node, "margin_outer"));
    let margin_top = optional_dimension_prop(zenith_node, "margin-top")
        .or_else(|| optional_dimension_prop(zenith_node, "margin_top"));
    let margin_bottom = optional_dimension_prop(zenith_node, "margin-bottom")
        .or_else(|| optional_dimension_prop(zenith_node, "margin_bottom"));

    let children_doc = zenith_node.children().ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::MissingZenithRoot,
            "`zenith` node has no children block",
        )
    })?;

    let mut project: Option<Project> = None;
    let mut assets = AssetBlock::default();
    let mut libraries: Vec<LibraryDef> = Vec::new();
    let mut actions: Vec<ActionDef> = Vec::new();
    let mut tokens = TokenBlock::default();
    let mut styles = StyleBlock::default();
    let mut components: Vec<ComponentDef> = Vec::new();
    let mut masters: Vec<MasterDef> = Vec::new();
    let mut sections: Vec<SectionDef> = Vec::new();
    let mut provenance: Vec<ProvenanceDef> = Vec::new();
    let mut variants: Vec<VariantDef> = Vec::new();
    let mut recipes: Vec<RecipeDef> = Vec::new();
    let mut diagnostic_policy = DiagnosticPolicy::default();
    let mut brand_contract = BrandContract::default();
    let mut body: Option<DocumentBody> = None;

    for child in children_doc.nodes() {
        match child.name().value() {
            "project" => {
                project = Some(transform_project(child)?);
            }
            "assets" => {
                assets = transform_assets(child)?;
            }
            "libraries" => {
                libraries = transform_libraries(child)?;
            }
            "actions" => {
                actions = transform_actions(child)?;
            }
            "tokens" => {
                tokens = transform_tokens(child)?;
            }
            "styles" => {
                styles = transform_styles(child)?;
            }
            "components" => {
                components = transform_components(child)?;
            }
            "masters" => {
                masters = transform_masters(child)?;
            }
            "sections" => {
                sections = transform_sections(child)?;
            }
            "provenance" => {
                provenance = transform_provenance(child)?;
            }
            "variants" => {
                variants = transform_variants(child)?;
            }
            "recipes" => {
                recipes = transform_recipes(child)?;
            }
            "diagnostics" => {
                diagnostic_policy = transform_diagnostic_policy(child)?;
            }
            "brand" => {
                brand_contract = transform_brand_contract(child)?;
            }
            "document" => {
                body = Some(transform_document_body(child)?);
            }
            // Any other unknown top-level children are accepted without error
            // (forward-compat); they simply are not represented in the v0 AST.
            _ => {}
        }
    }

    let body = body.ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::MissingZenithRoot,
            "`zenith` node is missing a `document` child",
        )
    })?;

    Ok(Document {
        version,
        colorspace,
        doc_id,
        mirror_margins,
        facing_pages,
        spread_gutter,
        page_progression,
        page_parity_start,
        margin_inner,
        margin_outer,
        margin_top,
        margin_bottom,
        project,
        assets,
        libraries,
        actions,
        tokens,
        styles,
        components,
        masters,
        sections,
        provenance,
        variants,
        recipes,
        diagnostic_policy,
        brand_contract,
        body,
    })
}

// ---------------------------------------------------------------------------
// Diagnostics policy
// ---------------------------------------------------------------------------

/// Transform the document-level `diagnostics { … }` block into a
/// [`DiagnosticPolicy`].
///
/// Each child node is one policy entry whose NAME is the verb (`allow`, `deny`,
/// or `warn`) and whose FIRST positional argument is the diagnostic code string:
///
/// ```text
/// diagnostics {
///   allow "layout.off_canvas"
///   allow "layout.off_canvas" "bg.glow" "bg.rim"
///   deny  "token.unused"
///   warn  "node.unknown_property"
/// }
/// ```
///
/// A child whose name is not a recognized verb is silently ignored
/// (forward-compat — same posture as every other document block). A recognized
/// verb whose code argument is missing or non-string is a hard [`ParseError`]
/// (the entry is meaningless without a code). Declaration order is preserved;
/// last-wins resolution happens at consult time (see [`DiagnosticPolicy::verb_for`]).
pub(crate) fn transform_diagnostic_policy(node: &KdlNode) -> Result<DiagnosticPolicy, ParseError> {
    let mut entries: Vec<PolicyEntry> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let (verb, verb_name) = match child.name().value() {
                "allow" => (PolicyVerb::Allow, "allow"),
                "deny" => (PolicyVerb::Deny, "deny"),
                "warn" => (PolicyVerb::Warn, "warn"),
                // Unknown verb → ignore (forward-compat).
                _ => continue,
            };
            let mut positional = child
                .entries()
                .iter()
                .filter(|entry| entry.name().is_none());
            let code = match positional.next().map(|entry| entry.value()) {
                Some(KdlValue::String(s)) => s.clone(),
                _ => {
                    return Err(ParseError::spanless(
                        ParseErrorCode::InvalidPropertyValue,
                        format!(
                            "diagnostics `{verb_name}` entry requires a quoted diagnostic-code \
                             string as its first argument, e.g. `{verb_name} \"layout.off_canvas\"`"
                        ),
                    ));
                }
            };
            let mut subjects: Vec<String> = Vec::new();
            for (idx, entry) in positional.enumerate() {
                match entry.value() {
                    KdlValue::String(s) => subjects.push(s.clone()),
                    _ => {
                        let subject_index = idx + 1;
                        return Err(ParseError::spanless(
                            ParseErrorCode::InvalidPropertyValue,
                            format!(
                                "diagnostics `{verb_name}` subject argument {subject_index} must \
                                 be a quoted subject-id string, e.g. `{verb_name} \
                                 \"layout.off_canvas\" \"bg.glow\"`"
                            ),
                        ));
                    }
                }
            }
            if child.entries().iter().any(|entry| {
                entry
                    .name()
                    .map(|name| name.value() == "subject" || name.value() == "subjects")
                    .unwrap_or(false)
            }) {
                return Err(ParseError::spanless(
                    ParseErrorCode::InvalidPropertyValue,
                    "diagnostics scoped subjects must be positional strings after the \
                     diagnostic code, e.g. `allow \"layout.off_canvas\" \"bg.glow\"`",
                ));
            }
            entries.push(PolicyEntry {
                verb,
                code,
                subjects,
                source_span: node_span(child),
            });
        }
    }
    Ok(DiagnosticPolicy { entries })
}

// ---------------------------------------------------------------------------
// Brand contract
// ---------------------------------------------------------------------------

/// Transform the document-level `brand { … }` block into a [`BrandContract`].
///
/// Each child node is one category constraint:
/// - `colors "#hex1" "#hex2" …` — approved color hex strings.
/// - `fonts  "Family One" "Family Two" …` — approved font-family names.
/// - `weights 400 700 …` — approved font weight integers (100–900).
///
/// Absent child = unconstrained for that category. Unknown children are
/// silently ignored (forward-compat). Declaration order within each child
/// node's arguments is preserved.
///
/// Errors:
/// - A `colors` or `fonts` argument that is not a KDL string → [`ParseError`].
/// - A `weights` argument that is not an integer → [`ParseError`].
/// - A `weights` argument that is out of the 100–900 range → [`ParseError`].
pub(crate) fn transform_brand_contract(node: &KdlNode) -> Result<BrandContract, ParseError> {
    let source_span = node_span(node);
    let mut allowed_colors: Option<Vec<String>> = None;
    let mut allowed_fonts: Option<Vec<String>> = None;
    let mut allowed_weights: Option<Vec<u32>> = None;

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "colors" => {
                    let mut colors: Vec<String> = Vec::new();
                    // Only positional arguments (name is None) are color values.
                    let positional: Vec<_> = child
                        .entries()
                        .iter()
                        .filter(|e| e.name().is_none())
                        .collect();
                    for (idx, entry) in positional.iter().enumerate() {
                        match entry.value() {
                            KdlValue::String(s) => {
                                // Store hex strings in lowercase so comparisons
                                // are case-insensitive without repeated conversion.
                                colors.push(s.to_lowercase());
                            }
                            _ => {
                                return Err(ParseError::spanless(
                                    ParseErrorCode::InvalidPropertyValue,
                                    format!(
                                        "brand `colors` argument {idx} must be a quoted string \
                                         (hex color), e.g. `colors \"#0b1f33\" \"#ffffff\"`"
                                    ),
                                ));
                            }
                        }
                    }
                    allowed_colors = Some(colors);
                }
                "fonts" => {
                    let mut fonts: Vec<String> = Vec::new();
                    let positional: Vec<_> = child
                        .entries()
                        .iter()
                        .filter(|e| e.name().is_none())
                        .collect();
                    for (idx, entry) in positional.iter().enumerate() {
                        match entry.value() {
                            KdlValue::String(s) => {
                                fonts.push(s.clone());
                            }
                            _ => {
                                return Err(ParseError::spanless(
                                    ParseErrorCode::InvalidPropertyValue,
                                    format!(
                                        "brand `fonts` argument {idx} must be a quoted string \
                                         (font-family name), e.g. `fonts \"Noto Sans\"`"
                                    ),
                                ));
                            }
                        }
                    }
                    allowed_fonts = Some(fonts);
                }
                "weights" => {
                    let mut weights: Vec<u32> = Vec::new();
                    let positional: Vec<_> = child
                        .entries()
                        .iter()
                        .filter(|e| e.name().is_none())
                        .collect();
                    for (idx, entry) in positional.iter().enumerate() {
                        match entry.value() {
                            KdlValue::Integer(n) => {
                                // KDL integers are i128; we need a u32 in 100..=900.
                                let n_val = *n;
                                if !(100..=900).contains(&n_val) {
                                    return Err(ParseError::spanless(
                                        ParseErrorCode::InvalidPropertyValue,
                                        format!(
                                            "brand `weights` argument {idx} must be an integer \
                                             in the range 100-900 (got {n_val})"
                                        ),
                                    ));
                                }
                                // Infallible: 100..=900 fits in u32 and is positive.
                                let w = u32::try_from(n_val).map_err(|_| {
                                    ParseError::spanless(
                                        ParseErrorCode::InvalidPropertyValue,
                                        format!(
                                            "brand `weights` argument {idx} is out of range \
                                             for a u32 weight (got {n_val})"
                                        ),
                                    )
                                })?;
                                weights.push(w);
                            }
                            _ => {
                                return Err(ParseError::spanless(
                                    ParseErrorCode::InvalidPropertyValue,
                                    format!(
                                        "brand `weights` argument {idx} must be an integer \
                                         (font weight), e.g. `weights 400 700`"
                                    ),
                                ));
                            }
                        }
                    }
                    allowed_weights = Some(weights);
                }
                // Unknown children are silently ignored (forward-compat).
                _ => {}
            }
        }
    }

    Ok(BrandContract {
        allowed_colors,
        allowed_fonts,
        allowed_weights,
        source_span,
    })
}

// ---------------------------------------------------------------------------
// Masters
// ---------------------------------------------------------------------------

/// Transform the document-level `masters { … }` block into a list of
/// [`MasterDef`]. Each `master id="..." { <child nodes> }` becomes one
/// definition whose children are parsed exactly like page/group children (via
/// [`transform_node`]). Non-`master` children inside the block are silently
/// ignored (forward-compat). Mirrors [`transform_components`].
fn transform_masters(node: &KdlNode) -> Result<Vec<MasterDef>, ParseError> {
    let mut defs: Vec<MasterDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "master" {
                defs.push(transform_master_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_master_def(node: &KdlNode) -> Result<MasterDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let children = transform_children(node)?;
    Ok(MasterDef {
        id,
        children,
        source_span: node_span(node),
    })
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// Transform the document-level `sections { … }` block into a list of
/// [`SectionDef`]. Each `section id="…" name="…" start-page="…" …` is a leaf
/// marker (it takes no children); non-`section` children inside the block are
/// silently ignored (forward-compat). Mirrors [`transform_masters`].
fn transform_sections(node: &KdlNode) -> Result<Vec<SectionDef>, ParseError> {
    let mut defs: Vec<SectionDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "section" {
                defs.push(transform_section_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_section_def(node: &KdlNode) -> Result<SectionDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let name = required_string_prop(node, "name")?.to_owned();
    let start_page = required_string_prop_aliased(node, "start-page", "start_page")?.to_owned();

    // `folio-start` / `folio_start`: optional non-negative integer.
    // `optional_u32_prop` silently drops negative or non-integer values, which
    // is the same forward-compat posture used for other optional integer props
    // (e.g. `tab-width`, `page-parity-start`).
    let folio_start = optional_u32_prop(node, "folio-start")
        .or_else(|| optional_u32_prop(node, "folio_start"))
        .map(|n| n as usize);

    // `folio-style` / `folio_style`: optional string.
    let folio_style =
        optional_string_prop_aliased(node, "folio-style", "folio_style").map(str::to_owned);

    Ok(SectionDef {
        id,
        name,
        folio_start,
        folio_style,
        start_page,
        source_span: node_span(node),
    })
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

const LIBRARY_KNOWN_PROPS: &[&str] = &["id", "version", "hash"];

/// Transform the document-level `libraries { … }` block into a list of
/// [`LibraryDef`]. Each `library id="…" version="…" hash="…"` is a leaf marker
/// (it takes no children); non-`library` children inside the block are silently
/// ignored (forward-compat). Mirrors [`transform_sections`].
fn transform_libraries(node: &KdlNode) -> Result<Vec<LibraryDef>, ParseError> {
    let mut defs: Vec<LibraryDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "library" {
                defs.push(transform_library_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_library_def(node: &KdlNode) -> Result<LibraryDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let version = optional_string_prop(node, "version").map(str::to_owned);
    let hash = optional_string_prop(node, "hash").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, LIBRARY_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(LibraryDef {
        id,
        version,
        hash,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

const ACTION_KNOWN_PROPS: &[&str] = &["id", "label", "version"];

/// Transform the document-level `actions { … }` block into a list of
/// [`ActionDef`]. Each `action id="…" label="…" version="…" { tx "…" }` is a
/// block node whose `tx` child carries the opaque JSON payload as a positional
/// string argument; non-`action` children inside the block are silently ignored
/// (forward-compat). Mirrors [`transform_libraries`].
fn transform_actions(node: &KdlNode) -> Result<Vec<ActionDef>, ParseError> {
    let mut defs: Vec<ActionDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "action" {
                defs.push(transform_action_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_action_def(node: &KdlNode) -> Result<ActionDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let label = optional_string_prop(node, "label").map(str::to_owned);
    let version = optional_string_prop(node, "version").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, ACTION_KNOWN_PROPS);
    let source_span = node_span(node);

    // The `tx_json` payload lives in a `tx` child node whose first positional
    // argument is the decoded string. Exactly like `content` in CodeNode, the
    // value is stored decoded; the writer re-encodes it on format.
    let tx_json = node
        .children()
        .and_then(|doc| {
            doc.nodes().iter().find_map(|child| {
                if child.name().value() != "tx" {
                    return None;
                }
                child.get(0).and_then(|v| match v {
                    KdlValue::String(s) => Some(s.clone()),
                    _ => None,
                })
            })
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("node `action` id=\"{id}\" is missing required `tx` child node"),
            )
        })?;

    Ok(ActionDef {
        id,
        label,
        version,
        tx_json,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

const PROVENANCE_KNOWN_PROPS: &[&str] = &["id", "node", "library", "item", "linked"];

/// Transform the document-level `provenance { … }` block into a list of
/// [`ProvenanceDef`]. Each `origin id="…" node="…" library="…" …` is a leaf
/// marker (it takes no children); non-`origin` children inside the block are
/// silently ignored (forward-compat). Mirrors [`transform_libraries`].
fn transform_provenance(node: &KdlNode) -> Result<Vec<ProvenanceDef>, ParseError> {
    let mut defs: Vec<ProvenanceDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "origin" {
                defs.push(transform_provenance_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_provenance_def(node: &KdlNode) -> Result<ProvenanceDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let document_node = required_string_prop(node, "node")?.to_owned();
    let library = required_string_prop(node, "library")?.to_owned();
    let item = optional_string_prop(node, "item").map(str::to_owned);
    let linked = optional_bool_prop(node, "linked");
    let unknown_props = collect_unknown_props(node, PROVENANCE_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(ProvenanceDef {
        id,
        node: document_node,
        library,
        item,
        linked,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

const VARIANT_KNOWN_PROPS: &[&str] = &["id", "source", "w", "h"];
const VARIANT_OVERRIDE_KNOWN_PROPS: &[&str] =
    &["node", "visible", "x", "y", "w", "h", "fill", "text"];

/// Transform the document-level `variants { … }` block into a list of
/// [`VariantDef`]. Each `variant id="…" source="…" w=(px)N h=(px)N { … }` is
/// a block node; non-`variant` children inside the block are silently ignored
/// (forward-compat). Mirrors [`transform_provenance`].
fn transform_variants(node: &KdlNode) -> Result<Vec<VariantDef>, ParseError> {
    let mut defs: Vec<VariantDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "variant" {
                defs.push(transform_variant_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_variant_def(node: &KdlNode) -> Result<VariantDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let source = required_string_prop(node, "source")?.to_owned();

    let w = node
        .entry("w")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("variant `{id}` is missing required property `w`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "w"))?;

    let h = node
        .entry("h")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("variant `{id}` is missing required property `h`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "h"))?;

    let unknown_props = collect_unknown_props(node, VARIANT_KNOWN_PROPS);
    let source_span = node_span(node);

    let mut overrides: Vec<VariantOverride> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "override" {
                overrides.push(transform_variant_override(child)?);
            }
        }
    }

    Ok(VariantDef {
        id,
        source,
        w,
        h,
        overrides,
        source_span,
        unknown_props,
    })
}

fn transform_variant_override(node: &KdlNode) -> Result<VariantOverride, ParseError> {
    let target_node = required_string_prop(node, "node")?.to_owned();
    let visible = optional_bool_prop(node, "visible");
    let x = optional_dimension_prop(node, "x");
    let y = optional_dimension_prop(node, "y");
    let w = optional_dimension_prop(node, "w");
    let h = optional_dimension_prop(node, "h");
    let fill = node
        .entry("fill")
        .and_then(|e| entry_to_property_value(e).ok());
    let text = optional_string_prop(node, "text").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, VARIANT_OVERRIDE_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(VariantOverride {
        node: target_node,
        visible,
        x,
        y,
        w,
        h,
        fill,
        text,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Recipes
// ---------------------------------------------------------------------------

const RECIPE_KNOWN_PROPS: &[&str] = &["id", "kind", "seed", "generator", "bounds", "detached"];
const RECIPE_PARAM_KNOWN_PROPS: &[&str] = &["name", "value"];

/// Transform the document-level `recipes { … }` block into a list of
/// [`RecipeDef`]. Each `recipe id="…" kind="…" …` is a block node; non-`recipe`
/// children inside the block are silently ignored (forward-compat). Mirrors
/// [`transform_variants`].
fn transform_recipes(node: &KdlNode) -> Result<Vec<RecipeDef>, ParseError> {
    let mut defs: Vec<RecipeDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "recipe" {
                defs.push(transform_recipe_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_recipe_def(node: &KdlNode) -> Result<RecipeDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let kind = required_string_prop(node, "kind")?.to_owned();

    // Optional integer seed: negative seeds are valid, so read as i64 not u32.
    let seed = optional_i64_prop(node, "seed");

    let generator = optional_string_prop(node, "generator").map(str::to_owned);
    let bounds = optional_string_prop(node, "bounds").map(str::to_owned);
    let detached = optional_bool_prop(node, "detached");

    let unknown_props = collect_unknown_props(node, RECIPE_KNOWN_PROPS);
    let source_span = node_span(node);

    let mut params: Vec<RecipeParam> = Vec::new();
    let mut palette: Vec<String> = Vec::new();
    let mut expanded: Vec<String> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "param" => {
                    params.push(transform_recipe_param(child)?);
                }
                "palette" => {
                    palette.push(required_string_prop(child, "token")?.to_owned());
                }
                "expanded" => {
                    expanded.push(required_string_prop(child, "node")?.to_owned());
                }
                _ => {}
            }
        }
    }

    Ok(RecipeDef {
        id,
        kind,
        seed,
        generator,
        bounds,
        detached,
        params,
        palette,
        expanded,
        source_span,
        unknown_props,
    })
}

fn transform_recipe_param(node: &KdlNode) -> Result<RecipeParam, ParseError> {
    let name = required_string_prop(node, "name")?.to_owned();
    let value = node
        .entry("value")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("recipe `param` `{name}` is missing required property `value`"),
            )
        })
        .and_then(entry_to_property_value)?;
    let unknown_props = collect_unknown_props(node, RECIPE_PARAM_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(RecipeParam {
        name,
        value,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Transform the document-level `components { … }` block into a list of
/// [`ComponentDef`]. Each `component id="..." { <child nodes> }` becomes one
/// definition whose children are parsed exactly like page/group children (via
/// [`transform_node`]). Non-`component` children inside the block are silently
/// ignored (forward-compat).
fn transform_components(node: &KdlNode) -> Result<Vec<ComponentDef>, ParseError> {
    let mut defs: Vec<ComponentDef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "component" {
                defs.push(transform_component_def(child)?);
            }
        }
    }
    Ok(defs)
}

fn transform_component_def(node: &KdlNode) -> Result<ComponentDef, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let children = transform_children(node)?;
    Ok(ComponentDef {
        id,
        children,
        source_span: node_span(node),
    })
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

fn transform_project(node: &KdlNode) -> Result<Project, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let name = required_string_prop(node, "name")?.to_owned();
    let author = node.children().and_then(|doc| {
        doc.nodes()
            .iter()
            .find(|n| n.name().value() == "author")
            .and_then(|n| n.get(0))
            .and_then(|v| {
                if let KdlValue::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
    });
    Ok(Project { id, name, author })
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

/// Canonical set of property names recognised on an `asset` declaration node.
///
/// Used by `zenith-core::schema` to surface the authorable attribute list for
/// the `zenith schema asset` subcommand.
pub(crate) const ASSET_KNOWN_PROPS: &[&str] = &[
    "id",
    "kind",
    "src",
    "sha256",
    "ai-prompt",
    "ai-model",
    "ai-provider",
    "ai-seed",
    "ai-generation-date",
    "ai-license",
    "ai-source-rights",
    "ai-safety-status",
    "ai-reuse-policy",
];

fn transform_assets(node: &KdlNode) -> Result<AssetBlock, ParseError> {
    let source_span = node_span(node);
    let mut asset_list: Vec<AssetDecl> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "asset" {
                asset_list.push(transform_asset_decl(child)?);
            }
            // Non-`asset` child nodes inside assets block are silently ignored
            // (forward-compat).
        }
    }

    Ok(AssetBlock {
        assets: asset_list,
        source_span,
    })
}

fn transform_asset_decl(node: &KdlNode) -> Result<AssetDecl, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let kind_str = required_string_prop(node, "kind")?;
    let kind = AssetKind::from_kind_str(kind_str);
    let src = required_string_prop(node, "src")?.to_owned();
    let sha256 = optional_string_prop(node, "sha256").map(str::to_owned);
    let ai_prompt = optional_string_prop(node, "ai-prompt").map(str::to_owned);
    let ai_model = optional_string_prop(node, "ai-model").map(str::to_owned);
    let ai_provider = optional_string_prop(node, "ai-provider").map(str::to_owned);
    let ai_seed = optional_i64_prop(node, "ai-seed");
    let ai_generation_date = optional_string_prop(node, "ai-generation-date").map(str::to_owned);
    let ai_license = optional_string_prop(node, "ai-license").map(str::to_owned);
    let ai_source_rights = optional_string_prop(node, "ai-source-rights").map(str::to_owned);
    let ai_safety_status = optional_string_prop(node, "ai-safety-status").map(str::to_owned);
    let ai_reuse_policy = optional_string_prop(node, "ai-reuse-policy").map(str::to_owned);
    let unknown_props = collect_unknown_props(node, ASSET_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(AssetDecl {
        id,
        kind,
        src,
        sha256,
        ai_prompt,
        ai_model,
        ai_provider,
        ai_seed,
        ai_generation_date,
        ai_license,
        ai_source_rights,
        ai_safety_status,
        ai_reuse_policy,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Document body / pages
// ---------------------------------------------------------------------------

fn transform_document_body(node: &KdlNode) -> Result<DocumentBody, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let title = optional_string_prop(node, "title").map(str::to_owned);

    let mut block_styles: Vec<BlockStyle> = Vec::new();
    let mut pages: Vec<Page> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "block" => block_styles.push(transform_block_style(child)?),
                "page" => pages.push(transform_page(child)?),
                _ => {}
            }
        }
    }

    Ok(DocumentBody {
        id,
        title,
        block_styles,
        pages,
    })
}

/// Iterate a KDL node's children block and transform each child into a
/// [`Node`].  Returns an empty `Vec` when the node has no children block.
///
/// Both `transform_page` and `transform_group` use this helper to avoid
/// duplicating the child-iteration logic.
///
/// # Known limitation
/// Groups nest recursively via `transform_node` → `transform_group` →
/// `transform_children` with no depth guard.  This is an accepted v0
/// limitation; stack overflow is only possible with pathologically deep trees.
pub(super) fn transform_children(node: &KdlNode) -> Result<Vec<Node>, ParseError> {
    let mut children: Vec<Node> = Vec::new();
    if let Some(doc) = node.children() {
        for child in doc.nodes() {
            children.push(transform_node(child)?);
        }
    }
    Ok(children)
}
