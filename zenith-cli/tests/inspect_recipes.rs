//! Integration tests for `zenith inspect` — recipes block surfacing.
//!
//! Exercises the public [`zenith_cli::commands::inspect::run`] function directly
//! (same pattern as the unit tests in `commands/inspect/mod.rs`).

use zenith_cli::commands::inspect::run;

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Document with a fully-specified `recipes` block (two recipes).
const DOC_WITH_RECIPES: &str = r##"zenith version=1 {
  project id="proj.ri" name="Recipes Inspect Integration"
  tokens format="zenith-token-v1" {
    color id="color.sky" value="#87CEEB"
    color id="color.dusk" value="#FFB347"
  }
  styles {}
  recipes {
    recipe id="recipe.aurora" kind="aurora" seed=7 generator="aurora@2" bounds="page.ri" detached=#true {
      param name="density" value=(px)24
      param name="label" value="test"
      palette token="color.sky"
      palette token="color.dusk"
      expanded node="rect.a"
      expanded node="rect.b"
    }
    recipe id="recipe.scatter" kind="scatter" seed=99 {
      param name="count" value=(px)5
    }
  }
  document id="doc.ri" title="Recipes Inspect Integration" {
    page id="page.ri" w=(px)800 h=(px)600 {
      rect id="rect.a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="rect.b" x=(px)110 y=(px)0 w=(px)100 h=(px)100
    }
  }
}
"##;

/// Document with no `recipes` block at all.
const DOC_NO_RECIPES: &str = r##"zenith version=1 {
  project id="proj.nr2" name="No Recipes Integration"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.nr2" title="No Recipes Integration" {
    page id="page.nr2" w=(px)400 h=(px)300 {
      rect id="rect.nr2" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;

// ── Human output: document with recipes ──────────────────────────────────────

#[test]
fn human_output_includes_recipe_ids() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("recipe.aurora"),
        "human output must include first recipe id; got:\n{out}"
    );
    assert!(
        out.contains("recipe.scatter"),
        "human output must include second recipe id; got:\n{out}"
    );
}

#[test]
fn human_output_includes_recipe_kinds() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("kind=aurora"),
        "human output must show kind=aurora; got:\n{out}"
    );
    assert!(
        out.contains("kind=scatter"),
        "human output must show kind=scatter; got:\n{out}"
    );
}

#[test]
fn human_output_includes_seed_and_generator() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("seed=7"),
        "human output must show seed; got:\n{out}"
    );
    assert!(
        out.contains("generator=aurora@2"),
        "human output must show generator; got:\n{out}"
    );
}

#[test]
fn human_output_includes_bounds_and_detached() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("bounds=page.ri"),
        "human output must show bounds; got:\n{out}"
    );
    assert!(
        out.contains("detached=true"),
        "human output must show detached=true; got:\n{out}"
    );
}

#[test]
fn human_output_includes_params() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("param density"),
        "human output must include density param; got:\n{out}"
    );
    assert!(
        out.contains("(px)24"),
        "human output must include dimension value; got:\n{out}"
    );
    assert!(
        out.contains("param label"),
        "human output must include label param; got:\n{out}"
    );
    assert!(
        out.contains("test"),
        "human output must include literal value; got:\n{out}"
    );
}

#[test]
fn human_output_includes_palette_tokens() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("color.sky"),
        "human output must include palette token color.sky; got:\n{out}"
    );
    assert!(
        out.contains("color.dusk"),
        "human output must include palette token color.dusk; got:\n{out}"
    );
}

#[test]
fn human_output_includes_expanded_nodes() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("rect.a"),
        "human output must include expanded node rect.a; got:\n{out}"
    );
    assert!(
        out.contains("rect.b"),
        "human output must include expanded node rect.b; got:\n{out}"
    );
}

#[test]
fn human_output_also_contains_pages() {
    let out = run(DOC_WITH_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        out.contains("page page.ri"),
        "human output must still include the pages section; got:\n{out}"
    );
}

// ── Human output: document without recipes ───────────────────────────────────

#[test]
fn human_output_no_recipes_section_when_empty() {
    let out = run(DOC_NO_RECIPES, None, false).expect("inspect must succeed");
    assert!(
        !out.contains("recipe"),
        "human output must not contain 'recipe' when doc has no recipes block; got:\n{out}"
    );
    // Pages must still appear.
    assert!(
        out.contains("page page.nr2"),
        "pages section must still appear; got:\n{out}"
    );
}

// ── JSON output: document with recipes ───────────────────────────────────────

#[test]
fn json_output_includes_recipes_array() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
    let arr = v["recipes"]
        .as_array()
        .expect("recipes must be a JSON array");
    assert_eq!(arr.len(), 2, "must have 2 recipe entries");
}

#[test]
fn json_output_recipe_ids_in_source_order() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["recipes"].as_array().unwrap();
    assert_eq!(arr[0]["id"], "recipe.aurora");
    assert_eq!(arr[1]["id"], "recipe.scatter");
}

#[test]
fn json_output_recipe_scalars() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let aurora = &v["recipes"][0];
    assert_eq!(aurora["kind"], "aurora");
    assert_eq!(aurora["seed"], 7);
    assert_eq!(aurora["generator"], "aurora@2");
    assert_eq!(aurora["bounds"], "page.ri");
    assert_eq!(aurora["detached"], true);
}

#[test]
fn json_output_recipe_scalars_absent_for_minimal_recipe() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let scatter = &v["recipes"][1];
    // Fields absent in source must be omitted (skip_serializing_if = Option::is_none).
    assert!(
        scatter.get("generator").is_none(),
        "generator must be absent"
    );
    assert!(scatter.get("bounds").is_none(), "bounds must be absent");
    assert!(scatter.get("detached").is_none(), "detached must be absent");
}

#[test]
fn json_output_recipe_params() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let params = v["recipes"][0]["params"].as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["name"], "density");
    assert_eq!(params[0]["value"], "(px)24");
    assert_eq!(params[1]["name"], "label");
    assert_eq!(params[1]["value"], "test");
}

#[test]
fn json_output_recipe_palette_and_expanded() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let aurora = &v["recipes"][0];
    let palette = aurora["palette"].as_array().unwrap();
    assert_eq!(palette, &["color.sky", "color.dusk"]);
    let expanded = aurora["expanded"].as_array().unwrap();
    assert_eq!(expanded, &["rect.a", "rect.b"]);
}

#[test]
fn json_output_schema_and_pages_unaffected() {
    let out = run(DOC_WITH_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], "zenith-inspect-v1");
    let pages = v["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0]["id"], "page.ri");
}

// ── JSON output: document without recipes ────────────────────────────────────

#[test]
fn json_output_empty_recipes_array_when_no_recipes_block() {
    let out = run(DOC_NO_RECIPES, None, true).expect("inspect must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v["recipes"]
        .as_array()
        .expect("recipes must be present as empty array");
    assert!(arr.is_empty(), "recipes array must be empty; got: {arr:?}");
}
