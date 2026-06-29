//! Shared cell placement: the deterministic HTML-table occupancy walk plus the
//! geometry/declared-box helpers reused by the sizing, collapse, and emit passes.

use std::collections::{BTreeMap, BTreeSet};

use zenith_core::{ResolvedToken, TableRow};

use super::super::util::resolve_geometry_px;

/// One placed cell after the HTML-table occupancy walk: its top-left grid
/// position plus resolved column/row spans. Shared by the width, height, and
/// emission passes so cell placement is byte-identical across all three.
pub(in crate::compile) struct PlacedCell<'a> {
    /// 0-based starting row.
    pub(super) row: usize,
    /// 0-based starting column.
    pub(super) col: usize,
    /// Column span (≥1, clamped to the grid).
    pub(super) cs: usize,
    /// Row span (≥1, clamped to the grid).
    pub(super) rs: usize,
    pub(super) cell: &'a zenith_core::TableCell,
}

/// Walk the table's rows with a deterministic occupancy grid (HTML-table cell
/// flow honoring `colspan`/`rowspan`) and return every placed cell in emission
/// order. This is the SINGLE placement walk reused by the auto-width pass, the
/// row-height pass, and the emit pass, so a cell occupies byte-identical slots
/// in measurement and rendering.
pub(in crate::compile) fn place_cells(
    rows: &[TableRow],
    col_count: usize,
    row_count: usize,
) -> Vec<PlacedCell<'_>> {
    let mut placed: Vec<PlacedCell> = Vec::new();
    let mut occupied: BTreeSet<(usize, usize)> = BTreeSet::new();

    for (r, row) in rows.iter().enumerate() {
        let mut col_cursor = 0usize;
        for cell in &row.cells {
            while col_cursor < col_count && occupied.contains(&(r, col_cursor)) {
                col_cursor += 1;
            }
            if col_cursor >= col_count {
                break;
            }
            let cs = (cell.colspan.max(1) as usize).min(col_count - col_cursor);
            let rs = (cell.rowspan.max(1) as usize).min(row_count - r);
            for dr in 0..rs {
                for dc in 0..cs {
                    occupied.insert((r + dr, col_cursor + dc));
                }
            }
            placed.push(PlacedCell {
                row: r,
                col: col_cursor,
                cs,
                rs,
                cell,
            });
            col_cursor += cs;
        }
    }
    placed
}

/// Geometry of one placed cell in absolute page pixels (already including the
/// table origin but NOT the cell-padding inset).
pub(super) struct CellRect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
}

/// The declared `(w, h)` of a cell child in pixels, when the kind carries a
/// box and the dimensions resolve. Used to compute alignment slack. Kinds
/// without a resolvable box yield `(None, None)`.
pub(super) fn child_declared_box(
    node: &zenith_core::Node,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> (Option<f64>, Option<f64>) {
    use zenith_core::Node;
    let px = |d: &Option<zenith_core::PropertyValue>| resolve_geometry_px(d.as_ref(), resolved);
    match node {
        Node::Rect(n) => (px(&n.w), px(&n.h)),
        Node::Ellipse(n) => (px(&n.w), px(&n.h)),
        Node::Text(n) => (px(&n.w), px(&n.h)),
        Node::Code(n) => (px(&n.w), px(&n.h)),
        Node::Image(n) => (px(&n.w), px(&n.h)),
        Node::Frame(n) => (px(&n.w), px(&n.h)),
        Node::Group(n) => (px(&n.w), px(&n.h)),
        Node::Field(n) => (px(&n.w), px(&n.h)),
        Node::Toc(n) => (px(&n.w), px(&n.h)),
        Node::Table(n) => (px(&n.w), px(&n.h)),
        Node::Shape(n) => (px(&n.w), px(&n.h)),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Mesh(_)
        | Node::Unknown(_) => (None, None),
    }
}

/// The declared `y` of a cell child in pixels, when the kind carries a `y`
/// field and it resolves. Used alongside [`child_declared_box`] to compute the
/// bottom extent of a child within the cell content box. Kinds without a `y`
/// field yield `None`.
pub(super) fn child_declared_y(
    node: &zenith_core::Node,
    resolved: &BTreeMap<String, ResolvedToken>,
) -> Option<f64> {
    use zenith_core::Node;
    let px = |d: &Option<zenith_core::PropertyValue>| resolve_geometry_px(d.as_ref(), resolved);
    match node {
        Node::Rect(n) => px(&n.y),
        Node::Ellipse(n) => px(&n.y),
        Node::Text(n) => px(&n.y),
        Node::Code(n) => px(&n.y),
        Node::Image(n) => px(&n.y),
        Node::Frame(n) => px(&n.y),
        Node::Group(n) => px(&n.y),
        Node::Field(n) => px(&n.y),
        Node::Toc(n) => px(&n.y),
        Node::Table(n) => px(&n.y),
        Node::Shape(n) => px(&n.y),
        Node::Line(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Footnote(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Mesh(_)
        | Node::Unknown(_) => None,
    }
}
