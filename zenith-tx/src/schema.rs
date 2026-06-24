//! Static schema metadata for the transaction op set.
//!
//! Exposes the canonical list of op names (as their JSON `op` tag strings),
//! one-line summaries per op, field-level schema (name, type hint, required
//! flag) per op, a minimal JSON example per op, and compile-time drift guards
//! that force a compile error whenever a new `Op` variant is added without
//! updating this module.

// ── Canonical op name list ────────────────────────────────────────────────────

/// All transaction op names in their JSON `op` tag form (snake_case).
///
/// The list is sorted for deterministic output. The drift-guard test
/// `op_summary_covers_every_op` enforces that this list exactly matches the
/// `Op` enum variants.
pub fn op_names() -> &'static [&'static str] {
    &[
        "add_asset",
        "add_node",
        "add_page",
        "align_nodes",
        "align_to_edge",
        "create_recipe",
        "create_token",
        "delete_page",
        "delete_recipe",
        "detach_pattern",
        "distribute_nodes",
        "duplicate_node",
        "duplicate_page",
        "find_replace_text",
        "group",
        "move_backward",
        "move_forward",
        "move_to_back",
        "move_to_front",
        "reparent",
        "reorder_pages",
        "remove_node",
        "replace_text",
        "set_asset",
        "set_fill",
        "set_geometry",
        "set_locked",
        "set_opacity",
        "set_page_size",
        "set_points",
        "set_stroke",
        "set_stroke_width",
        "set_style_property",
        "set_text_align",
        "set_text_direction",
        "set_text_overflow",
        "set_visible",
        "ungroup",
        "update_recipe",
        "update_token_value",
    ]
}

// ── One-line summaries ────────────────────────────────────────────────────────

/// Return a one-line description of the named op, or `None` if unrecognised.
pub fn op_summary(name: &str) -> Option<&'static str> {
    match name {
        "set_text_align" => Some("Set the text alignment of a text node."),
        "move_forward" => {
            Some("Move a node one sibling position toward the front (top of z-order).")
        }
        "move_backward" => {
            Some("Move a node one sibling position toward the back (bottom of z-order).")
        }
        "move_to_front" => Some("Move a node to the topmost (last-child) position in its parent."),
        "move_to_back" => {
            Some("Move a node to the bottommost (first-child) position in its parent.")
        }
        "set_fill" => Some("Set the fill color of a node to a token reference."),
        "set_stroke" => Some("Set the stroke (outline) color of a node to a token reference."),
        "set_stroke_width" => {
            Some("Set the stroke width of a node to a dimension token reference.")
        }
        "set_visible" => Some("Show or hide a node by toggling its visible property."),
        "set_locked" => Some("Lock or unlock a node to prevent accidental edits."),
        "set_geometry" => Some("Move and/or resize a node by setting x, y, w, h, or rotate."),
        "set_points" => Some("Replace the full vertex list of a polygon or polyline node."),
        "add_node" => Some("Parse a .zen source fragment and insert it into a container."),
        "remove_node" => Some("Remove a node and its subtree from the document."),
        "set_opacity" => Some("Set the opacity of a node (0.0 = fully transparent, 1.0 = opaque)."),
        "replace_text" => Some("Replace all text spans of a text or shape node."),
        "duplicate_node" => Some("Clone a leaf node and insert the copy after the original."),
        "duplicate_page" => Some("Deep-clone a page and insert the copy after the original."),
        "group" => Some("Wrap a set of sibling nodes inside a new group node."),
        "ungroup" => Some("Dissolve a group node, moving its children up to the parent."),
        "reparent" => Some("Move a node into a different container (page, group, or frame)."),
        "align_nodes" => Some("Align a set of nodes to a common edge or centre along one axis."),
        "set_text_overflow" => {
            Some("Set the overflow mode (fit, clip, or visible) of a text or code node.")
        }
        "add_page" => Some("Create a new empty page and insert it at the given index."),
        "delete_page" => Some("Remove a page and its entire subtree from the document."),
        "reorder_pages" => Some("Reorder all document pages to match the given id permutation."),
        "add_asset" => Some("Declare a new asset (image, svg, or font) in the assets block."),
        "set_asset" => Some("Assign an asset reference to an image node."),
        "distribute_nodes" => {
            Some("Evenly space a set of nodes along a horizontal or vertical axis.")
        }
        "create_token" => Some("Create a new scalar design token in the tokens block."),
        "update_token_value" => Some("Replace the literal value of an existing design token."),
        "set_style_property" => {
            Some("Set a recognized visual property on a named style to a token reference.")
        }
        "set_text_direction" => Some("Set the text direction (ltr or rtl) of a text node."),
        "find_replace_text" => Some("Literal find-and-replace across text and shape label spans."),
        "set_page_size" => Some("Resize a page by setting new width and height dimensions."),
        "align_to_edge" => {
            Some("Snap a node's edge or centre to the boundary of its containing page.")
        }
        "create_recipe" => Some("Create a new recipe entry in the document's recipes block."),
        "update_recipe" => Some("Replace the scalar fields of an existing recipe."),
        "delete_recipe" => Some("Remove a recipe from the document's recipes block."),
        "detach_pattern" => {
            Some("Materialize a pattern node into an editable group of native shapes.")
        }
        _ => None,
    }
}

