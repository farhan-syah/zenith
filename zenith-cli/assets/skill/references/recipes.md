# Visual recipes — procedural backgrounds & effects

Deterministic, **editable** backgrounds and effects built from native nodes — no flattened
raster, no AI-image lottery. Every snippet uses syntax confirmed in the repo's `examples/*.zen`.

> **Hard rule the validator enforces:** a node's visual properties (`fill`, `stroke`,
> `background`, …) must reference a **token** — raw hex on a node is rejected with
> `token.raw_visual_literal`. So colors live in `token`s (whose _values_ are hex, alpha
> allowed), and nodes reference them with `(token)"id"`. The snippets below follow this; keep
> doing it. After pasting any recipe, run `zenith validate <file> --json` and render it.

Confirmed building blocks (see the named example):

- Gradient tokens — `gradient.zen` (`angle=(deg)`, or `radial=#true center-x center-y radius`, `stop offset color=(token)`).
- Translucency — put alpha in the **token value**: `value="#0f172a55"` (last two hex digits = alpha). `shadow.zen`.
- Per-node blur — `blur=(px)N`; blend — `blend-mode="screen"` etc. (`blur.zen`).
- Shadows & glows — `shadow` token with one or more `layer dx dy blur color=(token)` (`shadow.zen`).
- Rounded corners — `radius=(token)` (`shadow.zen`).
- Lines & paths — `line`, `polyline { point x y … }`, `polygon { point … }` (`line.zen`, `polyline.zen`, `polygon.zen`).
- Image treatment — `filter` token (e.g. `duotone`) via `image … filter=(token)` (`filter.zen`).
- Clipping — `frame x y w h { … }` clips children (`frame.zen`).

---

## Recipe: layered gradient + translucent blobs (premium abstract)

Dark gradient field with large, translucent, partly off-canvas color shapes behind the content.

```kdl
tokens format="zenith-token-v1" {
  token id="color.bg.top" type="color" value="#0b1020"
  token id="color.bg.bottom" type="color" value="#1e1b4b"
  token id="color.blob.in" type="color" value="#22d3ee66"   # alpha in the value
  token id="color.blob.out" type="color" value="#22d3ee00"  # fully transparent edge
  token id="grad.bg" type="gradient" angle=(deg)135 {
    stop offset=0.0 color=(token)"color.bg.top"
    stop offset=1.0 color=(token)"color.bg.bottom"
  }
  token id="grad.blob" type="gradient" radial=#true center-x=0.5 center-y=0.5 radius=0.6 {
    stop offset=0.0 color=(token)"color.blob.in"
    stop offset=1.0 color=(token)"color.blob.out"
  }
}
# on the page:
page id="page.hero" w=(px)1080 h=(px)1080 background=(token)"grad.bg" {
  ellipse id="bg.blob.1" x=(px)-120 y=(px)-80 w=(px)640 h=(px)640 fill=(token)"grad.blob"
  ellipse id="bg.blob.2" x=(px)620 y=(px)500 w=(px)700 h=(px)700 fill=(token)"grad.blob"
}
```

Tune mood by editing only the stop tokens — geometry stays put.

## Recipe: aurora glow (soft blurred fields on dark)

Large gradient/solid ellipses, blurred and screen-blended over a near-black background.

```kdl
tokens format="zenith-token-v1" {
  token id="color.night" type="color" value="#05060a"
  token id="color.cyan" type="color" value="#22d3ee"
  token id="color.violet" type="color" value="#7c3aed"
  token id="color.green" type="color" value="#34d399"
}
page id="page.aurora" w=(px)1920 h=(px)1080 background=(token)"color.night" {
  ellipse id="bg.aurora.cyan"   x=(px)200  y=(px)200 w=(px)900 h=(px)700 fill=(token)"color.cyan"   blur=(px)120 blend-mode="screen"
  ellipse id="bg.aurora.violet" x=(px)700  y=(px)120 w=(px)900 h=(px)800 fill=(token)"color.violet" blur=(px)140 blend-mode="screen"
  ellipse id="bg.aurora.green"  x=(px)1100 y=(px)420 w=(px)800 h=(px)700 fill=(token)"color.green"  blur=(px)120 blend-mode="screen"
}
```

