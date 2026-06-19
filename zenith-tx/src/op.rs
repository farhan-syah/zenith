//! Transaction envelope: [`Transaction`] and the [`Op`] enum.
//!
//! Deserializes from JSON like:
//! ```json
//! {"ops":[
//!   {"op":"set_text_align","node":"label","align":"center"},
//!   {"op":"set_fill","node":"box","fill":"color.accent"},
//!   {"op":"set_visible","node":"box","visible":false},
//!   {"op":"set_locked","node":"box","locked":true},
//!   {"op":"set_geometry","node":"r","x":10,"w":200},
//!   {"op":"set_points","node":"poly","points":[{"x":0,"y":0},{"x":100,"y":0},{"x":50,"y":80}]}
//! ]}
//! ```

use crate::TxError;

/// A 2-D vertex used by [`Op::SetPoints`], expressed in pixels.
///
/// JSON shape: `{"x": 50.0, "y": 80.0}`
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct OpPoint {
    /// X coordinate in document pixels.
    pub x: f64,
    /// Y coordinate in document pixels.
    pub y: f64,
}

/// A batch of operations to apply to a document in order.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Transaction {
    pub ops: Vec<Op>,
}

impl Transaction {
    /// Parse a `Transaction` from a JSON string.
    pub fn from_json(s: &str) -> Result<Transaction, TxError> {
        serde_json::from_str(s).map_err(|e| TxError {
            message: format!("failed to parse transaction JSON: {e}"),
        })
    }
}

/// A single operation within a [`Transaction`].
///
/// The `op` field in JSON is the snake_case tag, e.g. `"set_text_align"`.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Set the `align` property on a text node.
    ///
    /// Valid values: `start`, `center`, `end`, `justify`.
    SetTextAlign {
        /// The stable node `id` to target.
        node: String,
        /// The new alignment value.
        align: String,
    },
    /// Move a node one sibling position toward the end (front/top of z-order).
    ///
    /// Has no effect if the node is already last in its parent's children.
    MoveForward {
        /// The stable node `id` to target.
        node: String,
    },
    /// Move a node one sibling position toward the beginning (back/bottom of z-order).
    ///
    /// Has no effect if the node is already first in its parent's children.
    MoveBackward {
        /// The stable node `id` to target.
        node: String,
    },
    /// Move a node to the topmost position (last child) in its parent's children.
    ///
    /// Has no effect if the node is already the last sibling (frontmost/topmost).
    MoveToFront {
        /// The stable node `id` to target.
        node: String,
    },
    /// Move a node to the bottommost position (first child) in its parent's children.
    ///
    /// Has no effect if the node is already the first sibling (backmost/bottommost).
    MoveToBack {
        /// The stable node `id` to target.
        node: String,
    },
    /// Set the `fill` property on a node that supports fill.
    ///
    /// The `fill` value is a token id (e.g. `"color.accent"`); the engine
    /// wraps it as `PropertyValue::TokenRef(fill)`. Post-validation rejects
    /// unknown token ids automatically.
    ///
    /// Supported nodes: `rect`, `ellipse`, `text`, `polygon`, `polyline`.
    /// Unsupported: `line`, `frame`, `group`, `image` — yields
    /// `tx.unsupported_property`.
    SetFill {
        /// The stable node `id` to target.
        node: String,
        /// Token id to set as the fill (e.g. `"color.brand"`).
        fill: String,
    },
    /// Show or hide a node by setting its `visible` property.
    ///
    /// All known node variants except `Unknown` support this property.
    SetVisible {
        /// The stable node `id` to target.
        node: String,
        /// `false` hides the node; `true` makes it visible.
        visible: bool,
    },
    /// Lock or unlock a node by setting its `locked` property.
    ///
    /// All known node variants except `Unknown` support this property.
    SetLocked {
        /// The stable node `id` to target.
        node: String,
        /// `true` locks the node; `false` unlocks it.
        locked: bool,
    },
    /// Move and/or resize a bbox node by updating its `x`, `y`, `w`, `h`
    /// geometry fields. All four fields are optional — only the fields present
    /// in the JSON payload are changed; omitted fields are left untouched.
    ///
    /// Values are in document pixels (`(px)` unit).
    ///
    /// Supported nodes: `rect`, `ellipse`, `frame`, `image`.
    /// Unsupported: `line` (uses x1/y1/x2/y2), `polygon`, `polyline` (no bbox),
    /// `text`, `group`, `unknown` — yields `tx.unsupported_property`.
    ///
    /// If all four fields are omitted, an advisory `tx.noop` is emitted and no
    /// node is recorded as affected.
    ///
    /// JSON example (partial — only x and w change):
    /// ```json
    /// {"op":"set_geometry","node":"r","x":10,"w":200}
    /// ```
    SetGeometry {
        /// The stable node `id` to target.
        node: String,
        /// New left edge in pixels. Omit to leave unchanged.
        #[serde(default)]
        x: Option<f64>,
        /// New top edge in pixels. Omit to leave unchanged.
        #[serde(default)]
        y: Option<f64>,
        /// New width in pixels. Omit to leave unchanged.
        #[serde(default)]
        w: Option<f64>,
        /// New height in pixels. Omit to leave unchanged.
        #[serde(default)]
        h: Option<f64>,
    },
    /// Replace the entire vertex list of a `polygon` or `polyline` node.
    ///
    /// Post-validation rejects automatically if the new point count falls
    /// below the node's minimum (`polygon` needs ≥ 3, `polyline` needs ≥ 2).
    ///
    /// Supported nodes: `polygon`, `polyline`.
    /// Unsupported: all other variants — yields `tx.unsupported_property`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"set_points","node":"poly","points":[{"x":0,"y":0},{"x":100,"y":0},{"x":50,"y":80}]}
    /// ```
    SetPoints {
        /// The stable node `id` to target.
        node: String,
        /// Replacement vertex list. Each vertex is in document pixels.
        points: Vec<OpPoint>,
    },
}
