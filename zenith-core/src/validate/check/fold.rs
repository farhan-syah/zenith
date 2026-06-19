//! Page-level fold-line validation.
//!
//! Authors declare non-printing fold-line positions on a page (`fold` children)
//! for tri-fold / bi-fold print layouts. This check compares each direct page
//! child node's bounding box against every declared fold line and emits a
//! `fold.content_crossing` ADVISORY when a node straddles the fold:
//!
//! - **Vertical** fold at `x = P` → crossed when a node bbox spans the line,
//!   i.e. `nx < P < nx + nw`.
//! - **Horizontal** fold at `y = P` → crossed when `ny < P < ny + nh`.
//!
//! Folds are page metadata, never rendered; this check is purely advisory.

use crate::ast::document::{Fold, Page};
use crate::ast::value::dim_to_px;
use crate::diagnostics::Diagnostic;

use super::nodes::{node_bbox, node_id_and_span};

/// Resolve a fold's authored position to pixels.
///
/// Returns `None` when the position is absent or unresolvable; the caller then
/// skips the fold (no panic, no diagnostic).
fn fold_position_px(fold: &Fold) -> Option<f64> {
    let pos = fold.position.as_ref()?;
    dim_to_px(pos.value, &pos.unit)
}

/// Validate every direct page child against every declared fold line.
///
/// Deterministic: folds are iterated in declared order, nodes in child order.
pub(super) fn check_folds(
    page: &Page,
    page_w: f64,
    page_h: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for fold in &page.folds {
        let Some(position) = fold_position_px(fold) else {
            // Missing or unresolvable position — skip this fold entirely.
            continue;
        };

        let is_horizontal = fold.orientation == "horizontal";

        for node in &page.children {
            let Some((nx, ny, nw, nh)) = node_bbox(node, page_w, page_h) else {
                continue;
            };

            let crosses = if is_horizontal {
                ny < position && position < ny + nh
            } else {
                nx < position && position < nx + nw
            };

            if crosses {
                let (node_id, node_span) = node_id_and_span(node);
                let axis = if is_horizontal {
                    "horizontal"
                } else {
                    "vertical"
                };
                diagnostics.push(Diagnostic::advisory(
                    "fold.content_crossing",
                    format!(
                        "node '{}' crosses {} fold '{}' at position {}",
                        node_id, axis, fold.id, position
                    ),
                    node_span,
                    Some(node_id.to_owned()),
                ));
            }
        }
    }
}