Adjust `blur` for softness; overlap the blobs.

## Recipe: soft card / frosted-looking panel

```kdl
tokens format="zenith-token-v1" {
  token id="color.glass" type="color" value="#ffffff22"   # translucent white
  token id="color.shadow" type="color" value="#0f172a55"
  token id="size.radius" type="dimension" value=(px)24
  token id="shadow.soft" type="shadow" { layer dx=(px)0 dy=(px)12 blur=(px)32 color=(token)"color.shadow" }
}
rect id="card.panel" x=(px)120 y=(px)300 w=(px)840 h=(px)480 fill=(token)"color.glass" radius=(token)"size.radius" shadow=(token)"shadow.soft"
```

This is _translucency_, not true backdrop blur (blurring the pixels behind the card). If true
glassmorphism is required, confirm backdrop-blur support in `examples/`/`--help` before
promising it.

## Recipe: topographic / contour line pattern

Repeated thin polylines make a terrain motif. Record the generation parameters in a `note` so
it is replayable, and group the lines so the motif edits as one unit.

```kdl
tokens format="zenith-token-v1" {
  token id="color.topo" type="color" value="#94a3b8"
  token id="size.hair" type="dimension" value=(px)2
}
note "topographic bg: 24 lines, amplitude 40px, spacing 28px, seed 42"
group id="bg.topo" {
  polyline id="bg.topo.01" stroke=(token)"color.topo" stroke-width=(token)"size.hair" {
    point x=(px)0 y=(px)420
    point x=(px)360 y=(px)380
    point x=(px)720 y=(px)440
    point x=(px)1080 y=(px)400
  }
  # …repeat with stable ids bg.topo.02, .03, … at increasing y
}
```

Keep the stroke low-contrast and the group below text.

## Recipe: seeded geometric motif

Scatter `ellipse` / `polygon` / `line` nodes from brand tokens. Determinism is on you: compute
positions from a fixed seed and store the seed + params in a `note`.

```kdl
tokens format="zenith-token-v1" {
  token id="color.tri" type="color" value="#6366f133"
  token id="color.dot" type="color" value="#22d3ee2a"
}
note "geometric motif: circles+triangles, seed 7, count 18"
group id="bg.motif" {
  polygon id="bg.motif.tri.01" fill=(token)"color.tri" {
    point x=(px)160 y=(px)40
    point x=(px)260 y=(px)170
    point x=(px)60 y=(px)170
  }
  ellipse id="bg.motif.dot.01" x=(px)400 y=(px)120 w=(px)90 h=(px)90 fill=(token)"color.dot"
  # …
}
```

## Recipe: grain / paper texture

Use the native `noise` filter kind — no external asset required. Declare it inside a `filter`
token and apply it to any node via `filter=(token)`. `seed` pins the deterministic grain, `scale`
sets the cell size (1 = fine, larger = coarser blocks), `amount` (0..1) sets intensity.

```kdl
tokens format="zenith-token-v1" {
  token id="filter.grain" type="filter" { noise seed=7 scale=1 amount=0.35 }
}
rect id="bg" x=(px)0 y=(px)0 w=(px)1080 h=(px)1080 fill=(token)"color.bg" filter=(token)"filter.grain"
```

Keep `amount` low so it never harms text contrast. See `examples/noise.zen` for fine vs coarse
grain side by side.

## Recipe: drop shadow & outer glow

`shadow` tokens take one or more `layer`s. Offset = drop shadow; zero-offset + large blur =
glow. The layer `color` references a token.

