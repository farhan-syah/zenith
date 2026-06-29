use std::path::Path;

use zenith_core::{TokenLiteral, TokenType, TokenValue};
use zenith_tx::Transaction;

use crate::commands::serialize_pretty;
use crate::library::{ItemKind, load_pack_document, parse_spec, resolve_packs};

/// JSON shape for `library show --json`.
#[derive(Debug, serde::Serialize)]
struct LibraryShowOutput {
    schema: &'static str,
    package: String,
    item: String,
    kind: &'static str,
    detail: ShowDetail,
    to_use: String,
}

/// Kind-specific content for `library show`.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ShowDetail {
    Token {
        token_type: String,
        /// Human-readable ops / value summary derived from the token literal.
        summary: String,
    },
    Component {
        /// The root node kind (kind of the first child), or "empty" when none.
        root_node_kind: String,
        /// Total direct child count of the component.
        child_count: usize,
        /// Short breakdown of the child node kinds present (sorted, e.g.
        /// `"shape(1)"`).
        node_kinds: String,
    },
    Action {
        /// The op names extracted from the tx JSON (snake_case), in source order.
        ops: Vec<String>,
        /// Human-readable label from the action def, if present.
        label: Option<String>,
    },
}

/// Error produced by the `library show` command.
#[derive(Debug)]
pub struct ShowCmdErr {
    /// Human-readable message.
    pub message: String,
    /// Recommended exit code.
    pub exit_code: u8,
}

impl ShowCmdErr {
    fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }
}

