# Layout — pages, anchors, frames, spreads

## Pages

```kdl
document id="doc.social" title="Social" {
  page id="page.sq" w=(px)1080 h=(px)1080 background=(token)"color.bg" {
    # nodes…
  }
}
```

- `w` / `h` set the canvas in px; `background` takes a color **or** gradient token.
- A document can hold multiple pages (deck slides, book pages, size variants). Render one with
  `--page N`, all with `--all-pages <dir>`, or a facing-page `--spread A-B`.

## Coordinates vs anchors

Nodes take explicit `x y w h`. Instead of hand-computing position, set `anchor` to a nine-point
name and let it resolve to deterministic geometry (identical bytes to the hand-placed version).
An explicitly-authored `x` or `y` always wins over the anchor-derived value **per axis**, and the
node's `w`/`h` must be present (px) for derivation.

Nine-point names (see `examples/anchors.zen`):
`top-left top-center top-right center-left center center-right bottom-left bottom-center bottom-right`.

```kdl
rect id="logo" w=(px)160 h=(px)60 fill=(token)"color.brand" anchor="top-left"
text id="cta"  w=(px)300 h=(px)80  anchor="bottom-right" font-size=(token)"size.body" { span "Buy now" }
```

### Reference frame: page, safe-zone, parent, or sibling

The nine-point name says *which corner*; a second attribute chooses *what it's relative to*.
Default is the page. Precedence when more than one is present: **zone > sibling > parent > page**.

- **Page** (default) — `anchor="bottom-right"` → relative to the page box.
- **Safe-zone** — declare a `safe-zone` in the page, then anchor into it:
  ```kdl
  page id="page.main" w=(px)1080 h=(px)1080 {
    safe-zone id="sz.body" type="required" x=(px)80 y=(px)80 w=(px)920 h=(px)920
    text id="cta" w=(px)300 h=(px)80 anchor="bottom-right" anchor-zone="sz.body" { span "Buy now" }
  }
  ```
  Keeps content off the bleed/margins — anchor relative to the safe area, not the page edge.
- **Parent container** — `anchor-parent=#true` anchors within the node's direct `frame`/`group`
  box instead of the page (e.g. a label pinned to the corner of its card).
- **Sibling** — `anchor-sibling="<id>"` anchors relative to a sibling node's box in the same
  container (e.g. a badge clinging to the top-right of a logo). The sibling must be an
  anchor-bearing node with known `w`/`h`; cycles are rejected (`anchor.cycle`).

All four resolve to explicit, deterministic geometry at compile time (absent anchor = byte-identical
to hand-placed coords). Use them for logos, page numbers, captions, badges, and CTAs so they stay
correctly placed across size variants (see `references/format-variants.md`).

### Adjacent-edge placement (`anchor-edge`, `anchor-gap`)

When you want a node placed **outside** a sibling — stacked above, below, before (left), or after
(right) — add `anchor-edge` alongside `anchor-sibling`. Unlike the nine-point `anchor` (which
positions a node *inside* the sibling's box), `anchor-edge` places the node flush against the
named edge:

| `anchor-edge` | Main-axis position | Cross-axis default |
|---|---|---|
| `below` | `sib_y + sib_h + gap` | left-aligned with sibling (`sib_x`) |
| `above` | `sib_y − gap − node_h` | left-aligned with sibling (`sib_x`) |
| `after` | `sib_x + sib_w + gap` | top-aligned with sibling (`sib_y`) |
| `before` | `sib_x − gap − node_w` | top-aligned with sibling (`sib_y`) |

`anchor-gap=(px)N` inserts a pixel gap between the sibling's edge and the node (default 0).

**Cross-axis alignment** is controlled by the nine-point `anchor` value — only the relevant
component is used:

- For `above`/`below`: the *horizontal* component (`top-left`/`top-center`/`top-right` etc.)
  left-, center-, or right-aligns the node relative to the sibling's width.
- For `before`/`after`: the *vertical* component top-, center-, or bottom-aligns relative to the
  sibling's height.
- When `anchor` is absent, the cross-axis defaults to the leading edge (left for `above`/`below`;
  top for `before`/`after`).

When `anchor-edge` is present, `anchor` and explicit `x`/`y` are both optional — `anchor-edge`
derives the full position without them. Explicit `x` or `y` still win per-axis over any
anchor-derived value.

```kdl
// Stack a caption card directly below a title, centered, with an 8 px gap.
text id="title" x=(px)40 y=(px)30 w=(px)320 h=(px)48 font-size=(token)"size.heading" { span "Launch" }
rect id="card" anchor-sibling="title" anchor-edge="below" anchor-gap=(px)8 anchor="top-center"
     w=(px)240 h=(px)120 fill=(token)"color.card" radius=(token)"size.radius"
```

**Diagnostics:**

- `anchor.edge_without_sibling` (Warning) — `anchor-edge` is set but `anchor-sibling` is absent;
  the placement has no effect.
- `anchor.unknown_edge` (Error) — the `anchor-edge` value is not one of `above below before after`.
- `anchor.gap_invalid_unit` (Warning) — `anchor-gap` unit cannot be resolved to px.

## Frames (clipping) and groups

- `frame id x y w h { … }` clips its children to its box — use it for image windows, cards,
  and any "nothing escapes this region" layout (`examples/frame.zen`).
- `group id { … }` bundles nodes logically (no clip) so a whole motif moves/dims/deletes as a
  unit (`examples/group.zen`). Opacity and transforms cascade through groups/frames.

## Dividers and rules

`line x1 y1 x2 y2 stroke=(token) stroke-width=(token)` for separators/rules
(`examples/line.zen`).

## Multi-size variants

For square/story/banner from one design, **declare a `variants` block and run `zenith variant`**
— a first-class, deterministic regeneration model (see `references/format-variants.md`). It
expands one canonical page into N named target sizes, with per-variant `override`s and automatic
token propagation. Anchored nodes reflow to each size; reposition free-coordinate decorative nodes
via overrides. (This is size/format variation — for varying *content* across data rows, that's
`zenith merge`; see `references/variants.md`.)

## Always verify

Anchors, frames, and clipping interact; render (`--all-pages` for a contact sheet) and look
before finalizing, and `zenith validate` to catch off-canvas / overflow.
