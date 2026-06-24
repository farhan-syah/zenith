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
