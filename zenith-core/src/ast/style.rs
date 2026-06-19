//! Style block and style definition types.

use std::collections::BTreeMap;

use crate::ast::span::Span;
use crate::ast::value::PropertyValue;

/// An unknown property child encountered inside a `style` block.
///
/// The name was not in the recognized set of visual style keys, so it cannot
/// be applied as a visual cascade.  It is preserved here so that the
/// validator can emit `style.unknown_property` warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownStyleProp {
    /// The raw property value (first positional argument of the child node).
    pub raw: String,
}

/// A named style definition, holding a map of recognized visual properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Globally unique style ID.
    pub id: String,
    /// Recognized visual properties, keyed by their canonical hyphenated name.
    ///
    /// Canonical keys: `fill`, `stroke`, `stroke-width`, `stroke-alignment`,
    /// `font-family`, `font-size`, `font-weight`, `line-height`, `radius`.
    pub properties: BTreeMap<String, PropertyValue>,
    /// Unknown (unrecognized) child node names encountered in the style block.
    ///
    /// These are preserved so the validator can emit `style.unknown_property`
    /// warnings without re-parsing the source.
    pub unknown_props: BTreeMap<String, UnknownStyleProp>,
    /// Byte-range of this `style` node in the source (for diagnostics).
    pub source_span: Option<Span>,
}

/// The top-level `styles` block containing named style definitions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleBlock {
    /// Ordered list of style definitions.
    pub styles: Vec<Style>,
    /// Byte-range of the `styles` block in the source (for diagnostics).
    pub source_span: Option<Span>,
}
