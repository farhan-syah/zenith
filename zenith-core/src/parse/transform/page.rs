//! Transforms for the page-structural nodes: page, fold, and safe-zone.
//!
//! These parsers handle the three KDL node kinds that appear as children of the
//! document body's `page` sequence and carry page-level geometry metadata
//! (dimensions, margins, bleed, baseline grid) together with inline structural
//! decorations (folds and safe-zones) that are separated from renderable
//! children before the main `transform_node` dispatch runs.

use kdl::KdlNode;

/// Canonical set of property names recognised on a `page` node.
///
/// Both the hyphenated spelling (canonical) and the underscored alias are
/// included for each attribute that accepts either form, matching the lenient
/// parser behaviour. Used by `zenith-core::schema` to surface the authorable
/// attribute list and by tests as a drift guard.
pub(crate) const PAGE_KNOWN_PROPS: &[&str] = &[
    "id",
    "name",
    "w",
    "h",
    "background",
    "bleed",
    "margin-inner",
    "margin_inner",
    "margin-outer",
    "margin_outer",
    "margin-top",
    "margin_top",
    "margin-bottom",
    "margin_bottom",
    "baseline-grid",
    "baseline_grid",
    "line-jumps",
    "line_jumps",
    "parity",
    "master",
];

use crate::ast::document::{Fold, Page, SafeZone, SafeZoneType};
use crate::ast::node::Node;
use crate::error::{ParseError, ParseErrorCode};

use super::helpers::{
    entry_to_dimension, entry_to_property_value, node_span, optional_dimension_prop,
    optional_string_prop, required_string_prop,
};
use super::node::transform_node;

pub(super) fn transform_page(node: &KdlNode) -> Result<Page, ParseError> {
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
