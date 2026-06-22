//! Zenith library subsystem: pack format, registry, and resolver.
//!
//! A library "pack" is a `.zen` file whose IDENTITY is declared by a single
//! `library` SELF-entry in its own `libraries` block, for example:
//!
//! ```kdl
//! libraries { library id="@zenith/flowchart" version="1.0.0" }
//! ```
//!
//! That entry's `id` is the package id and `version` is the pack version. A
//! pack's ITEMS are its `components`, filter/mask `tokens`, and `actions`:
//! item `decision` in pack `@zenith/flowchart` is addressed
//! `@zenith/flowchart#decision`.
//!
//! PRESET packs are embedded in the binary via [`include_str!`] (see
//! [`registry::EMBEDDED_PACKS`]); PROJECT packs live in
//! `<project_dir>/libraries/*.zen` and are scanned at runtime. Resolution order
//! is project packs first, then embedded presets (a project pack shadows an
//! embedded pack of the same id).
//!
//! This module contains pure pack-loading/registry logic only; the CLI command
//! that consumes it lives in [`crate::commands::library`]. The submodules group
//! by concern: [`registry`] (pack model + parse + resolve), [`add`] (shared
//! materialization machinery), and one module per `materialize*` flavor
//! ([`component`], [`token`], [`action`]).

mod action;
mod add;
mod component;
mod registry;
mod token;

#[cfg(test)]
mod tests;

// ── Public API (crate-internal callers in `commands::library` / `lib.rs`) ─────

pub use registry::{
    EMBEDDED_PACKS, ItemKind, LibraryPack, PackError, PackItem, PackSource, load_embedded_packs,
    load_project_packs, parse_pack, resolve_packs,
};

pub use add::{AddError, AddOutcome, collect_all_ids, load_pack_document, parse_spec};

pub use action::{ActionAddOutcome, materialize_action};
pub use component::materialize;
pub use token::{TokenAddOutcome, materialize_token};
