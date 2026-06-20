//! KDL-node-tree → Zenith AST transform.
//!
//! All fallible helpers return `Result<_, ParseError>` so no `.unwrap()` or
//! `.expect()` appears anywhere in this file.

use std::collections::BTreeMap;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

use crate::ast::{
    Span,
    asset::{AssetBlock, AssetDecl, AssetKind},
    document::{
        ComponentDef, Document, DocumentBody, Fold, MasterDef, Page, Project, SafeZone,
        SafeZoneType,
    },
    node::{
        CodeNode, EllipseNode, FieldNode, FootnoteNode, FrameNode, GroupNode, ImageNode,
        InstanceNode, LineNode, Node, ObjectPosition, Override, Point, PolygonNode, PolylineNode,
        RectNode, TextNode, TextSpan, UnknownNode, UnknownProperty, UnknownValue,
    },
    style::{Style, StyleBlock, UnknownStyleProp},
    token::{
        GradientLiteral, GradientStopRef, ShadowLayerRef, ShadowLiteral, Token, TokenBlock,
        TokenLiteral, TokenType, TokenValue,
    },
    value::{Dimension, PropertyValue, Unit},
};
use crate::error::{ParseError, ParseErrorCode};
use crate::tokens::SyntaxTheme;

// ---------------------------------------------------------------------------
// Span helpers
// ---------------------------------------------------------------------------

fn node_span(node: &KdlNode) -> Option<Span> {
    // `KdlNode::span()` returns `miette::SourceSpan` (a transitive type from the
    // `kdl` crate). We read its offset/len via inherent methods and convert at
    // this boundary so the external span type never leaks past the parser.
    let span = node.span();
    let start = span.offset();
    Some(Span {
        start,
        end: start + span.len(),
    })
}

// ---------------------------------------------------------------------------
// Value extraction helpers
// ---------------------------------------------------------------------------

/// Extract the type annotation string from a `KdlEntry`, if present.
fn entry_annotation(entry: &KdlEntry) -> Option<&str> {
    entry.ty().map(|id| id.value())
}

/// Convert a `KdlEntry` that carries an annotated or plain value into a
/// `PropertyValue`, handling `(token)"..."` annotations.
fn entry_to_property_value(entry: &KdlEntry) -> Result<PropertyValue, ParseError> {
    let annotation = entry_annotation(entry);
    match annotation {
        Some("token") => match entry.value() {
            KdlValue::String(s) => Ok(PropertyValue::TokenRef(s.clone())),
            other => Err(ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("(token) annotation requires a string value, got: {other:?}"),
            )),
        },
        // A known/unknown unit annotation on a numeric value → dimension literal.
        // This brings literal visual dimensions (e.g. `font-size=(px)24`) to
        // parity with token-backed dimensions. Non-numeric annotated values fall
        // through to the literal branch unchanged.
        Some(ann) => match kdl_value_to_f64(entry.value()) {
            Some(value) => Ok(PropertyValue::Dimension(Dimension {
                value,
                unit: Unit::from_annotation(ann),
            })),
            None => Ok(PropertyValue::Literal(kdl_value_to_literal_string(
                entry.value(),
            ))),
        },
        None => {
            // Treat as a literal, serialised to a string.
            let literal = kdl_value_to_literal_string(entry.value());
            Ok(PropertyValue::Literal(literal))
        }
    }
}

/// Extract an `f64` magnitude from a numeric `KdlValue` (`Integer`/`Float`).
///
/// Returns `None` for non-numeric values. Shared by the dimension extraction in
/// both the geometry and visual-property parse paths so the `KdlValue → f64`
/// conversion lives in exactly one place.
fn kdl_value_to_f64(v: &KdlValue) -> Option<f64> {
    match v {
        KdlValue::Integer(n) => Some(*n as f64),
        KdlValue::Float(f) => Some(*f),
        _ => None,
    }
}

fn kdl_value_to_literal_string(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(n) => n.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => "null".to_owned(),
    }
}

/// Convert a `KdlEntry` that carries a dimensioned number (e.g. `(px)640`)
/// into a `Dimension`.
fn entry_to_dimension(entry: &KdlEntry, prop: &str) -> Result<Dimension, ParseError> {
    let unit_str = entry_annotation(entry).ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::InvalidPropertyValue,
            format!("property `{prop}` requires a unit annotation such as (px) or (pt)"),
        )
    })?;
    let unit = Unit::from_annotation(unit_str);
    let value = kdl_value_to_f64(entry.value()).ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::InvalidPropertyValue,
            format!(
                "property `{prop}` must be numeric, got: {:?}",
                entry.value()
            ),
        )
    })?;
    Ok(Dimension { value, unit })
}

