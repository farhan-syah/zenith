//! Library subsystem tests: pack parsing/resolution and all `materialize*` paths.

use super::add::{px, sanitize_pkg, target_component_id};
use super::token::collect_filter_dep_ids;
use super::*;
use zenith_core::{Document, KdlAdapter, KdlSource, Node, validate};
use zenith_tx::TxStatus;

const FLOWCHART_SRC: &str = include_str!("../../assets/libraries/zenith-flowchart.zen");

/// A minimal target document with a single empty page `pg`.
const TARGET_SRC: &str = r#"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {}
  }
}
"#;

fn parse_target() -> Document {
    KdlAdapter
        .parse(TARGET_SRC.as_bytes())
        .expect("target parses")
}

fn hard_errors(doc: &Document) -> Vec<String> {
    validate(doc)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == zenith_core::Severity::Error)
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect()
}

fn first_page_instance_ids(doc: &Document) -> Vec<String> {
    doc.body.pages[0]
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Instance(i) => Some(i.id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn sanitize_pkg_strips_at_and_slash() {
    assert_eq!(sanitize_pkg("@zenith/flowchart"), "zenith.flowchart");
    assert_eq!(
        target_component_id("@zenith/flowchart", "decision"),
        "lib.zenith.flowchart.decision"
    );
}

#[test]
fn parse_spec_splits_pkg_and_item() {
    assert_eq!(
        parse_spec("@zenith/flowchart#decision").expect("ok"),
        ("@zenith/flowchart".to_owned(), "decision".to_owned())
    );
}

#[test]
fn parse_spec_rejects_malformed() {
    assert!(parse_spec("no-hash").is_err());
    assert!(parse_spec("#item").is_err());
    assert!(parse_spec("pkg#").is_err());
}

#[test]
fn materialize_adds_component_tokens_style_instance_provenance() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let outcome = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "pg",
        "decision",
        (10.0, 20.0),
    )
    .expect("materialize ok");

    // Component copied under namespaced id.
    assert_eq!(outcome.target_component_id, "lib.zenith.flowchart.decision");
    assert!(
        target
            .components
            .iter()
            .any(|c| c.id == "lib.zenith.flowchart.decision"),
        "component copied"
    );
    // Child ids are NOT rewritten (still local `shape`).
    let comp = target
        .components
        .iter()
        .find(|c| c.id == "lib.zenith.flowchart.decision")
        .unwrap();
    assert!(matches!(comp.children.first(), Some(Node::Shape(s)) if s.id == "shape"));

    // Dep tokens + style copied.
    assert!(target.tokens.tokens.iter().any(|t| t.id == "lib.flow.fill"));
    assert!(
        target
            .tokens
            .tokens
            .iter()
            .any(|t| t.id == "lib.flow.dec.fill")
    );
    assert!(
        target
            .styles
            .styles
            .iter()
            .any(|s| s.id == "lib.flow.label")
    );
    assert_eq!(target.tokens.format, "zenith-token-v1");

    // Instance inserted on the page referencing the component.
    let inst = target.body.pages[0]
        .children
        .iter()
        .find_map(|n| match n {
            Node::Instance(i) => Some(i),
            _ => None,
        })
        .expect("instance inserted");
    assert_eq!(inst.id, "decision");
    assert_eq!(inst.component, "lib.zenith.flowchart.decision");
    assert_eq!(inst.x, Some(px(10.0)));
    assert_eq!(inst.y, Some(px(20.0)));

    // Library + provenance recorded.
    assert!(target.libraries.iter().any(|l| l.id == "@zenith/flowchart"));
    let prov = target
        .provenance
        .iter()
        .find(|p| p.node == "decision")
        .expect("provenance recorded");
    assert_eq!(prov.library, "@zenith/flowchart");
    assert_eq!(prov.item.as_deref(), Some("decision"));
    assert_eq!(prov.linked, Some(true));
    assert_eq!(outcome.provenance_id, prov.id);
    assert!(outcome.warnings.is_empty());

    // Validates clean.
    assert!(
        hard_errors(&target).is_empty(),
        "errors: {:?}",
        hard_errors(&target)
    );
}

