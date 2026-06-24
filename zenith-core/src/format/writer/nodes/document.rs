//! The `document` body, the `page` node, and its page-metadata children
//! (`safe-zone`, `fold`).

use crate::ast::{DocumentBody, Fold, Page, SafeZone, SafeZoneType};

use crate::format::writer::{
    fmt_dimension, indent, write_opt_dimension, write_opt_property_value, write_opt_str,
};

use super::write_children_block;

// ---------------------------------------------------------------------------
// Document body
// ---------------------------------------------------------------------------

pub(in crate::format::writer) fn write_document_body(
    body: &DocumentBody,
    out: &mut String,
    depth: usize,
) {
    indent(out, depth);
    out.push_str("document");
    out.push_str(" id=\"");
    out.push_str(&body.id);
    out.push('"');
    write_opt_str(out, "title", &body.title);
    out.push_str(" {\n");

    for page in &body.pages {
        write_page(page, out, depth + 1);
    }

    indent(out, depth);
    out.push_str("}\n");
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

fn write_page(page: &Page, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("page");
    // Canonical order: id, name, w, h, background
    out.push_str(" id=\"");
    out.push_str(&page.id);
    out.push('"');
    write_opt_str(out, "name", &page.name);
    out.push_str(" w=");
    out.push_str(&fmt_dimension(&page.width));
    out.push_str(" h=");
    out.push_str(&fmt_dimension(&page.height));
    write_opt_property_value(out, "background", &page.background);
    write_opt_dimension(out, "bleed", &page.bleed);
    write_opt_dimension(out, "baseline-grid", &page.baseline_grid);
    write_opt_str(out, "line-jumps", &page.line_jumps);
    write_opt_dimension(out, "margin-inner", &page.margin_inner);
    write_opt_dimension(out, "margin-outer", &page.margin_outer);
    write_opt_dimension(out, "margin-top", &page.margin_top);
    write_opt_dimension(out, "margin-bottom", &page.margin_bottom);
    write_opt_str(out, "parity", &page.parity);
    write_opt_str(out, "master", &page.master);

    out.push_str(" {\n");
    // Safe-zones and folds are page metadata, emitted before the renderable
    // children.
    for zone in &page.safe_zones {
        write_safe_zone(zone, out, depth + 1);
    }
    for fold in &page.folds {
        write_fold(fold, out, depth + 1);
    }
    write_children_block(&page.children, out, depth);
    indent(out, depth);
    out.push_str("}\n");
}

/// Emit a single `safe-zone` line:
/// `safe-zone id="..." type="exclusion|required" x=(px)N y=(px)N w=(px)N h=(px)N label="..."`
/// (`label` is omitted when `None`).
fn write_safe_zone(zone: &SafeZone, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("safe-zone");
    out.push_str(" id=\"");
    out.push_str(&zone.id);
    out.push('"');
    out.push_str(" type=\"");
    out.push_str(match zone.zone_type {
        SafeZoneType::Exclusion => "exclusion",
        SafeZoneType::Required => "required",
    });
    out.push('"');
    out.push_str(" x=");
    out.push_str(&fmt_dimension(&zone.x));
    out.push_str(" y=");
    out.push_str(&fmt_dimension(&zone.y));
    out.push_str(" w=");
    out.push_str(&fmt_dimension(&zone.w));
    out.push_str(" h=");
    out.push_str(&fmt_dimension(&zone.h));
    write_opt_str(out, "label", &zone.label);
    out.push('\n');
}

/// Emit a single `fold` line:
/// `fold id="..." orientation="vertical|horizontal" position=(px)N`
/// (`position` is omitted when `None`).
fn write_fold(fold: &Fold, out: &mut String, depth: usize) {
    indent(out, depth);
    out.push_str("fold");
    out.push_str(" id=\"");
    out.push_str(&fold.id);
    out.push('"');
    out.push_str(" orientation=\"");
    out.push_str(&fold.orientation);
    out.push('"');
    write_opt_dimension(out, "position", &fold.position);
    out.push('\n');
}
