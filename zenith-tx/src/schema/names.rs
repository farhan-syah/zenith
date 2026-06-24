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
        "remove_node",
        "reorder_pages",
        "reparent",
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
