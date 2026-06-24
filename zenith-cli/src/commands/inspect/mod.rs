//! `zenith inspect` command — module wiring.
//!
//! - [`document`]    — error type, tree types, tree builders, node finder,
//!   geometry helpers, human renderers, and the public `run` entry point.
//! - [`recipes`]     — recipe-block JSON builder and human renderer.
//! - [`agent_runs`]  — agent-runs block JSON builder and human renderer.

pub mod agent_runs;
mod document;
pub mod recipes;

pub use document::{
    InspectCmdErr, InspectNodeOutput, InspectOutput, NodeEntry, NodeGeometry, PageEntry,
    build_doc_tree, find_node_tree, run,
};