#[test]
fn materialize_round_trips_format_parse() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "pg",
        "decision",
        (0.0, 0.0),
    )
    .expect("materialize ok");
    let bytes = KdlAdapter.format(&target).expect("format");
    let reparsed = KdlAdapter.parse(&bytes).expect("reparse");
    let bytes2 = KdlAdapter.format(&reparsed).expect("format2");
    assert_eq!(bytes, bytes2, "format→parse→format is stable");
}

#[test]
fn double_add_dedups_component_unique_instance_two_provenance() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let o1 = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "pg",
        "decision",
        (0.0, 0.0),
    )
    .expect("first add");
    let o2 = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "pg",
        "decision",
        (0.0, 0.0),
    )
    .expect("second add");

    // Component copied exactly once.
    assert_eq!(
        target
            .components
            .iter()
            .filter(|c| c.id == "lib.zenith.flowchart.decision")
            .count(),
        1
    );
    // Tokens not duplicated.
    assert_eq!(
        target
            .tokens
            .tokens
            .iter()
            .filter(|t| t.id == "lib.flow.fill")
            .count(),
        1
    );
    // Unique instance ids.
    assert_eq!(o1.instance_id, "decision");
    assert_eq!(o2.instance_id, "decision.1");
    assert_eq!(
        first_page_instance_ids(&target),
        vec!["decision", "decision.1"]
    );
    // Two provenance records.
    assert_eq!(target.provenance.len(), 2);
    assert_ne!(o1.provenance_id, o2.provenance_id);
    // One library entry only.
    assert_eq!(
        target
            .libraries
            .iter()
            .filter(|l| l.id == "@zenith/flowchart")
            .count(),
        1
    );
    assert!(hard_errors(&target).is_empty());
}

#[test]
fn materialize_unknown_page_errors_and_does_not_mutate() {
    let mut target = parse_target();
    let before = target.clone();
    let packs = resolve_packs(None);
    let err = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "nope",
        "decision",
        (0.0, 0.0),
    )
    .expect_err("unknown page errors");
    assert!(
        err.message.contains("page 'nope' not found"),
        "msg: {}",
        err.message
    );
    assert_eq!(target, before, "target untouched on page error");
}

#[test]
fn materialize_unknown_pkg_errors_with_available() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let err = materialize(
        &mut target,
        &packs,
        "@no/such",
        "decision",
        "pg",
        "decision",
        (0.0, 0.0),
    )
    .expect_err("unknown pkg errors");
    assert!(
        err.message.contains("@zenith/flowchart"),
        "lists available: {}",
        err.message
    );
}

#[test]
fn materialize_unknown_item_errors_with_available() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let err = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "nope",
        "pg",
        "decision",
        (0.0, 0.0),
    )
    .expect_err("unknown item errors");
    assert!(
        err.message.contains("process"),
        "lists available items: {}",
        err.message
    );
}

#[test]
fn materialize_id_override_used_as_base() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let o = materialize(
        &mut target,
        &packs,
        "@zenith/flowchart",
        "decision",
        "pg",
        "my.node",
        (0.0, 0.0),
    )
    .expect("ok");
    assert_eq!(o.instance_id, "my.node");
}

#[test]
fn parse_embedded_flowchart_identity_and_items() {
    let pack = parse_pack(FLOWCHART_SRC, PackSource::Preset).expect("flowchart pack parses");
    assert_eq!(pack.id, "@zenith/flowchart");
    assert_eq!(pack.version.as_deref(), Some("1.0.0"));
    assert_eq!(pack.source, PackSource::Preset);
    assert_eq!(
        pack.items,
        vec![
            PackItem {
                id: "process".to_owned(),
                kind: ItemKind::Component
            },
            PackItem {
                id: "decision".to_owned(),
                kind: ItemKind::Component
            },
            PackItem {
                id: "terminator".to_owned(),
                kind: ItemKind::Component
            },
        ]
    );
}

#[test]
fn parse_pack_with_actions_lists_action_items() {
    const ACTION_PACK_SRC: &str = r#"zenith version=1 {
  project id="@test/actions" name="Test Actions"
  libraries { library id="@test/actions" version="1.0.0" }
  actions {
    action id="apply-brand-kit" {
      tx "{\"ops\":[]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {
    }
  }
}
"#;
    let pack = parse_pack(ACTION_PACK_SRC, PackSource::Preset).expect("action pack parses");
    assert_eq!(pack.id, "@test/actions");
    assert!(
        pack.items.contains(&PackItem {
            id: "apply-brand-kit".to_owned(),
            kind: ItemKind::Action,
        }),
        "action item must be present; items: {:?}",
        pack.items
    );
}

