//! Document-level validation tests.
//!
//! Split out of the former single-file `validate/check/tests.rs` into
//! concern-grouped submodules. Every `#[test]` is moved verbatim — only the
//! surrounding module/import scaffolding changed. Shared builder helpers (and
//! the AST/diagnostic types the test bodies construct) live in [`common`] and
//! are pulled into each submodule via `use super::common::*;`.

mod common;

mod assets_image;
mod contrast;
mod ellipse_line;
mod fold;
mod group_frame;
mod instance;
mod library_provenance;
mod masters_fields;
mod nodes_basic;
mod offcanvas;
mod page_margin;
mod polygon_connector;
mod safezone;
mod sections_spread_toc;
mod styles;
mod table;
