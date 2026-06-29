//! Node types for the renderable layer of a `.zen` document.
//!
//! Wiring-only root: the node-layer types are grouped into cohesion-based
//! submodules and re-exported flat here so the crate's public surface
//! (`zenith_core::RectNode`, `ast::node::Anchor`, …) is unchanged.

mod anchor;
mod common;
mod container;
mod effect;
mod leaf;
mod special;

pub use anchor::{Anchor, AnchorEdge, anchor_xy, parse_anchor, parse_anchor_edge};
pub use common::{Node, ObjectPosition, Point, TextSpan, UnknownProperty, UnknownValue};
pub use container::{
    FrameNode, GroupNode, ProtectedRegion, TableCell, TableColumn, TableNode, TableRow,
};
pub use effect::LightNode;
pub use leaf::{
    ChartNode, ChartSeries, CodeNode, EllipseNode, ImageNode, LineNode, PatternNode, PolygonNode,
    PolylineNode, RectNode, TextNode,
};
pub use special::{
    ConnectorNode, FieldNode, FootnoteNode, InstanceNode, Override, ShapeNode, TocNode, UnknownNode,
};
