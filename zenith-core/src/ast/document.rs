//! Top-level document AST types.

use super::Span;
use super::asset::AssetBlock;
use super::node::Node;
use super::style::StyleBlock;
use super::token::TokenBlock;
use super::value::Dimension;
use super::value::PropertyValue;

/// Metadata for the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
}

/// A single page within a document body.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub id: String,
    pub name: Option<String>,
    /// Page width — required.
    pub width: Dimension,
    /// Page height — required.
    pub height: Dimension,
    pub background: Option<PropertyValue>,
    /// Author-declared safe/dead zones for this page. These are not rendering
    /// nodes; the validator checks page children against them.
    pub safe_zones: Vec<SafeZone>,
    /// Author-declared fold-line positions for this page (tri-fold/bi-fold
    /// print). These are non-printing page metadata, not rendering nodes; the
    /// validator advises when content crosses a fold line.
    pub folds: Vec<Fold>,
    /// Child content nodes in z-order (first = bottommost, last = topmost).
    pub children: Vec<Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The kind of a [`SafeZone`].
#[derive(Debug, Clone, PartialEq)]
pub enum SafeZoneType {
    /// Content must NOT overlap this zone (e.g. a platform UI dead zone).
    Exclusion,
    /// Content must overlap this zone (e.g. a guaranteed-visible region).
    Required,
}

/// A named safe/dead zone declared on a [`Page`].
///
/// Declared as a `safe-zone` child of a `page`; it is a sibling of rendering
/// nodes but is itself not rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct SafeZone {
    pub id: String,
    pub zone_type: SafeZoneType,
    pub x: Dimension,
    pub y: Dimension,
    pub w: Dimension,
    pub h: Dimension,
    pub label: Option<String>,
    pub source_span: Option<Span>,
}

/// A non-printing fold-line position declared on a [`Page`].
///
/// Declared as a `fold` child of a `page`; it is a sibling of rendering nodes
/// but is itself never rendered. A vertical fold has an `x` position; a
/// horizontal fold has a `y` position. Used for tri-fold / bi-fold print
/// layouts so the validator can advise when content crosses a fold line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fold {
    pub id: String,
    /// `"vertical"` (position is an x coordinate) or `"horizontal"` (position
    /// is a y coordinate). Any other / absent value defaults to `"vertical"`.
    pub orientation: String,
    /// The fold-line position: x for a vertical fold, y for a horizontal fold.
    /// `None` when the author omitted `position`.
    pub position: Option<Dimension>,
    pub source_span: Option<Span>,
}

/// The `document` child of the root `zenith` node.
///
/// Named `DocumentBody` to avoid clashing with the root `Document` type.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentBody {
    pub id: String,
    pub title: Option<String>,
    pub pages: Vec<Page>,
}

/// A reusable component definition: a named child-node subtree declared once
/// (in the document-level `components` block) and instanced into multiple places
/// via [`Node::Instance`](super::node::Node::Instance).
///
/// Declared as `component id="logo.block" { <any child nodes> }`. The component's
/// child node ids are LOCAL to the component: they are validated for uniqueness
/// only WITHIN the component, not globally, and they are prefixed with the
/// instance id when an instance is expanded at compile time. The `component` id
/// itself participates in the global id-uniqueness set.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDef {
    pub id: String,
    /// The component's child nodes in source order (the reusable subtree).
    pub children: Vec<super::node::Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The root `zenith` node — the complete parsed `.zen` document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Must be `1` in v0.
    pub version: u32,
    /// Declared export color space: `Some("srgb")` (default) or `Some("cmyk")`.
    /// `None` when the author omitted the `colorspace` attribute. In v0 this is
    /// informational export metadata only — it does NOT change PNG output (the
    /// PNG is always sRGB); a future PDF backend consults it. An invalid value
    /// is preserved here verbatim and surfaced as a validation warning.
    pub colorspace: Option<String>,
    pub project: Option<Project>,
    /// Asset declarations; empty when the `assets` block is absent.
    pub assets: AssetBlock,
    pub tokens: TokenBlock,
    pub styles: StyleBlock,
    /// Reusable component definitions; empty when the `components` block is
    /// absent. Instanced via [`Node::Instance`](super::node::Node::Instance).
    pub components: Vec<ComponentDef>,
    pub body: DocumentBody,
}
