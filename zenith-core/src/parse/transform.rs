//! KDL-node-tree → Zenith AST transform.
//!
//! All fallible helpers return `Result<_, ParseError>` so no `.unwrap()` or
//! `.expect()` appears anywhere in this file.

use std::collections::BTreeMap;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

use crate::ast::{
    Span,
    document::{Document, DocumentBody, Page, Project},
    node::{Node, RectNode, TextNode, TextSpan, UnknownNode, UnknownProperty, UnknownValue},
    style::{Style, StyleBlock},
    token::{Token, TokenBlock, TokenLiteral, TokenType, TokenValue},
    value::{Dimension, PropertyValue, Unit},
};
use crate::error::{ParseError, ParseErrorCode};

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
        _ => {
            // Treat as a literal, serialised to a string.
            let literal = kdl_value_to_literal_string(entry.value());
            Ok(PropertyValue::Literal(literal))
        }
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
    let value = match entry.value() {
        KdlValue::Integer(n) => *n as f64,
        KdlValue::Float(f) => *f,
        other => {
            return Err(ParseError::spanless(
                ParseErrorCode::InvalidPropertyValue,
                format!("property `{prop}` must be numeric, got: {other:?}"),
            ));
        }
    };
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

    let children_doc = zenith_node.children().ok_or_else(|| {
        ParseError::spanless(
            ParseErrorCode::MissingZenithRoot,
            "`zenith` node has no children block",
        )
    })?;

    let mut project: Option<Project> = None;
    let mut tokens = TokenBlock::default();
    let mut styles = StyleBlock::default();
    let mut body: Option<DocumentBody> = None;

    for child in children_doc.nodes() {
        match child.name().value() {
            "project" => {
                project = Some(transform_project(child)?);
            }
            "tokens" => {
                tokens = transform_tokens(child)?;
            }
            "styles" => {
                styles = transform_styles(child)?;
            }
            "document" => {
                body = Some(transform_document_body(child)?);
            }
            // `assets` and any other unknown top-level children are currently
            // accepted without error (forward-compat); they simply are not
            // represented in the v0 AST.
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
        project,
        tokens,
        styles,
        body,
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

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

fn transform_styles(node: &KdlNode) -> Result<StyleBlock, ParseError> {
    let mut style_list: Vec<Style> = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            if child.name().value() == "style" {
                let id = required_string_prop(child, "id")?.to_owned();
                style_list.push(Style { id });
            }
        }
    }
    Ok(StyleBlock { styles: style_list })
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

    let source_span = node_span(node);

    let mut children: Vec<Node> = Vec::new();
    if let Some(doc) = node.children() {
        for child in doc.nodes() {
            children.push(transform_node(child)?);
        }
    }

    Ok(Page {
        id,
        name,
        width,
        height,
        background,
        children,
        source_span,
    })
}

// ---------------------------------------------------------------------------
// Renderable nodes
// ---------------------------------------------------------------------------

fn transform_node(node: &KdlNode) -> Result<Node, ParseError> {
    match node.name().value() {
        "rect" => transform_rect(node).map(Node::Rect),
        "text" => transform_text(node).map(Node::Text),
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
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
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
    "style",
    "fill",
    "font-family",
    "font_family",
    "font-size",
    "font_size",
    "opacity",
    "visible",
    "locked",
    "rotate",
];

fn transform_text(node: &KdlNode) -> Result<TextNode, ParseError> {
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
        style: optional_string_prop(node, "style").map(str::to_owned),
        fill: optional_property_value(node, "fill"),
        font_family,
        font_size,
        opacity: optional_f64_prop(node, "opacity"),
        visible: optional_bool_prop(node, "visible"),
        locked: optional_bool_prop(node, "locked"),
        rotate: optional_dimension_prop(node, "rotate"),
        spans,
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

    Ok(TextSpan {
        text,
        fill,
        font_weight,
        italic,
        underline,
        strikethrough,
    })
}
