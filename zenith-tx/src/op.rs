//! Transaction envelope: [`Transaction`] and the [`Op`] enum.
//!
//! Deserializes from JSON like:
//! ```json
//! {"ops":[
//!   {"op":"set_text_align","node":"label","align":"center"},
//!   {"op":"set_fill","node":"box","fill":"color.accent"},
//!   {"op":"set_stroke","node":"box","stroke":"color.rule"},
//!   {"op":"set_stroke_width","node":"box","stroke_width":"size.stroke"},
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

/// A single text span used by [`Op::ReplaceText`].
///
/// JSON shape: `{"text":"Hello","fill":"color.brand","italic":true}`.
/// All fields except `text` are optional and default to `None`/absent.
/// `fill` and `font_weight` are token ids (like [`Op::SetFill`]), not raw values.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct OpSpan {
    /// The literal text content of this span.
    pub text: String,
    /// Token id to set as the per-span fill (e.g. `"color.brand"`). `None` = inherit.
    #[serde(default)]
    pub fill: Option<String>,
    /// Token id to set as the per-span font-weight. `None` = inherit.
    #[serde(default)]
    pub font_weight: Option<String>,
    /// Italic override. `None` = inherit.
    #[serde(default)]
    pub italic: Option<bool>,
    /// Underline decoration. `None` = inherit.
    #[serde(default)]
    pub underline: Option<bool>,
    /// Strikethrough decoration. `None` = inherit.
    #[serde(default)]
    pub strikethrough: Option<bool>,
}