const FILTERS_SRC: &str = include_str!("../../assets/libraries/zenith-filters.zen");

#[test]
fn parse_embedded_filters_lists_filter_token_items() {
    let pack = parse_pack(FILTERS_SRC, PackSource::Preset).expect("filters pack parses");
    assert_eq!(pack.id, "@zenith/filters");
    assert_eq!(pack.version.as_deref(), Some("1.0.0"));

    // Filter tokens are items; color dep tokens are NOT.
    assert!(pack.items.contains(&PackItem {
        id: "noir".to_owned(),
        kind: ItemKind::Token
    }));
    assert!(pack.items.contains(&PackItem {
        id: "duotone-gold".to_owned(),
        kind: ItemKind::Token
    }));
    // Color dep tokens are dependencies, not exported items.
    assert!(
        !pack
            .items
            .iter()
            .any(|i| i.id == "lib.filters.duo.gold.shadow"),
        "color dep tokens must not be items"
    );
    // The filters pack ships no components, so every item is a token.
    assert!(pack.items.iter().all(|i| i.kind == ItemKind::Token));
}

#[test]
fn collect_filter_dep_ids_duotone_and_simple() {
    let pack = load_pack_document(&parse_pack(FILTERS_SRC, PackSource::Preset).expect("pack"))
        .expect("pack doc");

    let gold = pack
        .tokens
        .tokens
        .iter()
        .find(|t| t.id == "duotone-gold")
        .expect("duotone-gold present");
    let deps = collect_filter_dep_ids(gold, &pack.tokens.tokens);
    let deps: Vec<String> = deps.into_iter().collect();
    assert_eq!(
        deps,
        vec![
            "lib.filters.duo.gold.highlight".to_owned(),
            "lib.filters.duo.gold.shadow".to_owned(),
        ]
    );

    let noir = pack
        .tokens
        .tokens
        .iter()
        .find(|t| t.id == "noir")
        .expect("noir present");
    assert!(
        collect_filter_dep_ids(noir, &pack.tokens.tokens).is_empty(),
        "non-duotone filters have no token deps"
    );
}

#[test]
fn materialize_token_copies_filter_and_deps_records_provenance() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let outcome = materialize_token(
        &mut target,
        &packs,
        "@zenith/filters",
        "duotone-gold",
        "duotone-gold",
    )
    .expect("materialize_token ok");

    // Filter token + its two color deps copied.
    assert!(target.tokens.tokens.iter().any(|t| t.id == "duotone-gold"));
    assert!(
        target
            .tokens
            .tokens
            .iter()
            .any(|t| t.id == "lib.filters.duo.gold.shadow")
    );
    assert!(
        target
            .tokens
            .tokens
            .iter()
            .any(|t| t.id == "lib.filters.duo.gold.highlight")
    );
    assert_eq!(
        outcome.dep_token_ids,
        vec![
            "lib.filters.duo.gold.highlight".to_owned(),
            "lib.filters.duo.gold.shadow".to_owned(),
        ]
    );
    assert_eq!(outcome.token_id, "duotone-gold");

    // Library + provenance recorded; provenance.node is the TOKEN id.
    assert!(target.libraries.iter().any(|l| l.id == "@zenith/filters"));
    let prov = target
        .provenance
        .iter()
        .find(|p| p.node == "duotone-gold")
        .expect("provenance recorded");
    assert_eq!(prov.library, "@zenith/filters");
    assert_eq!(prov.item.as_deref(), Some("duotone-gold"));
    assert_eq!(outcome.provenance_id, prov.id);

    assert!(
        hard_errors(&target).is_empty(),
        "errors: {:?}",
        hard_errors(&target)
    );
}

#[test]
fn embedded_masks_pack_lists_mask_token_items() {
    let packs = resolve_packs(None);
    let masks = packs
        .iter()
        .find(|p| p.id == "@zenith/masks")
        .expect("@zenith/masks embedded");
    // Mask tokens are exported as token items.
    assert!(
        masks
            .items
            .iter()
            .any(|it| it.id == "vignette" && it.kind == ItemKind::Token),
        "vignette listed as a token item"
    );
    assert!(masks.items.iter().any(|it| it.id == "spotlight"));
}