// ── Field-level schema ────────────────────────────────────────────────────────

/// One JSON field belonging to a transaction op (excluding the `"op"` tag).
#[derive(Debug, Clone, PartialEq)]
pub struct OpFieldSchema {
    /// The JSON key name for this field.
    pub name: &'static str,
    /// Short human/agent-readable type hint, e.g. `"node id"`, `"token ref"`,
    /// `"string"`, `"f64"`, `"bool"`, `"enum: left|center|right"`.
    pub ty: &'static str,
    /// `true` when the field MUST be present; `false` when it may be omitted.
    pub required: bool,
}

/// Return the JSON fields for a named op (excluding the `"op"` tag itself).
///
/// Returns an empty slice for ops that have no fields (none exist in v0, but
/// the signature is consistent). Returns `None` if `name` is not a known op.
pub fn op_fields(name: &str) -> Option<&'static [OpFieldSchema]> {
    // Each arm returns a reference to a `&'static [OpFieldSchema]`.
    // The slices are defined as `static` inside the match arms to satisfy the
    // `'static` bound without heap allocation.
    match name {
        "set_text_align" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "align",
                    ty: "enum: start|center|end|justify",
                    required: true,
                },
            ];
            Some(F)
        }
        "move_forward" | "move_backward" | "move_to_front" | "move_to_back" | "remove_node"
        | "detach_pattern" => {
            static F: &[OpFieldSchema] = &[OpFieldSchema {
                name: "node",
                ty: "node id",
                required: true,
            }];
            Some(F)
        }
        "set_fill" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "fill",
                    ty: "token ref",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_stroke" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "stroke",
                    ty: "token ref",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_stroke_width" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "stroke_width",
                    ty: "token ref",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_visible" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "visible",
                    ty: "bool",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_locked" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "locked",
                    ty: "bool",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_geometry" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "x",
                    ty: "px",
                    required: false,
                },
                OpFieldSchema {
                    name: "y",
                    ty: "px",
                    required: false,
                },
                OpFieldSchema {
                    name: "w",
                    ty: "px",
                    required: false,
                },
                OpFieldSchema {
                    name: "h",
                    ty: "px",
                    required: false,
                },
                OpFieldSchema {
                    name: "rotate",
                    ty: "f64",
                    required: false,
                },
            ];
            Some(F)
        }
        "set_points" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "points",
                    ty: "[{x:f64,y:f64}]",
                    required: true,
                },
            ];
            Some(F)
        }
        "add_node" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "parent",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "source",
                    ty: ".zen fragment",
                    required: true,
                },
                OpFieldSchema {
                    name: "position",
                    ty: r#"{at:"last"|"first"|"index"|"before"|"after"}"#,
                    required: false,
                },
            ];
            Some(F)
        }
        "set_opacity" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "opacity",
                    ty: "f64",
                    required: true,
                },
            ];
            Some(F)
        }
        "replace_text" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "spans",
                    ty: "[{text,fill?,font_weight?,italic?,…}]",
                    required: true,
                },
            ];
            Some(F)
        }
        "duplicate_node" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "new_id",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "duplicate_page" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "page",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "new_id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "id_suffix",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "group" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node_ids",
                    ty: "node-id[]",
                    required: true,
                },
                OpFieldSchema {
                    name: "group_id",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "ungroup" => {
            static F: &[OpFieldSchema] = &[OpFieldSchema {
                name: "group_id",
                ty: "node id",
                required: true,
            }];
            Some(F)
        }
        "reparent" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "new_parent",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "position",
                    ty: r#"{at:"last"|"first"|"index"|"before"|"after"}"#,
                    required: false,
                },
            ];
            Some(F)
        }
        "align_nodes" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node_ids",
                    ty: "node-id[]",
                    required: true,
                },
                OpFieldSchema {
                    name: "align",
                    ty: "enum: left|hcenter|right|top|vcenter|bottom",
                    required: true,
                },
                OpFieldSchema {
                    name: "anchor",
                    ty: "string",
                    required: false,
                },
            ];
            Some(F)
        }
        "set_text_overflow" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node_id",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "overflow",
                    ty: "enum: fit|clip|visible",
                    required: true,
                },
            ];
            Some(F)
        }
        "add_page" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "w",
                    ty: "px",
                    required: true,
                },
                OpFieldSchema {
                    name: "h",
                    ty: "px",
                    required: true,
                },
                OpFieldSchema {
                    name: "background",
                    ty: "token ref",
                    required: false,
                },
                OpFieldSchema {
                    name: "index",
                    ty: "i64",
                    required: false,
                },
            ];
            Some(F)
        }
        "delete_page" => {
            static F: &[OpFieldSchema] = &[OpFieldSchema {
                name: "page",
                ty: "node id",
                required: true,
            }];
            Some(F)
        }
        "reorder_pages" => {
            static F: &[OpFieldSchema] = &[OpFieldSchema {
                name: "order",
                ty: "node-id[]",
                required: true,
            }];
            Some(F)
        }
        "add_asset" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "kind",
                    ty: "enum: image|svg|font",
                    required: true,
                },
                OpFieldSchema {
                    name: "src",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "sha256",
                    ty: "string",
                    required: false,
                },
            ];
            Some(F)
        }
        "set_asset" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node_id",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "asset_id",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "distribute_nodes" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node_ids",
                    ty: "node-id[]",
                    required: true,
                },
                OpFieldSchema {
                    name: "axis",
                    ty: "enum: horizontal|vertical",
                    required: true,
                },
            ];
            Some(F)
        }
        "create_token" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "type",
                    ty: "enum: color|dimension|number|fontFamily|fontWeight",
                    required: true,
                },
                OpFieldSchema {
                    name: "value",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "update_token_value" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "value",
                    ty: "string",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_style_property" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "style_id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "property",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "value",
                    ty: "token ref",
                    required: true,
                },
            ];
            Some(F)
        }
        "set_text_direction" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "direction",
                    ty: "enum: ltr|rtl",
                    required: true,
                },
            ];
            Some(F)
        }
        "find_replace_text" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "find",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "replace",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: false,
                },
            ];
            Some(F)
        }
        "set_page_size" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "page",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "w",
                    ty: "px",
                    required: true,
                },
                OpFieldSchema {
                    name: "h",
                    ty: "px",
                    required: true,
                },
            ];
            Some(F)
        }
        "align_to_edge" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "node",
                    ty: "node id",
                    required: true,
                },
                OpFieldSchema {
                    name: "edge",
                    ty: "enum: left|right|top|bottom|hcenter|vcenter",
                    required: true,
                },
                OpFieldSchema {
                    name: "margin",
                    ty: "f64",
                    required: false,
                },
            ];
            Some(F)
        }
        "create_recipe" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "kind",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "seed",
                    ty: "i64",
                    required: false,
                },
                OpFieldSchema {
                    name: "generator",
                    ty: "string",
                    required: false,
                },
                OpFieldSchema {
                    name: "bounds",
                    ty: "node id",
                    required: false,
                },
                OpFieldSchema {
                    name: "detached",
                    ty: "bool",
                    required: false,
                },
            ];
            Some(F)
        }
        "update_recipe" => {
            static F: &[OpFieldSchema] = &[
                OpFieldSchema {
                    name: "id",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "kind",
                    ty: "string",
                    required: true,
                },
                OpFieldSchema {
                    name: "seed",
                    ty: "i64",
                    required: false,
                },
                OpFieldSchema {
                    name: "generator",
                    ty: "string",
                    required: false,
                },
                OpFieldSchema {
                    name: "bounds",
                    ty: "node id",
                    required: false,
                },
                OpFieldSchema {
                    name: "detached",
                    ty: "bool",
                    required: false,
                },
            ];
            Some(F)
        }
        "delete_recipe" => {
            static F: &[OpFieldSchema] = &[OpFieldSchema {
                name: "id",
                ty: "string",
                required: true,
            }];
            Some(F)
        }
        _ => None,
    }
}

