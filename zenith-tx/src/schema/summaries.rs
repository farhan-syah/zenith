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
