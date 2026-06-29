//! WCAG 3 (APCA) contrast advisory check.
//!
//! Compares text-node fills against the colour they visually sit ON — the
//! topmost preceding sibling shape (rect / ellipse / frame) that fully
//! contains the text and has an opaque fill, falling back to the page
//! background colour — and emits a `contrast.low` warning when the APCA
//! lightness contrast (`Lc`) is below the WCAG 3 minimum.

use std::collections::BTreeMap;

use crate::ast::node::Node;
use crate::ast::style::Style;
use crate::ast::value::{PropertyValue, dim_to_px};
use crate::color::{apca_lc, parse_rgb};
use crate::diagnostics::Diagnostic;
use crate::tokens::{ResolvedToken, ResolvedValue};

use super::nodes::node_bbox;

/// Minimum alpha for a backdrop fill to be treated as opaque enough to act as
/// the effective background. Fills below this (e.g. a translucent scrim) are
/// skipped so they don't override a more solid backdrop or the page colour.
const BACKDROP_OPAQUE_ALPHA: u8 = 128;

/// Resolve a fill property to an `(r, g, b, a)` tuple.
///
/// Mirrors the text-fill resolution path: a direct `fill` property, falling
/// back to the referenced `style` block's `fill`, must be a `TokenRef` →
/// `Color` token whose hex parses. The alpha byte is recovered from the hex
/// (`#rrggbbaa`); a 6-digit `#rrggbb` is treated as fully opaque (`255`).
///
/// Returns `None` when no fill is set, it doesn't reference a colour token, or
/// the hex fails to parse.
fn resolve_fill_rgba(
    fill: &Option<PropertyValue>,
    style: &Option<String>,
    style_map: &BTreeMap<&str, &Style>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
) -> Option<(u8, u8, u8, u8)> {
    let style_fill = || {
        style_map
            .get(style.as_deref()?)
            .and_then(|s| s.properties.get("fill"))
    };

    let pv = fill.as_ref().or_else(style_fill)?;
    let PropertyValue::TokenRef(id) = pv else {
        return None;
    };
    let rt = resolved_tokens.get(id.as_str())?;
    let ResolvedValue::Color(hex) = &rt.value else {
        return None;
    };

    let (r, g, b) = parse_rgb(hex)?;
    // Recover alpha from an 8-digit `#rrggbbaa`; default opaque otherwise.
    let alpha = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 8)
        .and_then(|h| u8::from_str_radix(&h[6..8], 16).ok())
        .unwrap_or(255);
    Some((r, g, b, alpha))
}

/// Find the effective backdrop colour for a text node: the topmost preceding
/// sibling shape (rect / ellipse / frame) that fully contains the text bbox and
/// has an opaque-enough fill. Returns `None` when no such shape qualifies, in
/// which case the caller falls back to the page background.
fn backdrop_rgb(
    text_bbox: (f64, f64, f64, f64),
    preceding_siblings: &[Node],
    page_w: f64,
    page_h: f64,
    style_map: &BTreeMap<&str, &Style>,
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
) -> Option<(u8, u8, u8)> {
    let (tx, ty, tw, th) = text_bbox;
    // FrameNode has no `fill` field, so its backdrop colour can only come from
    // a referenced style; this `None` stands in for the absent direct fill.
    let no_fill: Option<PropertyValue> = None;

    // Iterate topmost-first (later siblings paint on top of earlier ones).
    for sibling in preceding_siblings.iter().rev() {
        // Only fillable backdrop shapes qualify.
        let (fill, style) = match sibling {
            Node::Rect(r) => (&r.fill, &r.style),
            Node::Ellipse(e) => (&e.fill, &e.style),
            Node::Frame(f) => (&no_fill, &f.style),
            Node::Line(_)
            | Node::Text(_)
            | Node::Code(_)
            | Node::Group(_)
            | Node::Image(_)
            | Node::Polygon(_)
            | Node::Polyline(_)
            | Node::Instance(_)
            | Node::Field(_)
            | Node::Footnote(_)
            | Node::Toc(_)
            | Node::Table(_)
            | Node::Shape(_)
            | Node::Connector(_)
            | Node::Pattern(_)
            | Node::Chart(_)
            | Node::Light(_)
            | Node::Unknown(_) => continue,
        };

        let Some((r, g, b, a)) = resolve_fill_rgba(fill, style, style_map, resolved_tokens) else {
            continue;
        };
        // Skip mostly-transparent fills so a scrim doesn't override.
        if a < BACKDROP_OPAQUE_ALPHA {
            continue;
        }

        let Some((bx, by, bw, bh)) = node_bbox(sibling, page_w, page_h) else {
            continue;
        };
        // The text must lie fully inside the shape.
        if bx <= tx && by <= ty && bx + bw >= tx + tw && by + bh >= ty + th {
            return Some((r, g, b));
        }
    }
    None
}