/// Get a required string property value from a node.
fn required_string_prop<'a>(node: &'a KdlNode, key: &str) -> Result<&'a str, ParseError> {
    node.get(key)
        .and_then(|v| {
            if let KdlValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!(
                    "node `{}` is missing required string property `{key}`",
                    node.name().value()
                ),
            )
        })
}

/// Get a required integer property from a node and convert to u32.
fn required_u32_prop(node: &KdlNode, key: &str) -> Result<u32, ParseError> {
    node.get(key)
        .and_then(|v| {
            if let KdlValue::Integer(n) = v {
                u32::try_from(*n).ok()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!(
                    "node `{}` is missing required integer property `{key}`",
                    node.name().value()
                ),
            )
        })
}

/// Extract an optional non-negative integer property and convert to u32.
///
/// Absent properties, non-integer values, and out-of-range/negative integers
/// (which fail `u32::try_from`) all yield `None`.
fn optional_u32_prop(node: &KdlNode, key: &str) -> Option<u32> {
    node.get(key).and_then(|v| {
        if let KdlValue::Integer(n) = v {
            u32::try_from(*n).ok()
        } else {
            None
        }
    })
}

/// Extract an optional boolean property value from a node.
///
/// KDL v2 booleans are the `KdlValue::Bool` variant (`#true`/`#false`).
fn optional_bool_prop(node: &KdlNode, key: &str) -> Option<bool> {
    node.get(key).and_then(|v| {
        if let KdlValue::Bool(b) = v {
            Some(*b)
        } else {
            None
        }
    })
}

/// Extract an optional f64 property.
fn optional_f64_prop(node: &KdlNode, key: &str) -> Option<f64> {
    node.get(key).and_then(|v| match v {
        KdlValue::Float(f) => Some(*f),
        KdlValue::Integer(n) => Some(*n as f64),
        _ => None,
    })
}

/// Extract an optional string property.
fn optional_string_prop<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.get(key).and_then(|v| {
        if let KdlValue::String(s) = v {
            Some(s.as_str())
        } else {
            None
        }
    })
}

/// Extract an optional dimension property from a node's entries.
fn optional_dimension_prop(node: &KdlNode, key: &str) -> Option<Dimension> {
    let entry = node.entry(key)?;
    entry_to_dimension(entry, key).ok()
}

/// Extract an optional object-position property from a node.
///
/// Accepts EITHER a plain string anchor (`"start"`/`"center"`/`"end"`) OR a
/// KDL `(pct)N` annotated number → `ObjectPosition::Pct(N)`. Any other string
/// or shape yields `None` (the property is simply absent / unrecognized).
fn optional_object_position_prop(node: &KdlNode, key: &str) -> Option<ObjectPosition> {
    let entry = node.entry(key)?;
    // A `(pct)N` annotated number → Pct(N).
    if entry_annotation(entry) == Some("pct") {
        let value = match entry.value() {
            KdlValue::Integer(n) => *n as f64,
            KdlValue::Float(f) => *f,
            _ => return None,
        };
        return Some(ObjectPosition::Pct(value));
    }
    // Otherwise a plain string anchor.
    match entry.value() {
        KdlValue::String(s) => match s.as_str() {
            "start" => Some(ObjectPosition::Start),
            "center" => Some(ObjectPosition::Center),
            "end" => Some(ObjectPosition::End),
            _ => None,
        },
        _ => None,
    }
}

/// Extract an optional property value (token ref or literal) from a node.
fn optional_property_value(node: &KdlNode, key: &str) -> Option<PropertyValue> {
    let entry = node.entry(key)?;
    entry_to_property_value(entry).ok()
}

/// Try `primary_key` first, then `alias_key` (supports both hyphenated and
/// underscored spellings of the same property).
fn optional_property_value_aliased(
    node: &KdlNode,
    primary_key: &str,
    alias_key: &str,
) -> Option<PropertyValue> {
    optional_property_value(node, primary_key).or_else(|| optional_property_value(node, alias_key))
}

/// Try `primary_key` first, then `alias_key` for string props.
fn optional_string_prop_aliased<'a>(
    node: &'a KdlNode,
    primary_key: &str,
    alias_key: &str,
) -> Option<&'a str> {
    optional_string_prop(node, primary_key).or_else(|| optional_string_prop(node, alias_key))
}

/// Map a `KdlValue` to its `UnknownValue` counterpart, preserving type.
fn kdl_value_to_unknown_value(v: &KdlValue) -> UnknownValue {
    match v {
        KdlValue::String(s) => UnknownValue::String(s.clone()),
        KdlValue::Integer(n) => UnknownValue::Integer(*n),
        KdlValue::Float(f) => UnknownValue::Float(*f),
        KdlValue::Bool(b) => UnknownValue::Bool(*b),
        KdlValue::Null => UnknownValue::Null,
    }
}

