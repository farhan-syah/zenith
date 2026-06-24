# Recipe model — the `recipes` provenance block

`references/recipes.md` is about the *techniques* (how to compose gradients, blur, grain, etc.
into premium visuals). This file is about **recording** such a generated motif as a first-class,
auditable `recipes` block so it can be inspected, re-parameterized, and reproduced.

The recipe block is **model + provenance only** — it does not add a new render primitive. The
actual pixels come from the nodes the recipe materialized (its `expanded` nodes) and/or composed
shipped primitives. The block captures *how* those were generated so the work is replayable, not a
black box.

## The block

Document-level, additive (absent = byte-identical output):

```kdl
recipes {
  recipe id="recipe.aurora" kind="aurora" seed=42 generator="aurora@1" bounds="page.hero" detached=#false {
    param name="density"    value=(number)0.6
    param name="complexity" value=(number)3
    palette token="color.brand.navy"
    palette token="color.brand.cyan"
    expanded node="blob.1"
    expanded node="blob.2"
  }
  recipe id="recipe.scatter" kind="scatter" {
  }
}
```

- `kind` — generator family (`"aurora"`, `"scatter"`, …).
- `seed` — integer seed for deterministic generation (optional).
- `generator` — generator version/hash (e.g. `"aurora@1"`) so a regen is pinned (optional).
- `bounds` — the page or frame id the motif fills (optional).
- `detached` — `#true` once the materialized nodes are "frozen" away from the recipe (optional).
- `param name=… value=…` — numeric/scalar generation parameters.
- `palette token=…` — the **color tokens** the motif draws from (these count as token usage, so
  they won't trip `token.unused`).
- `expanded node=…` — the ids of the real nodes the recipe materialized into the document.

## Validation

`zenith validate` enforces (all Error): `recipe.duplicate_id`, `recipe.unknown_palette_token`
(must reference a declared **color** token), `recipe.unknown_expanded_node`, and
`recipe.unknown_bounds` (bounds id must be a page or node id).

## Inspect

`zenith inspect <file>` (and `--json`) surfaces the `recipes` block alongside the page tree — use
it to see which recipes a document declares and which nodes each expanded into.

## Editing via transactions

Manage recipes with typed `tx` ops (dry-run by default; `--apply` to write):

- `CreateRecipe` — `{ id, kind, seed?, generator?, bounds?, detached? }`
- `UpdateRecipe` — change scalar fields; `params`/`palette`/`expanded` are preserved. Re-parameterize
  by updating `seed`/`generator`/params, then regenerate. **Detach** = `UpdateRecipe { detached: true }`.
- `DeleteRecipe` — remove the record.

Errors mirror token ops: `tx.duplicate_id`, `tx.unknown_recipe`.

## Workflow

1. Build the motif by composing primitives (`references/recipes.md`), giving the generated nodes
   stable ids.
2. Record a `recipe` capturing `kind`, `seed`, `generator`, `param`s, the `palette` tokens, and the
   `expanded` node ids — so the look is reproducible and re-tunable, not a one-off.
3. `zenith validate`, then `zenith inspect` to confirm the recipe and its nodes.

> Note: the `pattern` node (grid and scatter tiling) **is now shipped** — see
> `references/pattern.md` for attributes, diagnostics, and the `detach_pattern` op. Procedural
> **noise/grain** is also shipped, as a `noise` filter kind (`noise seed=.. scale=.. amount=..`
> inside a `filter` token); see `examples/noise.zen`.