#[test]
fn embedded_brand_kit_pack_lists_action_items() {
    let packs = resolve_packs(None);
    let brand = packs
        .iter()
        .find(|p| p.id == "@zenith/brand-kit")
        .expect("@zenith/brand-kit embedded");
    // Actions are exported as action items.
    assert!(
        brand
            .items
            .iter()
            .any(|it| it.id == "apply-2026" && it.kind == ItemKind::Action),
        "apply-2026 listed as an action item"
    );
    assert!(brand.items.iter().any(|it| it.id == "apply-mono"));
}

#[test]
fn embedded_brand_kit_action_applies_via_materialize_action() {
    const TARGET: &str = r##"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {
    token id="color.brand" type="color" value="#000000"
    token id="color.accent" type="color" value="#000000"
    token id="color.ink" type="color" value="#000000"
  }
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)400 h=(px)300 {}
  }
}
"##;
    let packs = resolve_packs(None);
    let outcome = materialize_action(TARGET, &packs, "@zenith/brand-kit", "apply-2026")
        .expect("materialize_action ok");
    let final_src = outcome.final_source.expect("accepted → final_source");
    // The 2026 palette is applied and the action is recorded with provenance.
    assert!(
        final_src.contains("#e11d48"),
        "brand color applied:\n{}",
        final_src
    );
    assert!(final_src.contains("#3b82f6"), "accent color applied");
    assert!(final_src.contains("apply-2026"), "action recorded in doc");
    assert!(
        outcome.provenance_id.is_some(),
        "provenance recorded for the applied action"
    );
}

#[test]
fn materialize_token_mask_applies_via_mask_property_no_deps() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let outcome = materialize_token(&mut target, &packs, "@zenith/masks", "vignette", "vignette")
        .expect("materialize_token ok");
    // The mask token is copied; masks are self-contained (no deps).
    assert!(target.tokens.tokens.iter().any(|t| t.id == "vignette"));
    assert!(outcome.dep_token_ids.is_empty());
    // It applies through the `mask` property (not `filter`).
    assert_eq!(outcome.apply_property, "mask");
    // Provenance recorded against the token id.
    assert!(target.provenance.iter().any(|p| p.node == "vignette"));
    assert!(hard_errors(&target).is_empty());
}

#[test]
fn materialize_token_simple_filter_no_deps() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let outcome = materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir")
        .expect("materialize_token ok");
    assert!(target.tokens.tokens.iter().any(|t| t.id == "noir"));
    assert!(outcome.dep_token_ids.is_empty());
    assert!(hard_errors(&target).is_empty());
}

#[test]
fn materialize_token_double_add_dedups_token_and_provenance() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let o1 =
        materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir").expect("first");
    let o2 =
        materialize_token(&mut target, &packs, "@zenith/filters", "noir", "noir").expect("second");

    // Token copied exactly once.
    assert_eq!(
        target
            .tokens
            .tokens
            .iter()
            .filter(|t| t.id == "noir")
            .count(),
        1
    );
    // Identical provenance is not duplicated.
    assert_eq!(target.provenance.len(), 1);
    assert_eq!(o1.provenance_id, o2.provenance_id);
    // One library entry only.
    assert_eq!(
        target
            .libraries
            .iter()
            .filter(|l| l.id == "@zenith/filters")
            .count(),
        1
    );
    assert!(hard_errors(&target).is_empty());
}

#[test]
fn materialize_token_unknown_item_errors_with_available() {
    let mut target = parse_target();
    let packs = resolve_packs(None);
    let err = materialize_token(&mut target, &packs, "@zenith/filters", "nope", "nope")
        .expect_err("unknown filter token errors");
    assert!(
        err.message.contains("noir"),
        "lists available: {}",
        err.message
    );
}

#[test]
fn embedded_flowchart_parses_and_validates_clean() {
    let doc = KdlAdapter
        .parse(FLOWCHART_SRC.as_bytes())
        .expect("embedded pack must parse");
    let report = validate(&doc);
    let errors: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == zenith_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "embedded pack must validate with no errors; got: {:?}",
        errors
    );
}

#[test]
fn load_embedded_packs_includes_flowchart() {
    let packs = load_embedded_packs();
    assert!(
        packs.iter().any(|p| p.id == "@zenith/flowchart"),
        "embedded packs must include @zenith/flowchart"
    );
}