/// Collect all entries that are NOT in `known_keys` into `unknown_props`.
fn collect_unknown_props(node: &KdlNode, known_keys: &[&str]) -> BTreeMap<String, UnknownProperty> {
    let mut map = BTreeMap::new();
    for entry in node.entries() {
        if let Some(name_id) = entry.name() {
            let key = name_id.value();
            if !known_keys.contains(&key) {
                map.insert(
                    key.to_owned(),
                    UnknownProperty {
                        value: kdl_value_to_unknown_value(entry.value()),
                    },
                );
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Top-level transform entry point
// ---------------------------------------------------------------------------

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
    let mut tokens = TokenBlock::default();
    let mut styles = StyleBlock::default();
    let mut components: Vec<ComponentDef> = Vec::new();
    let mut masters: Vec<MasterDef> = Vec::new();
    let mut body: Option<DocumentBody> = None;

    for child in children_doc.nodes() {
        match child.name().value() {
            "project" => {
                project = Some(transform_project(child)?);
            }
            "assets" => {
                assets = transform_assets(child)?;
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
        mirror_margins,
        page_progression,
        page_parity_start,
        margin_inner,
        margin_outer,
        margin_top,
        margin_bottom,
        project,
        assets,
        tokens,
        styles,
        components,
        masters,
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

const ASSET_KNOWN_PROPS: &[&str] = &["id", "kind", "src", "sha256"];

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
    let unknown_props = collect_unknown_props(node, ASSET_KNOWN_PROPS);
    let source_span = node_span(node);

    Ok(AssetDecl {
        id,
        kind,
        src,
        sha256,
        source_span,
        unknown_props,
    })
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

fn transform_tokens(node: &KdlNode) -> Result<TokenBlock, ParseError> {
    let format = required_string_prop(node, "format")?.to_owned();

    let mut token_list: Vec<Token> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "token" {
                token_list.push(transform_token(child)?);
            }
        }
    }

    Ok(TokenBlock {
        format,
        tokens: token_list,
    })
}

fn transform_token(node: &KdlNode) -> Result<Token, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let type_str = required_string_prop(node, "type")?;
    let token_type = TokenType::from_type_name(type_str);

    // Gradient tokens carry no scalar `value=`; they are built from an optional
    // `angle=(deg)N` prop plus child `stop` nodes. Prefer this child-node form
    // even if a stray `value=` entry is also present.
    if token_type == TokenType::Gradient {
        let token_value = transform_gradient(node);
        let source_span = node_span(node);
        return Ok(Token {
            id,
            token_type,
            value: token_value,
            source_span,
        });
    }

    // Shadow tokens carry no scalar `value=`; they are built from child `layer`
    // nodes. Prefer this child-node form even if a stray `value=` is present.
    if token_type == TokenType::Shadow {
        let token_value = transform_shadow(node);
        let source_span = node_span(node);
        return Ok(Token {
            id,
            token_type,
            value: token_value,
            source_span,
        });
    }

    let value_entry = node.entry("value").ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::InvalidPropertyValue,
            format!("token `{id}` is missing required property `value`"),
        )
    })?;

    let token_value = match entry_annotation(value_entry) {
        Some("token") => match value_entry.value() {
            KdlValue::String(s) => TokenValue::Reference {
                token_id: s.clone(),
            },
            other => {
                return Err(ParseError::spanless(
                    ParseErrorCode::InvalidPropertyValue,
                    format!("token `{id}` has (token) annotation but non-string value: {other:?}"),
                ));
            }
        },
        Some(unit_str) => {
            // Annotated number → dimension literal.
            let unit = Unit::from_annotation(unit_str);
            let numeric = match value_entry.value() {
                KdlValue::Integer(n) => *n as f64,
                KdlValue::Float(f) => *f,
                other => {
                    return Err(ParseError::spanless(
                        ParseErrorCode::InvalidPropertyValue,
                        format!(
                            "token `{id}` has unit annotation but non-numeric value: {other:?}"
                        ),
                    ));
                }
            };
            TokenValue::Literal(TokenLiteral::Dimension(Dimension {
                value: numeric,
                unit,
            }))
        }
        None => {
            let literal = match value_entry.value() {
                KdlValue::String(s) => TokenLiteral::String(s.clone()),
                KdlValue::Integer(n) => TokenLiteral::Number(*n as f64),
                KdlValue::Float(f) => TokenLiteral::Number(*f),
                other => {
                    return Err(ParseError::spanless(
                        ParseErrorCode::InvalidPropertyValue,
                        format!("token `{id}` has unsupported value type: {other:?}"),
                    ));
                }
            };
            TokenValue::Literal(literal)
        }
    };

    let source_span = node_span(node);
    Ok(Token {
        id,
        token_type,
        value: token_value,
        source_span,
    })
}

/// Default gradient angle (degrees, clockwise from +x) when `angle=` is absent
/// or cannot be read as a dimension: 90 = top→bottom.
const DEFAULT_GRADIENT_ANGLE_DEG: f64 = 90.0;

/// Read the `color=(token)"id"` stop entry as a color token id.
///
/// Reuses the `(token)` annotation idiom from `entry_to_property_value`: only a
/// `token`-annotated string entry yields a stop color. Any other shape (missing,
/// unannotated, non-string) yields `None` so the stop is skipped.
fn stop_color_token(node: &KdlNode) -> Option<String> {
    let entry = node.entry("color")?;
    match (entry_annotation(entry), entry.value()) {
        (Some("token"), KdlValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Build a gradient `TokenValue` from a `token` node's `angle` prop and `stop`
/// children. Infallible: a malformed gradient simply yields fewer/zero stops,
/// which the resolver later reports via `gradient.too_few_stops`.
fn transform_gradient(node: &KdlNode) -> TokenValue {
    // `angle=(deg)N` is read like other `(deg)` dimensions: take the dimension
    // `.value` directly as degrees (no dim_to_px conversion). Absent or
    // unparseable → default.
    let angle_deg = optional_dimension_prop(node, "angle")
        .map(|d| d.value)
        .unwrap_or(DEFAULT_GRADIENT_ANGLE_DEG);

    let mut stops: Vec<GradientStopRef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() != "stop" {
                continue;
            }
            // A stop without a usable color token ref is meaningless; skip it.
            let Some(color_token) = stop_color_token(child) else {
                continue;
            };
            let offset = optional_f64_prop(child, "offset").unwrap_or(0.0);
            stops.push(GradientStopRef {
                offset,
                color_token,
            });
        }
    }

    TokenValue::Literal(TokenLiteral::Gradient(GradientLiteral { angle_deg, stops }))
}

/// Read the `color=(token)"id"` layer entry as a color token id.
///
/// Mirrors `stop_color_token`: only a `token`-annotated string entry yields a
/// layer color. Any other shape yields `None` so the layer is skipped.
fn layer_color_token(node: &KdlNode) -> Option<String> {
    let entry = node.entry("color")?;
    match (entry_annotation(entry), entry.value()) {
        (Some("token"), KdlValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Build a shadow `TokenValue` from a `token` node's `layer` children. Each
/// layer reads `dx`/`dy`/`blur` as `(px)` dimensions (pixel value taken
/// directly; absent → 0) plus a `color=(token)"id"` color token id. Infallible:
/// a malformed shadow simply yields fewer/zero layers, which the resolver later
/// reports via `shadow.no_layers`.
fn transform_shadow(node: &KdlNode) -> TokenValue {
    let mut layers: Vec<ShadowLayerRef> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() != "layer" {
                continue;
            }
            // A layer without a usable color token ref is meaningless; skip it.
            let Some(color_token) = layer_color_token(child) else {
                continue;
            };
            let dx = optional_dimension_prop(child, "dx")
                .map(|d| d.value)
                .unwrap_or(0.0);
            let dy = optional_dimension_prop(child, "dy")
                .map(|d| d.value)
                .unwrap_or(0.0);
            let blur = optional_dimension_prop(child, "blur")
                .map(|d| d.value)
                .unwrap_or(0.0);
            layers.push(ShadowLayerRef {
                dx,
                dy,
                blur,
                color_token,
            });
        }
    }

    TokenValue::Literal(TokenLiteral::Shadow(ShadowLiteral { layers }))
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

/// Canonical hyphenated keys for recognized style visual properties.
///
/// Underscore variants are normalized to these forms during parsing.
const STYLE_RECOGNIZED_KEYS: &[&str] = &[
    "fill",
    "stroke",
    "stroke-width",
    "stroke-alignment",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "radius",
    "padding",
    "gap",
];

/// Map underscore-spelled style child names to their canonical hyphenated form.
///
/// Returns `None` if the name is not in the recognized set (after
/// normalization).
fn canonicalize_style_key(name: &str) -> Option<&'static str> {
    // Normalize underscore to hyphen for comparison.
    let normalized: &str = match name {
        "stroke_width" => "stroke-width",
        "stroke_alignment" => "stroke-alignment",
        "font_family" => "font-family",
        "font_size" => "font-size",
        "font_weight" => "font-weight",
        "line_height" => "line-height",
        other => other,
    };
    STYLE_RECOGNIZED_KEYS
        .iter()
        .copied()
        .find(|&k| k == normalized)
}

fn transform_styles(node: &KdlNode) -> Result<StyleBlock, ParseError> {
    let source_span = node_span(node);
    let mut style_list: Vec<Style> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "style" {
                let id = required_string_prop(child, "id")?.to_owned();
                let style_source_span = node_span(child);

                let mut properties: BTreeMap<String, PropertyValue> = BTreeMap::new();
                let mut unknown_props: BTreeMap<String, UnknownStyleProp> = BTreeMap::new();

                // Each child node of the `style` node is a property declaration.
                // Its NAME is the property key; its FIRST positional argument
                // is the value (e.g. `fill (token)"color.text.primary"`).
                if let Some(prop_nodes) = child.children() {
                    for prop_node in prop_nodes.nodes() {
                        let prop_name = prop_node.name().value();
                        if let Some(canonical) = canonicalize_style_key(prop_name) {
                            // Read the first positional (unnamed) entry as a PropertyValue.
                            let first_positional =
                                prop_node.entries().iter().find(|e| e.name().is_none());
                            if let Some(entry) = first_positional
                                && let Ok(pv) = entry_to_property_value(entry)
                            {
                                properties.insert(canonical.to_owned(), pv);
                            }
                        } else {
                            // Unrecognized property: preserve for validator warnings.
                            let raw = prop_node
                                .entries()
                                .iter()
                                .find(|e| e.name().is_none())
                                .map(|e| kdl_value_to_literal_string(e.value()))
                                .unwrap_or_default();
                            unknown_props.insert(prop_name.to_owned(), UnknownStyleProp { raw });
                        }
                    }
                }

                style_list.push(Style {
                    id,
                    properties,
                    unknown_props,
                    source_span: style_source_span,
                });
            }
        }
    }

    Ok(StyleBlock {
        styles: style_list,
        source_span,
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
fn transform_children(node: &KdlNode) -> Result<Vec<Node>, ParseError> {
    let mut children: Vec<Node> = Vec::new();
    if let Some(doc) = node.children() {
        for child in doc.nodes() {
            children.push(transform_node(child)?);
        }
    }
    Ok(children)
}

// ---------------------------------------------------------------------------
// Renderable nodes
// ---------------------------------------------------------------------------

fn transform_node(node: &KdlNode) -> Result<Node, ParseError> {
    match node.name().value() {
        "rect" => transform_rect(node).map(Node::Rect),
        "ellipse" => transform_ellipse(node).map(Node::Ellipse),
        "line" => transform_line(node).map(Node::Line),
        "text" => transform_text(node).map(|t| Node::Text(Box::new(t))),
        "code" => transform_code(node).map(Node::Code),
        "frame" => transform_frame(node).map(Node::Frame),
        "group" => transform_group(node).map(Node::Group),
        "image" => transform_image(node).map(Node::Image),
        "polygon" => transform_polygon(node).map(Node::Polygon),
        "polyline" => transform_polyline(node).map(Node::Polyline),
        "instance" => transform_instance(node).map(Node::Instance),
        "field" => transform_field(node).map(Node::Field),
        "footnote" => transform_footnote(node).map(Node::Footnote),
        _ => Ok(Node::Unknown(UnknownNode {
            kind: node.name().value().to_owned(),
            source_span: node_span(node),
        })),
    }
}

const RECT_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "radius",
    "style",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "stroke-alignment",
    "stroke_alignment",
    "shadow",
    "opacity",
    "visible",
    "locked",
    "rotate",
];

fn transform_rect(node: &KdlNode) -> Result<RectNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_alignment =
        optional_string_prop_aliased(node, "stroke-alignment", "stroke_alignment")
            .map(str::to_owned);

    let unknown_props = collect_unknown_props(node, RECT_KNOWN_PROPS);

    Ok(RectNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        radius: optional_property_value(node, "radius"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_alignment,
        shadow: optional_property_value(node, "shadow"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        source_span: node_span(node),
        unknown_props,
    })
}

const IMAGE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "asset",
    "x",
    "y",
    "w",
    "h",
    "fit",
    "clip",
    "clip-radius",
    "clip_radius",
    "object-position-x",
    "object_position_x",
    "object-position-y",
    "object_position_y",
    "shadow",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

fn transform_image(node: &KdlNode) -> Result<ImageNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let asset = required_string_prop(node, "asset")?.to_owned();

    // object-position accepts hyphenated or underscored spellings.
    let object_position_x = optional_object_position_prop(node, "object-position-x")
        .or_else(|| optional_object_position_prop(node, "object_position_x"));
    let object_position_y = optional_object_position_prop(node, "object-position-y")
        .or_else(|| optional_object_position_prop(node, "object_position_y"));

    let unknown_props = collect_unknown_props(node, IMAGE_KNOWN_PROPS);

    Ok(ImageNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        asset,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        fit: optional_string_prop(node, "fit").map(str::to_owned),
        clip: optional_string_prop(node, "clip").map(str::to_owned),
        clip_radius: optional_property_value_aliased(node, "clip-radius", "clip_radius"),
        object_position_x,
        object_position_y,
        shadow: optional_property_value(node, "shadow"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        source_span: node_span(node),
        unknown_props,
    })
}

const ELLIPSE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "style",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "shadow",
    "opacity",
    "visible",
    "locked",
    "rotate",
];

fn transform_ellipse(node: &KdlNode) -> Result<EllipseNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");

    let unknown_props = collect_unknown_props(node, ELLIPSE_KNOWN_PROPS);

    Ok(EllipseNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        shadow: optional_property_value(node, "shadow"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        source_span: node_span(node),
        unknown_props,
    })
}

