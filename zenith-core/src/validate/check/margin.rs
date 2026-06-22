//! Page-level book-margin (live-area) validation.
//!
//! Authors declare book live-area margins on a page (`margin-inner`,
//! `margin-outer`, `margin-top`, `margin-bottom`). Combined with the document
//! `mirror-margins` toggle and the page's 1-based index, these define the
//! parity-correct LIVE AREA rectangle for the page. This check compares each
//! direct page child node's authored bounding box against that rectangle and
//! emits a `margin.violation` ADVISORY when the node protrudes past any edge.
//!
//! Page parity (1-based index, in document source order):
//! - **recto** = ODD page (1, 3, 5 …) → binding on the LEFT.
//! - **verso** = EVEN page (2, 4, 6 …) → binding on the RIGHT.
//!
//! Live-area left edge (LTR books, `page-progression` absent or "ltr"):
//! - `mirror_margins == Some(true)`:
//!   - recto → `live_x = margin_inner` (inner/binding margin on the left);
//!   - verso → `live_x = margin_outer` (inner/binding margin on the right, so
//!     the OUTER margin is the left inset).
//! - otherwise → `live_x = margin_inner` on every page (uniform).
//!
//! For an RTL book (`page-progression="rtl"`) the spread is MIRRORED: the
//! binding (inner) margin sits on the RIGHT for recto and on the LEFT for verso.
//! So with `mirror_margins`, recto → `live_x = margin_outer` and verso →
//! `live_x = margin_inner` — the exact opposite of the LTR parity. Width, top,
//! and bottom are unchanged.
//!
//! In all cases:
//! - `live_y = margin_top`
//! - `live_w = page_w - margin_inner - margin_outer`
//! - `live_h = page_h - margin_top - margin_bottom`
//!
//! This validates the margin grid. It is purely advisory: margins are v0
//! metadata and do NOT auto-reposition nodes (that is master-page / flow-frame
//! work). Nodes with `role="guide"` are exempt (guides intentionally live in the
//! margins). The check is skipped entirely when any margin is absent.

use crate::ast::document::{Document, Page};
use crate::ast::value::dim_to_px;
use crate::diagnostics::Diagnostic;

use super::nodes::{node_bbox, node_id_and_span, node_role};

/// Fully-resolved book live-area rectangle in pixels: `(x, y, w, h)`.
struct LiveArea {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Page pixel dimensions plus the parity/mirroring context needed to resolve a
/// page's parity-correct live area.
#[derive(Clone, Copy)]
pub(super) struct PageMarginCtx {
    pub(super) page_w: f64,
    pub(super) page_h: f64,
    /// Page's resolved recto/verso parity (single source of truth).
    pub(super) is_recto: bool,
    /// Document `mirror-margins` toggle.
    pub(super) mirror_margins: bool,
    /// `true` for an RTL book (`page-progression="rtl"`).
    pub(super) rtl: bool,
}

/// Resolve the parity-correct live area for `page`, given the page pixel
/// dimensions, the page's resolved recto/verso parity, and the document mirror
/// toggle.
///
/// `is_recto` is the page's parity as resolved by
/// [`Document::page_is_recto`](crate::ast::document::Document::page_is_recto) —
/// the single source of truth (explicit `page.parity` > document
/// `page-parity-start` > default `index % 2 == 1`). This function never
/// recomputes parity from the page index.
///
/// Returns `None` when any of the four EFFECTIVE margins is absent or resolves
/// to a non-pixel (pct/deg/unknown) unit — the caller then skips the check (no
/// panic, no diagnostic): margins are advisory and an unresolvable margin
/// simply yields no live area to validate against.
///
/// Each side's effective margin is the page's own value when set, else the
/// document-level default ([`Document::effective_margins`]) — the single source
/// of truth for the document→page margin cascade. With no document margins set
/// this reads exactly the page's own values, so the default-off path is
/// byte-identical.
fn live_area(doc: &Document, page: &Page, ctx: PageMarginCtx) -> Option<LiveArea> {
    let PageMarginCtx {
        page_w,
        page_h,
        is_recto,
        mirror_margins,
        rtl,
    } = ctx;
    let (inner_opt, outer_opt, top_opt, bottom_opt) = doc.effective_margins(page);
    let inner_dim = inner_opt.as_ref()?;
    let outer_dim = outer_opt.as_ref()?;
    let top_dim = top_opt.as_ref()?;
    let bottom_dim = bottom_opt.as_ref()?;

    let inner = dim_to_px(inner_dim.value, &inner_dim.unit)?;
    let outer = dim_to_px(outer_dim.value, &outer_dim.unit)?;
    let top = dim_to_px(top_dim.value, &top_dim.unit)?;
    let bottom = dim_to_px(bottom_dim.value, &bottom_dim.unit)?;

    // `is_recto` is the page's resolved parity (single source of truth). For an
    // LTR book the binding (inner) margin is on the LEFT for recto; for an RTL
    // book (page-progression="rtl") the whole spread is mirrored, so the binding
    // is on the RIGHT for recto. `inner_on_right` is then true for recto under RTL
    // and for verso under LTR.
    let inner_on_right = if rtl { is_recto } else { !is_recto };
    let left_inset = if mirror_margins && inner_on_right {
        // Binding (inner) is on the RIGHT, so the OUTER margin insets the left
        // edge.
        outer
    } else {
        // Inner margin insets the left edge.
        inner
    };

    Some(LiveArea {
        x: left_inset,
        y: top,
        w: page_w - inner - outer,
        h: page_h - top - bottom,
    })
}

/// Validate every direct page child against the page's parity-correct live area.
///
/// `is_recto` is the page's resolved parity (from
/// [`Document::page_is_recto`](crate::ast::document::Document::page_is_recto)).
/// Deterministic: nodes are iterated in child order. Skipped when any margin is
/// absent/unresolvable, and skipped per-node for `role="guide"` nodes.
pub(super) fn check_margins(
    doc: &Document,
    page: &Page,
    ctx: PageMarginCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (page_w, page_h, is_recto) = (ctx.page_w, ctx.page_h, ctx.is_recto);
    let Some(area) = live_area(doc, page, ctx) else {
        // Some margin is absent or unresolvable — nothing to validate against.
        return;
    };

    const EPSILON: f64 = 0.5;
    let parity = if is_recto { "recto" } else { "verso" };

    for node in &page.children {
        // Guides intentionally live in the margins; exempt them.
        if node_role(node) == Some("guide") {
            continue;
        }
        let Some((nx, ny, nw, nh)) = node_bbox(node, page_w, page_h) else {
            continue;
        };

        // Collect every violated edge so the advisory names which side(s).
        let mut edges: Vec<&str> = Vec::new();
        if nx < area.x - EPSILON {
            edges.push("left");
        }
        if ny < area.y - EPSILON {
            edges.push("top");
        }
        if nx + nw > area.x + area.w + EPSILON {
            edges.push("right");
        }
        if ny + nh > area.y + area.h + EPSILON {
            edges.push("bottom");
        }

        if edges.is_empty() {
            continue;
        }

        let (node_id, node_span) = node_id_and_span(node);
        diagnostics.push(Diagnostic::advisory(
            "margin.violation",
            format!(
                "node '{}' falls outside the {} page live area \
                 (x {:.0}, y {:.0}, w {:.0}, h {:.0}); crosses the {} margin edge(s)",
                node_id,
                parity,
                area.x,
                area.y,
                area.w,
                area.h,
                edges.join(", ")
            ),
            node_span,
            Some(node_id.to_owned()),
        ));
    }
}