#[test]
fn pack_without_self_entry_errors() {
    let src = r#"zenith version=1 {
  project id="proj.x" name="No Library"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)10 h=(px)10 {}
  }
}
"#;
    let err = parse_pack(src, PackSource::Preset).expect_err("must require a self-entry");
    assert!(err.message.contains("library self-entry"));
}

#[test]
fn load_project_packs_finds_libraries_dir_pack() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib_dir = dir.path().join("libraries");
    std::fs::create_dir_all(&lib_dir).expect("create libraries dir");
    std::fs::write(lib_dir.join("foo.zen"), FLOWCHART_SRC).expect("write pack");

    let packs = load_project_packs(dir.path());
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].id, "@zenith/flowchart");
    assert!(matches!(packs[0].source, PackSource::Project(_)));
}

#[test]
fn load_project_packs_missing_dir_is_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(load_project_packs(dir.path()).is_empty());
}

#[test]
fn resolve_packs_includes_embedded_when_no_project_dir() {
    let packs = resolve_packs(None);
    assert!(packs.iter().any(|p| p.id == "@zenith/flowchart"));
}

#[test]
fn resolve_packs_is_sorted_by_id() {
    let packs = resolve_packs(None);
    let mut sorted = packs.clone();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<_> = packs.iter().map(|p| &p.id).collect();
    let sorted_ids: Vec<_> = sorted.iter().map(|p| &p.id).collect();
    assert_eq!(ids, sorted_ids);
}

// ── materialize_action tests ─────────────────────────────────────────────

/// A minimal action pack that updates a single color token `color.brand`.
const ACTION_PACK_SRC_UPDATE: &str = r##"zenith version=1 {
  project id="@test/brandkit" name="Brand Kit"
  libraries { library id="@test/brandkit" version="2.0.0" }
  actions {
    action id="apply-brand-color" label="Apply Brand Color" {
      tx "{\"ops\":[{\"op\":\"update_token_value\",\"id\":\"color.brand\",\"value\":\"#e11d48\"}]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {
    }
  }
}
"##;

/// A target doc that declares the `color.brand` token the action touches.
const ACTION_TARGET_SRC: &str = r##"zenith version=1 {
  project id="proj.x" name="Target"
  tokens format="zenith-token-v1" {
    token id="color.brand" type="color" value="#111111"
  }
  styles {}
  document id="d" title="x" {
    page id="pg" w=(px)800 h=(px)600 {}
  }
}
"##;

/// Build a project-backed [`LibraryPack`] from inline `.zen` source by
/// writing it to a temp file, so [`load_pack_document`] can re-read it.
/// The returned [`tempfile::TempDir`] must be kept alive for the duration of
/// the test (dropping it deletes the backing file).
fn pack_from_src(src: &str) -> (tempfile::TempDir, LibraryPack) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("pack.zen");
    std::fs::write(&path, src).expect("write pack");
    let pack = parse_pack(src, PackSource::Project(path)).expect("pack parses");
    (dir, pack)
}

#[test]
fn materialize_action_accepted_updates_token_records_action_library_provenance() {
    let (_dir, pack) = pack_from_src(ACTION_PACK_SRC_UPDATE);
    let packs = vec![pack];
    let outcome = materialize_action(
        ACTION_TARGET_SRC,
        &packs,
        "@test/brandkit",
        "apply-brand-color",
    )
    .expect("materialize_action ok");

    // Status is Accepted or AcceptedWithWarnings.
    assert!(
        matches!(
            outcome.tx_result.status,
            TxStatus::Accepted | TxStatus::AcceptedWithWarnings
        ),
        "expected Accepted/AcceptedWithWarnings, got {:?}",
        outcome.tx_result.status
    );

    let final_src = outcome
        .final_source
        .expect("final_source must be Some on Accepted");
    let provenance_id = outcome.provenance_id.expect("provenance_id must be Some");

    // The updated token value is present in the output.
    assert!(
        final_src.contains("#e11d48"),
        "updated value must appear in final_source; got:\n{}",
        final_src
    );

    // An `actions` block with the action id is present.
    assert!(
        final_src.contains("apply-brand-color"),
        "action id must appear in final_source; got:\n{}",
        final_src
    );

    // A libraries import for the pack is present.
    assert!(
        final_src.contains("@test/brandkit"),
        "library import must appear in final_source; got:\n{}",
        final_src
    );

    // A provenance record referencing the action id is present.
    assert!(
        final_src.contains(&provenance_id),
        "provenance id must appear in final_source"
    );

    // Re-parse and validate the final source — must have no hard errors.
    let reparsed = KdlAdapter
        .parse(final_src.as_bytes())
        .expect("final_source must re-parse");
    assert!(
        hard_errors(&reparsed).is_empty(),
        "final_source must validate clean; errors: {:?}",
        hard_errors(&reparsed)
    );

    // Confirm the action + library + provenance appear in the parsed tree.
    assert!(
        reparsed.actions.iter().any(|a| a.id == "apply-brand-color"),
        "action must be in parsed actions"
    );
    assert!(
        reparsed.libraries.iter().any(|l| l.id == "@test/brandkit"),
        "library must be in parsed libraries"
    );
    assert!(
        reparsed
            .provenance
            .iter()
            .any(|p| p.node == "apply-brand-color"),
        "provenance node must be the action id"
    );

    // No warnings on a clean apply.
    assert!(
        outcome.warnings.is_empty(),
        "unexpected warnings: {:?}",
        outcome.warnings
    );
}