const LINE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x1",
    "y1",
    "x2",
    "y2",
    "style",
    "stroke",
    "stroke-width",
    "stroke_width",
    "opacity",
    "visible",
    "locked",
    // NOTE: "stroke-alignment" is intentionally absent — it does not apply to
    // line nodes. An author who writes it will receive a node.unknown_property
    // warning, which is the correct diagnostic for inapplicable properties.
];

fn transform_line(node: &KdlNode) -> Result<LineNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    // Handle both hyphenated and underscored variants for forward-compat.
    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");

    let unknown_props = collect_unknown_props(node, LINE_KNOWN_PROPS);

    Ok(LineNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x1: optional_dimension_prop(node, "x1"),
        y1: optional_dimension_prop(node, "y1"),
        x2: optional_dimension_prop(node, "x2"),
        y2: optional_dimension_prop(node, "y2"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        source_span: node_span(node),
        unknown_props,
    })
}

const TEXT_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "align",
    "direction",
    "overflow",
    "overflow-wrap",
    "overflow_wrap",
    "style",
    "fill",
    "contrast-bg",
    "contrast_bg",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "font-size-min",
    "font_size_min",
    "font-weight",
    "font_weight",
    "shadow",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "chain",
    "drop-cap-lines",
    "drop_cap_lines",
    "hyphenate",
    "widow-orphan",
    "widow_orphan",
    "tab-leader",
    "tab_leader",
    "text-exclusion",
    "text_exclusion",
    "padding-left",
    "padding_left",
    "text-indent",
    "text_indent",
];

