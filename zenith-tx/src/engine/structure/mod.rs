//! Structural op application: reorder, add/remove, group/ungroup, reparent,
//! duplicate — plus the container/child finders and tree-mutation helpers they
//! share.
//!
//! This is a wiring-only module root: it declares the concern-grouped submodules
//! and re-exports the items the sibling engine modules dispatch to.

mod add_remove;
mod duplicate;
mod finders;
mod group;
mod page;
mod reorder;

pub(in crate::engine) use add_remove::{apply_add_node, apply_remove_node};
pub(in crate::engine) use duplicate::{apply_duplicate_node, apply_duplicate_page};
pub(in crate::engine) use group::{apply_group, apply_reparent, apply_ungroup};
pub(in crate::engine) use page::{
    AddPageSpec, apply_add_page, apply_delete_page, apply_reorder_pages, apply_set_page_size,
    parse_dimension_str,
};
pub(in crate::engine) use reorder::{ReorderKind, apply_reorder};
