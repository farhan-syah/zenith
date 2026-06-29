//! Effect-producing node structs.
//!
//! These nodes are authored as ordinary page/container children and emit ink,
//! but they live outside the geometric-shape family so future scene effects can
//! share a coherent AST home.

use std::collections::BTreeMap;

use crate::ast::Span;
use crate::ast::value::{Dimension, PropertyValue};

use super::common::UnknownProperty;

/// A soft light source rendered as ambient scene ink.
#[derive(Debug, Clone, PartialEq)]
pub struct LightNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Light family. Recognized values are validated, but the string is kept
    /// open for forward-compatible authoring.
    pub kind: Option<String>,
    /// Center X, page/container-relative.
    pub x: Option<PropertyValue>,
    /// Center Y, page/container-relative.
    pub y: Option<PropertyValue>,
    /// Radial falloff radius.
    pub radius: Option<PropertyValue>,
    /// Inner light color; token refs must resolve to color tokens.
    pub color: Option<PropertyValue>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
    /// Unknown properties preserved for forward-compat.
    pub unknown_props: BTreeMap<String, UnknownProperty>,
    /// Reserved for future focused/angled light families.
    pub angle: Option<Dimension>,
}

/// A procedural grid/perspective mesh rendered as generated stroke ink.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshNode {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<String>,
    /// Mesh family. `None` is interpreted as `orthographic`.
    pub kind: Option<String>,
    pub x: Option<PropertyValue>,
    pub y: Option<PropertyValue>,
    pub w: Option<PropertyValue>,
    pub h: Option<PropertyValue>,
    /// Number of cells on the horizontal axis. Emits `columns + 1` vertical lines.
    pub columns: Option<u32>,
    /// Number of cells on the vertical axis. Emits `rows + 1` horizontal lines.
    pub rows: Option<u32>,
    /// One-point perspective vanishing point X. Required when kind is `perspective`.
    pub vanishing_x: Option<PropertyValue>,
    /// One-point perspective vanishing point Y. Required when kind is `perspective`.
    pub vanishing_y: Option<PropertyValue>,
    /// Intentional extension beyond the authored bbox for bleed/overscan.
    pub extend: Option<PropertyValue>,
    pub stroke: Option<PropertyValue>,
    pub stroke_width: Option<PropertyValue>,
    pub stroke_dash: Option<PropertyValue>,
    pub stroke_gap: Option<PropertyValue>,
    pub stroke_linecap: Option<String>,
    pub opacity: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub source_span: Option<Span>,
    pub unknown_props: BTreeMap<String, UnknownProperty>,
}