fn transform_text(node: &KdlNode) -> Result<TextNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let font_size_min = optional_property_value_aliased(node, "font-size-min", "font_size_min");
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let drop_cap_lines = optional_u32_prop(node, "drop-cap-lines")
        .or_else(|| optional_u32_prop(node, "drop_cap_lines"));
    let hyphenate = optional_bool_prop(node, "hyphenate");
    let widow_orphan =
        optional_u32_prop(node, "widow-orphan").or_else(|| optional_u32_prop(node, "widow_orphan"));
    let tab_leader = optional_string_prop(node, "tab-leader")
        .or_else(|| optional_string_prop(node, "tab_leader"))
        .map(str::to_owned);
    let text_exclusion = optional_string_prop(node, "text-exclusion")
        .or_else(|| optional_string_prop(node, "text_exclusion"))
        .map(str::to_owned);
    // Optional signed geometry dimensions (text-indent may be NEGATIVE for a
    // hanging indent). Accept both hyphenated and underscored spellings.
    let padding_left = optional_dimension_prop(node, "padding-left")
        .or_else(|| optional_dimension_prop(node, "padding_left"));
    let text_indent = optional_dimension_prop(node, "text-indent")
        .or_else(|| optional_dimension_prop(node, "text_indent"));

    let mut spans: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                spans.push(transform_span(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, TEXT_KNOWN_PROPS);

    Ok(TextNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        align: optional_string_prop(node, "align").map(str::to_owned),
        direction: optional_string_prop(node, "direction").map(str::to_owned),
        overflow: optional_string_prop(node, "overflow").map(str::to_owned),
        overflow_wrap: optional_string_prop(node, "overflow-wrap")
            .or_else(|| optional_string_prop(node, "overflow_wrap"))
            .map(str::to_owned),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        contrast_bg: optional_property_value_aliased(node, "contrast-bg", "contrast_bg"),
        font_family,
        font_size,
        font_size_min,
        font_weight,
        shadow: optional_property_value(node, "shadow"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        chain: optional_string_prop(node, "chain").map(str::to_owned),
        drop_cap_lines,
        hyphenate,
        widow_orphan,
        tab_leader,
        text_exclusion,
        padding_left,
        text_indent,
        spans,
        source_span: node_span(node),
        unknown_props,
    })
}

