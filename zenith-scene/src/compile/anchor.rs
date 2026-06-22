//! 9-point anchor pre-pass (A-1: page-relative; A-2: safe-zone-relative;
//! A-3: parent-container-relative).
//!
//! A node may carry `anchor="<name>"` where name is one of the nine positions:
//! `top-left`, `top-center`, `top-right`, `center-left`, `center`,
//! `center-right`, `bottom-left`, `bottom-center`, `bottom-right`. When present
//! and recognized, the compile step derives the node's x and/or y from a
//! reference rectangle and the node's resolved w/h. An explicitly-authored x or
//! y always wins over the anchor-derived value.
//!
//! **A-1 (page-relative):** reference rectangle is the full page.
//!
//! **A-2 (safe-zone-relative):** when the node also carries
//! `anchor-zone="<id>"` and a safe-zone with that id is declared on the same
//! page, the reference rectangle is that zone's rect instead of the page.
//! Unrecognized zone ids and non-px zone dimensions silently fall back to no
//! anchor entry (the validator emits `anchor.unresolved_zone`).
//!
//! **A-3 (parent-relative):** when the node carries `anchor-parent="true"`
//! (and NOT `anchor-zone`, which takes precedence), the reference rectangle is
//! its DIRECT PARENT CONTAINER's box (a `frame` or `group`). The pre-pass
//! recurses into frame/group children, threading the parent box and the
//! cumulative group translation so the stored value cancels the `ctx.dx`/
//! `ctx.dy` that the leaf compiler re-applies.
//!
//! ## Pre-pass
//!
//! [`build_anchor_map`] is called once per page compile, AFTER `page_w`/
//! `page_h` are resolved, and walks the page tree, descending into `frame` and
//! `group` containers (only those two are A-3 anchor-parent containers). For
//! each node that carries a recognized anchor AND has both `w` and `h` in a
//! px-convertible unit, the map stores the derived `(x, y)` pair keyed by node
//! id.
//!
//! ## Leaf application
//!
//! Each leaf compiler (`compile_rect`, `compile_ellipse`, etc.) receives the
//! `AnchorMap` by reference. When the node's own `x` is `None`, the compiler
//! looks up the node id in the map and, if found, uses the pre-derived x
//! (adding the usual `ctx.dx` translation). When `x` is `Some`, it is used
//! as-is (explicit wins). Same for y.

use std::collections::BTreeMap;

use zenith_core::{Dimension, Node, Page, SafeZone, anchor_xy, dim_to_px, parse_anchor};

/// Pre-derived anchor coordinates keyed by node id.
///
/// A node appears in this map if and only if it carries a recognized anchor
/// value AND its `w` and `h` both resolved to px. The stored pair is the raw
/// coordinate `(x, y)` BEFORE the `ctx.dx`/`ctx.dy` group-translation offset is
/// applied by the leaf compiler; the anchor-parent derivation pre-subtracts the
/// accumulated group translation so adding `ctx.dx`/`ctx.dy` lands the node at
/// the intended device position.
pub(crate) type AnchorMap = BTreeMap<String, (f64, f64)>;

/// Walk-wide immutable pre-pass environment (page dims + zone table).
#[derive(Clone, Copy)]
struct PrePassEnv<'a> {
    page_w: f64,
    page_h: f64,
    safe_zones: &'a [SafeZone],
}

/// Per-recursion container context for parent-relative (A-3) derivation.
///
/// `parent_box` = `Some((ref_x, ref_y, ref_w, ref_h))` is the enclosing
/// container's reference rectangle, or `None` at the page root (and when a
/// container box is unresolvable). `acc_dx`/`acc_dy` is the cumulative GROUP
/// translation that will be active as `ctx.dx`/`ctx.dy` when the current node
/// compiles; the parent-relative derivation subtracts it so the leaf's re-add
/// cancels to the intended device coordinate.
#[derive(Clone, Copy)]
struct ParentCtx {
    parent_box: Option<(f64, f64, f64, f64)>,
    acc_dx: f64,
    acc_dy: f64,
}

