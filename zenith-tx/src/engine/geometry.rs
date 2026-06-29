//! Geometry op application: `set_geometry` and `align_nodes`, plus the bbox
//! geometry accessors they share.

use zenith_core::{Diagnostic, Dimension, Document, Node, PropertyValue, Unit, dim_to_px};

use super::structure::parse_dimension_str;
use super::{
    find_node_any_mut, find_node_shared, node_kind_str, px, record_affected, subtree_contains,
};

/// Valid alignment directions for `Op::AlignNodes`.
const VALID_ALIGN_DIRS: &[&str] = &["left", "hcenter", "right", "top", "vcenter", "bottom"];

/// Parse an explicit dimension string of the canonical `"(unit)value"` form
/// (e.g. `"(px)120"`, `"(pt)90"`) into a px magnitude.
///
/// Delegates parsing to [`parse_dimension_str`] (the single canonical
/// `"(unit)value"` parser) then resolves to px via [`dim_to_px`], so the
/// arithmetic matches the rest of the engine. Returns `None` if the string is
/// not parenthesized-unit-prefixed, the numeric tail is not a finite number, or
/// the unit does not resolve to px (e.g. `pct`, `deg`).
fn parse_px_dimension(s: &str) -> Option<f64> {
    let dim = parse_dimension_str(s)?;
    dim_to_px(dim.value, &dim.unit)
}

/// Mutable references to a node's four bbox geometry slots `(x, y, w, h)`.
type GeometryMut<'a> = (
    &'a mut Option<PropertyValue>,
    &'a mut Option<PropertyValue>,
    &'a mut Option<PropertyValue>,
    &'a mut Option<PropertyValue>,
);

/// Return mutable references to the four bbox geometry fields `(x, y, w, h)`,
/// or `None` for node variants excluded from `set_geometry`.
///
/// The bbox nodes — `Rect`, `Ellipse`, `Frame`, `Image`, `Text`, `Code`, and
/// `Group` — are settable: each carries canonical `x/y/w/h` fields (a text/code
/// node's `x/y/w/h` is its text box; a group's `x/y` is a real translation
/// offset applied to its children at render time).
///
/// `Line` is excluded because it has no bbox — it uses `x1/y1/x2/y2` endpoints.
/// `Polygon` and `Polyline` are excluded because they have no bbox either — their
/// geometry is the `points` list. `Unknown` is excluded because its schema is opaque.
pub(in crate::engine) fn node_geometry_mut(node: &mut Node) -> Option<GeometryMut<'_>> {
    match node {
        Node::Rect(r) => Some((&mut r.x, &mut r.y, &mut r.w, &mut r.h)),
        Node::Ellipse(e) => Some((&mut e.x, &mut e.y, &mut e.w, &mut e.h)),
        Node::Frame(f) => Some((&mut f.x, &mut f.y, &mut f.w, &mut f.h)),
        Node::Image(i) => Some((&mut i.x, &mut i.y, &mut i.w, &mut i.h)),
        Node::Text(t) => Some((&mut t.x, &mut t.y, &mut t.w, &mut t.h)),
        Node::Code(c) => Some((&mut c.x, &mut c.y, &mut c.w, &mut c.h)),
        Node::Group(g) => Some((&mut g.x, &mut g.y, &mut g.w, &mut g.h)),
        // A field carries a real x/y/w/h box (the resolved single-line text box),
        // so set_geometry applies to it like any other bbox node.
        Node::Field(f) => Some((&mut f.x, &mut f.y, &mut f.w, &mut f.h)),
        // A toc likewise carries a real x/y/w/h box.
        Node::Toc(t) => Some((&mut t.x, &mut t.y, &mut t.w, &mut t.h)),
        // A table carries a real x/y/w/h box.
        Node::Table(t) => Some((&mut t.x, &mut t.y, &mut t.w, &mut t.h)),
        // A shape carries a real x/y/w/h background box.
        Node::Shape(s) => Some((&mut s.x, &mut s.y, &mut s.w, &mut s.h)),
        // A pattern carries a real x/y/w/h box (the region it tiles over).
        Node::Pattern(p) => Some((&mut p.x, &mut p.y, &mut p.w, &mut p.h)),
        // A chart carries a real x/y/w/h box.
        Node::Chart(c) => Some((&mut c.x, &mut c.y, &mut c.w, &mut c.h)),
        Node::Mesh(m) => Some((&mut m.x, &mut m.y, &mut m.w, &mut m.h)),
        Node::Light(_) => None,
        // `Instance` is excluded: it carries only an x/y origin, no w/h box, so
        // the four-slot bbox setter does not apply. A set_geometry on an instance
        // honestly surfaces tx.unsupported_property rather than silently dropping
        // the requested w/h.
        // A footnote has NO x/y/w/h box (the renderer positions it in the
        // footnote zone), so set_geometry does not apply — it honestly surfaces
        // tx.unsupported_property rather than silently dropping the request.
        // A connector has NO authored box (its endpoints are derived from its
        // targets' boxes), so set_geometry does not apply either.
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Unknown(_) => None,
    }
}