const CODE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "x",
    "y",
    "w",
    "h",
    "overflow",
    "language",
    "line-numbers",
    "line_numbers",
    "tab-width",
    "tab_width",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "font-weight",
    "font_weight",
    "syntax-theme",
    "syntax_theme",
    "opacity",
    "visible",
    "locked",
    "rotate",
];

fn transform_code(node: &KdlNode) -> Result<CodeNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let line_numbers = optional_bool_prop(node, "line-numbers")
        .or_else(|| optional_bool_prop(node, "line_numbers"));
    let tab_width =
        optional_u32_prop(node, "tab-width").or_else(|| optional_u32_prop(node, "tab_width"));
    let syntax_theme = optional_string_prop(node, "syntax-theme")
        .or_else(|| optional_string_prop(node, "syntax_theme"))
        .and_then(SyntaxTheme::from_name);

    // The verbatim source is carried by a `content` child node whose first
    // positional argument is the DECODED string. KDL v2 multi-line string
    // dedent rules make a bare `r#"..."#` form lossy, so the carrier uses a
    // single-line escaped string which round-trips `\n \t \" \\` exactly.
    // Stored decoded here; `write_code` re-encodes the escapes.
    let mut content = String::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "content" {
                if let Some(KdlValue::String(s)) = child.get(0) {
                    content = s.clone();
                }
                break;
            }
        }
    }

    let unknown_props = collect_unknown_props(node, CODE_KNOWN_PROPS);

    Ok(CodeNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        overflow: optional_string_prop(node, "overflow").map(str::to_owned),
        language: optional_string_prop(node, "language").map(str::to_owned),
        line_numbers,
        tab_width,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        font_weight,
        syntax_theme,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        content,
        source_span: node_span(node),
        unknown_props,
    })
}