/// Inspect the library item named by `spec` (`<package>#<item>`) and return
/// `(stdout_text, exit_code)`.
///
/// Resolves packs from `project_dir` (embedded presets + project packs), finds
/// the named item, loads the pack's full document to derive content detail, and
/// formats human or JSON output.
///
/// # Errors
///
/// Returns [`ShowCmdErr`] on a malformed spec, unknown package, or unknown item.
pub fn show(spec: &str, project_dir: Option<&Path>, json: bool) -> Result<String, ShowCmdErr> {
    let (pkg_id, item_id) = parse_spec(spec).map_err(|e| ShowCmdErr::new(e.message, 2))?;

    let packs = resolve_packs(project_dir);

    // Find the pack.
    let pack = packs.iter().find(|p| p.id == pkg_id).ok_or_else(|| {
        let mut available: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
        available.sort_unstable();
        available.dedup();
        ShowCmdErr::new(
            format!(
                "unknown library package '{}' (available: {})",
                pkg_id,
                if available.is_empty() {
                    "none".to_owned()
                } else {
                    available.join(", ")
                }
            ),
            2,
        )
    })?;

    // Find the item in the pack's metadata.
    let pack_item = pack
        .items
        .iter()
        .find(|it| it.id == item_id)
        .ok_or_else(|| {
            let available: Vec<&str> = pack.items.iter().map(|it| it.id.as_str()).collect();
            ShowCmdErr::new(
                format!(
                    "unknown item '{}' in package '{}' (available: {})",
                    item_id,
                    pkg_id,
                    if available.is_empty() {
                        "none".to_owned()
                    } else {
                        available.join(", ")
                    }
                ),
                2,
            )
        })?;

    let kind = pack_item.kind;

    // Load the pack's full document to derive content detail.
    let pack_doc = load_pack_document(pack).map_err(|e| ShowCmdErr::new(e.message, 2))?;

    let detail = match kind {
        ItemKind::Token => {
            // Find the token in the pack document.
            let token = pack_doc
                .tokens
                .tokens
                .iter()
                .find(|t| t.id == item_id)
                .ok_or_else(|| {
                    ShowCmdErr::new(
                        format!(
                            "internal error: item '{}' not found in pack document",
                            item_id
                        ),
                        2,
                    )
                })?;

            let token_type = match &token.token_type {
                TokenType::Filter => "filter".to_owned(),
                TokenType::Mask => "mask".to_owned(),
                TokenType::Color => "color".to_owned(),
                TokenType::Dimension => "dimension".to_owned(),
                TokenType::Number => "number".to_owned(),
                TokenType::FontFamily => "fontFamily".to_owned(),
                TokenType::FontWeight => "fontWeight".to_owned(),
                TokenType::Gradient => "gradient".to_owned(),
                TokenType::Shadow => "shadow".to_owned(),
                TokenType::Unknown(s) => s.clone(),
            };

            let summary = match &token.value {
                TokenValue::Reference { token_id } => {
                    format!("alias to {}", token_id)
                }
                TokenValue::Literal(lit) => match lit {
                    TokenLiteral::Filter(lit) => {
                        let ops: Vec<String> = lit
                            .ops
                            .iter()
                            .map(|op| op.kind.as_op_name().to_owned())
                            .collect();
                        format!("ops: {}", ops.join(", "))
                    }
                    TokenLiteral::Mask(lit) => {
                        let parts: Vec<String> = {
                            let mut v = vec![lit.shape.as_shape_name().to_owned()];
                            if lit.feather > 0.0 {
                                v.push(format!("feather={}", lit.feather));
                            }
                            if lit.invert {
                                v.push("invert=true".to_owned());
                            }
                            v
                        };
                        format!("shape: {}", parts.join(", "))
                    }
                    TokenLiteral::String(s) => s.clone(),
                    TokenLiteral::Dimension(d) => {
                        format!("({}){}", d.unit.as_annotation(), d.value)
                    }
                    TokenLiteral::Number(n) => n.to_string(),
                    TokenLiteral::Gradient(g) => {
                        format!("gradient with {} stop(s)", g.stops.len())
                    }
                    TokenLiteral::Shadow(s) => {
                        format!("shadow with {} layer(s)", s.layers.len())
                    }
                },
            };

            ShowDetail::Token {
                token_type,
                summary,
            }
        }

        ItemKind::Component => {
            let comp = pack_doc
                .components
                .iter()
                .find(|c| c.id == item_id)
                .ok_or_else(|| {
                    ShowCmdErr::new(
                        format!(
                            "internal error: component '{}' not found in pack document",
                            item_id
                        ),
                        2,
                    )
                })?;

            let child_count = comp.children.len();
            let root_node_kind = comp
                .children
                .first()
                .map(|n| node_kind_name(n).to_owned())
                .unwrap_or_else(|| "empty".to_owned());

            // Count each node kind among direct children.
            let mut kind_counts: std::collections::BTreeMap<&'static str, usize> =
                std::collections::BTreeMap::new();
            for child in &comp.children {
                *kind_counts.entry(node_kind_name(child)).or_insert(0) += 1;
            }
            let node_kinds: Vec<String> = kind_counts
                .iter()
                .map(|(k, n)| format!("{}({})", k, n))
                .collect();
            let node_kinds = if node_kinds.is_empty() {
                "none".to_owned()
            } else {
                node_kinds.join(", ")
            };

            ShowDetail::Component {
                root_node_kind,
                child_count,
                node_kinds,
            }
        }

        ItemKind::Action => {
            let action_def = pack_doc
                .actions
                .iter()
                .find(|a| a.id == item_id)
                .ok_or_else(|| {
                    ShowCmdErr::new(
                        format!(
                            "internal error: action '{}' not found in pack document",
                            item_id
                        ),
                        2,
                    )
                })?;

            let label = action_def.label.clone();

            // Parse the tx JSON with Transaction::from_json to extract op names.
            let tx = Transaction::from_json(&action_def.tx_json).map_err(|e| {
                ShowCmdErr::new(
                    format!("malformed tx-script in action '{}': {}", item_id, e.message),
                    2,
                )
            })?;

            let ops: Vec<String> = tx.ops.iter().map(op_name).collect();

            ShowDetail::Action { ops, label }
        }
    };

    let to_use = match kind {
        ItemKind::Component => format!(
            "zenith library add {}#{} --into <doc.zen> --page <page-id>",
            pkg_id, item_id
        ),
        ItemKind::Token | ItemKind::Action => {
            format!("zenith library add {}#{} --into <doc.zen>", pkg_id, item_id)
        }
    };

    if json {
        let out = LibraryShowOutput {
            schema: "zenith-library-show-v1",
            package: pkg_id,
            item: item_id,
            kind: kind.label(),
            detail,
            to_use,
        };
        Ok(serialize_pretty(&out))
    } else {
        Ok(format_show_human(&pkg_id, &item_id, kind, &detail, &to_use))
    }
}

/// Format the human-readable output for `library show`.
fn format_show_human(
    pkg_id: &str,
    item_id: &str,
    kind: ItemKind,
    detail: &ShowDetail,
    to_use: &str,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("package : {}", pkg_id));
    lines.push(format!("item    : {}", item_id));
    lines.push(format!("kind    : {}", kind.label()));
    lines.push(String::new());

    match detail {
        ShowDetail::Token {
            token_type,
            summary,
        } => {
            lines.push(format!("type    : {}", token_type));
            lines.push(format!("content : {}", summary));
        }
        ShowDetail::Component {
            root_node_kind,
            child_count,
            node_kinds,
        } => {
            lines.push(format!("children: {} node(s)", child_count));
            lines.push(format!("root    : {}", root_node_kind));
            lines.push(format!("nodes   : {}", node_kinds));
        }
        ShowDetail::Action { ops, label } => {
            if let Some(lbl) = label {
                lines.push(format!("label   : {}", lbl));
            }
            lines.push(format!(
                "ops     : {}",
                if ops.is_empty() {
                    "(none)".to_owned()
                } else {
                    ops.join(", ")
                }
            ));
        }
    }

    lines.push(String::new());
    lines.push(format!("To use  : {}", to_use));
    lines.join("\n")
}