// ── SetGeometry ───────────────────────────────────────────────────────────────

/// Bundled geometry deltas passed to [`apply_set_geometry`].
///
/// Grouping these avoids pushing `apply_set_geometry` past the
/// `clippy::too_many_arguments` threshold without using `#[allow]`.
pub(super) struct GeometryDelta {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub rotate: Option<f64>,
}

/// Return a mutable reference to a node's `rotate` slot, or `None` for node
/// variants that do not carry a `rotate` field.
///
/// Supported: `Rect`, `Ellipse`, `Frame`, `Image`, `Text`, `Code`, `Group`,
/// `Polygon`, `Polyline`, `Table`, `Shape`, `Connector`.
/// Unsupported: `Line`, `Instance`, `Field`, `Toc`, `Footnote`, `Unknown`.
fn node_rotate_mut(node: &mut Node) -> Option<&mut Option<Dimension>> {
    match node {
        Node::Rect(n) => Some(&mut n.rotate),
        Node::Ellipse(n) => Some(&mut n.rotate),
        Node::Frame(n) => Some(&mut n.rotate),
        Node::Image(n) => Some(&mut n.rotate),
        Node::Text(n) => Some(&mut n.rotate),
        Node::Code(n) => Some(&mut n.rotate),
        Node::Group(n) => Some(&mut n.rotate),
        Node::Polygon(n) => Some(&mut n.rotate),
        Node::Polyline(n) => Some(&mut n.rotate),
        Node::Table(n) => Some(&mut n.rotate),
        Node::Shape(n) => Some(&mut n.rotate),
        Node::Connector(n) => Some(&mut n.rotate),
        Node::Pattern(n) => Some(&mut n.rotate),
        Node::Chart(n) => Some(&mut n.rotate),
        Node::Mesh(_) => None,
        Node::Light(_) => None,
        // Line has no rotate field.
        // Instance has no rotate field.
        // Field has no rotate field.
        // Toc has no rotate field.
        // Footnote has no rotate field.
        // Unknown has no rotate field.
        Node::Line(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Toc(_)
        | Node::Footnote(_)
        | Node::Unknown(_) => None,
    }
}

pub(super) fn apply_set_geometry(
    node_id: &str,
    delta: GeometryDelta,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    let GeometryDelta { x, y, w, h, rotate } = delta;

    // Early-out: if every field is None this is a no-op — emit advisory.
    if x.is_none() && y.is_none() && w.is_none() && h.is_none() && rotate.is_none() {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            format!(
                "set_geometry on {:?} specified no fields; document is unchanged",
                node_id
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    match find_node_any_mut(doc, node_id) {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            let kind = node_kind_str(node);

            // Apply x/y/w/h only when the node supports bbox geometry.
            // When x/y/w/h are all None but rotate is Some, we still proceed
            // (no unsupported_property for the geometry side).
            let has_geom_delta = x.is_some() || y.is_some() || w.is_some() || h.is_some();
            if has_geom_delta {
                match node_geometry_mut(node) {
                    None => {
                        diagnostics.push(Diagnostic::error(
                            "tx.unsupported_property",
                            format!(
                                "set_geometry is not supported on a {} node (no x/y/w/h)",
                                kind
                            ),
                            None,
                            Some(node_id.to_owned()),
                        ));
                        return;
                    }
                    Some((nx, ny, nw, nh)) => {
                        if let Some(v) = x {
                            *nx = Some(PropertyValue::Dimension(px(v)));
                        }
                        if let Some(v) = y {
                            *ny = Some(PropertyValue::Dimension(px(v)));
                        }
                        if let Some(v) = w {
                            *nw = Some(PropertyValue::Dimension(px(v)));
                        }
                        if let Some(v) = h {
                            *nh = Some(PropertyValue::Dimension(px(v)));
                        }
                    }
                }
            }

            // Apply rotate when requested.
            if let Some(r) = rotate {
                match node_rotate_mut(node) {
                    None => {
                        diagnostics.push(Diagnostic::error(
                            "tx.unsupported_property",
                            format!("set_geometry: rotate is not supported on {} nodes", kind),
                            None,
                            Some(node_id.to_owned()),
                        ));
                        return;
                    }
                    Some(slot) => {
                        *slot = Some(Dimension {
                            value: r,
                            unit: Unit::Deg,
                        });
                    }
                }
            }

            record_affected(node_id, affected);
        }
    }
}

// ── AlignNodes ────────────────────────────────────────────────────────────────

/// Read the four bbox dimensions of a node as px values, if the node kind
/// supports geometry and all four fields are present and resolvable.
///
/// Returns `Some((x, y, w, h))` or `None` if the node is unsupported, any
/// field is absent, or any unit cannot be converted to px (e.g. `%`, `deg`).
fn read_geometry_px(node: &Node) -> Option<(f64, f64, f64, f64)> {
    let (x, y, w, h) = match node {
        Node::Rect(r) => (r.x.as_ref(), r.y.as_ref(), r.w.as_ref(), r.h.as_ref()),
        Node::Ellipse(e) => (e.x.as_ref(), e.y.as_ref(), e.w.as_ref(), e.h.as_ref()),
        Node::Frame(f) => (f.x.as_ref(), f.y.as_ref(), f.w.as_ref(), f.h.as_ref()),
        Node::Image(i) => (i.x.as_ref(), i.y.as_ref(), i.w.as_ref(), i.h.as_ref()),
        Node::Text(t) => (t.x.as_ref(), t.y.as_ref(), t.w.as_ref(), t.h.as_ref()),
        Node::Code(c) => (c.x.as_ref(), c.y.as_ref(), c.w.as_ref(), c.h.as_ref()),
        Node::Group(g) => (g.x.as_ref(), g.y.as_ref(), g.w.as_ref(), g.h.as_ref()),
        Node::Shape(s) => (s.x.as_ref(), s.y.as_ref(), s.w.as_ref(), s.h.as_ref()),
        Node::Pattern(p) => (p.x.as_ref(), p.y.as_ref(), p.w.as_ref(), p.h.as_ref()),
        Node::Chart(c) => (c.x.as_ref(), c.y.as_ref(), c.w.as_ref(), c.h.as_ref()),
        Node::Mesh(m) => (m.x.as_ref(), m.y.as_ref(), m.w.as_ref(), m.h.as_ref()),
        Node::Light(_) => return None,
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Footnote(_)
        | Node::Toc(_)
        | Node::Table(_)
        | Node::Connector(_)
        | Node::Unknown(_) => return None,
    };
    // Resolve a geometry slot to a px value. Only `Dimension` variants are
    // resolvable here — `set_geometry` always writes `PropertyValue::Dimension(px)`
    // so this is the common case. `TokenRef` geometry (an author-set token
    // reference) is opaque to tx in v1: the token table is not available on the
    // read path, so we return `None` and the node is treated as having
    // unresolvable geometry (skipped or surfaced as `tx.geometry_unresolved`).
    let resolve = |pv: Option<&PropertyValue>| -> Option<f64> {
        match pv {
            Some(PropertyValue::Dimension(d)) => dim_to_px(d.value, &d.unit),
            Some(PropertyValue::TokenRef(_)) => None,
            Some(PropertyValue::Literal(_)) => None,
            Some(PropertyValue::DataRef(_)) => None,
            None => None,
        }
    };
    Some((resolve(x)?, resolve(y)?, resolve(w)?, resolve(h)?))
}

pub(super) fn apply_align_nodes(
    node_ids: &[String],
    align: &str,
    anchor: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate align value.
    if !VALID_ALIGN_DIRS.contains(&align) {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!("align_nodes: unknown align {:?}", align),
            None,
            None,
        ));
        return;
    }

    // `anchor` is "page", "selection", an explicit dimension like "(px)120",
    // or a node id (align relative to that node's bbox). The dimension and
    // node-id forms are resolved when the reference rectangle is computed below;
    // an unparseable dimension or unknown id is rejected there.
    //
    // An explicit-dimension anchor names a single absolute coordinate on the
    // active axis. We detect it eagerly (a leading '(') so a node whose id
    // happened to start with '(' cannot shadow it.
    let dimension_anchor: Option<f64> = if anchor.starts_with('(') {
        match parse_px_dimension(anchor) {
            Some(v) => Some(v),
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.invalid_value",
                    format!(
                        "align_nodes: anchor {:?} is not a resolvable dimension (expected e.g. \"(px)120\")",
                        anchor
                    ),
                    None,
                    None,
                ));
                return;
            }
        }
    } else {
        None
    };

    // ── Phase 1: shared scan — gather bbox and check existence ────────────────
    //
    // We need a shared borrow to read geometry before taking the exclusive borrow
    // to write back. Collect (id, x, y, w, h) for every alignable node.
    // Nodes that are not found or lack resolvable geometry are skipped with
    // advisories; the loop continues so the remaining nodes are still aligned.

    struct NodeBbox {
        id: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    let mut alignable: Vec<NodeBbox> = Vec::new();

    for node_id in node_ids {
        // Shared-borrow scan across all pages.
        let found: Option<Option<(f64, f64, f64, f64)>> = 'page_scan: {
            for page in doc.body.pages.iter() {
                if let Some(node) = find_node_shared(&page.children, node_id) {
                    break 'page_scan Some(read_geometry_px(node));
                }
            }
            None // not found in any page
        };

        match found {
            None => {
                // Node not found at all.
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("align_nodes: node {:?} not found in document", node_id),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(None) => {
                // Node found but geometry is unresolvable (wrong kind or missing/pct field).
                // Use Warning so the caller sees AcceptedWithWarnings and knows a
                // node was silently skipped.
                diagnostics.push(Diagnostic::warning(
                    "tx.geometry_unresolved",
                    format!(
                        "align_nodes: node {:?} has no resolvable x/y/w/h geometry; skipped",
                        node_id
                    ),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(Some((x, y, w, h))) => {
                alignable.push(NodeBbox {
                    id: node_id.clone(),
                    x,
                    y,
                    w,
                    h,
                });
            }
        }
    }

    // Need at least one alignable node to proceed.
    if alignable.is_empty() {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            "align_nodes: no alignable nodes with resolvable geometry; document is unchanged"
                .to_owned(),
            None,
            None,
        ));
        return;
    }

    // ── Compute the reference rectangle ───────────────────────────────────────

    let (ref_left, ref_right, ref_top, ref_bottom) = if let Some(coord) = dimension_anchor {
        // Explicit-dimension anchor: `coord` is the absolute target coordinate
        // on the active axis. Collapse the reference rectangle to that single
        // coordinate on every edge. Only the active axis (selected by `align`)
        // is ever read, so the inactive-axis edges are harmless.
        (coord, coord, coord, coord)
    } else if anchor == "page" {
        // Find the page that contains the first alignable node.
        let first_id = &alignable[0].id;
        let page_opt = doc
            .body
            .pages
            .iter()
            .find(|page| page.children.iter().any(|n| subtree_contains(n, first_id)));
        match page_opt {
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.invalid_parent",
                    format!(
                        "align_nodes: could not locate page containing node {:?}",
                        first_id
                    ),
                    None,
                    Some(first_id.clone()),
                ));
                return;
            }
            Some(page) => {
                let pw = dim_to_px(page.width.value, &page.width.unit);
                let ph = dim_to_px(page.height.value, &page.height.unit);
                match (pw, ph) {
                    (Some(w), Some(h)) => (0.0_f64, w, 0.0_f64, h),
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            "tx.invalid_parent",
                            "align_nodes: page width/height cannot be resolved to px".to_owned(),
                            None,
                            None,
                        ));
                        return;
                    }
                }
            }
        }
    } else if anchor == "selection" {
        // union bbox of all alignable nodes.
        let ref_left = alignable.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let ref_right = alignable
            .iter()
            .map(|n| n.x + n.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let ref_top = alignable.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let ref_bottom = alignable
            .iter()
            .map(|n| n.y + n.h)
            .fold(f64::NEG_INFINITY, f64::max);
        (ref_left, ref_right, ref_top, ref_bottom)
    } else {
        // anchor is a NODE ID: align relative to that node's bbox.
        let found: Option<Option<(f64, f64, f64, f64)>> = 'anchor_scan: {
            for page in doc.body.pages.iter() {
                if let Some(node) = find_node_shared(&page.children, anchor) {
                    break 'anchor_scan Some(read_geometry_px(node));
                }
            }
            None
        };
        match found {
            Some(Some((x, y, w, h))) => (x, x + w, y, y + h),
            Some(None) => {
                diagnostics.push(Diagnostic::error(
                    "tx.unsupported_property",
                    format!(
                        "align_nodes: anchor node {:?} has no resolvable x/y/w/h geometry",
                        anchor
                    ),
                    None,
                    Some(anchor.to_owned()),
                ));
                return;
            }
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!(
                        "align_nodes: anchor {:?} is not \"page\", \"selection\", or a known node id",
                        anchor
                    ),
                    None,
                    Some(anchor.to_owned()),
                ));
                return;
            }
        }
    };

    // ── Phase 2: exclusive borrow — write new x or y for each node ───────────
    //
    // Compute the new position per node from the captured bbox, then apply via
    // find_node_any_mut + node_geometry_mut, mirroring apply_set_geometry's
    // write path.

    for bbox in &alignable {
        let new_x = match align {
            "left" => Some(ref_left),
            "hcenter" => Some((ref_left + ref_right) / 2.0 - bbox.w / 2.0),
            "right" => Some(ref_right - bbox.w),
            _ => None,
        };
        let new_y = match align {
            "top" => Some(ref_top),
            "vcenter" => Some((ref_top + ref_bottom) / 2.0 - bbox.h / 2.0),
            "bottom" => Some(ref_bottom - bbox.h),
            _ => None,
        };

        // At least one of new_x/new_y is Some (align was validated above).
        match find_node_any_mut(doc, &bbox.id) {
            None => {
                // Should not happen: we found it in phase 1, but guard anyway.
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("align_nodes: node {:?} disappeared between phases", bbox.id),
                    None,
                    Some(bbox.id.clone()),
                ));
            }
            Some(node) => {
                // node_geometry_mut is guaranteed Some here: we filtered on
                // read_geometry_px which uses the same set of node kinds.
                if let Some((nx, ny, _, _)) = node_geometry_mut(node) {
                    if let Some(v) = new_x {
                        *nx = Some(PropertyValue::Dimension(px(v)));
                    }
                    if let Some(v) = new_y {
                        *ny = Some(PropertyValue::Dimension(px(v)));
                    }
                    record_affected(&bbox.id, affected);
                }
            }
        }
    }
}

