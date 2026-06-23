//! Recipe-block rendering for `zenith inspect`.
//!
//! The public surface is two pure functions:
//! - [`build_recipe_entries`] — converts `&[RecipeDef]` to
//!   `Vec<RecipeInspectJson>` for the `--json` path.
//! - [`render_recipes_human`] — formats the same data as a human-readable
//!   section string, mirroring the style used for `pages` output.

use zenith_core::{PropertyValue, RecipeDef};

use crate::json_types::{RecipeInspectJson, RecipeParamInspectJson};

// ── JSON builder ──────────────────────────────────────────────────────────────

/// Convert a slice of [`RecipeDef`] to [`RecipeInspectJson`] entries (source
/// order is preserved).
pub fn build_recipe_entries(recipes: &[RecipeDef]) -> Vec<RecipeInspectJson> {
    recipes.iter().map(recipe_to_json).collect()
}

fn recipe_to_json(r: &RecipeDef) -> RecipeInspectJson {
    RecipeInspectJson {
        id: r.id.clone(),
        kind: r.kind.clone(),
        seed: r.seed,
        generator: r.generator.clone(),
        bounds: r.bounds.clone(),
        detached: r.detached,
        params: r
            .params
            .iter()
            .map(|p| RecipeParamInspectJson {
                name: p.name.clone(),
                value: property_value_str(&p.value),
            })
            .collect(),
        palette: r.palette.clone(),
        expanded: r.expanded.clone(),
    }
}

// ── Human renderer ────────────────────────────────────────────────────────────

/// Render the `recipes` section for human output.
///
/// Returns an empty string when `recipes` is empty (consistent with how the
/// pages section simply omits pages that do not exist — the caller emits nothing
/// in that case).
pub fn render_recipes_human(recipes: &[RecipeDef]) -> String {
    if recipes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for r in recipes {
        // Header line: `recipe <id>  kind=<kind>`
        out.push_str(&format!("recipe {}  kind={}\n", r.id, r.kind));

        // Salient scalar attributes (only when present)
        if let Some(seed) = r.seed {
            out.push_str(&format!("  seed={}\n", seed));
        }
        if let Some(ref g) = r.generator {
            out.push_str(&format!("  generator={}\n", g));
        }
        if let Some(ref bounds) = r.bounds {
            out.push_str(&format!("  bounds={}\n", bounds));
        }
        if let Some(detached) = r.detached {
            out.push_str(&format!("  detached={}\n", detached));
        }

        // Params: one line each
        for p in &r.params {
            out.push_str(&format!(
                "  param {}  {}\n",
                p.name,
                property_value_str(&p.value)
            ));
        }

        // Palette: one line with all token ids, or a count when ≥ 5 entries.
        if !r.palette.is_empty() {
            if r.palette.len() < 5 {
                out.push_str(&format!("  palette [{}]\n", r.palette.join(", ")));
            } else {
                out.push_str(&format!("  palette [{} tokens]\n", r.palette.len()));
            }
        }

        // Expanded: one line with all node ids, or a count when ≥ 5 entries.
        if !r.expanded.is_empty() {
            if r.expanded.len() < 5 {
                out.push_str(&format!("  expanded [{}]\n", r.expanded.join(", ")));
            } else {
                out.push_str(&format!("  expanded [{} nodes]\n", r.expanded.len()));
            }
        }
    }
    out.trim_end().to_owned()
}

// ── PropertyValue stringifier ─────────────────────────────────────────────────

