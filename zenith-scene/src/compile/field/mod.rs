//! Field-node resolution and the master-page / running-head / folio system.
//!
//! A `field` is the building block of the master-page projection: at compile
//! time each field is resolved against its page (folio index, parity, live
//! area, and document-wide page-index lookups) into a concrete single-line
//! [`zenith_core::TextNode`] that the caller compiles through the normal text
//! path. This module is wiring only; the concerns live in submodules:
//!
//! - [`resolve`] — the per-page [`FieldCtx`] and `resolve_field_to_text`.
//! - [`section`] — per-page section assignment for section-relative folios.
//! - [`projection`] — document/page node indexing, node-box collection, and
//!   live-area computation.
//! - [`folio`] — folio-number formatting (decimal / Roman).

mod folio;
mod projection;
mod resolve;
mod section;

pub(in crate::compile) use folio::format_folio;
pub(in crate::compile) use projection::{
    build_node_boxes, build_page_index_map, compute_live_area,
};
pub(in crate::compile) use resolve::{FieldCtx, resolve_field_to_text};
pub(in crate::compile) use section::build_section_assignments;