/// Return the short, stable node-kind name for a [`zenith_core::Node`] variant.
fn node_kind_name(node: &zenith_core::Node) -> &'static str {
    match node {
        zenith_core::Node::Rect(_) => "rect",
        zenith_core::Node::Ellipse(_) => "ellipse",
        zenith_core::Node::Line(_) => "line",
        zenith_core::Node::Text(_) => "text",
        zenith_core::Node::Code(_) => "code",
        zenith_core::Node::Image(_) => "image",
        zenith_core::Node::Polygon(_) => "polygon",
        zenith_core::Node::Polyline(_) => "polyline",
        zenith_core::Node::Frame(_) => "frame",
        zenith_core::Node::Group(_) => "group",
        zenith_core::Node::Instance(_) => "instance",
        zenith_core::Node::Field(_) => "field",
        zenith_core::Node::Toc(_) => "toc",
        zenith_core::Node::Footnote(_) => "footnote",
        zenith_core::Node::Table(_) => "table",
        zenith_core::Node::Shape(_) => "shape",
        zenith_core::Node::Connector(_) => "connector",
        zenith_core::Node::Pattern(_) => "pattern",
        zenith_core::Node::Chart(_) => "chart",
        zenith_core::Node::Light(_) => "light",
        zenith_core::Node::Unknown(_) => "unknown",
    }
}

