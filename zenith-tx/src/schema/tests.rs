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
        let fields = op_fields(name)
            .unwrap_or_else(|| panic!("op_fields(\"{name}\") returned None — update op_fields()"));
        let documented: std::collections::BTreeSet<&str> = fields.iter().map(|f| f.name).collect();

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
