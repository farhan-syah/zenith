//! Vector leaf-node compilation: rect, ellipse, line, polygon, polyline, shape,
//! and connector.
//!
//! Each `compile_*` entry mirrors the shared leaf signature and emits the same
//! `SceneCommand` stream that the original inline `compile_node` arms produced.
//! This module root is wiring only — the per-concern logic lives in the
//! submodules and is re-exported here so the dispatcher's `use` paths resolve
//! unchanged.

mod common;
mod connector;
mod poly;
mod rect_ellipse;
mod routing;
mod shape;

pub(super) use connector::{ConnectorEnv, compile_connector};
pub(super) use poly::{compile_line, compile_polygon, compile_polyline};
pub(super) use rect_ellipse::{compile_ellipse, compile_rect};
pub(super) use shape::{ShapeCompileEnv, compile_shape};