impl ParentCtx {
    const ROOT: ParentCtx = ParentCtx {
        parent_box: None,
        acc_dx: 0.0,
        acc_dy: 0.0,
    };
}

/// Walk the page tree and build the [`AnchorMap`].
///
/// Top-level nodes resolve page/zone-relative anchors (A-1/A-2). Frame and
/// group children additionally resolve parent-relative anchors (A-3) against
/// their enclosing container's box. Only nodes with a recognized anchor,
/// present `w`/`h`, and px-convertible `w`/`h` produce entries; all others are
/// absent (byte-identical to before for any node not using anchor-parent).
pub(crate) fn build_anchor_map(page: &Page, page_w: f64, page_h: f64) -> AnchorMap {
    let env = PrePassEnv {
        page_w,
        page_h,
        safe_zones: &page.safe_zones,
    };
    let mut map = AnchorMap::new();
    for node in &page.children {
        collect_anchor(node, env, ParentCtx::ROOT, &mut map);
    }
    map
}

/// The `(id, anchor, anchor_zone, anchor_sibling, anchor_parent, w, h)` fields
/// pulled from a node that may carry an anchor.
type AnchorFields<'a> = (
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<bool>,
    Option<&'a Dimension>,
    Option<&'a Dimension>,
);

/// Extract the anchor-bearing fields of a node, or `None` for kinds that never
/// carry an `anchor`.
fn anchor_fields(node: &Node) -> Option<AnchorFields<'_>> {
    let f = match node {
        Node::Rect(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Ellipse(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Text(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Code(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Image(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Frame(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Group(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Shape(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Table(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Field(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
            n.w.as_ref(),
            n.h.as_ref(),
        ),
        Node::Toc(n) => (
            n.id.as_str(),
            n.anchor.as_deref(),
            n.anchor_zone.as_deref(),
            n.anchor_sibling.as_deref(),
            n.anchor_parent,
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
        | Node::Unknown(_) => return None,
    };
    Some(f)
}

/// Resolve the px box `(x, y, w, h)` of a node from its four geometry dims,
/// returning `None` when any of the four is absent or non-px.
fn px_box(
    x: Option<&Dimension>,
    y: Option<&Dimension>,
    w: Option<&Dimension>,
    h: Option<&Dimension>,
) -> Option<(f64, f64, f64, f64)> {
    let x = x.and_then(|d| dim_to_px(d.value, &d.unit))?;
    let y = y.and_then(|d| dim_to_px(d.value, &d.unit))?;
    let w = w.and_then(|d| dim_to_px(d.value, &d.unit))?;
    let h = h.and_then(|d| dim_to_px(d.value, &d.unit))?;
    Some((x, y, w, h))
}

/// Try to build an anchor map entry for a single node, then recurse into
/// `frame`/`group` containers carrying their box as the parent reference for
/// A-3 anchor-parent children.
fn collect_anchor(node: &Node, env: PrePassEnv, ctx: ParentCtx, map: &mut AnchorMap) {
    if let Some(fields) = anchor_fields(node) {
        derive_entry(fields, env, ctx, map);
    }

    // Recurse into the two A-3 anchor-parent containers: frame (clip-only — does
    // NOT translate children) and group (translates children by group_x/group_y).
    // Other node kinds are leaves for anchor purposes (matching the prior pre-pass
    // which did not recurse at all), so adding only frame/group recursion is the
    // sole additive change.
    match node {
        Node::Frame(frame) => {
            // Frame box is ABSOLUTE; children inherit acc_dx/acc_dy unchanged.
            let frame_box = px_box(
                frame.x.as_ref(),
                frame.y.as_ref(),
                frame.w.as_ref(),
                frame.h.as_ref(),
            );
            let child_ctx = ParentCtx {
                parent_box: frame_box,
                acc_dx: ctx.acc_dx,
                acc_dy: ctx.acc_dy,
            };
            for child in &frame.children {
                collect_anchor(child, env, child_ctx, map);
            }
        }
        Node::Group(group) => {
            // Group translates children by group_x/group_y (default 0 if absent
            // or non-px). The child's compile context acc becomes acc + group_x.
            let group_x = group
                .x
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.0);
            let group_y = group
                .y
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.0);
            let child_dx = ctx.acc_dx + group_x;
            let child_dy = ctx.acc_dy + group_y;
            // The group reference box origin is its device origin (child_dx,
            // child_dy); width/height come from the declared w/h. When either w
            // or h is absent/non-px the box is unknown → no parent-relative entry
            // for the group's children (validator flags it).
            let group_box = group
                .w
                .as_ref()
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .zip(group.h.as_ref().and_then(|d| dim_to_px(d.value, &d.unit)))
                .map(|(gw, gh)| (child_dx, child_dy, gw, gh));
            let child_ctx = ParentCtx {
                parent_box: group_box,
                acc_dx: child_dx,
                acc_dy: child_dy,
            };
            for child in &group.children {
                collect_anchor(child, env, child_ctx, map);
            }
        }
        // Every other node kind is a leaf for anchor pre-pass purposes.
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Text(_)
        | Node::Code(_)
        | Node::Image(_)
        | Node::Shape(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Connector(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Table(_)
        | Node::Unknown(_) => {}
    }
}

/// Derive and insert the anchor map entry for one node from its fields.
fn derive_entry(fields: AnchorFields<'_>, env: PrePassEnv, ctx: ParentCtx, map: &mut AnchorMap) {
    // anchor_sibling is threaded through for A-4b (sibling-relative positioning);
    // it is inert in this unit and not used for derivation yet.
    let (id, anchor_str, anchor_zone_str, _anchor_sibling, anchor_parent, w_dim, h_dim) = fields;

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

    // Reference rectangle precedence:
    //   1. anchor-zone (A-2) wins when set — resolve the zone rect; skip on
    //      unknown id / non-px dims (validator diagnoses).
    //   2. anchor-parent (A-3) when no zone — use the enclosing container box
    //      and pre-subtract the accumulated group translation.
    //   3. page-relative (A-1) otherwise.
    if let Some(zone_id) = anchor_zone_str {
        let (ref_x, ref_y, ref_w, ref_h) = match env.safe_zones.iter().find(|z| z.id == zone_id) {
            Some(zone) => match (
                dim_to_px(zone.x.value, &zone.x.unit),
                dim_to_px(zone.y.value, &zone.y.unit),
                dim_to_px(zone.w.value, &zone.w.unit),
                dim_to_px(zone.h.value, &zone.h.unit),
            ) {
                (Some(zx), Some(zy), Some(zw), Some(zh)) => (zx, zy, zw, zh),
                _ => return,
            },
            None => return,
        };
        let (ox, oy) = anchor_xy(anchor, ref_w, ref_h, node_w, node_h);
        map.insert(id.to_owned(), (ref_x + ox, ref_y + oy));
        return;
    }

    if anchor_parent == Some(true) {
        // Parent-relative: requires a usable enclosing container box. When the
        // node is not inside a frame/group, or the container box is unknown,
        // no entry is produced (the validator emits anchor.unresolvable_parent).
        let Some((rx, ry, rw, rh)) = ctx.parent_box else {
            return;
        };
        let (ox, oy) = anchor_xy(anchor, rw, rh, node_w, node_h);
        // Subtract the accumulated group translation: the leaf compiler re-adds
        // ctx.dx/ctx.dy (== acc_dx/acc_dy) so the device coordinate lands at
        // (rx + ox, ry + oy).
        map.insert(id.to_owned(), (rx + ox - ctx.acc_dx, ry + oy - ctx.acc_dy));
        return;
    }

    // Page-relative (A-1): origin is (0, 0).
    let (ox, oy) = anchor_xy(anchor, env.page_w, env.page_h, node_w, node_h);
    map.insert(id.to_owned(), (ox, oy));
}