// ── AlignToEdge ───────────────────────────────────────────────────────────────

/// Valid edge values for `Op::AlignToEdge`.
const VALID_EDGES: &[&str] = &["left", "right", "top", "bottom", "hcenter", "vcenter"];

/// Snap a single node's edge (or centre) to the boundary of the page that
/// contains it, with an optional margin inset.
///
/// Horizontal edges (`left`, `right`, `hcenter`) set x; vertical edges
/// (`top`, `bottom`, `vcenter`) set y. The opposite coordinate is untouched.
pub(super) fn apply_align_to_edge(
    node_id: &str,
    edge: &str,
    margin: f64,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate edge value early, before touching the document.
    if !VALID_EDGES.contains(&edge) {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!(
                "align_to_edge: edge {:?} must be one of left,right,top,bottom,hcenter,vcenter",
                edge
            ),
            None,
            Some(node_id.to_owned()),
        ));
        return;
    }

    // ── Phase 1 (shared scan): read the node's geometry and find its page ────

    // Find the node and read its geometry (x, y, w, h) in px.
    let node_geom: Option<Option<(f64, f64, f64, f64)>> = 'node_scan: {
        for page in doc.body.pages.iter() {
            if let Some(node) = find_node_shared(&page.children, node_id) {
                break 'node_scan Some(read_geometry_px(node));
            }
        }
        None // not found in any page
    };

    let (_, _, node_w, node_h) = match node_geom {
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!("align_to_edge: node {:?} not found in document", node_id),
                None,
                Some(node_id.to_owned()),
            ));
            return;
        }
        Some(None) => {
            diagnostics.push(Diagnostic::error(
                "tx.unsupported_property",
                format!(
                    "align_to_edge: node {:?} has no resolvable x/y/w/h geometry",
                    node_id
                ),
                None,
                Some(node_id.to_owned()),
            ));
            return;
        }
        Some(Some(geom)) => geom,
    };

    // Find the page that contains the node (same pattern as align_nodes' page branch).
    let page_bounds: Option<(f64, f64)> = doc.body.pages.iter().find_map(|page| {
        if page.children.iter().any(|n| subtree_contains(n, node_id)) {
            let pw = dim_to_px(page.width.value, &page.width.unit);
            let ph = dim_to_px(page.height.value, &page.height.unit);
            pw.zip(ph)
        } else {
            None
        }
    });

    let (page_w, page_h) = match page_bounds {
        Some(bounds) => bounds,
        None => {
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!(
                    "align_to_edge: could not locate page containing node {:?}",
                    node_id
                ),
                None,
                Some(node_id.to_owned()),
            ));
            return;
        }
    };

    // ── Compute new coordinate(s) from shared data ─────────────────────────

    let new_x: Option<f64> = match edge {
        "left" => Some(margin),
        "right" => Some(page_w - node_w - margin),
        "hcenter" => Some((page_w - node_w) / 2.0),
        _ => None,
    };
    let new_y: Option<f64> = match edge {
        "top" => Some(margin),
        "bottom" => Some(page_h - node_h - margin),
        "vcenter" => Some((page_h - node_h) / 2.0),
        _ => None,
    };

    // ── Phase 2 (exclusive borrow): write the new coordinate ─────────────────

    match find_node_any_mut(doc, node_id) {
        None => {
            // Should not happen: we found it in phase 1.
            diagnostics.push(Diagnostic::error(
                "tx.unknown_node",
                format!(
                    "align_to_edge: node {:?} disappeared between phases",
                    node_id
                ),
                None,
                Some(node_id.to_owned()),
            ));
        }
        Some(node) => {
            // node_geometry_mut is guaranteed Some here: read_geometry_px succeeded
            // above, which uses the same set of node variants.
            if let Some((nx, ny, _, _)) = node_geometry_mut(node) {
                if let Some(v) = new_x {
                    *nx = Some(PropertyValue::Dimension(px(v)));
                }
                if let Some(v) = new_y {
                    *ny = Some(PropertyValue::Dimension(px(v)));
                }
                record_affected(node_id, affected);
            }
        }
    }
}