/// Render a [`PropertyValue`] as a canonical display string.
///
/// - `TokenRef(id)`  → `"<id>"` (bare token id)
/// - `Literal(s)`    → the raw literal string
/// - `Dimension(d)`  → `"(<unit>)<value>"`, e.g. `"(px)24"` or `"(pt)13.5"`
pub fn property_value_str(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::TokenRef(id) => id.clone(),
        PropertyValue::Literal(s) => s.clone(),
        PropertyValue::Dimension(d) => d.to_kdl_string(),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_core::{KdlAdapter, KdlSource};

    // A document with a two-recipe `recipes` block.
    const DOC_WITH_RECIPES: &str = r##"zenith version=1 {
  project id="proj.r" name="Recipe Inspect Test"
  tokens format="zenith-token-v1" {
    color id="color.sky" value="#87CEEB"
    color id="color.dusk" value="#FFB347"
  }
  styles {}
  recipes {
    recipe id="recipe.aurora" kind="aurora" seed=42 generator="aurora@1" bounds="page.1" detached=#false {
      param name="density" value=(px)16
      param name="label" value="hello"
      palette token="color.sky"
      palette token="color.dusk"
      expanded node="node.a"
      expanded node="node.b"
    }
    recipe id="recipe.scatter" kind="scatter" {
      param name="count" value=(px)8
    }
  }
  document id="doc.r" title="Recipe Inspect Test" {
    page id="page.1" w=(px)800 h=(px)600 {
      rect id="node.a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="node.b" x=(px)110 y=(px)0 w=(px)100 h=(px)100
    }
  }
}
"##;

    // A document with NO recipes block.
    const DOC_NO_RECIPES: &str = r##"zenith version=1 {
  project id="proj.nr" name="No Recipes"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.nr" title="No Recipes" {
    page id="page.nr" w=(px)400 h=(px)300 {
      rect id="rect.nr" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;

    // ── build_recipe_entries ──────────────────────────────────────────────────

    #[test]
    fn build_entries_preserves_source_order() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let entries = build_recipe_entries(&doc.recipes);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "recipe.aurora");
        assert_eq!(entries[1].id, "recipe.scatter");
    }

    #[test]
    fn build_entries_scalars_present() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let entries = build_recipe_entries(&doc.recipes);
        let aurora = &entries[0];
        assert_eq!(aurora.kind, "aurora");
        assert_eq!(aurora.seed, Some(42));
        assert_eq!(aurora.generator.as_deref(), Some("aurora@1"));
        assert_eq!(aurora.bounds.as_deref(), Some("page.1"));
        assert_eq!(aurora.detached, Some(false));
    }

    #[test]
    fn build_entries_scalars_absent_for_minimal_recipe() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let entries = build_recipe_entries(&doc.recipes);
        let scatter = &entries[1];
        assert_eq!(scatter.seed, None);
        assert_eq!(scatter.generator, None);
        assert_eq!(scatter.bounds, None);
        assert_eq!(scatter.detached, None);
    }

    #[test]
    fn build_entries_params_and_palette_and_expanded() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let entries = build_recipe_entries(&doc.recipes);
        let aurora = &entries[0];
        assert_eq!(aurora.params.len(), 2);
        assert_eq!(aurora.params[0].name, "density");
        assert_eq!(aurora.params[0].value, "(px)16");
        assert_eq!(aurora.params[1].name, "label");
        assert_eq!(aurora.params[1].value, "hello");
        assert_eq!(aurora.palette, vec!["color.sky", "color.dusk"]);
        assert_eq!(aurora.expanded, vec!["node.a", "node.b"]);
    }

    #[test]
    fn build_entries_empty_when_no_recipes() {
        let doc = KdlAdapter.parse(DOC_NO_RECIPES.as_bytes()).unwrap();
        let entries = build_recipe_entries(&doc.recipes);
        assert!(entries.is_empty());
    }

    // ── render_recipes_human ─────────────────────────────────────────────────

    #[test]
    fn human_output_contains_recipe_ids_and_kinds() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let out = render_recipes_human(&doc.recipes);
        assert!(
            out.contains("recipe.aurora"),
            "must contain first recipe id"
        );
        assert!(
            out.contains("kind=aurora"),
            "must contain first recipe kind"
        );
        assert!(
            out.contains("recipe.scatter"),
            "must contain second recipe id"
        );
        assert!(
            out.contains("kind=scatter"),
            "must contain second recipe kind"
        );
    }

    #[test]
    fn human_output_contains_scalars() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let out = render_recipes_human(&doc.recipes);
        assert!(out.contains("seed=42"), "must contain seed");
        assert!(out.contains("generator=aurora@1"), "must contain generator");
        assert!(out.contains("bounds=page.1"), "must contain bounds");
        assert!(out.contains("detached=false"), "must contain detached");
    }

    #[test]
    fn human_output_contains_params() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let out = render_recipes_human(&doc.recipes);
        assert!(out.contains("param density"), "must contain density param");
        assert!(out.contains("(px)16"), "must contain dimension value");
        assert!(out.contains("param label"), "must contain label param");
        assert!(out.contains("hello"), "must contain literal value");
    }

    #[test]
    fn human_output_contains_palette_and_expanded() {
        let doc = KdlAdapter.parse(DOC_WITH_RECIPES.as_bytes()).unwrap();
        let out = render_recipes_human(&doc.recipes);
        assert!(out.contains("color.sky"), "must contain palette token");
        assert!(out.contains("color.dusk"), "must contain palette token");
        assert!(out.contains("node.a"), "must contain expanded node");
        assert!(out.contains("node.b"), "must contain expanded node");
    }

    #[test]
    fn human_output_empty_when_no_recipes() {
        let doc = KdlAdapter.parse(DOC_NO_RECIPES.as_bytes()).unwrap();
        let out = render_recipes_human(&doc.recipes);
        assert!(
            out.is_empty(),
            "must return empty string for doc with no recipes"
        );
    }

    // ── property_value_str ────────────────────────────────────────────────────

    #[test]
    fn property_value_str_token_ref() {
        let pv = PropertyValue::TokenRef("color.primary".to_owned());
        assert_eq!(property_value_str(&pv), "color.primary");
    }

    #[test]
    fn property_value_str_literal() {
        let pv = PropertyValue::Literal("hello".to_owned());
        assert_eq!(property_value_str(&pv), "hello");
    }

    #[test]
    fn property_value_str_dimension_px() {
        use zenith_core::{Dimension, Unit};
        let pv = PropertyValue::Dimension(Dimension {
            value: 16.0,
            unit: Unit::Px,
        });
        assert_eq!(property_value_str(&pv), "(px)16");
    }

    #[test]
    fn property_value_str_dimension_pt_fractional() {
        use zenith_core::{Dimension, Unit};
        let pv = PropertyValue::Dimension(Dimension {
            value: 13.5,
            unit: Unit::Pt,
        });
        assert_eq!(property_value_str(&pv), "(pt)13.5");
    }
}