const FRAME_KNOWN_PROPS: &[&str] = &[
    "id", "name", "role", "x", "y", "w", "h", "layout", "columns", "rows", "opacity", "visible",
    "locked", "rotate", "style",
];

fn transform_frame(node: &KdlNode) -> Result<FrameNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let unknown_props = collect_unknown_props(node, FRAME_KNOWN_PROPS);

    Ok(FrameNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        layout: optional_string_prop(node, "layout").map(str::to_owned),
        columns: optional_u32_prop(node, "columns"),
        rows: optional_u32_prop(node, "rows"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        children: transform_children(node)?,
        source_span: node_span(node),
        unknown_props,
    })
}

const GROUP_KNOWN_PROPS: &[&str] = &[
    "id", "name", "role", "x", "y", "w", "h", "opacity", "visible", "locked", "rotate", "style",
];

fn transform_group(node: &KdlNode) -> Result<GroupNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let unknown_props = collect_unknown_props(node, GROUP_KNOWN_PROPS);

    Ok(GroupNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        children: transform_children(node)?,
        source_span: node_span(node),
        unknown_props,
    })
}

const POLYGON_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "stroke-alignment",
    "stroke_alignment",
    "fill-rule",
    "fill_rule",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

// NOTE: polyline intentionally omits stroke-alignment (doc 09) — an author
// writing it gets a node.unknown_property warning, which is correct.
const POLYLINE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "fill",
    "stroke",
    "stroke-width",
    "stroke_width",
    "fill-rule",
    "fill_rule",
    "opacity",
    "visible",
    "locked",
    "rotate",
    "style",
];

/// Transform a `point` child node into a [`Point`].
///
/// `x` and `y` are optional at parse time; validate checks their presence.
fn transform_point(node: &KdlNode) -> Point {
    Point {
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
    }
}

fn transform_polygon(node: &KdlNode) -> Result<PolygonNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let stroke_alignment =
        optional_string_prop_aliased(node, "stroke-alignment", "stroke_alignment")
            .map(str::to_owned);
    let fill_rule = optional_string_prop_aliased(node, "fill-rule", "fill_rule").map(str::to_owned);

    // Collect `point` child nodes — this is where the vertex list lives.
    let mut points: Vec<Point> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "point" {
                points.push(transform_point(child));
            }
        }
    }

    let unknown_props = collect_unknown_props(node, POLYGON_KNOWN_PROPS);

    Ok(PolygonNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        stroke_alignment,
        fill_rule,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        points,
        source_span: node_span(node),
        unknown_props,
    })
}

fn transform_polyline(node: &KdlNode) -> Result<PolylineNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let stroke_width = optional_property_value_aliased(node, "stroke-width", "stroke_width");
    let fill_rule = optional_string_prop_aliased(node, "fill-rule", "fill_rule").map(str::to_owned);

    // Collect `point` child nodes.
    let mut points: Vec<Point> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "point" {
                points.push(transform_point(child));
            }
        }
    }

    let unknown_props = collect_unknown_props(node, POLYLINE_KNOWN_PROPS);

    Ok(PolylineNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        stroke: optional_property_value(node, "stroke"),
        stroke_width,
        fill_rule,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        points,
        source_span: node_span(node),
        unknown_props,
    })
}

const INSTANCE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "component",
    "x",
    "y",
    "opacity",
    "visible",
    "locked",
];