/// Return the snake_case op name for a [`zenith_tx::Op`] variant.
fn op_name(op: &zenith_tx::Op) -> String {
    match op {
        zenith_tx::Op::SetTextAlign { .. } => "set_text_align",
        zenith_tx::Op::MoveForward { .. } => "move_forward",
        zenith_tx::Op::MoveBackward { .. } => "move_backward",
        zenith_tx::Op::MoveToFront { .. } => "move_to_front",
        zenith_tx::Op::MoveToBack { .. } => "move_to_back",
        zenith_tx::Op::SetVisible { .. } => "set_visible",
        zenith_tx::Op::SetLocked { .. } => "set_locked",
        zenith_tx::Op::SetFill { .. } => "set_fill",
        zenith_tx::Op::SetStroke { .. } => "set_stroke",
        zenith_tx::Op::SetStrokeWidth { .. } => "set_stroke_width",
        zenith_tx::Op::SetOpacity { .. } => "set_opacity",
        zenith_tx::Op::SetGeometry { .. } => "set_geometry",
        zenith_tx::Op::SetPoints { .. } => "set_points",
        zenith_tx::Op::ReplaceText { .. } => "replace_text",
        zenith_tx::Op::DuplicateNode { .. } => "duplicate_node",
        zenith_tx::Op::DuplicatePage { .. } => "duplicate_page",
        zenith_tx::Op::AddNode { .. } => "add_node",
        zenith_tx::Op::RemoveNode { .. } => "remove_node",
        zenith_tx::Op::Group { .. } => "group",
        zenith_tx::Op::Ungroup { .. } => "ungroup",
        zenith_tx::Op::Reparent { .. } => "reparent",
        zenith_tx::Op::AlignNodes { .. } => "align_nodes",
        zenith_tx::Op::SetTextOverflow { .. } => "set_text_overflow",
        zenith_tx::Op::AddPage { .. } => "add_page",
        zenith_tx::Op::DeletePage { .. } => "delete_page",
        zenith_tx::Op::ReorderPages { .. } => "reorder_pages",
        zenith_tx::Op::AddAsset { .. } => "add_asset",
        zenith_tx::Op::SetAsset { .. } => "set_asset",
        zenith_tx::Op::DistributeNodes { .. } => "distribute_nodes",
        zenith_tx::Op::UpdateTokenValue { .. } => "update_token_value",
        zenith_tx::Op::SetStyleProperty { .. } => "set_style_property",
        zenith_tx::Op::SetTextDirection { .. } => "set_text_direction",
        zenith_tx::Op::FindReplaceText { .. } => "find_replace_text",
        zenith_tx::Op::SetPageSize { .. } => "set_page_size",
        zenith_tx::Op::AlignToEdge { .. } => "align_to_edge",
        zenith_tx::Op::CreateToken { .. } => "create_token",
        zenith_tx::Op::CreateRecipe { .. } => "create_recipe",
        zenith_tx::Op::UpdateRecipe { .. } => "update_recipe",
        zenith_tx::Op::DeleteRecipe { .. } => "delete_recipe",
        zenith_tx::Op::DetachPattern { .. } => "detach_pattern",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── `library show` tests ───────────────────────────────────────────────────

    #[test]
    fn show_filter_token_human() {
        let out = show("@zenith/filters#sepia", None, false).expect("show ok");
        assert!(out.contains("package : @zenith/filters"), "pkg: {}", out);
        assert!(out.contains("item    : sepia"), "item: {}", out);
        assert!(out.contains("kind    : token"), "kind: {}", out);
        assert!(out.contains("type    : filter"), "type: {}", out);
        assert!(out.contains("ops: sepia"), "ops: {}", out);
        assert!(out.contains("To use"), "to_use: {}", out);
        assert!(
            out.contains("--into <doc.zen>"),
            "to_use invocation: {}",
            out
        );
    }

    #[test]
    fn show_mask_token_human() {
        let out = show("@zenith/masks#vignette", None, false).expect("show ok");
        assert!(out.contains("kind    : token"), "kind: {}", out);
        assert!(out.contains("type    : mask"), "type: {}", out);
        assert!(out.contains("shape: rounded"), "shape: {}", out);
        assert!(out.contains("invert=true"), "invert: {}", out);
    }

    #[test]
    fn show_component_human() {
        let out = show("@zenith/flowchart#decision", None, false).expect("show ok");
        assert!(out.contains("package : @zenith/flowchart"), "pkg: {}", out);
        assert!(out.contains("item    : decision"), "item: {}", out);
        assert!(out.contains("kind    : component"), "kind: {}", out);
        assert!(out.contains("children:"), "children: {}", out);
        assert!(out.contains("root    : shape"), "root: {}", out);
        // to_use for a component must include --page
        assert!(out.contains("--page <page-id>"), "to_use: {}", out);
    }

    #[test]
    fn show_action_human() {
        let out = show("@zenith/brand-kit#apply-2026", None, false).expect("show ok");
        assert!(out.contains("package : @zenith/brand-kit"), "pkg: {}", out);
        assert!(out.contains("item    : apply-2026"), "item: {}", out);
        assert!(out.contains("kind    : action"), "kind: {}", out);
        assert!(out.contains("update_token_value"), "ops: {}", out);
        assert!(out.contains("To use"), "to_use: {}", out);
        assert!(
            out.contains("--into <doc.zen>"),
            "to_use invocation: {}",
            out
        );
    }

    #[test]
    fn show_filter_token_json() {
        let out = show("@zenith/filters#sepia", None, true).expect("show ok");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["schema"], "zenith-library-show-v1");
        assert_eq!(v["package"], "@zenith/filters");
        assert_eq!(v["item"], "sepia");
        assert_eq!(v["kind"], "token");
        assert_eq!(v["detail"]["token_type"], "filter");
        assert!(
            v["detail"]["summary"]
                .as_str()
                .unwrap_or("")
                .contains("sepia"),
            "filter summary: {}",
            v["detail"]["summary"]
        );
        assert!(
            v["to_use"].as_str().unwrap_or("").contains("--into"),
            "to_use: {}",
            v["to_use"]
        );
    }

    #[test]
    fn show_component_json() {
        let out = show("@zenith/flowchart#decision", None, true).expect("show ok");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["schema"], "zenith-library-show-v1");
        assert_eq!(v["kind"], "component");
        assert_eq!(v["detail"]["root_node_kind"], "shape");
        assert!(
            v["detail"]["child_count"].as_u64().unwrap_or(0) >= 1,
            "child_count"
        );
        assert!(
            v["to_use"].as_str().unwrap_or("").contains("--page"),
            "component to_use needs --page: {}",
            v["to_use"]
        );
    }

    #[test]
    fn show_action_json() {
        let out = show("@zenith/brand-kit#apply-2026", None, true).expect("show ok");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["schema"], "zenith-library-show-v1");
        assert_eq!(v["kind"], "action");
        let ops = v["detail"]["ops"].as_array().expect("ops array");
        assert!(!ops.is_empty(), "ops must not be empty");
        assert!(
            ops.iter().any(|o| o == "update_token_value"),
            "must contain update_token_value; ops: {:?}",
            ops
        );
    }

    #[test]
    fn show_unknown_package_errors() {
        let err = show("@no/such#item", None, false).expect_err("unknown pkg errors");
        assert_eq!(err.exit_code, 2);
        assert!(
            err.message.contains("unknown library package"),
            "{}",
            err.message
        );
        assert!(err.message.contains("@zenith/"), "{}", err.message);
    }

    #[test]
    fn show_unknown_item_errors() {
        let err = show("@zenith/filters#nope", None, false).expect_err("unknown item errors");
        assert_eq!(err.exit_code, 2);
        assert!(err.message.contains("unknown item"), "{}", err.message);
        assert!(err.message.contains("sepia"), "{}", err.message);
    }

    #[test]
    fn show_malformed_spec_errors() {
        let err = show("no-hash", None, false).expect_err("malformed spec errors");
        assert_eq!(err.exit_code, 2);
    }
}
