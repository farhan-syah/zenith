//! Flow / grid layout helpers shared by the frame compilers: participation
//! predicates, declared-box readers, and the box-injection clone used to place a
//! child at absolute coordinates.

use std::collections::BTreeMap;

use zenith_core::{Dimension, Node, PropertyValue, ResolvedToken, Unit};

use super::super::node_role;
use super::super::util::resolve_geometry_px;

/// Whether a child is excluded from flow layout entirely (consumes no space):
/// `visible == Some(false)` or `role == "guide"`.
pub(super) fn node_skipped_in_flow(node: &Node) -> bool {
    node_role(node) == Some("guide") || node_visible(node) == Some(false)
}

/// The `visible` flag of any node kind, if set (kinds without the property
/// — `Unknown` — yield `None`).
fn node_visible(node: &Node) -> Option<bool> {
    match node {
        Node::Rect(n) => n.visible,
        Node::Ellipse(n) => n.visible,
        Node::Line(n) => n.visible,
        Node::Text(n) => n.visible,
        Node::Code(n) => n.visible,
        Node::Frame(n) => n.visible,
        Node::Group(n) => n.visible,
        Node::Image(n) => n.visible,
        Node::Polygon(n) => n.visible,
        Node::Polyline(n) => n.visible,
        Node::Instance(n) => n.visible,
        Node::Field(n) => n.visible,
        Node::Toc(n) => n.visible,
        Node::Table(n) => n.visible,
        Node::Shape(n) => n.visible,
        Node::Connector(n) => n.visible,
        Node::Pattern(n) => n.visible,
        Node::Chart(n) => n.visible,
        Node::Light(n) => n.visible,
        // A footnote has no `visible` flag.
        Node::Footnote(_) => None,
        Node::Unknown(_) => None,
    }
}

/// The declared `w` of a node in pixels, if the node kind carries a `w`/`h`
/// box and the dimension resolves to pixels. Geometry-less kinds (line,
/// polygon, polyline, unknown) yield `None`.
pub(super) fn node_declared_w(
    node: &Node,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> Option<f64> {
    let w = match node {
        Node::Rect(n) => n.w.as_ref(),
        Node::Ellipse(n) => n.w.as_ref(),
        Node::Text(n) => n.w.as_ref(),
        Node::Code(n) => n.w.as_ref(),
        Node::Frame(n) => n.w.as_ref(),
        Node::Group(n) => n.w.as_ref(),
        Node::Image(n) => n.w.as_ref(),
        Node::Field(n) => n.w.as_ref(),
        Node::Toc(n) => n.w.as_ref(),
        Node::Table(n) => n.w.as_ref(),
        Node::Shape(n) => n.w.as_ref(),
        Node::Pattern(n) => n.w.as_ref(),
        Node::Chart(n) => n.w.as_ref(),
        Node::Light(_) => None,
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => None,
    }?;
    resolve_geometry_px(Some(w), resolved)
}

/// The declared `h` of a node in pixels, if the node kind carries a `w`/`h`
/// box and the dimension resolves to pixels. Geometry-less kinds yield `None`.
pub(super) fn node_declared_h(
    node: &Node,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> Option<f64> {
    let h = match node {
        Node::Rect(n) => n.h.as_ref(),
        Node::Ellipse(n) => n.h.as_ref(),
        Node::Text(n) => n.h.as_ref(),
        Node::Code(n) => n.h.as_ref(),
        Node::Frame(n) => n.h.as_ref(),
        Node::Group(n) => n.h.as_ref(),
        Node::Image(n) => n.h.as_ref(),
        Node::Field(n) => n.h.as_ref(),
        Node::Toc(n) => n.h.as_ref(),
        Node::Table(n) => n.h.as_ref(),
        Node::Shape(n) => n.h.as_ref(),
        Node::Pattern(n) => n.h.as_ref(),
        Node::Chart(n) => n.h.as_ref(),
        Node::Light(_) => None,
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => None,
    }?;
    resolve_geometry_px(Some(h), resolved)
}

/// Clone `node` and overwrite its `x`/`y`/`w`/`h` box with the injected
/// flow coordinates (all in absolute page px). `h` is set only when the flow
/// path resolved one (declared height); a `None` `h` leaves the clone's `h`
/// unset so the child auto-measures its own intrinsic height.
///
/// Kinds without an `x`/`y`/`w`/`h` box (`Line`/`Polygon`/`Polyline`/
/// `Unknown`) are returned unchanged — the flow path advances its cursor by
/// `0.0` for those.
pub(super) fn with_flow_box(node: &Node, x: f64, y: f64, w: f64, h: Option<f64>) -> Node {
    let px = |v: f64| {
        Some(PropertyValue::Dimension(Dimension {
            value: v,
            unit: Unit::Px,
        }))
    };
    let h_dim = h.and_then(px);

    let mut out = node.clone();
    match &mut out {
        Node::Rect(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Ellipse(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Text(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Code(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Image(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Frame(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Group(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Field(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Toc(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Table(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Shape(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Pattern(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Chart(n) => {
            n.x = px(x);
            n.y = px(y);
            n.w = px(w);
            n.h = h_dim;
        }
        Node::Light(_) => {}
        // Geometry-less kinds: no x/y/w/h box to inject. (An instance carries
        // only an x/y origin, no w/h box, so flow layout does not reposition it
        // — it renders at its authored origin and advances the cursor by 0.)
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => {}
    }
    out
}