/// Transform an `instance` node into an [`InstanceNode`].
///
/// Reads required `id` and `component`; optional `x`/`y` origin dimensions,
/// `opacity`/`visible`/`locked`; and collects `override` child nodes into
/// [`Override`]s. Non-`override` children are ignored (forward-compat).
fn transform_instance(node: &KdlNode) -> Result<InstanceNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let component = required_string_prop(node, "component")?.to_owned();

    let mut overrides: Vec<Override> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "override" {
                overrides.push(transform_override(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, INSTANCE_KNOWN_PROPS);

    Ok(InstanceNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        component,
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        overrides,
        source_span: node_span(node),
        unknown_props,
    })
}

/// Transform an `override ref="..." { … }` instance child into an [`Override`].
///
/// `ref` (required) names a component-LOCAL descendant id. Supported v0 override
/// payload: `span` children (→ `spans`), a `fill` prop, and a `visible` prop.
fn transform_override(node: &KdlNode) -> Result<Override, ParseError> {
    let ref_id = required_string_prop(node, "ref")?.to_owned();

    // Collect `span` children; only set `spans` when at least one is present so
    // an override that does not touch text leaves the target's spans untouched.
    let mut span_list: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                span_list.push(transform_span(child)?);
            }
        }
    }
    let spans = if span_list.is_empty() {
        None
    } else {
        Some(span_list)
    };

    Ok(Override {
        ref_id,
        spans,
        fill: optional_property_value(node, "fill"),
        visible: optional_bool_prop(node, "visible"),
        source_span: node_span(node),
    })
}

const FIELD_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "type",
    "recto",
    "verso",
    "target",
    "x",
    "y",
    "w",
    "h",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "opacity",
    "visible",
    "locked",
];

/// Transform a `field` node into a [`FieldNode`].
///
/// Reads required `id` and `type`; optional `recto`/`verso`/`target` strings;
/// optional `x`/`y`/`w`/`h` geometry; and visual props (`style`/`fill`/
/// `font-family`/`font-size`). The `type` value is preserved verbatim (the
/// validator warns on an unknown type), as is `target` (the compiler resolves
/// the page-ref and the validator warns on an unresolved one).
fn transform_field(node: &KdlNode) -> Result<FieldNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();
    let field_type = required_string_prop(node, "type")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");

    let unknown_props = collect_unknown_props(node, FIELD_KNOWN_PROPS);

    Ok(FieldNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        field_type,
        recto: optional_string_prop(node, "recto").map(str::to_owned),
        verso: optional_string_prop(node, "verso").map(str::to_owned),
        target: optional_string_prop(node, "target").map(str::to_owned),
        x: optional_dimension_prop(node, "x"),
        y: optional_dimension_prop(node, "y"),
        w: optional_dimension_prop(node, "w"),
        h: optional_dimension_prop(node, "h"),
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        source_span: node_span(node),
        unknown_props,
    })
}

const FOOTNOTE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "role",
    "marker",
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
];

/// Transform a `footnote` node into a [`FootnoteNode`].
///
/// Reads the required `id`; the optional `marker` override; visual props
/// (`style`/`fill`/`font-family`/`font-size`); and the content `span` children
/// (the same span model a `text` node uses). A footnote has NO geometry.
fn transform_footnote(node: &KdlNode) -> Result<FootnoteNode, ParseError> {
    let id = required_string_prop(node, "id")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");

    let mut spans: Vec<TextSpan> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "span" {
                spans.push(transform_span(child)?);
            }
        }
    }

    let unknown_props = collect_unknown_props(node, FOOTNOTE_KNOWN_PROPS);

    Ok(FootnoteNode {
        id,
        name: optional_string_prop(node, "name").map(str::to_owned),
        role: optional_string_prop(node, "role").map(str::to_owned),
        marker: optional_string_prop(node, "marker").map(str::to_owned),
        spans,
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        source_span: node_span(node),
        unknown_props,
    })
}

fn transform_span(node: &KdlNode) -> Result<TextSpan, ParseError> {
    // First positional argument is the text content.
    let text = node
        .get(0)
        .and_then(|v| {
            if let KdlValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                "`span` node must have a string argument as its first value",
            )
        })?;

    let fill = node
        .entry("fill")
        .and_then(|e| entry_to_property_value(e).ok());
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let italic = optional_bool_prop(node, "italic");
    let underline = optional_bool_prop(node, "underline");
    let strikethrough = optional_bool_prop(node, "strikethrough");
    let vertical_align =
        optional_string_prop_aliased(node, "vertical-align", "vertical_align").map(str::to_owned);
    let footnote_ref =
        optional_string_prop_aliased(node, "footnote-ref", "footnote_ref").map(str::to_owned);

    Ok(TextSpan {
        text,
        fill,
        font_weight,
        italic,
        underline,
        strikethrough,
        vertical_align,
        footnote_ref,
    })
}
