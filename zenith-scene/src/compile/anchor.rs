//! Page-relative 9-point anchor pre-pass.
//!
//! A node may carry `anchor="<name>"` where name is one of the nine page-
//! relative positions (`top-left`, `top-center`, `top-right`, `center-left`,
//! `center`, `center-right`, `bottom-left`, `bottom-center`, `bottom-right`).
//! When present and recognized, the compile step derives the node's x and/or y
//! from the page dimensions and the node's resolved w/h. An explicitly-authored
//! x or y always wins over the anchor-derived value.
//!
//! ## Pre-pass
//!
//! [`build_anchor_map`] is called once per page compile, AFTER `page_w`/
//! `page_h` are resolved, and walks the top-level page children (no recursion
//! into frames/groups — anchors are page-relative and only meaningful on direct
//! page children or equivalently-positioned nodes). For each node that carries
//! a recognized anchor AND has both `w` and `h` in a px-convertible unit, the
//! map stores the derived `(x, y)` pair keyed by node id. Nodes without
//! a recognized anchor or with non-px dimensions are absent from the map.
//!
//! ## Leaf application
//!
//! Each leaf compiler (`compile_rect`, `compile_ellipse`, etc.) receives the
//! `AnchorMap` by reference. When the node's own `x` is `None`, the compiler
//! looks up the node id in the map and, if found, uses the pre-derived x
//! (adding the usual `ctx.dx` translation). When `x` is `Some`, it is used
//! as-is (explicit wins). Same for y.

use std::collections::BTreeMap;

use zenith_core::{Node, Page, anchor_xy, dim_to_px, parse_anchor};

/// Pre-derived anchor coordinates keyed by node id.
///
/// A node appears in this map if and only if it carries a recognized anchor
/// value AND its `w` and `h` both resolved to px. The stored pair is the raw
/// page-coordinate `(x, y)` BEFORE any `ctx.dx`/`ctx.dy` group-translation
/// offset is applied.
pub(crate) type AnchorMap = BTreeMap<String, (f64, f64)>;

/// Walk the top-level children of `page` and build the [`AnchorMap`].
///
/// Only nodes with a recognized anchor, present `w`/`h`, and px-convertible
/// `w`/`h` produce entries. All others are absent (default: no anchor
/// derivation, byte-identical to before).
pub(crate) fn build_anchor_map(page: &Page, page_w: f64, page_h: f64) -> AnchorMap {
    let mut map = AnchorMap::new();
    for node in &page.children {
        collect_anchor(node, page_w, page_h, &mut map);
    }
    map
}

/// Try to build an anchor map entry for a single node.
///
/// Recursion into containers (frame/group) is intentionally absent: anchors
/// are page-relative and derived from page_w/page_h, so they only make sense
/// for direct page-level nodes (or those compiled in the same absolute
/// coordinate space). Container children are compiled with a potentially
/// different translation and are excluded here.
fn collect_anchor(node: &Node, page_w: f64, page_h: f64, map: &mut AnchorMap) {
    let (id, anchor_str, w_dim, h_dim) = match node {
        Node::Rect(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Ellipse(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Text(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Code(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Image(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Frame(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Group(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Shape(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Table(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Field(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Toc(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        // Nodes that never carry an `anchor` property are listed explicitly so
        // that adding a future node kind forces a decision here rather than
        // silently falling through.
        Node::Line(_)
        | Node::Connector(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Footnote(_)
        | Node::Instance(_)
        | Node::Unknown(_) => return,
    };

    // No anchor string → no entry.
    let anchor_name = match anchor_str {
        Some(s) => s,
        None => return,
    };

    // Unrecognized anchor → no entry (the validator already errors on this).
    let anchor = match parse_anchor(anchor_name) {
        Some(a) => a,
        None => return,
    };

    // Both w and h must be present and px-convertible for derivation.
    let (Some(w_dim), Some(h_dim)) = (w_dim, h_dim) else {
        return;
    };
    let (Some(node_w), Some(node_h)) = (
        dim_to_px(w_dim.value, &w_dim.unit),
        dim_to_px(h_dim.value, &h_dim.unit),
    ) else {
        return;
    };

    let (ax, ay) = anchor_xy(anchor, page_w, page_h, node_w, node_h);
    map.insert(id.to_owned(), (ax, ay));
}
