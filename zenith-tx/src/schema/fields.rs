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