#[test]
fn materialize_action_rejected_when_token_not_found() {
    /// A pack that references a non-existent token id.
    const REJECT_PACK_SRC: &str = r##"zenith version=1 {
  project id="@test/reject" name="Reject Test"
  libraries { library id="@test/reject" version="1.0.0" }
  actions {
    action id="no-such-token" {
      tx "{\"ops\":[{\"op\":\"update_token_value\",\"id\":\"does.not.exist\",\"value\":\"#fff\"}]}"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {}
  }
}
"##;
    let (_dir, pack) = pack_from_src(REJECT_PACK_SRC);
    let packs = vec![pack];
    let outcome = materialize_action(ACTION_TARGET_SRC, &packs, "@test/reject", "no-such-token")
        .expect("materialize_action itself must succeed (rejected tx is not an Err)");

    assert_eq!(
        outcome.tx_result.status,
        TxStatus::Rejected,
        "tx must be Rejected"
    );
    assert!(
        outcome.final_source.is_none(),
        "final_source must be None on Rejected"
    );
    assert!(
        outcome.provenance_id.is_none(),
        "provenance_id must be None on Rejected"
    );
}

#[test]
fn materialize_action_unknown_pkg_errors_with_available() {
    let (_dir, pack) = pack_from_src(ACTION_PACK_SRC_UPDATE);
    let packs = vec![pack];
    let err = materialize_action(ACTION_TARGET_SRC, &packs, "@no/such", "apply-brand-color")
        .expect_err("unknown pkg errors");
    assert!(
        err.message.contains("unknown library package"),
        "msg: {}",
        err.message
    );
    assert!(
        err.message.contains("@test/brandkit"),
        "must list available packages; msg: {}",
        err.message
    );
}

#[test]
fn materialize_action_unknown_action_errors_with_available() {
    let (_dir, pack) = pack_from_src(ACTION_PACK_SRC_UPDATE);
    let packs = vec![pack];
    let err = materialize_action(
        ACTION_TARGET_SRC,
        &packs,
        "@test/brandkit",
        "no-such-action",
    )
    .expect_err("unknown action errors");
    assert!(
        err.message.contains("unknown action item"),
        "msg: {}",
        err.message
    );
    assert!(
        err.message.contains("apply-brand-color"),
        "must list available actions; msg: {}",
        err.message
    );
}

#[test]
fn materialize_action_malformed_tx_json_errors() {
    const MALFORMED_PACK_SRC: &str = r#"zenith version=1 {
  project id="@test/malformed" name="Malformed"
  libraries { library id="@test/malformed" version="1.0.0" }
  actions {
    action id="bad-action" {
      tx "not valid json"
    }
  }
  document id="d" title="x" {
    page id="pg" w=(px)100 h=(px)100 {}
  }
}
"#;
    let (_dir, pack) = pack_from_src(MALFORMED_PACK_SRC);
    let packs = vec![pack];
    let err = materialize_action(ACTION_TARGET_SRC, &packs, "@test/malformed", "bad-action")
        .expect_err("malformed tx_json must error");
    assert!(
        err.message.contains("malformed tx-script"),
        "msg: {}",
        err.message
    );
    assert!(
        err.message.contains("bad-action"),
        "error must name the action; msg: {}",
        err.message
    );
}
