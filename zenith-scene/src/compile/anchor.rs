//! 9-point anchor pre-pass supporting page-relative, safe-zone-relative,
//! parent-container-relative, and sibling-relative anchoring.
//!
//! A node may carry `anchor="<name>"` where name is one of the nine positions:
//! `top-left`, `top-center`, `top-right`, `center-left`, `center`,
//! `center-right`, `bottom-left`, `bottom-center`, `bottom-right`. When present
//! and recognized, the compile step derives the node's x and/or y from a
//! reference rectangle and the node's resolved w/h. An explicitly-authored x or
//! y always wins over the anchor-derived value.
//!
//! **Page-relative:** reference rectangle is the full page.
//!
//! **Safe-zone-relative:** when the node also carries
//! `anchor-zone="<id>"` and a safe-zone with that id is declared on the same
//! page, the reference rectangle is that zone's rect instead of the page.
//! Unrecognized zone ids and non-px zone dimensions silently fall back to no
//! anchor entry (the validator emits `anchor.unresolved_zone`).
//!
//! **Parent-relative:** when the node carries `anchor-parent="true"`
//! (and NOT `anchor-zone`, which takes precedence), the reference rectangle is
//! its DIRECT PARENT CONTAINER's box (a `frame` or `group`). The pre-pass
//! recurses into frame/group children, threading the parent box and the
//! cumulative group translation so the stored value cancels the `ctx.dx`/
//! `ctx.dy` that the leaf compiler re-applies.
//!
//! **Sibling-relative:** when the node carries `anchor-sibling="<id>"`
//! (and NOT `anchor-zone`, which takes precedence), the reference rectangle is
//! the resolved box of the named sibling in the SAME scope (same direct
//! parent's children). Because node and sibling share the same accumulated
//! group translation, this derivation is purely local — no `acc` term is added
//! or subtracted. Each scope is processed in sibling-dependency (topological)
//! order so a referenced sibling's entry exists before its dependent derives.
//!
//! ## Pre-pass
//!
//! [`build_anchor_map`] is called once per page compile, AFTER `page_w`/
//! `page_h` are resolved, and walks the page tree, descending into `frame` and
//! `group` containers (only those two are anchor-parent containers). For
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

use std::collections::{BTreeMap, BTreeSet};

use zenith_core::{
    Anchor, AnchorEdge, Dimension, Node, Page, PropertyValue, ResolvedToken, SafeZone, anchor_xy,
    dim_to_px, parse_anchor, parse_anchor_edge,
};

use super::util::resolve_geometry_px;

/// Pre-derived anchor coordinates keyed by node id.
///
/// A node appears in this map if and only if it carries a recognized anchor
/// value AND its `w` and `h` both resolved to px. The stored pair is the raw
/// coordinate `(x, y)` BEFORE the `ctx.dx`/`ctx.dy` group-translation offset is
/// applied by the leaf compiler; the anchor-parent derivation pre-subtracts the
/// accumulated group translation so adding `ctx.dx`/`ctx.dy` lands the node at
/// the intended device position.
pub(crate) type AnchorMap = BTreeMap<String, (f64, f64)>;

/// Walk-wide immutable pre-pass environment (page dims + zone table + token
/// table). The token table resolves geometry token refs (`(token)"dim.h"`) on
/// box nodes during anchor derivation.
#[derive(Clone, Copy)]
struct PrePassEnv<'a> {
    page_w: f64,
    page_h: f64,
    safe_zones: &'a [SafeZone],
    resolved: &'a BTreeMap<String, ResolvedToken>,
}