/// Insertion position for [`Op::AddNode`] within a container's children.
///
/// JSON shapes: `{"at":"last"}`, `{"at":"first"}`, `{"at":"index","index":2}`,
/// `{"at":"before","id":"sibling"}`, `{"at":"after","id":"sibling"}`.
#[derive(serde::Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum Position {
    /// Insert as the last child (topmost in z-order). Default.
    #[default]
    Last,
    /// Insert as the first child (bottommost in z-order).
    First,
    /// Insert at an explicit index (clamped to the children length).
    Index { index: usize },
    /// Insert immediately before the sibling with this id.
    Before { id: String },
    /// Insert immediately after the sibling with this id.
    After { id: String },
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
    /// Set the `stroke` (outline color) property on a node that supports stroke.
    ///
    /// The `stroke` value is a token id (e.g. `"color.rule"`); the engine wraps it
    /// as `PropertyValue::TokenRef(stroke)`. Post-validation rejects unknown token
    /// ids automatically.
    ///
    /// Supported nodes: `rect`, `line`, `polygon`, `polyline`.
    /// Unsupported: `ellipse` (fill-only), `text`, `frame`, `group`, `image` —
    /// yields `tx.unsupported_property`.
    SetStroke {
        /// The stable node `id` to target.
        node: String,
        /// Token id to set as the stroke color (e.g. `"color.rule"`).
        stroke: String,
    },
    /// Set the `stroke-width` property on a node that supports stroke.
    ///
    /// The value is a **dimension token id** (e.g. `"size.stroke"`), stored as
    /// `PropertyValue::TokenRef`. A token (not a raw number) is required because
    /// v0 stroke-width only resolves through dimension tokens at compile time;
    /// post-validation rejects unknown token ids automatically.
    ///
    /// Supported nodes: `rect`, `line`, `polygon`, `polyline`.
    /// Unsupported: `ellipse`, `text`, `frame`, `group`, `image` — yields
    /// `tx.unsupported_property`.
    SetStrokeWidth {
        /// The stable node `id` to target.
        node: String,
        /// Dimension token id to set as the stroke width (e.g. `"size.stroke"`).
        stroke_width: String,
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
    /// Construct a new node from a `.zen` source fragment and insert it into a
    /// container (a page, group, or frame) at a chosen position.
    ///
    /// `source` is a single `.zen` node fragment, e.g.
    /// `rect id="box" x=(px)10 y=(px)10 w=(px)100 h=(px)80 fill=(token)"color.accent"`.
    /// It is parsed through the canonical KDL parser, so every node kind, nested
    /// children (for group/frame), tokens, and properties are supported with no
    /// per-field mapping. Exactly one top-level node must be present.
    ///
    /// Post-validation rejects an incomplete/invalid node automatically (missing
    /// required geometry, duplicate id, unknown token/asset ref, too few points, …).
    AddNode {
        /// Stable id of the container to insert into: a page id, or a group/frame id.
        parent: String,
        /// Where among the container's children to insert. Defaults to `last`.
        #[serde(default)]
        position: Position,
        /// A single `.zen` node fragment to construct and insert.
        source: String,
    },
    /// Remove a node (and its subtree) by id from whatever container holds it.
    ///
    /// Rejects with `tx.unknown_node` if no node with that id exists.
    RemoveNode {
        /// The stable node `id` to remove.
        node: String,
    },
    /// Set the `opacity` of a node (0.0 = fully transparent, 1.0 = fully opaque).
    ///
    /// The value is clamped to `[0.0, 1.0]` before being stored.
    ///
    /// Supported nodes: all concrete variants (`rect`, `ellipse`, `line`, `text`,
    /// `code`, `frame`, `group`, `image`, `polygon`, `polyline`).
    /// Unsupported: `unknown` — yields `tx.unsupported_property`.
    SetOpacity {
        /// The stable node `id` to target.
        node: String,
        /// New opacity value; clamped to `[0.0, 1.0]`.
        opacity: f64,
    },
    /// Replace the entire span list of a `text` node with a new set of spans.
    ///
    /// The `spans` vec fully replaces `TextNode.spans`. Replacing with an empty
    /// vec is valid and clears all text content. `fill` and `font_weight` in each
    /// [`OpSpan`] are token ids wrapped as `PropertyValue::TokenRef`; post-validation
    /// rejects unknown token ids automatically (same as `set_fill`).
    ///
    /// Supported nodes: `text` only.
    /// Unsupported: all other variants — yields `tx.unsupported_property`.
    ReplaceText {
        /// The stable node `id` to target.
        node: String,
        /// Replacement span list. Each span's `text` is required; all other fields
        /// are optional and default to `None` (inherit from node-level styles).
        spans: Vec<OpSpan>,
    },
    /// Duplicate a leaf node, assigning it a new id, and insert the clone
    /// immediately after the original in the same parent's children.
    ///
    /// **v0 scope — leaf nodes only.** Duplicating a container (`frame` or
    /// `group`) is rejected with `tx.unsupported_property`. A deep-clone would
    /// copy all descendant ids, producing duplicate ids throughout the subtree;
    /// re-id'ing an entire subtree is deferred to a future version.
    ///
    /// Post-validation catches a `new_id` that collides with an existing node
    /// id via the `id.duplicate` diagnostic (same as [`Op::AddNode`]).
    ///
    /// Rejects with `tx.unknown_node` if `node` does not exist in the document.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"duplicate_node","node":"box","new_id":"box-copy"}
    /// ```
    DuplicateNode {
        /// The stable id of the node to duplicate.
        node: String,
        /// The id to assign to the newly created clone.
        new_id: String,
    },
    /// Wrap a set of sibling nodes inside a new group node.
    ///
    /// All `node_ids` must be **direct siblings under the same parent**
    /// (a page, group, or frame). If any id is not found, or if the ids
    /// do not all share one common parent, the op is rejected with
    /// `tx.invalid_parent`.
    ///
    /// The new group is inserted at the position of the **earliest** (lowest
    /// index) member, preserving z-order. The grouped nodes are transferred
    /// into the new group in their original relative order.
    ///
    /// Post-validation catches a `group_id` that collides with an existing
    /// node id via the `id.duplicate` diagnostic.
    ///
    /// **v0 note:** the group is created with `x`/`y` = `None` (no translation
    /// offset). Children keep their authored coordinates; any visual shift must
    /// be handled by the caller by adjusting child geometry separately.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"group","node_ids":["rect1","rect2"],"group_id":"grp-new"}
    /// ```
    Group {
        /// Ids of the nodes to group. Must be ≥ 1 and share a common parent.
        node_ids: Vec<String>,
        /// The id to assign to the newly created group node.
        group_id: String,
    },
    /// Dissolve a group node, moving its children up to the group's parent.
    ///
    /// The group is replaced in-place by its children (spliced at the group's
    /// original index), preserving source order.
    ///
    /// Rejects with `tx.unknown_node` if `group_id` is not found.
    /// Rejects with `tx.unsupported_property` ("not a group") if the node is
    /// not a `group` variant.
    ///
    /// **v0 limitation:** the group's own `x`/`y` translation is NOT applied
    /// to children on ungroup (children keep their authored coordinates). If the
    /// group had a non-zero `x`/`y` offset, the rendered positions of children
    /// may shift after ungroup. An advisory is emitted in that case.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"ungroup","group_id":"grp1"}
    /// ```
    Ungroup {
        /// The id of the group node to dissolve.
        group_id: String,
    },
    /// Move a node to a different container (page, group, or frame).
    ///
    /// Rejects with `tx.unknown_node` if `node` is not found.
    /// Rejects with `tx.invalid_parent` if `new_parent` is not a container
    /// (page, group, or frame), or if `new_parent` is `node` itself or a
    /// descendant of `node` (cycle detection).
    ///
    /// `position` controls where in the new parent's children the node is
    /// inserted; defaults to [`Position::Last`] (top of z-order).
    ///
    /// JSON example:
    /// ```json
    /// {"op":"reparent","node":"rect1","new_parent":"grp1","position":{"at":"last"}}
    /// ```
    Reparent {
        /// The stable id of the node to move.
        node: String,
        /// The id of the container to move the node into.
        new_parent: String,
        /// Where to insert the node in the new parent. Defaults to `last`.
        #[serde(default)]
        position: Position,
    },
    /// Align a set of nodes to a common edge or centre along one axis.
    ///
    /// `align` controls the alignment target:
    /// - Horizontal: `"left"`, `"hcenter"`, `"right"`
    /// - Vertical: `"top"`, `"vcenter"`, `"bottom"`
    ///
    /// `anchor` controls the reference rectangle:
    /// - `"selection"` (default): the union bounding box of all alignable nodes.
    /// - `"page"`: the page that contains the nodes (0,0 to page w/h).
    ///
    /// Only nodes supported by `set_geometry` (`rect`, `ellipse`, `frame`,
    /// `image`) with resolvable `x/y/w/h` in px/pt are alignable. Any node
    /// that lacks full geometry is skipped with a `tx.unsupported_property`
    /// warning; the rest are still aligned.
    ///
    /// An unknown `align` value is rejected with `tx.unsupported_property`.
    /// An unknown `anchor` value is rejected with `tx.unsupported_property`.
    /// Fewer than one alignable node emits `tx.noop`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"align_nodes","node_ids":["r1","r2","r3"],"align":"left"}
    /// ```
    AlignNodes {
        /// Ids of the nodes to align.
        node_ids: Vec<String>,
        /// Which edge or centre to align to: `left`, `hcenter`, `right`,
        /// `top`, `vcenter`, or `bottom`.
        align: String,
        /// Reference rectangle: `"selection"` (union bbox) or `"page"`.
        /// Defaults to `"selection"`.
        #[serde(default = "default_anchor")]
        anchor: String,
    },
}

fn default_anchor() -> String {
    "selection".to_owned()
}
