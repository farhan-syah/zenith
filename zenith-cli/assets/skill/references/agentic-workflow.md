# Agentic visual workflow

How an agent takes a vague brief to a finished, auditable design without polluting the final
file. This encodes the full loop using **today's** Zenith primitives. Where a step is a
_convention_ rather than a first-class engine feature, that is called out — follow the
convention; don't claim the engine enforces it.

> Exact transaction op names and flags: `zenith tx --help` and `examples/*.tx.json`
> (e.g. `examples/center.tx.json`). Verify before you rely on a specific op.

## 1. Capture the brief and plan (traceability)

Before generating anything, write the brief down _in the document_ so the result can be traced
back to intent:

- Put the goal, palette, mood, and layer plan in `note` / `role="guide"` content, or a sidecar
  `*.brief.md` next to the `.zen`. These do not render.
- Reference intended layer groups by the **stable ids** you will create (`bg.*`, `hero.*`,
  `cta.*`), so the plan and the final source line up.
- List measurable acceptance criteria (e.g. "title contrast ≥ Lc 60 (APCA)", "product safe area kept
  clear"). You will check these with `zenith validate` and by inspecting the render.

## 2. Scratch experiments (don't pollute the final)

Tag experiment pages with the page **`workspace-role`** metadata field (free-form, e.g.
`workspace-role="scratch"`) plus a clear naming convention (`zenith schema page` lists the fields):

- Final pages: `page.<name>` (e.g. `page.hero`).
- Experiments: `page.scratch.<topic>.<NN>` (e.g. `page.scratch.bg.01`), tagged `workspace-role="scratch"`.
- Keep experiments clearly tagged, render them, and delete the losers before final export. Nothing
  in a scratch page should reach the deliverable unless you deliberately promote it (step 5).

## 3. Generate multiple candidates from one plan

Explore directions instead of committing to the first idea:

- Create several candidate pages (`page.scratch.bg.01/02/03`), each a different take on the
  _same_ plan and palette tokens.
- Track each candidate's lifecycle on the page itself: **`candidate-status`** (`"draft"` →
  `"selected"`/`"rejected"`; other values warn via `page.invalid_candidate_status`), point the
  winner at its destination with **`promotion-target`** (the final page id/label), and record the
  variant intent, seed, or cleanup intent in **`notes`** / **`cleanup-policy`** so the choice is
  replayable. (`zenith schema page` lists these fields.)
- Keep all candidates referencing the **same tokens** so a later palette change is one edit.

## 4. Render-preview and self-critique

Inspect output before trusting it:

```bash
zenith validate doc.zen --json                 # hard diagnostics must be empty
zenith render doc.zen --all-pages preview/      # contact sheet: one PNG per page
```

- Validation already catches many issues: text fit/overflow, contrast, off-canvas nodes,
  missing assets, token problems. Treat every **Error** as blocking.
- Then _look at_ the PNGs: is the headline legible over the motif? Is the product safe area
  clear? Is the texture too noisy? Revise the offending nodes (by id) and re-render.
- Do not finalize while hard diagnostics remain.
- For an audit trail, record each preview in the document-level `previews` block — a `preview`
  entry per candidate captures its `candidate` page id, the `source-hash` it was rendered from, the
  `output` path + `output-hash`, and the `parent-revision`. It is pure provenance (never affects
  render); `zenith inspect` surfaces it.

## 5. Promote the chosen candidate into the final page

Mark the winner with `candidate-status="selected"` (and `promotion-target` = the final page id),
then use the **`promote_candidate`** tx op — it deep-copies the selected candidate page's content
into the target export page, appending an `id_suffix` so ids stay unique:

```bash
# promote.json: {"ops":[{"op":"promote_candidate","source_page":"page.scratch.hero.02","target_page":"page.hero","id_suffix":".final"}]}
zenith tx doc.zen promote.json --apply
```

- The source must have `candidate-status="selected"`; the target page's content is replaced.
- See `zenith schema op promote_candidate` for the exact fields.
- `validate` and `render` again after promotion.

## 6. Finalize and clean up

Use the **`finalize_run`** tx op: for each page in the run whose `candidate-status="rejected"`, it
applies that page's `cleanup-policy` — `"delete"` removes the page; `"archive"` (or absent policy)
sets its `workspace-role` to `"archived"`:

```bash
# finalize.json: {"ops":[{"op":"finalize_run","run_pages":["page.scratch.hero.01","page.scratch.hero.02"]}]}
zenith tx doc.zen finalize.json --apply
```

- See `zenith schema op finalize_run` for the fields.
- Then check `zenith tokens <file>` / validation for now-unused-token advisories.
- Final source must `validate` with no hard diagnostics and `render` cleanly.

## 7. Durable history and undo

Zenith has real local history — use it instead of ad-hoc backups:

```bash
zenith history doc.zen          # list versions
zenith version doc.zen "v1-pre-promote"   # name a checkpoint
zenith undo doc.zen / zenith redo doc.zen
zenith restore doc.zen <rev>    # restore a past version
zenith sync doc.zen             # capture an external/hand edit into history
```

Name a checkpoint before risky steps (e.g. before promotion) so you can restore precisely.

## 8. Later semantic edits

Because you used stable ids, tokens, and semantic groups, later edits are precise transactions:

- "Reduce the grunge" → `set_opacity` on `bg.grunge`.
- "Stronger neuron glow" → update the shadow/token the glow references.
- "Remove honeycomb near the headline" → delete/clip only the intended nodes in `bg.honeycomb`.

If instead the background were a flattened image or anonymous nodes, none of this is possible —
which is why steps 1–3 insist on ids, tokens, and groups.

## Known gaps (do not pretend these exist)

The candidate lifecycle is now first-class: page metadata (`workspace-role`, `candidate-status`,
`promotion-target`, `notes`, `cleanup-policy`), the `promote_candidate` / `finalize_run` tx ops
(steps 5–6), and a document-level `agent-runs` provenance block (a structured run log) all ship —
prefer them over manual conventions. Still **not** implemented; do not generate source that assumes
them: brush/stamp definitions and a built-in automated critique report. Use the conventions above
with today's primitives until the engine ships these.
