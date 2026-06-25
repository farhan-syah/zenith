//! Parser transform for `block role="…"` declarations.
//!
//! A `block` decl is a leaf child recognised at three scopes: document body,
//! page, and text node.  This module provides the single shared helper
//! [`transform_block_style`] used by all three parse sites.

use kdl::KdlNode;

use crate::ast::block_style::BlockStyle;
use crate::error::ParseError;

use super::helpers::{
    optional_bool_prop, optional_dimension_prop, optional_property_value,
    optional_property_value_aliased, optional_string_prop, required_string_prop,
};

/// Transform a `block role="…" …` KDL node into a [`BlockStyle`].
///
/// The only required field is `role`; all style/spacing fields are optional.
/// Unrecognized properties on a `block` node are silently ignored (the node is
/// a declaration-only construct with no unknown-props BTreeMap).
pub(super) fn transform_block_style(node: &KdlNode) -> Result<BlockStyle, ParseError> {
    let role = required_string_prop(node, "role")?.to_owned();

    let font_family = optional_property_value_aliased(node, "font-family", "font_family");
    let font_size = optional_property_value_aliased(node, "font-size", "font_size");
    let font_weight = optional_property_value_aliased(node, "font-weight", "font_weight");
    let fill = optional_property_value(node, "fill");
    let align = optional_string_prop(node, "align").map(str::to_owned);
    let italic = optional_bool_prop(node, "italic");
    let space_before = optional_dimension_prop(node, "space-before")
        .or_else(|| optional_dimension_prop(node, "space_before"));
    let space_after = optional_dimension_prop(node, "space-after")
        .or_else(|| optional_dimension_prop(node, "space_after"));

    Ok(BlockStyle {
        role,
        font_family,
        font_size,
        font_weight,
        fill,
        align,
        italic,
        space_before,
        space_after,
    })
}