/// Return a minimal single-op JSON object example for a named op, or `None`
/// if the op name is not recognised.
///
/// The returned string is valid JSON and includes the `"op"` tag field.
pub fn op_example(name: &str) -> Option<&'static str> {
    match name {
        "set_text_align" => Some(r#"{"op":"set_text_align","node":"text.hello","align":"center"}"#),
        "move_forward" => Some(r#"{"op":"move_forward","node":"hero"}"#),
        "move_backward" => Some(r#"{"op":"move_backward","node":"hero"}"#),
        "move_to_front" => Some(r#"{"op":"move_to_front","node":"hero"}"#),
        "move_to_back" => Some(r#"{"op":"move_to_back","node":"hero"}"#),
        "set_fill" => Some(r#"{"op":"set_fill","node":"hero","fill":"color.brand"}"#),
        "set_stroke" => Some(r#"{"op":"set_stroke","node":"box","stroke":"color.rule"}"#),
        "set_stroke_width" => {
            Some(r#"{"op":"set_stroke_width","node":"box","stroke_width":"size.stroke"}"#)
        }
        "set_visible" => Some(r#"{"op":"set_visible","node":"caption","visible":false}"#),
        "set_locked" => Some(r#"{"op":"set_locked","node":"bg","locked":true}"#),
        "set_geometry" => Some(r#"{"op":"set_geometry","node":"r","x":10,"w":200,"rotate":45}"#),
        "set_points" => Some(
            r#"{"op":"set_points","node":"poly","points":[{"x":0,"y":0},{"x":100,"y":0},{"x":50,"y":80}]}"#,
        ),
        "add_node" => Some(
            r#"{"op":"add_node","parent":"page.main","source":"rect id=\"box\" x=(px)10 y=(px)10 w=(px)100 h=(px)80 fill=(token)\"color.accent\""}"#,
        ),
        "remove_node" => Some(r#"{"op":"remove_node","node":"old-rect"}"#),
        "set_opacity" => Some(r#"{"op":"set_opacity","node":"overlay","opacity":0.4}"#),
        "replace_text" => Some(
            r#"{"op":"replace_text","node":"label","spans":[{"text":"Hello"},{"text":" World","fill":"color.accent","italic":true}]}"#,
        ),
        "duplicate_node" => Some(r#"{"op":"duplicate_node","node":"box","new_id":"box-copy"}"#),
        "duplicate_page" => {
            Some(r#"{"op":"duplicate_page","page":"page.x","new_id":"page.x2","id_suffix":".v2"}"#)
        }
        "group" => Some(r#"{"op":"group","node_ids":["rect1","rect2"],"group_id":"grp-new"}"#),
        "ungroup" => Some(r#"{"op":"ungroup","group_id":"grp1"}"#),
        "reparent" => {
            Some(r#"{"op":"reparent","node":"rect1","new_parent":"grp1","position":{"at":"last"}}"#)
        }
        "align_nodes" => Some(
            r#"{"op":"align_nodes","node_ids":["a","b","caption"],"align":"left","anchor":"(px)120"}"#,
        ),
        "set_text_overflow" => {
            Some(r#"{"op":"set_text_overflow","node_id":"body","overflow":"visible"}"#)
        }
        "add_page" => {
            Some(r#"{"op":"add_page","id":"page.new","w":"(px)1800","h":"(px)1200","index":1}"#)
        }
        "delete_page" => Some(r#"{"op":"delete_page","page":"page.old"}"#),
        "reorder_pages" => Some(r#"{"op":"reorder_pages","order":["page.b","page.a","page.c"]}"#),
        "add_asset" => Some(
            r#"{"op":"add_asset","id":"asset.logo","kind":"image","src":"images/logo.png","sha256":"abc123"}"#,
        ),
        "set_asset" => Some(r#"{"op":"set_asset","node_id":"pic","asset_id":"asset.hero"}"#),
        "distribute_nodes" => {
            Some(r#"{"op":"distribute_nodes","node_ids":["p1","p2","p3"],"axis":"horizontal"}"#)
        }
        "create_token" => {
            Some(r##"{"op":"create_token","id":"color.brand","type":"color","value":"#e11d48"}"##)
        }
        "update_token_value" => {
            Some(r##"{"op":"update_token_value","id":"color.brand","value":"#3b82f6"}"##)
        }
        "set_style_property" => Some(
            r#"{"op":"set_style_property","style_id":"heading","property":"font-family","value":"font.body"}"#,
        ),
        "set_text_direction" => {
            Some(r#"{"op":"set_text_direction","node":"label","direction":"rtl"}"#)
        }
        "find_replace_text" => {
            Some(r#"{"op":"find_replace_text","find":"Draft","replace":"Final"}"#)
        }
        "set_page_size" => {
            Some(r#"{"op":"set_page_size","page":"page.main","w":"(px)794","h":"(px)1123"}"#)
        }
        "align_to_edge" => {
            Some(r#"{"op":"align_to_edge","node":"logo","edge":"right","margin":24}"#)
        }
        "create_recipe" => {
            Some(r#"{"op":"create_recipe","id":"recipe.scatter","kind":"scatter","seed":42}"#)
        }
        "update_recipe" => {
            Some(r#"{"op":"update_recipe","id":"recipe.scatter","kind":"scatter","detached":true}"#)
        }
        "delete_recipe" => Some(r#"{"op":"delete_recipe","id":"recipe.scatter"}"#),
        "detach_pattern" => Some(r#"{"op":"detach_pattern","node":"dots"}"#),
        _ => None,
    }
}

// ── Drift-guard tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::Op;
    use std::collections::BTreeSet;

    /// Exhaustive map from an `Op` reference to its JSON tag string.
    ///
    /// The exhaustive `match` here is the **compile-time drift guard**: when a
    /// new `Op` variant is added the compiler forces this fn to be updated,
    /// which in turn forces `op_names()` and `op_summary()` to be updated.
    fn op_tag(op: &Op) -> &'static str {
        match op {
            Op::SetTextAlign { .. } => "set_text_align",
            Op::MoveForward { .. } => "move_forward",
            Op::MoveBackward { .. } => "move_backward",
            Op::MoveToFront { .. } => "move_to_front",
            Op::MoveToBack { .. } => "move_to_back",
            Op::SetFill { .. } => "set_fill",
            Op::SetStroke { .. } => "set_stroke",
            Op::SetStrokeWidth { .. } => "set_stroke_width",
            Op::SetVisible { .. } => "set_visible",
            Op::SetLocked { .. } => "set_locked",
            Op::SetGeometry { .. } => "set_geometry",
            Op::SetPoints { .. } => "set_points",
            Op::AddNode { .. } => "add_node",
            Op::RemoveNode { .. } => "remove_node",
            Op::SetOpacity { .. } => "set_opacity",
            Op::ReplaceText { .. } => "replace_text",
            Op::DuplicateNode { .. } => "duplicate_node",
            Op::DuplicatePage { .. } => "duplicate_page",
            Op::Group { .. } => "group",
            Op::Ungroup { .. } => "ungroup",
            Op::Reparent { .. } => "reparent",
            Op::AlignNodes { .. } => "align_nodes",
            Op::SetTextOverflow { .. } => "set_text_overflow",
            Op::AddPage { .. } => "add_page",
            Op::DeletePage { .. } => "delete_page",
            Op::ReorderPages { .. } => "reorder_pages",
            Op::AddAsset { .. } => "add_asset",
            Op::SetAsset { .. } => "set_asset",
            Op::DistributeNodes { .. } => "distribute_nodes",
            Op::CreateToken { .. } => "create_token",
            Op::UpdateTokenValue { .. } => "update_token_value",
            Op::SetStyleProperty { .. } => "set_style_property",
            Op::SetTextDirection { .. } => "set_text_direction",
            Op::FindReplaceText { .. } => "find_replace_text",
            Op::SetPageSize { .. } => "set_page_size",
            Op::AlignToEdge { .. } => "align_to_edge",
            Op::CreateRecipe { .. } => "create_recipe",
            Op::UpdateRecipe { .. } => "update_recipe",
            Op::DeleteRecipe { .. } => "delete_recipe",
            Op::DetachPattern { .. } => "detach_pattern",
        }
    }

    /// Canonical set of all op tags as derived from the exhaustive match above.
    ///
    /// Kept in sync with `op_tag` by the assertions in the test below.
    fn all_exhaustive_tags() -> BTreeSet<&'static str> {
        BTreeSet::from([
            "set_text_align",
            "move_forward",
            "move_backward",
            "move_to_front",
            "move_to_back",
            "set_fill",
            "set_stroke",
            "set_stroke_width",
            "set_visible",
            "set_locked",
            "set_geometry",
            "set_points",
            "add_node",
            "remove_node",
            "set_opacity",
            "replace_text",
            "duplicate_node",
            "duplicate_page",
            "group",
            "ungroup",
            "reparent",
            "align_nodes",
            "set_text_overflow",
            "add_page",
            "delete_page",
            "reorder_pages",
            "add_asset",
            "set_asset",
            "distribute_nodes",
            "create_token",
            "update_token_value",
            "set_style_property",
            "set_text_direction",
            "find_replace_text",
            "set_page_size",
            "align_to_edge",
            "create_recipe",
            "update_recipe",
            "delete_recipe",
            "detach_pattern",
        ])
    }

    #[test]
    fn op_summary_covers_every_op() {
        let exhaustive = all_exhaustive_tags();
        let listed: BTreeSet<&str> = op_names().iter().copied().collect();

        // The exhaustive set and op_names() must match exactly.
        let missing_from_names: BTreeSet<_> = exhaustive.difference(&listed).collect();
        assert!(
            missing_from_names.is_empty(),
            "op_names() is missing op tags present in the exhaustive match: {:?}",
            missing_from_names,
        );

        let extra_in_names: BTreeSet<_> = listed.difference(&exhaustive).collect();
        assert!(
            extra_in_names.is_empty(),
            "op_names() has tags not in the exhaustive match (add Op variant or remove stale entry): {:?}",
            extra_in_names,
        );

        // Every listed op must have a summary.
        for name in op_names() {
            assert!(
                op_summary(name).is_some(),
                "op_summary(\"{name}\") returned None — add a one-liner to op_summary()",
            );
        }
    }

    /// Verify the `op_tag` helper itself is consistent with `all_exhaustive_tags`.
    ///
    /// We build one representative `Op` value per variant and check the tag it
    /// produces is in our constant set. This catches copy-paste errors in
    /// `op_tag` (wrong string literal for a variant).
    #[test]
    fn op_tag_strings_match_exhaustive_set() {
        let set = all_exhaustive_tags();
        let samples: &[Op] = &[
            Op::SetTextAlign {
                node: String::new(),
                align: String::new(),
            },
            Op::MoveForward {
                node: String::new(),
            },
            Op::MoveBackward {
                node: String::new(),
            },
            Op::MoveToFront {
                node: String::new(),
            },
            Op::MoveToBack {
                node: String::new(),
            },
            Op::SetFill {
                node: String::new(),
                fill: String::new(),
            },
            Op::SetStroke {
                node: String::new(),
                stroke: String::new(),
            },
            Op::SetStrokeWidth {
                node: String::new(),
                stroke_width: String::new(),
            },
            Op::SetVisible {
                node: String::new(),
                visible: true,
            },
            Op::SetLocked {
                node: String::new(),
                locked: false,
            },
            Op::SetGeometry {
                node: String::new(),
                x: None,
                y: None,
                w: None,
                h: None,
                rotate: None,
            },
            Op::SetPoints {
                node: String::new(),
                points: vec![],
            },
            Op::AddNode {
                parent: String::new(),
                position: Default::default(),
                source: String::new(),
            },
            Op::RemoveNode {
                node: String::new(),
            },
            Op::SetOpacity {
                node: String::new(),
                opacity: 1.0,
            },
            Op::ReplaceText {
                node: String::new(),
                spans: vec![],
            },
            Op::DuplicateNode {
                node: String::new(),
                new_id: String::new(),
            },
            Op::DuplicatePage {
                page: String::new(),
                new_id: String::new(),
                id_suffix: String::new(),
            },
            Op::Group {
                node_ids: vec![],
                group_id: String::new(),
            },
            Op::Ungroup {
                group_id: String::new(),
            },
            Op::Reparent {
                node: String::new(),
                new_parent: String::new(),
                position: Default::default(),
            },
            Op::AlignNodes {
                node_ids: vec![],
                align: String::new(),
                anchor: "selection".to_owned(),
            },
            Op::SetTextOverflow {
                node_id: String::new(),
                overflow: String::new(),
            },
            Op::AddPage {
                id: String::new(),
                w: String::new(),
                h: String::new(),
                background: None,
                index: None,
            },
            Op::DeletePage {
                page: String::new(),
            },
            Op::ReorderPages { order: vec![] },
            Op::AddAsset {
                id: String::new(),
                kind: String::new(),
                src: String::new(),
                sha256: None,
            },
            Op::SetAsset {
                node_id: String::new(),
                asset_id: String::new(),
            },
            Op::DistributeNodes {
                node_ids: vec![],
                axis: String::new(),
            },
            Op::CreateToken {
                id: String::new(),
                token_type: String::new(),
                value: String::new(),
            },
            Op::UpdateTokenValue {
                id: String::new(),
                value: String::new(),
            },
            Op::SetStyleProperty {
                style_id: String::new(),
                property: String::new(),
                value: String::new(),
            },
            Op::SetTextDirection {
                node: String::new(),
                direction: String::new(),
            },
            Op::FindReplaceText {
                find: String::new(),
                replace: String::new(),
                node: None,
            },
            Op::SetPageSize {
                page: String::new(),
                w: String::new(),
                h: String::new(),
            },
            Op::AlignToEdge {
                node: String::new(),
                edge: String::new(),
                margin: 0.0,
            },
            Op::CreateRecipe {
                id: String::new(),
                kind: String::new(),
                seed: None,
                generator: None,
                bounds: None,
                detached: None,
            },
            Op::UpdateRecipe {
                id: String::new(),
                kind: String::new(),
                seed: None,
                generator: None,
                bounds: None,
                detached: None,
            },
            Op::DeleteRecipe { id: String::new() },
            Op::DetachPattern {
                node: String::new(),
            },
        ];

        for op in samples {
            let tag = op_tag(op);
            assert!(
                set.contains(tag),
                "op_tag produced \"{tag}\" which is not in all_exhaustive_tags() — fix the mismatch",
            );
        }

        // Count check: every variant must be represented exactly once.
        assert_eq!(
            samples.len(),
            set.len(),
            "samples count ({}) != exhaustive set size ({}): add/remove a sample",
            samples.len(),
            set.len(),
        );
    }

    /// Every op must have a non-`None` `op_fields` result.
    ///
    /// This is a **drift guard**: a new op variant added to `op_names()` must
    /// also appear in `op_fields()` or this test fails at compile+run time.
    #[test]
    fn op_fields_covers_every_op() {
        for &name in op_names() {
            assert!(
                op_fields(name).is_some(),
                "op_fields(\"{name}\") returned None — add an arm to op_fields()",
            );
        }
    }

    /// Every op must have a non-`None` `op_example` result, and the returned
    /// string must parse as valid JSON whose `"op"` field matches the op name.
    ///
    /// This is a **drift guard**: a new op that lacks an example fails here.
    #[test]
    fn op_example_covers_every_op() {
        for &name in op_names() {
            let example = op_example(name).unwrap_or_else(|| {
                panic!("op_example(\"{name}\") returned None — add an arm to op_example()")
            });
            // Must parse as a JSON object.
            let v: serde_json::Value = serde_json::from_str(example).unwrap_or_else(|e| {
                panic!("op_example(\"{name}\") is not valid JSON: {e}\n  value: {example}")
            });
            // The "op" field must match the op name.
            let op_field = v
                .get("op")
                .and_then(|f| f.as_str())
                .unwrap_or_else(|| panic!("op_example(\"{name}\") has no string \"op\" field"));
            assert_eq!(
                op_field, name,
                "op_example(\"{name}\") has wrong \"op\" tag: got \"{op_field}\"",
            );
        }
    }

    /// Every key in a serialized representative `Op` value (other than `"op"`)
    /// must appear in the `op_fields()` list for that op.
    ///
    /// This is the **serde field-name drift guard**: if a field is renamed or
    /// added in `Op` but `op_fields()` is not updated, the serialized key will
    /// be absent from the documented list and this test will fail.
    #[test]
    fn op_fields_names_match_serde_keys() {
        use crate::op::{Op, OpPoint, OpSpan, Position};

        // Build one representative `Op` per variant that has non-optional
        // fields set to real values so serde emits all keys (including
        // skip_serializing_if=None fields that ARE present here as Some).
        // We deliberately make every Option<T> a Some(_) so the serialized
        // output contains every possible key.
        let samples: &[(&str, Op)] = &[
            (
                "set_text_align",
                Op::SetTextAlign {
                    node: "n".into(),
                    align: "center".into(),
                },
            ),
            ("move_forward", Op::MoveForward { node: "n".into() }),
            ("move_backward", Op::MoveBackward { node: "n".into() }),
            ("move_to_front", Op::MoveToFront { node: "n".into() }),
            ("move_to_back", Op::MoveToBack { node: "n".into() }),
            (
                "set_fill",
                Op::SetFill {
                    node: "n".into(),
                    fill: "color.brand".into(),
                },
            ),
            (
                "set_stroke",
                Op::SetStroke {
                    node: "n".into(),
                    stroke: "color.rule".into(),
                },
            ),
            (
                "set_stroke_width",
                Op::SetStrokeWidth {
                    node: "n".into(),
                    stroke_width: "size.stroke".into(),
                },
            ),
            (
                "set_visible",
                Op::SetVisible {
                    node: "n".into(),
                    visible: true,
                },
            ),
            (
                "set_locked",
                Op::SetLocked {
                    node: "n".into(),
                    locked: false,
                },
            ),
            (
                "set_geometry",
                Op::SetGeometry {
                    node: "n".into(),
                    x: Some(0.0),
                    y: Some(0.0),
                    w: Some(100.0),
                    h: Some(100.0),
                    rotate: Some(0.0),
                },
            ),
            (
                "set_points",
                Op::SetPoints {
                    node: "n".into(),
                    points: vec![OpPoint { x: 0.0, y: 0.0 }],
                },
            ),
            (
                "add_node",
                Op::AddNode {
                    parent: "p".into(),
                    position: Position::Last,
                    source: "rect id=\"x\"".into(),
                },
            ),
            ("remove_node", Op::RemoveNode { node: "n".into() }),
            (
                "set_opacity",
                Op::SetOpacity {
                    node: "n".into(),
                    opacity: 1.0,
                },
            ),
            (
                "replace_text",
                Op::ReplaceText {
                    node: "n".into(),
                    spans: vec![OpSpan {
                        text: "hi".into(),
                        fill: Some("color.brand".into()),
                        font_weight: Some("font.bold".into()),
                        italic: Some(true),
                        underline: Some(false),
                        strikethrough: Some(false),
                        vertical_align: Some("super".into()),
                        footnote_ref: Some("fn1".into()),
                    }],
                },
            ),
            (
                "duplicate_node",
                Op::DuplicateNode {
                    node: "n".into(),
                    new_id: "n2".into(),
                },
            ),
            (
                "duplicate_page",
                Op::DuplicatePage {
                    page: "p".into(),
                    new_id: "p2".into(),
                    id_suffix: ".v2".into(),
                },
            ),
            (
                "group",
                Op::Group {
                    node_ids: vec!["a".into()],
                    group_id: "g".into(),
                },
            ),
            (
                "ungroup",
                Op::Ungroup {
                    group_id: "g".into(),
                },
            ),
            (
                "reparent",
                Op::Reparent {
                    node: "n".into(),
                    new_parent: "p".into(),
                    position: Position::Last,
                },
            ),
            (
                "align_nodes",
                Op::AlignNodes {
                    node_ids: vec!["a".into()],
                    align: "left".into(),
                    anchor: "selection".into(),
                },
            ),
            (
                "set_text_overflow",
                Op::SetTextOverflow {
                    node_id: "n".into(),
                    overflow: "clip".into(),
                },
            ),
            (
                "add_page",
                Op::AddPage {
                    id: "p".into(),
                    w: "(px)1800".into(),
                    h: "(px)1200".into(),
                    background: Some("color.bg".into()),
                    index: Some(0),
                },
            ),
            ("delete_page", Op::DeletePage { page: "p".into() }),
            (
                "reorder_pages",
                Op::ReorderPages {
                    order: vec!["a".into()],
                },
            ),
            (
                "add_asset",
                Op::AddAsset {
                    id: "asset.logo".into(),
                    kind: "image".into(),
                    src: "img/logo.png".into(),
                    sha256: Some("abc".into()),
                },
            ),
            (
                "set_asset",
                Op::SetAsset {
                    node_id: "pic".into(),
                    asset_id: "asset.hero".into(),
                },
            ),
            (
                "distribute_nodes",
                Op::DistributeNodes {
                    node_ids: vec!["a".into()],
                    axis: "horizontal".into(),
                },
            ),
            (
                "create_token",
                Op::CreateToken {
                    id: "color.brand".into(),
                    token_type: "color".into(),
                    value: "#e11d48".into(),
                },
            ),
            (
                "update_token_value",
                Op::UpdateTokenValue {
                    id: "color.brand".into(),
                    value: "#3b82f6".into(),
                },
            ),
            (
                "set_style_property",
                Op::SetStyleProperty {
                    style_id: "heading".into(),
                    property: "font-family".into(),
                    value: "font.body".into(),
                },
            ),
            (
                "set_text_direction",
                Op::SetTextDirection {
                    node: "n".into(),
                    direction: "ltr".into(),
                },
            ),
            (
                "find_replace_text",
                Op::FindReplaceText {
                    find: "Draft".into(),
                    replace: "Final".into(),
                    node: Some("label".into()),
                },
            ),
            (
                "set_page_size",
                Op::SetPageSize {
                    page: "p".into(),
                    w: "(px)794".into(),
                    h: "(px)1123".into(),
                },
            ),
            (
                "align_to_edge",
                Op::AlignToEdge {
                    node: "n".into(),
                    edge: "right".into(),
                    margin: 0.0,
                },
            ),
            (
                "create_recipe",
                Op::CreateRecipe {
                    id: "recipe.scatter".into(),
                    kind: "scatter".into(),
                    seed: Some(42),
                    generator: Some("scatter@1".into()),
                    bounds: Some("frame1".into()),
                    detached: Some(false),
                },
            ),
            (
                "update_recipe",
                Op::UpdateRecipe {
                    id: "recipe.scatter".into(),
                    kind: "scatter".into(),
                    seed: Some(42),
                    generator: Some("scatter@1".into()),
                    bounds: Some("frame1".into()),
                    detached: Some(true),
                },
            ),
            ("delete_recipe", Op::DeleteRecipe { id: "r".into() }),
            (
                "detach_pattern",
                Op::DetachPattern {
                    node: "dots".into(),
                },
            ),
        ];

        for (name, op) in samples {
            // Serialize the Op to JSON.
            let json_str = serde_json::to_string(op)
                .unwrap_or_else(|e| panic!("failed to serialize Op sample for \"{name}\": {e}"));
            let v: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("failed to re-parse serialized Op for \"{name}\": {e}"));
            let obj = v
                .as_object()
                .unwrap_or_else(|| panic!("serialized Op for \"{name}\" is not a JSON object"));

            // Collect the documented field names for this op.
            let fields = op_fields(name).unwrap_or_else(|| {
                panic!("op_fields(\"{name}\") returned None — update op_fields()")
            });
            let documented: std::collections::BTreeSet<&str> =
                fields.iter().map(|f| f.name).collect();

            // Every serialized key (except "op") must be in the documented set.
            for key in obj.keys() {
                if key == "op" {
                    continue;
                }
                assert!(
                    documented.contains(key.as_str()),
                    "op \"{name}\": serialized key \"{key}\" is not in op_fields() — \
                     update op_fields() to document this field",
                );
            }
        }

        // Count check: every variant in op_names() must appear in samples.
        let sample_names: std::collections::BTreeSet<&str> =
            samples.iter().map(|(name, _)| *name).collect();
        let all_names: std::collections::BTreeSet<&str> = op_names().iter().copied().collect();
        let missing: std::collections::BTreeSet<_> = all_names.difference(&sample_names).collect();
        assert!(
            missing.is_empty(),
            "op_fields_names_match_serde_keys is missing samples for ops: {:?}",
            missing,
        );
    }
}