```kdl
tokens format="zenith-token-v1" {
  token id="color.glow" type="color" value="#22d3eeaa"
  token id="color.orb" type="color" value="#22d3ee"
  token id="shadow.glow" type="shadow" { layer dx=(px)0 dy=(px)0 blur=(px)40 color=(token)"color.glow" }
}
ellipse id="hero.orb" x=(px)440 y=(px)360 w=(px)200 h=(px)200 fill=(token)"color.orb" shadow=(token)"shadow.glow"
```

## Recipe: duotone / tinted photo

```kdl
tokens format="zenith-token-v1" {
  token id="color.duo.shadow" type="color" value="#1e1b4b"
  token id="color.duo.highlight" type="color" value="#fbbf24"
  token id="filter.duo" type="filter" { duotone shadow=(token)"color.duo.shadow" highlight=(token)"color.duo.highlight" }
}
image id="hero.photo" asset="asset.photo" x=(px)0 y=(px)0 w=(px)1080 h=(px)600 fit="cover" filter=(token)"filter.duo"
```

## Recipe: re-skin to another palette (one edit)

Because every fill is a token, a palette swap is token-value edits only. Do it with a
transaction (dry-run first):

```bash
zenith tx doc.zen palette.tx.json            # preview the diff
zenith tx doc.zen palette.tx.json --apply    # write it
```

See `references/brand.md` for brand-kit packs that bundle these swaps as reusable `actions`,
and `references/color.md` for the token model. Run `zenith tx --help` for the op schema.

## Recipe: repeating tiled motif via pattern grid

The `pattern` node tiles a single motif template across a bounds box deterministically — no
manual node authoring, no seed-computing loop. Change `spacing` for density; add `jitter` for
organic variety. See `references/pattern.md` for the full attribute reference.

```kdl
tokens format="zenith-token-v1" {
  token id="color.dot" type="color" value="#22d3ee55"
}
pattern id="bg.dots" kind="grid" x=(px)0 y=(px)0 w=(px)1080 h=(px)1080 spacing=(px)48 jitter=0.15 seed=7 {
  ellipse id="dot" x=(px)0 y=(px)0 w=(px)8 h=(px)8 fill=(token)"color.dot"
}
```

- `spacing`, like `x`/`y`/`w`/`h`, is a literal dimension (`(px)N`) — it is **not** a token
  reference. Writing `spacing=(token)"…"` leaves the grid with no spacing and fires
  `pattern.grid_missing_spacing`.
- A token used **only** inside the motif (here `color.dot`) currently reads as `token.unused`
  (advisory) — the validator does not descend into the template. The pattern still renders; the
  advisory is harmless.
- The bounds box clips instances at the edges.
- `jitter=0.15` displaces each cell by up to ±15 % of `spacing` per axis (seed-derived, x/y
  uncorrelated).
- To freeze the tiled instances into individually-editable nodes: `zenith tx doc.zen detach.json`
  where `detach.json` is `{"ops":[{"op":"detach_pattern","node":"bg.dots"}]}` (tx files always wrap
  their ops in an `ops` array).

## Recipe: random scatter via pattern scatter

Scatter a fixed count of motif instances at seed-derived pseudo-random positions inside the
bounds. Change `seed` to rotate the arrangement; change `count` to control density.

```kdl
tokens format="zenith-token-v1" {
  token id="color.accent" type="color" value="#f59e0b99"
}
pattern id="bg.scatter" kind="scatter" x=(px)0 y=(px)0 w=(px)1080 h=(px)1080 count=25 seed=42 {
  rect id="star" x=(px)0 y=(px)0 w=(px)12 h=(px)12 fill=(token)"color.accent" rotate=(deg)45
}
```

The motif `id` (`star`) is a template — it is not addressable. Rendered instances carry ids
`bg.scatter.0` … `bg.scatter.24`. For the full attribute table and `detach_pattern` op, see
`references/pattern.md`.
