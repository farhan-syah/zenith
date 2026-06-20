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
    /// Vertical alignment (`"super"` / `"sub"`). `None` = baseline (inherit).
    #[serde(default)]
    pub vertical_align: Option<String>,
    /// Footnote reference — the id of a page-level footnote. `None` = no ref.
    #[serde(default)]
    pub footnote_ref: Option<String>,
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

/// Per-transaction permission flags that relax otherwise-enforced guards.
///
/// Carried in a transaction's optional `"permissions"` object, e.g.
/// `{"permissions":{"allow_locked":false,"allow_raw_visual_literals":false}}`.
/// Both flags default to `false`, so a transaction JSON that omits the
/// `permissions` key still parses with all guards active.
#[derive(serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Permissions {
    /// When `true`, mutating ops are allowed to target locked nodes.
    /// When `false` (default), a guarded op against a locked node is rejected
    /// with a `node.locked` diagnostic.
    #[serde(default)]
    pub allow_locked: bool,
    /// When `true`, raw (non-token) visual literal values are permitted.
    #[serde(default)]
    pub allow_raw_visual_literals: bool,
}

/// A batch of operations to apply to a document in order.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Transaction {
    pub ops: Vec<Op>,
    /// Permission flags relaxing per-op guards. Defaults to all-`false`
    /// (every guard active) when the `permissions` key is absent from JSON.
    #[serde(default)]
    pub permissions: Permissions,
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
    /// geometry fields, and optionally set its `rotate` angle. All five fields
    /// are optional — only the fields present in the JSON payload are changed;
    /// omitted fields are left untouched.
    ///
    /// Values are in document pixels (`(px)` unit) for `x`/`y`/`w`/`h`.
    /// `rotate` is in degrees (`(deg)` unit at storage; pass a raw `f64` here).
    ///
    /// Supported nodes for x/y/w/h: `rect`, `ellipse`, `frame`, `image`,
    /// `text`, `code`, `group`, `field`.
    /// Supported nodes for rotate: `rect`, `ellipse`, `frame`, `image`, `text`,
    /// `code`, `group`, `polygon`, `polyline`.
    /// Unsupported for rotate: `line`, `instance`, `field`, `footnote`,
    /// `unknown` — yields `tx.unsupported_property`.
    ///
    /// If all five fields are omitted, an advisory `tx.noop` is emitted and no
    /// node is recorded as affected.
    ///
    /// JSON example (partial — only x, w, and rotate change):
    /// ```json
    /// {"op":"set_geometry","node":"r","x":10,"w":200,"rotate":45}
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
        /// New rotation in degrees. Omit to leave unchanged.
        #[serde(default)]
        rotate: Option<f64>,
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
    /// Duplicate an entire page (and its full subtree), inserting the copy
    /// immediately after the source page in the document body.
    ///
    /// Unlike [`Op::DuplicateNode`] (leaf-only, v0), this performs a deep clone:
    /// the new page gets `new_id`, and **every descendant node id** in the copy
    /// is suffixed with `id_suffix` so all ids stay unique. Any page-level
    /// `safe_zones[].id` is suffixed the same way.
    ///
    /// `duplicate_page` only *creates* new content and never mutates the source,
    /// so it is exempt from lock enforcement.
    ///
    /// Rejects with `tx.unknown_node` if no page with id `page` exists.
    /// Post-validation rejects the transaction if `id_suffix` fails to keep ids
    /// unique (e.g. an empty suffix) via the `id.duplicate` diagnostic — that is
    /// the safety net; an empty suffix also emits a helpful advisory.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"duplicate_page","page":"page.x","new_id":"page.x2","id_suffix":".v2"}
    /// ```
    DuplicatePage {
        /// Source page id to clone.
        page: String,
        /// Id for the new (duplicated) page.
        new_id: String,
        /// Suffix appended to EVERY descendant node id in the copy (keeps ids unique).
        id_suffix: String,
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
    /// - a node id: the bbox of that node.
    /// - an explicit dimension like `"(px)120"`: align the chosen edge of every
    ///   listed node to that absolute page coordinate. For the horizontal edges
    ///   (`left`, `hcenter`, `right`) the value is an X coordinate; for the
    ///   vertical edges (`top`, `vcenter`, `bottom`) it is a Y coordinate.
    ///
    /// Only nodes supported by `set_geometry` (`rect`, `ellipse`, `frame`,
    /// `image`) with resolvable `x/y/w/h` in px/pt are alignable. Any node
    /// that lacks full geometry is skipped with a `tx.unsupported_property`
    /// warning; the rest are still aligned.
    ///
    /// An unknown `align` value is rejected with `tx.unsupported_property`.
    /// An unknown `anchor` value is rejected with `tx.unsupported_property`.
    /// A `"(px)…"` anchor whose dimension cannot be parsed is rejected with
    /// `tx.invalid_value`.
    /// Fewer than one alignable node emits `tx.noop`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"align_nodes","node_ids":["a","b","caption"],"align":"left","anchor":"(px)120"}
    /// ```
    AlignNodes {
        /// Ids of the nodes to align.
        node_ids: Vec<String>,
        /// Which edge or centre to align to: `left`, `hcenter`, `right`,
        /// `top`, `vcenter`, or `bottom`.
        align: String,
        /// Reference rectangle: `"selection"` (union bbox), `"page"`, a node id,
        /// or an explicit dimension like `"(px)120"`. Defaults to `"selection"`.
        #[serde(default = "default_anchor")]
        anchor: String,
    },
    /// Set the `overflow` property of a `text` or `code` node.
    ///
    /// Valid values: `"fit"`, `"clip"`, `"visible"`. Any other value is rejected
    /// with `tx.invalid_value`.
    ///
    /// Supported nodes: `text`, `code`.
    /// Unsupported: all other variants — yields `tx.wrong_node_type`.
    /// A missing node yields `tx.unknown_node`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"set_text_overflow","node_id":"body","overflow":"visible"}
    /// ```
    SetTextOverflow {
        /// The stable node `id` to target.
        node_id: String,
        /// The new overflow value: `fit`, `clip`, or `visible`.
        overflow: String,
    },
    /// Create a new EMPTY page (no children) and insert it into the document
    /// body at `index` (0-based) or, when `index` is `None`, append it at the
    /// end.
    ///
    /// `w` and `h` are canonical dimension strings like `"(px)1800"` / `"(pt)90"`
    /// (the same `(unit)value` form parsed by other ops). `background`, when
    /// present, is a token-ref id (e.g. `"color.bg"`) stored as
    /// `PropertyValue::TokenRef` — exactly like [`Op::SetFill`].
    ///
    /// Rejects with `tx.duplicate_id` if a page (or any node) already uses `id`.
    /// Rejects with `tx.invalid_value` if `w`/`h` fail to parse as a dimension.
    /// Rejects with `tx.out_of_range` if `index` is past the end of the page list.
    ///
    /// The new page carries no children, safe-zones, folds, margins, or bleed —
    /// it is a blank canvas. Post-validation still runs over the whole document.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"add_page","id":"page.new","w":"(px)1800","h":"(px)1200","index":1}
    /// ```
    AddPage {
        /// Stable id for the new page (must be unique document-wide).
        id: String,
        /// Page width as a canonical dimension string, e.g. `"(px)1800"`.
        w: String,
        /// Page height as a canonical dimension string, e.g. `"(px)1200"`.
        h: String,
        /// Optional background token-ref id (e.g. `"color.bg"`). `None` = no fill.
        #[serde(default)]
        background: Option<String>,
        /// 0-based insert position. `None` appends at the end.
        #[serde(default)]
        index: Option<usize>,
    },
    /// Remove the page whose id == `page` (and its entire subtree) from the
    /// document body.
    ///
    /// Rejects with `tx.unknown_node` if no page with that id exists.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"delete_page","page":"page.old"}
    /// ```
    DeletePage {
        /// Id of the page to remove.
        page: String,
    },
    /// Reorder the document body's pages to match `order`.
    ///
    /// `order` must be a permutation of the existing page ids: the same set,
    /// with no duplicates and nothing missing or extra. On success the pages are
    /// rearranged so their ids follow `order` exactly.
    ///
    /// Rejects with `tx.invalid_value` if `order` is not a permutation (an id is
    /// missing, extra, duplicated, or unknown).
    ///
    /// JSON example:
    /// ```json
    /// {"op":"reorder_pages","order":["page.b","page.a","page.c"]}
    /// ```
    ReorderPages {
        /// The new full ordering of page ids (a permutation of the existing set).
        order: Vec<String>,
    },
    /// Declare a new asset in the document's `assets` block.
    ///
    /// `kind` must be one of `"image"`, `"svg"`, or `"font"`. `src` is a relative
    /// path to the asset file. `sha256` is an optional content-integrity digest.
    ///
    /// Rejected immediately with `tx.duplicate_id` if an asset with `id` already
    /// exists. Post-validation catches `asset.invalid_src` (absolute paths, `../`
    /// components, URLs) and `asset.invalid_kind` (unrecognized kinds).
    ///
    /// JSON example:
    /// ```json
    /// {"op":"add_asset","id":"asset.logo","kind":"image","src":"images/logo.png","sha256":"abc123"}
    /// ```
    AddAsset {
        /// Globally unique asset id (e.g. `"asset.logo"`).
        id: String,
        /// Asset kind string: `"image"`, `"svg"`, or `"font"`.
        kind: String,
        /// Relative path to the asset file.
        src: String,
        /// Optional SHA-256 hex digest for content integrity.
        #[serde(default)]
        sha256: Option<String>,
    },
    /// Set the asset reference on an `image` node.
    ///
    /// The `asset_id` must reference a declared asset. An unknown `asset_id` is
    /// permitted here (post-validation catches it via `asset.unknown_reference`).
    /// An asset of kind `font` is eagerly rejected with `tx.invalid_value` because
    /// image nodes require an `image` or `svg` asset.
    ///
    /// Rejected with `tx.unknown_node` if `node_id` is not found.
    /// Rejected with `tx.wrong_node_type` if `node_id` is not an `image` node.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"set_asset","node_id":"pic","asset_id":"asset.hero"}
    /// ```
    SetAsset {
        /// The stable `id` of the image node to update.
        node_id: String,
        /// The asset id to assign to the image node's `asset` field.
        asset_id: String,
    },
    /// Evenly distribute a set of nodes along one axis so the gaps between
    /// consecutive nodes are equal, keeping the first and last node's outer
    /// edges fixed (standard "distribute spacing" semantics).
    ///
    /// The nodes are ordered by their current position on the chosen axis
    /// before distributing. Requires ≥ 3 alignable nodes; fewer than three
    /// emits `tx.noop` (consistent with `align_nodes`' degenerate-input
    /// convention) and leaves the document unchanged.
    ///
    /// Only nodes supported by `set_geometry` (`rect`, `ellipse`, `frame`,
    /// `image`, `text`, `code`, `group`) with resolvable `x/y/w/h` are
    /// distributable. A listed node that is missing yields `tx.unknown_node`;
    /// a node found but lacking resolvable geometry yields a
    /// `tx.unsupported_property` warning and is skipped.
    ///
    /// An unknown `axis` value is rejected with `tx.unsupported_property`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"distribute_nodes","node_ids":["p1","p2","p3"],"axis":"horizontal"}
    /// ```
    DistributeNodes {
        /// Ids of the nodes to distribute.
        node_ids: Vec<String>,
        /// Axis to distribute along: `"horizontal"` or `"vertical"`.
        axis: String,
    },
    /// Create a new design token in the document's `tokens` block.
    ///
    /// `token_type` is one of `"color"`, `"dimension"`, `"number"`,
    /// `"fontFamily"`, `"fontWeight"`. `value` is the literal in string form:
    /// a color/family string (`"#e11d48"`, `"Inter"`), a dimension string
    /// (`"(px)40"`), or a number (`"700"`, `"1.05"`).
    ///
    /// Eagerly rejected with `tx.duplicate_id` if a token with `id` already
    /// exists.  Gradient/shadow/unknown types are rejected with
    /// `tx.invalid_value` (v0: scalar literal token types only; gradient/shadow
    /// tokens must be authored in source).
    ///
    /// JSON example:
    /// ```json
    /// {"op":"create_token","id":"color.brand","type":"color","value":"#e11d48"}
    /// ```
    CreateToken {
        /// Globally unique token id (e.g. `"color.brand"`).
        id: String,
        /// Token type string: `"color"`, `"dimension"`, `"number"`,
        /// `"fontFamily"`, or `"fontWeight"`.
        #[serde(rename = "type")]
        token_type: String,
        /// Literal value in string form appropriate for the declared type.
        value: String,
    },
    /// Replace the literal value of an existing token, preserving its declared
    /// type.
    ///
    /// `value` is parsed against the token's existing `token_type`; a value
    /// that does not parse for that type is rejected with `tx.invalid_value`.
    /// Rejected with `tx.unknown_token` if no token with `id` exists.
    /// Gradient/shadow tokens cannot be updated via this op → `tx.invalid_value`.
    ///
    /// JSON example:
    /// ```json
    /// {"op":"update_token_value","id":"color.brand","value":"#3b82f6"}
    /// ```
    UpdateTokenValue {
        /// The id of the token to update.
        id: String,
        /// New literal value in string form appropriate for the token's existing type.
        value: String,
    },
    /// Set one recognized visual property on a named style to a token reference.
    ///
    /// `property` is a style property key (`fill`, `stroke`, `stroke-width`,
    /// `font-family`, `font-size`, `font-weight`, `line-height`, `radius`,
    /// `padding`, `gap`, `stroke-alignment`); underscore spellings are accepted
    /// and canonicalized. `value` is a token id, stored as
    /// `PropertyValue::TokenRef`.
    ///
    /// Rejected with `tx.unknown_style` if no style with `style_id` exists, and
    /// `tx.unsupported_property` if `property` is not a recognized style key.
    /// Unknown/incompatible token refs are caught by post-validation
    /// (`token.unknown_reference` / `token.incompatible_property`).
    SetStyleProperty {
        /// The id of the style definition to update (matches `style id="…"`).
        style_id: String,
        /// The style property key to set (e.g. `font-family`, `fill`).
        /// Underscore spellings such as `font_family` are accepted.
        property: String,
        /// Token id to store as `PropertyValue::TokenRef` (e.g. `"font.body"`).
        value: String,
    },
    /// Set the `direction` property on a text node. Valid values: `"ltr"`, `"rtl"`.
    /// Any other value is rejected with `tx.invalid_value`. A missing node yields
    /// `tx.unknown_node`; a non-text node yields `tx.wrong_node_type`.
    SetTextDirection {
        /// The stable node `id` to target.
        node: String,
        /// The new direction value: `"ltr"` or `"rtl"`.
        direction: String,
    },
    /// Literal find-and-replace across text node spans, preserving per-span
    /// formatting. `find` is a literal substring (NOT a regex); all occurrences
    /// within each span's text are replaced. When `node` is given, only that text
    /// node is scoped; when omitted, ALL text nodes in the document are scanned.
    ///
    /// `find` must be non-empty (`tx.invalid_value` otherwise). A scoped `node`
    /// that is missing yields `tx.unknown_node`; a scoped non-text node yields
    /// `tx.wrong_node_type`. If no occurrence is found anywhere in scope, an
    /// advisory `tx.noop` is emitted and no node is recorded as affected.
    ///
    /// **Locked nodes:** a scoped locked node is guarded by the normal lock check
    /// (rejected unless `allow_locked`). In document-wide mode, locked text nodes
    /// are SKIPPED and reported via an advisory `tx.locked_skipped` (warning) that
    /// names them — they are never silently mutated.
    FindReplaceText {
        /// The literal substring to search for (not a regex). Must be non-empty.
        find: String,
        /// The replacement string (may be empty to delete occurrences).
        replace: String,
        /// When `Some(id)`, only the named text node is scoped.
        /// When `None`, all text nodes in the document are scanned.
        #[serde(default)]
        node: Option<String>,
    },
    /// Resize a page (artboard). `w`/`h` are canonical dimension strings like
    /// `"(px)794"` (same form parsed by `add_page`). Rejected with `tx.unknown_node`
    /// if no page with id `page` exists, and `tx.invalid_value` if `w`/`h` fail to
    /// parse or are not finite and > 0.
    ///
    /// NOTE: child node coordinates are NOT reflowed — after shrinking a page,
    /// children may fall outside the new bounds and trigger `off_canvas` advisories
    /// at validation. Repositioning children is a separate concern (set_geometry).
    SetPageSize {
        /// Id of the page to resize.
        page: String,
        /// New page width as a canonical dimension string, e.g. `"(px)794"`.
        w: String,
        /// New page height as a canonical dimension string, e.g. `"(px)1123"`.
        h: String,
    },
    /// Snap a single node's edge (or center) to the boundary of the page that
    /// contains it, with an optional margin inset.
    ///
    /// `edge`: `"left"`, `"right"`, `"top"`, `"bottom"`, `"hcenter"`, `"vcenter"`.
    /// `margin` (default 0) insets the node from that page edge (ignored for the
    /// center edges). For `left`/`top`/`hcenter`/`vcenter` margin is measured from
    /// the low edge; for `right`/`bottom` it is measured from the high edge.
    ///
    /// Computes: left → x = margin; right → x = page_w - node_w - margin;
    /// top → y = margin; bottom → y = page_h - node_h - margin;
    /// hcenter → x = (page_w - node_w)/2; vcenter → y = (page_h - node_h)/2.
    ///
    /// Rejected with `tx.unknown_node` if the node is missing, `tx.unsupported_property`
    /// if `edge` is not one of the six values or the node has no resolvable x/y/w/h
    /// geometry. (Composable: issue two ops — e.g. right + bottom — to snap to a corner.)
    AlignToEdge {
        /// The stable node `id` to snap.
        node: String,
        /// Which edge or centre to snap to: `left`, `right`, `top`, `bottom`,
        /// `hcenter`, or `vcenter`.
        edge: String,
        /// Margin in pixels inset from the page edge. Defaults to 0. Ignored for
        /// `hcenter` and `vcenter`.
        #[serde(default)]
        margin: f64,
    },
}

fn default_anchor() -> String {
    "selection".to_owned()
}