/// Per-recursion container context for parent-relative derivation.
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
/// Top-level nodes resolve page/zone-relative anchors. Frame and
/// group children additionally resolve parent-relative anchors against
/// their enclosing container's box. Only nodes with a recognized anchor,
/// present `w`/`h`, and px-convertible `w`/`h` produce entries; all others are
/// absent (byte-identical to before for any node not using anchor-parent).
pub(crate) fn build_anchor_map(
    page: &Page,
    page_w: f64,
    page_h: f64,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> AnchorMap {
    let env = PrePassEnv {
        page_w,
        page_h,
        safe_zones: &page.safe_zones,
        resolved,
    };
    let mut map = AnchorMap::new();
    // The page-children form one sibling scope; process them in sibling-
    // dependency order so a node referencing an earlier-resolved sibling sees
    // that sibling's entry already in the map.
    let scope: BTreeMap<&str, &Node> = page
        .children
        .iter()
        .filter_map(|n| anchor_fields(n).map(|f| (f.id, n)))
        .collect();
    for node in sibling_topo_order(&page.children) {
        collect_anchor(node, env, ParentCtx::ROOT, &scope, &mut map);
    }
    map
}

/// Order `children` so that any node carrying `anchor-sibling="<id>"` (where
/// `<id>` is an in-scope anchor-bearing sibling) is processed AFTER that
/// sibling. Uses Kahn's algorithm over the in-scope sibling-dependency graph
/// (edge: target → dependent). Anchor-bearing nodes with no in-scope sibling
/// dependency are emitted in sorted-id order (the ready-set is a `BTreeSet`);
/// their derivations are mutually independent, so the resulting anchor map is
/// identical regardless of their relative order. Non-anchor-bearing kinds, and
/// nodes left in a dependency cycle (nonzero in-degree after Kahn), are appended
/// at the end in source order; cyclic nodes naturally fail to resolve (the
/// validator reports the cycle separately). Deterministic throughout via
/// `BTreeSet`/`BTreeMap`.
fn sibling_topo_order(children: &[Node]) -> Vec<&Node> {
    // In-scope anchor-bearing ids, plus a quick id → node lookup.
    let mut by_id: BTreeMap<&str, &Node> = BTreeMap::new();
    for node in children {
        if let Some(f) = anchor_fields(node) {
            by_id.insert(f.id, node);
        }
    }

    // in_degree[id] = number of in-scope sibling targets `id` depends on (0 or
    // 1, since a node carries at most one anchor-sibling). adjacency[target] =
    // the dependents that reference `target`.
    let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (&id, node) in &by_id {
        in_degree.entry(id).or_insert(0);
        if let Some(f) = anchor_fields(node)
            && let Some(target) = f.anchor_sibling
            && target != id
            && by_id.contains_key(target)
        {
            adjacency.entry(target).or_default().insert(id);
            *in_degree.entry(id).or_insert(0) += 1;
        }
    }

    // Kahn: seed the ready-set with all zero-in-degree ids (sorted), emit, and
    // decrement dependents. A single pass; no recursion, no unbounded loop.
    let mut ready: BTreeSet<&str> = in_degree
        .iter()
        .filter_map(|(&id, &deg)| (deg == 0).then_some(id))
        .collect();
    // `emitted` retains Kahn's dequeue ORDER (a target is always dequeued before
    // its dependents), which is the topological order we must process in.
    let mut emitted: Vec<&str> = Vec::with_capacity(by_id.len());
    while let Some(&id) = ready.first() {
        ready.remove(id);
        emitted.push(id);
        if let Some(deps) = adjacency.get(id) {
            for &dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        ready.insert(dep);
                    }
                }
            }
        }
    }

    // Emit anchor-bearing nodes in topological (Kahn dequeue) order, then append
    // any node not emitted — non-anchor-bearing kinds and cycle members — in
    // source order. When no node has an in-scope anchor-sibling, every node is
    // zero-in-degree and dequeued in sorted id order; the source-order tail then
    // contributes nothing new, so the order is a stable permutation that still
    // honours dependencies. Independent equal-rank nodes resolve identically
    // regardless of order (their entries don't depend on each other).
    let mut order: Vec<&Node> = Vec::with_capacity(children.len());
    let mut placed: BTreeSet<&str> = BTreeSet::new();
    for &id in &emitted {
        if let Some(&node) = by_id.get(id)
            && placed.insert(id)
        {
            order.push(node);
        }
    }
    for node in children {
        match anchor_fields(node) {
            Some(f) if placed.contains(f.id) => {}
            _ => order.push(node),
        }
    }
    order
}

/// The anchor-bearing fields pulled from a node that may carry an anchor.
///
/// `x`/`y` are included (in addition to `w`/`h`) because sibling-relative
/// anchoring reads the sibling's authored origin per axis.
struct AnchorFields<'a> {
    id: &'a str,
    anchor: Option<&'a str>,
    anchor_zone: Option<&'a str>,
    anchor_sibling: Option<&'a str>,
    anchor_parent: Option<bool>,
    anchor_edge: Option<&'a str>,
    anchor_gap: Option<&'a Dimension>,
    x: Option<&'a PropertyValue>,
    y: Option<&'a PropertyValue>,
    w: Option<&'a PropertyValue>,
    h: Option<&'a PropertyValue>,
}

