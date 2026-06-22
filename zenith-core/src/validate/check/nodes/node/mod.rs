//! Per-kind node-validation helpers, split out of the `nodes` dispatcher.
//!
//! Each submodule holds the `check_*` function(s) for a group of node kinds;
//! [`super::nodes::walk_node`] is the thin dispatcher that runs the shared
//! prologue advisories, dispatches to these helpers, and performs container
//! recursion. The [`shared`] submodule holds geometry/AST/anchor/style helpers
//! reused by every per-kind check.

pub(super) mod shared;

mod container;
mod leaf;
mod shape;
mod special;
mod text;

pub(in crate::validate::check) use container::{check_frame, check_group, check_table};
pub(in crate::validate::check) use leaf::{check_code, check_ellipse, check_line, check_rect};
pub(in crate::validate::check) use shape::{check_connector, check_shape, check_unknown};
pub(in crate::validate::check) use special::{
    check_field, check_footnote, check_instance, check_polygon, check_polyline, check_toc,
};
pub(in crate::validate::check) use text::{check_image, check_text};