// ── DistributeNodes ─────────────────────────────────────────────────────────────

/// Valid axes for `Op::DistributeNodes`.
const VALID_DISTRIBUTE_AXES: &[&str] = &["horizontal", "vertical"];

/// A node's captured bbox during distribution, reduced to the active axis.
struct AxisBox {
    id: String,
    /// Leading-edge coordinate on the active axis (`x` for horizontal, `y` for vertical).
    pos: f64,
    /// Extent on the active axis (`w` for horizontal, `h` for vertical).
    size: f64,
}

pub(super) fn apply_distribute_nodes(
    node_ids: &[String],
    axis: &str,
    doc: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    affected: &mut Vec<String>,
) {
    // Validate axis value before touching the tree.
    if !VALID_DISTRIBUTE_AXES.contains(&axis) {
        diagnostics.push(Diagnostic::error(
            "tx.unsupported_property",
            format!("distribute_nodes: unknown axis {:?}", axis),
            None,
            None,
        ));
        return;
    }
    let horizontal = axis == "horizontal";

    // ── Phase 1: shared scan — gather active-axis geometry and check existence ──
    //
    // Mirrors apply_align_nodes' phase 1: a missing node is a hard error, a node
    // found without resolvable geometry is skipped with a warning, and the rest
    // are still distributed.
    let mut boxes: Vec<AxisBox> = Vec::new();

    for node_id in node_ids {
        let found: Option<Option<(f64, f64, f64, f64)>> = 'page_scan: {
            for page in doc.body.pages.iter() {
                if let Some(node) = find_node_shared(&page.children, node_id) {
                    break 'page_scan Some(read_geometry_px(node));
                }
            }
            None
        };

        match found {
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("distribute_nodes: node {:?} not found in document", node_id),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(None) => {
                diagnostics.push(Diagnostic::warning(
                    "tx.geometry_unresolved",
                    format!(
                        "distribute_nodes: node {:?} has no resolvable x/y/w/h geometry; skipped",
                        node_id
                    ),
                    None,
                    Some(node_id.clone()),
                ));
            }
            Some(Some((x, y, w, h))) => {
                let (pos, size) = if horizontal { (x, w) } else { (y, h) };
                boxes.push(AxisBox {
                    id: node_id.clone(),
                    pos,
                    size,
                });
            }
        }
    }

    // Distribute-spacing needs ≥ 3 nodes (two fixed endpoints + ≥ 1 interior).
    // Fewer is a no-op, mirroring align_nodes' degenerate-input convention.
    if boxes.len() < 3 {
        diagnostics.push(Diagnostic::advisory(
            "tx.noop",
            format!(
                "distribute_nodes: needs at least 3 alignable nodes but found {}; document is unchanged",
                boxes.len()
            ),
            None,
            None,
        ));
        return;
    }

    // Order by current leading-edge position on the active axis. Ties keep their
    // relative input order (stable sort) for determinism.
    boxes.sort_by(|a, b| a.pos.total_cmp(&b.pos));

    // Endpoints are fixed. Span = last trailing edge − first leading edge.
    // Equal gap = (span − Σ sizes) / (n − 1).
    let (Some(first), Some(last)) = (boxes.first(), boxes.last()) else {
        // Unreachable: len ≥ 3 was checked above.
        return;
    };
    let span = (last.pos + last.size) - first.pos;
    let total_size: f64 = boxes.iter().map(|b| b.size).sum();
    let gap = (span - total_size) / ((boxes.len() - 1) as f64);

    // Walk left-to-right, placing each interior node after the previous one plus
    // the equal gap. Endpoints keep their positions. Collect new positions first,
    // then apply with an exclusive borrow per node (mirrors align's phase 2).
    let mut new_positions: Vec<(String, f64)> = Vec::with_capacity(boxes.len());
    let mut cursor = first.pos;
    for (i, b) in boxes.iter().enumerate() {
        let new_pos = if i == 0 { first.pos } else { cursor + gap };
        new_positions.push((b.id.clone(), new_pos));
        cursor = new_pos + b.size;
    }

    // ── Phase 2: exclusive borrow — write the new active-axis position ──────────
    for (id, new_pos) in &new_positions {
        match find_node_any_mut(doc, id) {
            None => {
                diagnostics.push(Diagnostic::error(
                    "tx.unknown_node",
                    format!("distribute_nodes: node {:?} disappeared between phases", id),
                    None,
                    Some(id.clone()),
                ));
            }
            Some(node) => {
                if let Some((nx, ny, _, _)) = node_geometry_mut(node) {
                    if horizontal {
                        *nx = Some(PropertyValue::Dimension(px(*new_pos)));
                    } else {
                        *ny = Some(PropertyValue::Dimension(px(*new_pos)));
                    }
                    record_affected(id, affected);
                }
            }
        }
    }
}