/// Recursively check text nodes for WCAG AA contrast against their effective
/// background.
///
/// The effective background is the topmost preceding sibling shape (rect /
/// ellipse / frame) that fully contains the text and has an opaque-enough fill
/// — i.e. the filled shape the text visually sits ON — falling back to the page
/// background when no such shape qualifies.
///
/// `preceding_siblings` are the nodes painted UNDER `node` (lower z-order, same
/// parent); `page_w` / `page_h` are the resolved page pixel bounds used to
/// compute node bounding boxes.
///
/// # v0 Limitations
/// - Backdrop detection considers only DIRECT preceding siblings of the text
///   node (at page level, or within the same group/frame). A backdrop shape in
///   an outer scope, or group translation offsets, are not accumulated — bounds
///   use authored coordinates, matching the off_canvas advisory.
/// - Per-span fills (TextSpan.fill) are NOT individually checked; the node-level
///   `fill` is used as a proxy for all spans.
/// - Fill / font-size / font-weight are resolved from the node's direct
///   property when present, otherwise from the referenced `style` block's
///   matching property (`fill` / `font-size` / `font-weight`). A node with
///   neither a direct nor a style-inherited fill is simply skipped. Per-span
///   fills (TextSpan.fill) are still not individually consulted here.
pub(super) fn check_text_contrast(
    node: &Node,
    page_bg_rgb: Option<(u8, u8, u8)>,
    preceding_siblings: &[Node],
    page_size: (f64, f64),
    resolved_tokens: &BTreeMap<String, ResolvedToken>,
    style_map: &BTreeMap<&str, &Style>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (page_w, page_h) = page_size;
    match node {
        Node::Text(t) => {
            // Effective property = direct node property, falling back to the
            // referenced style block's matching property when the node omits it.
            let style_prop = |key: &str| -> Option<&PropertyValue> {
                style_map
                    .get(t.style.as_deref()?)
                    .and_then(|s| s.properties.get(key))
            };

            // Resolve the text fill color from a TokenRef → Color token.
            // If no fill is set or it doesn't resolve to a color, skip.
            let text_rgb = match t.fill.as_ref().or_else(|| style_prop("fill")) {
                Some(PropertyValue::TokenRef(id)) => {
                    resolved_tokens.get(id.as_str()).and_then(|rt| {
                        if let ResolvedValue::Color(hex) = &rt.value {
                            parse_rgb(hex)
                        } else {
                            None
                        }
                    })
                }
                // Literal / Dimension / DataRef fills are either caught as
                // raw_visual_literal errors elsewhere or will resolve at scene time;
                // no need to chase them here.
                Some(PropertyValue::Literal(_))
                | Some(PropertyValue::Dimension(_))
                | Some(PropertyValue::DataRef(_))
                | None => None,
            };

            let Some(fg_rgb) = text_rgb else {
                return;
            };

            // Resolve an explicit `contrast-bg` hint (TOP priority): a TokenRef →
            // Color token whose hex parses. Used for text over an `image` or
            // other non-fillable backdrop the validator cannot sample.
            let hint_rgb = match t.contrast_bg.as_ref() {
                Some(PropertyValue::TokenRef(id)) => {
                    resolved_tokens.get(id.as_str()).and_then(|rt| {
                        if let ResolvedValue::Color(hex) = &rt.value {
                            parse_rgb(hex)
                        } else {
                            None
                        }
                    })
                }
                // Literal / Dimension / DataRef hints are caught as raw_visual_literal
                // errors elsewhere or resolve at scene time; no need to chase them here.
                Some(PropertyValue::Literal(_))
                | Some(PropertyValue::Dimension(_))
                | Some(PropertyValue::DataRef(_))
                | None => None,
            };

            // Resolve the EFFECTIVE background. Precedence:
            //   contrast-bg hint > detected backdrop > page background.
            // If none is known we cannot compute contrast — bail.
            let backdrop = node_bbox(node, page_w, page_h).and_then(|tbbox| {
                backdrop_rgb(
                    tbbox,
                    preceding_siblings,
                    page_w,
                    page_h,
                    style_map,
                    resolved_tokens,
                )
            });
            let bg_source = if hint_rgb.is_some() {
                "contrast-bg hint"
            } else if backdrop.is_some() {
                "backdrop"
            } else {
                "page background"
            };
            let Some(bg_rgb) = hint_rgb.or(backdrop).or(page_bg_rgb) else {
                return;
            };

            // Resolve font-size in px (default 16.0 px when absent).
            let size_px: f64 = t
                .font_size
                .as_ref()
                .or_else(|| style_prop("font-size"))
                .and_then(|pv| {
                    if let PropertyValue::TokenRef(id) = pv {
                        resolved_tokens.get(id.as_str()).and_then(|rt| {
                            if let ResolvedValue::Dimension(dim) = &rt.value {
                                dim_to_px(dim.value, &dim.unit)
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(16.0);

            // Resolve font-weight as u32 (default 400 when absent).
            let weight: u32 = t
                .font_weight
                .as_ref()
                .or_else(|| style_prop("font-weight"))
                .and_then(|pv| {
                    if let PropertyValue::TokenRef(id) = pv {
                        resolved_tokens.get(id.as_str()).and_then(|rt| {
                            if let ResolvedValue::FontWeight(w) = &rt.value {
                                Some(*w)
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(400);

            // Large text needs less contrast: >= 24 px OR >= 18.66 px bold.
            // APCA Lc minimums: 60 for body/normal text, 45 for large/bold.
            let is_large = size_px >= 24.0 || (size_px >= 18.66 && weight >= 700);
            let threshold = if is_large { 45.0_f64 } else { 60.0_f64 };

            // APCA is polarity-aware; the readability bar is on the magnitude.
            let lc = apca_lc(fg_rgb, bg_rgb).abs();

            if lc < threshold {
                diagnostics.push(Diagnostic::warning(
                    "contrast.low",
                    format!(
                        "text '{}': APCA contrast Lc {:.1} of fill on {} \
                         is below the WCAG 3 minimum (Lc {:.0})",
                        t.id, lc, bg_source, threshold
                    ),
                    t.source_span,
                    Some(t.id.clone()),
                ));
            }
        }

        // Recurse into container nodes, passing the same page_bg through and
        // threading each child's own preceding siblings so a text node sitting
        // on a shape WITHIN the container is judged against that shape.
        // Group and Frame children may contain text nodes.
        Node::Group(g) => {
            for (i, child) in g.children.iter().enumerate() {
                check_text_contrast(
                    child,
                    page_bg_rgb,
                    &g.children[..i],
                    (page_w, page_h),
                    resolved_tokens,
                    style_map,
                    diagnostics,
                );
            }
        }
        Node::Frame(f) => {
            for (i, child) in f.children.iter().enumerate() {
                check_text_contrast(
                    child,
                    page_bg_rgb,
                    &f.children[..i],
                    (page_w, page_h),
                    resolved_tokens,
                    style_map,
                    diagnostics,
                );
            }
        }
        Node::Table(t) => {
            // Recurse into each cell's children, using the cell's effective
            // background fill as the fallback `page_bg_rgb` so that text inside
            // a filled cell is judged against the cell colour, not the page bg.
            //
            // Cell background precedence (mirrors emit.rs):
            //   header cell: cell.fill > table.header_fill > table.fill > page bg
            //   body cell:   cell.fill > table.fill > page bg
            let header_rows = t.header_rows.unwrap_or(0);

            // Resolve a table/cell fill PropertyValue to an opaque rgb.
            // Table and cell fills have no style-block fallback, so we pass
            // `&None` for the `style` argument; `resolve_fill_rgba` short-circuits
            // cleanly and returns the same result as the inline closure would.
            let resolve_fill = |pv: &Option<PropertyValue>| -> Option<(u8, u8, u8)> {
                let (r, g, b, a) = resolve_fill_rgba(pv, &None, style_map, resolved_tokens)?;
                if a >= BACKDROP_OPAQUE_ALPHA {
                    Some((r, g, b))
                } else {
                    None
                }
            };

            for (row_idx, row) in t.rows.iter().enumerate() {
                let is_header = (row_idx as u32) < header_rows;

                for cell in &row.cells {
                    // Resolve the cell's effective background through the
                    // precedence chain, stopping at the first opaque colour.
                    let cell_bg: Option<(u8, u8, u8)> = if let Some(rgb) = resolve_fill(&cell.fill)
                    {
                        Some(rgb)
                    } else if is_header {
                        resolve_fill(&t.header_fill)
                            .or_else(|| resolve_fill(&t.fill))
                            .or(page_bg_rgb)
                    } else {
                        resolve_fill(&t.fill).or(page_bg_rgb)
                    };

                    for (i, child) in cell.children.iter().enumerate() {
                        check_text_contrast(
                            child,
                            cell_bg,
                            &cell.children[..i],
                            (page_w, page_h),
                            resolved_tokens,
                            style_map,
                            diagnostics,
                        );
                    }
                }
            }
        }

        // Leaf node types carry no text children — nothing to check.
        Node::Rect(_)
        | Node::Ellipse(_)
        | Node::Line(_)
        | Node::Code(_)
        | Node::Image(_)
        | Node::Polygon(_)
        | Node::Polyline(_)
        | Node::Instance(_)
        | Node::Field(_)
        | Node::Footnote(_)
        | Node::Toc(_)
        | Node::Shape(_)
        | Node::Connector(_)
        | Node::Pattern(_)
        | Node::Chart(_)
        | Node::Light(_)
        | Node::Unknown(_) => {}
    }
}
