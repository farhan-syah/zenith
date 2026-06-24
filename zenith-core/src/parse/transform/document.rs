//! Top-level `transform` entry point and the document-level structural blocks
//! (project, assets, libraries, actions, masters, sections, provenance,
//! components, document body, pages, folds, safe-zones).

use kdl::{KdlDocument, KdlNode, KdlValue};

use crate::ast::{
    action::ActionDef,
    asset::{AssetBlock, AssetDecl, AssetKind},
    document::{
        ComponentDef, Document, DocumentBody, Fold, MasterDef, Page, Project, SafeZone,
        SafeZoneType, SectionDef,
    },
    library::LibraryDef,
    node::Node,
    provenance::ProvenanceDef,
    recipe::{RecipeDef, RecipeParam},
    style::StyleBlock,
    token::TokenBlock,
    variant::{VariantDef, VariantOverride},
};
use crate::error::{ParseError, ParseErrorCode};

use super::helpers::{
    collect_unknown_props, entry_to_dimension, entry_to_property_value, node_span,
    optional_bool_prop, optional_dimension_prop, optional_i64_prop, optional_string_prop,
    optional_string_prop_aliased, optional_u32_prop, required_string_prop,
    required_string_prop_aliased, required_u32_prop,
};
use super::node::transform_node;
use super::tokens::{transform_styles, transform_tokens};

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
        body,
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
const VARIANT_OVERRIDE_KNOWN_PROPS: &[&str] = &["node", "visible", "text", "fill"];

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
    let text = optional_string_prop(node, "text").map(str::to_owned);
    let fill = node
        .entry("fill")
        .and_then(|e| entry_to_property_value(e).ok());
    let unknown_props = collect_unknown_props(node, VARIANT_OVERRIDE_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(VariantOverride {
        node: target_node,
        visible,
        text,
        fill,
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

const ASSET_KNOWN_PROPS: &[&str] = &[
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

    let mut pages: Vec<Page> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "page" {
                pages.push(transform_page(child)?);
            }
        }
    }

    Ok(DocumentBody { id, title, pages })
}

fn transform_page(node: &KdlNode) -> Result<Page, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let name = optional_string_prop(node, "name").map(str::to_owned);

    let width = node
        .entry("w")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("page `{id}` is missing required property `w`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "w"))?;

    let height = node
        .entry("h")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("page `{id}` is missing required property `h`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "h"))?;

    let background = node
        .entry("background")
        .and_then(|e| entry_to_property_value(e).ok());

    // Optional uniform print-bleed margin (e.g. `bleed=(px)35`). Read like any
    // other dimension prop; unit validity (px/pt resolvable, >= 0) is checked by
    // the validator, never the parser, so an out-of-range/odd-unit value is
    // preserved verbatim for a precise warning.
    let bleed = optional_dimension_prop(node, "bleed");

    // Book live-area margins. Read like any other dimension prop; resolvability
    // (px/pt) and sign are checked by the validator's margin advisory, never the
    // parser, so odd-unit/odd-value margins are preserved verbatim. Both the
    // hyphenated and underscored spellings are accepted for forward-compat.
    let margin_inner = optional_dimension_prop(node, "margin-inner")
        .or_else(|| optional_dimension_prop(node, "margin_inner"));
    let margin_outer = optional_dimension_prop(node, "margin-outer")
        .or_else(|| optional_dimension_prop(node, "margin_outer"));
    let margin_top = optional_dimension_prop(node, "margin-top")
        .or_else(|| optional_dimension_prop(node, "margin_top"));
    let margin_bottom = optional_dimension_prop(node, "margin-bottom")
        .or_else(|| optional_dimension_prop(node, "margin_bottom"));

    // Optional page baseline-grid pitch (e.g. `baseline-grid=(px)14`). Read like
    // any other dimension prop; resolvability (px/pt) and sign are checked at
    // compile time (the snap ignores a non-positive/unresolvable value), never
    // the parser, so an odd value is preserved verbatim.
    let baseline_grid = optional_dimension_prop(node, "baseline-grid")
        .or_else(|| optional_dimension_prop(node, "baseline_grid"));

    // Optional page-level line-jump style (`line-jumps="arc"`). Value validity
    // ("none"|"arc"|"gap") is checked by the validator, not the parser, so an
    // unrecognized value is preserved verbatim for a precise warning. Both the
    // hyphenated and underscored spellings are accepted for forward-compat.
    let line_jumps = optional_string_prop(node, "line-jumps")
        .or_else(|| optional_string_prop(node, "line_jumps"))
        .map(str::to_owned);

    // Optional explicit per-page parity override (`parity="verso"`). Value
    // validity ("recto"|"verso") is checked by the validator, not the parser, so
    // an unrecognized value is preserved verbatim for a precise warning.
    let parity = optional_string_prop(node, "parity").map(str::to_owned);

    // Optional master-page reference (`master="m.body"`). Existence is checked by
    // the validator (master.unknown_reference), never the parser.
    let master = optional_string_prop(node, "master").map(str::to_owned);

    let source_span = node_span(node);

    // A page's children block mixes `safe-zone` and `fold` declarations (page
    // metadata, not rendering nodes) with renderable nodes. Split them here:
    // safe-zones go to `page.safe_zones`; folds to `page.folds`; everything
    // else through `transform_node`.
    let mut safe_zones: Vec<SafeZone> = Vec::new();
    let mut folds: Vec<Fold> = Vec::new();
    let mut children: Vec<Node> = Vec::new();
    if let Some(doc) = node.children() {
        for child in doc.nodes() {
            match child.name().value() {
                "safe-zone" => safe_zones.push(transform_safe_zone(child)?),
                "fold" => folds.push(transform_fold(child)?),
                _ => children.push(transform_node(child)?),
            }
        }
    }

    Ok(Page {
        id,
        name,
        width,
        height,
        background,
        bleed,
        margin_inner,
        margin_outer,
        margin_top,
        margin_bottom,
        baseline_grid,
        line_jumps,
        parity,
        master,
        safe_zones,
        folds,
        children,
        source_span,
    })
}

/// Transform a `fold` page child into a [`Fold`].
///
/// Reads required `id`; `orientation` maps a string (`"vertical"` /
/// `"horizontal"`, defaulting to `"vertical"` for any other / absent value);
/// `position` is an optional dimension (x for vertical, y for horizontal).
fn transform_fold(node: &KdlNode) -> Result<Fold, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let orientation = match optional_string_prop(node, "orientation") {
        Some("horizontal") => "horizontal".to_owned(),
        _ => "vertical".to_owned(),
    };

    let position = match node.entry("position") {
        Some(e) => Some(entry_to_dimension(e, "position")?),
        None => None,
    };

    Ok(Fold {
        id,
        orientation,
        position,
        source_span: node_span(node),
    })
}

/// Transform a `safe-zone` page child into a [`SafeZone`].
///
/// Reads required `id` and `x`/`y`/`w`/`h` dimensions; `type` maps to
/// [`SafeZoneType`] (`"exclusion"` → Exclusion, `"required"` → Required, any
/// other / absent value defaults to Exclusion); `label` is optional.
fn transform_safe_zone(node: &KdlNode) -> Result<SafeZone, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let zone_type = match optional_string_prop(node, "type") {
        Some("required") => SafeZoneType::Required,
        _ => SafeZoneType::Exclusion,
    };

    let x = node
        .entry("x")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("safe-zone `{id}` is missing required property `x`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "x"))?;
    let y = node
        .entry("y")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("safe-zone `{id}` is missing required property `y`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "y"))?;
    let w = node
        .entry("w")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("safe-zone `{id}` is missing required property `w`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "w"))?;
    let h = node
        .entry("h")
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("safe-zone `{id}` is missing required property `h`"),
            )
        })
        .and_then(|e| entry_to_dimension(e, "h"))?;

    let label = optional_string_prop(node, "label").map(str::to_owned);

    Ok(SafeZone {
        id,
        zone_type,
        x,
        y,
        w,
        h,
        label,
        source_span: node_span(node),
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