/// Extract the anchor-bearing fields of a node, or `None` for kinds that never
/// carry an `anchor`.
fn anchor_fields(node: &Node) -> Option<AnchorFields<'_>> {
    let f = match node {
        Node::Rect(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Ellipse(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Text(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Code(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Image(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Frame(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Group(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Shape(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Table(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Field(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Toc(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Pattern(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        Node::Chart(n) => AnchorFields {
            id: n.id.as_str(),
            anchor: n.anchor.as_deref(),
            anchor_zone: n.anchor_zone.as_deref(),
            anchor_sibling: n.anchor_sibling.as_deref(),
            anchor_parent: n.anchor_parent,
            anchor_edge: n.anchor_edge.as_deref(),
            anchor_gap: n.anchor_gap.as_ref(),
            x: n.x.as_ref(),
            y: n.y.as_ref(),
            w: n.w.as_ref(),
            h: n.h.as_ref(),
        },
        // Nodes that never carry an `anchor` property are listed explicitly so
        // that adding a future node kind forces a decision here rather than
        // silently falling through.
        Node::Line(_)
        | Node::Connector(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Footnote(_)
        | Node::Instance(_)
        | Node::Light(_)
        | Node::Mesh(_)
        | Node::Unknown(_) => return None,
    };
    Some(f)
}

/// Resolve the px box `(x, y, w, h)` of a node from its four geometry
/// properties, returning `None` when any of the four is absent, a non-dimension,
/// an unresolved token, or carries a non-px unit. Raw `(px)` dims are
/// byte-identical to the prior `dim_to_px` read; dimension token refs resolve
/// via the token table.
fn px_box(
    x: Option<&PropertyValue>,
    y: Option<&PropertyValue>,
    w: Option<&PropertyValue>,
    h: Option<&PropertyValue>,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> Option<(f64, f64, f64, f64)> {
    let x = resolve_geometry_px(x, resolved)?;
    let y = resolve_geometry_px(y, resolved)?;
    let w = resolve_geometry_px(w, resolved)?;
    let h = resolve_geometry_px(h, resolved)?;
    Some((x, y, w, h))
}

/// Try to build an anchor map entry for a single node, then recurse into
/// `frame`/`group` containers carrying their box as the parent reference for
/// anchor-parent children.
fn collect_anchor(
    node: &Node,
    env: PrePassEnv,
    ctx: ParentCtx,
    scope: &BTreeMap<&str, &Node>,
    map: &mut AnchorMap,
) {
    if let Some(fields) = anchor_fields(node) {
        derive_entry(fields, env, ctx, scope, map);
    }

    // Recurse into the two anchor-parent containers: frame (clip-only — does
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
                env.resolved,
            );
            let child_ctx = ParentCtx {
                parent_box: frame_box,
                acc_dx: ctx.acc_dx,
                acc_dy: ctx.acc_dy,
            };
            // The frame's direct children form a new sibling scope.
            let child_scope: BTreeMap<&str, &Node> = frame
                .children
                .iter()
                .filter_map(|n| anchor_fields(n).map(|f| (f.id, n)))
                .collect();
            for child in sibling_topo_order(&frame.children) {
                collect_anchor(child, env, child_ctx, &child_scope, map);
            }
        }
        Node::Group(group) => {
            // Group translates children by group_x/group_y (default 0 if absent
            // or non-px). The child's compile context acc becomes acc + group_x.
            let group_x = resolve_geometry_px(group.x.as_ref(), env.resolved).unwrap_or(0.0);
            let group_y = resolve_geometry_px(group.y.as_ref(), env.resolved).unwrap_or(0.0);
            let child_dx = ctx.acc_dx + group_x;
            let child_dy = ctx.acc_dy + group_y;
            // The group reference box origin is its device origin (child_dx,
            // child_dy); width/height come from the declared w/h. When either w
            // or h is absent/non-px the box is unknown → no parent-relative entry
            // for the group's children (validator flags it).
            let group_box = resolve_geometry_px(group.w.as_ref(), env.resolved)
                .zip(resolve_geometry_px(group.h.as_ref(), env.resolved))
                .map(|(gw, gh)| (child_dx, child_dy, gw, gh));
            let child_ctx = ParentCtx {
                parent_box: group_box,
                acc_dx: child_dx,
                acc_dy: child_dy,
            };
            // The group's direct children form a new sibling scope.
            let child_scope: BTreeMap<&str, &Node> = group
                .children
                .iter()
                .filter_map(|n| anchor_fields(n).map(|f| (f.id, n)))
                .collect();
            for child in sibling_topo_order(&group.children) {
                collect_anchor(child, env, child_ctx, &child_scope, map);
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
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Mesh(_)
        | Node::Unknown(_) => {}
    }
}

/// Compute the cross-axis horizontal coordinate (x) for Above/Below edges.
///
/// `anchor` is the optional 9-pt anchor (supplies horizontal alignment);
/// absent anchor → leading edge (left-align, i.e. `sib_x`).
fn cross_h(anchor: Option<Anchor>, sib_x: f64, sib_w: f64, node_w: f64) -> f64 {
    match anchor {
        None | Some(Anchor::TopLeft) | Some(Anchor::CenterLeft) | Some(Anchor::BottomLeft) => sib_x,
        Some(Anchor::TopCenter) | Some(Anchor::Center) | Some(Anchor::BottomCenter) => {
            sib_x + (sib_w - node_w) / 2.0
        }
        Some(Anchor::TopRight) | Some(Anchor::CenterRight) | Some(Anchor::BottomRight) => {
            sib_x + sib_w - node_w
        }
    }
}

/// Compute the cross-axis vertical coordinate (y) for Before/After edges.
///
/// `anchor` is the optional 9-pt anchor (supplies vertical alignment);
/// absent anchor → leading edge (top-align, i.e. `sib_y`).
fn cross_v(anchor: Option<Anchor>, sib_y: f64, sib_h: f64, node_h: f64) -> f64 {
    match anchor {
        None | Some(Anchor::TopLeft) | Some(Anchor::TopCenter) | Some(Anchor::TopRight) => sib_y,
        Some(Anchor::CenterLeft) | Some(Anchor::Center) | Some(Anchor::CenterRight) => {
            sib_y + (sib_h - node_h) / 2.0
        }
        Some(Anchor::BottomLeft) | Some(Anchor::BottomCenter) | Some(Anchor::BottomRight) => {
            sib_y + sib_h - node_h
        }
    }
}

/// Derive and insert the anchor map entry for one node from its fields.
fn derive_entry(
    fields: AnchorFields<'_>,
    env: PrePassEnv,
    ctx: ParentCtx,
    scope: &BTreeMap<&str, &Node>,
    map: &mut AnchorMap,
) {
    let AnchorFields {
        id,
        anchor: anchor_str,
        anchor_zone: anchor_zone_str,
        anchor_sibling,
        anchor_parent,
        anchor_edge: anchor_edge_str,
        anchor_gap,
        x: _,
        y: _,
        w: w_dim,
        h: h_dim,
    } = fields;

    // Resolve the edge placement request (may be None when anchor-edge is
    // absent or unrecognized). This is needed BEFORE the early-return so we
    // can decide whether to proceed even when anchor-string is absent.
    let edge = anchor_edge_str.and_then(parse_anchor_edge);

    // When BOTH anchor string and anchor-edge are absent, nothing to derive.
    if anchor_str.is_none() && edge.is_none() {
        return;
    }

    // Parse the 9-pt anchor string. For edge-placement paths, this is
    // OPTIONAL (supplies cross-axis alignment only); None means default
    // (leading edge). For the non-edge paths it is required — unrecognized
    // values exit early (validator already errors on them).
    let anchor_parsed: Option<Anchor> = match anchor_str {
        Some(s) => match parse_anchor(s) {
            Some(a) => Some(a),
            // Unrecognized anchor. For edge paths, don't block; treat as None.
            // For non-edge paths (classic derivation), exit early.
            None => {
                if edge.is_none() {
                    return;
                }
                None
            }
        },
        None => None,
    };

    // Both w and h must be present and px-convertible for derivation. Raw `(px)`
    // dims and dimension token refs both resolve via the token table.
    let (Some(node_w), Some(node_h)) = (
        resolve_geometry_px(w_dim, env.resolved),
        resolve_geometry_px(h_dim, env.resolved),
    ) else {
        return;
    };

    // Reference rectangle precedence:
    //   1. anchor-zone wins when set — resolve the zone rect; skip on
    //      unknown id / non-px dims (validator diagnoses).
    //      (Edge placement does NOT combine with anchor-zone; zone wins.)
    //   2. anchor-sibling when no zone — derive against the named
    //      sibling's resolved box, purely in local space.
    //      When anchor-edge is set here, use adjacent-edge placement instead
    //      of within-box anchor_xy.
    //   3. anchor-parent when no zone/sibling — use the enclosing
    //      container box and pre-subtract the accumulated group translation.
    //      (anchor-edge without sibling falls through to here or page.)
    //   4. page-relative otherwise.
    if let Some(zone_id) = anchor_zone_str {
        // For zone-relative paths the 9-pt anchor is required (classic path).
        let anchor = match anchor_parsed {
            Some(a) => a,
            None => return,
        };
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

    // Sibling-relative: the node's origin is derived from a named
    // sibling's resolved box. The node and its sibling share the SAME scope
    // (same direct parent's children) and hence the SAME accumulated group
    // translation, so this derivation is PURELY in local space — no acc term.
    if let Some(sib_id) = anchor_sibling {
        // Unresolved reference → no entry (the validator emits
        // anchor.unresolved_sibling).
        let Some(&sib_node) = scope.get(sib_id) else {
            return;
        };
        // Not an anchor-bearing kind → no entry.
        let Some(sib) = anchor_fields(sib_node) else {
            return;
        };
        // The sibling's size must be authored and px-convertible.
        let (Some(sib_w), Some(sib_h)) = (
            resolve_geometry_px(sib.w, env.resolved),
            resolve_geometry_px(sib.h, env.resolved),
        ) else {
            return;
        };
        // The sibling's origin: explicit-wins-per-axis (authored x/y), else its
        // own anchor-map entry, else unresolved (no entry for this node).
        let entry = map.get(sib_id).copied();
        let sib_x = resolve_geometry_px(sib.x, env.resolved).or(entry.map(|e| e.0));
        let sib_y = resolve_geometry_px(sib.y, env.resolved).or(entry.map(|e| e.1));
        let (Some(sib_x), Some(sib_y)) = (sib_x, sib_y) else {
            return;
        };

        // When anchor-edge is set, use adjacent-edge placement instead of
        // the within-box anchor_xy derivation.
        if let Some(edge) = edge {
            let gap = anchor_gap
                .and_then(|d| dim_to_px(d.value, &d.unit))
                .unwrap_or(0.0);
            let (x, y) = match edge {
                AnchorEdge::Below => (
                    cross_h(anchor_parsed, sib_x, sib_w, node_w),
                    sib_y + sib_h + gap,
                ),
                AnchorEdge::Above => (
                    cross_h(anchor_parsed, sib_x, sib_w, node_w),
                    sib_y - gap - node_h,
                ),
                AnchorEdge::After => (
                    sib_x + sib_w + gap,
                    cross_v(anchor_parsed, sib_y, sib_h, node_h),
                ),
                AnchorEdge::Before => (
                    sib_x - gap - node_w,
                    cross_v(anchor_parsed, sib_y, sib_h, node_h),
                ),
            };
            map.insert(id.to_owned(), (x, y));
            return;
        }

        // Classic within-box sibling derivation (anchor-edge absent).
        // anchor_parsed is Some here because we returned early above when both
        // anchor_str and edge are None, and zone path has returned. When
        // anchor_str was None and edge is also None we returned above, so
        // anchor_parsed must be Some for this branch.
        let anchor = match anchor_parsed {
            Some(a) => a,
            None => return,
        };
        let (ox, oy) = anchor_xy(anchor, sib_w, sib_h, node_w, node_h);
        map.insert(id.to_owned(), (sib_x + ox, sib_y + oy));
        return;
    }

    // Edge placement without an anchor-sibling: no entry (sibling is required
    // for edge placement; validation warns separately).
    if edge.is_some() {
        return;
    }

    // From here: classic non-edge paths (anchor-parent, page-relative).
    // anchor_parsed must be Some at this point (we exited early when both
    // anchor_str and edge are None; edge is None here; so anchor_str was Some
    // and anchor_parsed is Some unless it was unrecognized, but unrecognized
    // anchor with no edge already returned above).
    let anchor = match anchor_parsed {
        Some(a) => a,
        None => return,
    };

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

    // Page-relative: origin is (0, 0).
    let (ox, oy) = anchor_xy(anchor, env.page_w, env.page_h, node_w, node_h);
    map.insert(id.to_owned(), (ox, oy));
}
