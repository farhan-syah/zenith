# Agentic visual workflow

Take a vague brief to a finished, auditable design without polluting the final file.

The `.zen` holds ONLY final content. Process state — scratch candidates, their lifecycle, history
— lives in the app-managed store keyed by the document's `doc-id`, reachable through the
`zenith workspace` commands. You never hand-edit it.

> Op fields: `zenith schema op <name>`, `zenith tx --help`, `examples/*.tx.json`.
> Attributes: `zenith schema node <kind>` / `zenith schema page`.
> Store commands: `zenith workspace --help`.

## 1. Plan for addressability

There is no in-file brief block; keep the plan in the work itself.

- Give every node a stable `id` so each edit is a precise transaction.
- Give each layer group a `semantic-role` (+ optional `layer-priority`/`intensity`) so layers stay
  addressable: `group id="bg.grunge" semantic-role="background" layer-priority=0`.
- Write down acceptance criteria (e.g. "title contrast ≥ Lc 60 APCA") and check them with
  `zenith validate` and the render — these are your gate, not decoration.

## 2. Generate candidates from one plan

Edit the document to a take, then snapshot it into the store as a scratch candidate. Each
candidate is a content-addressed `.zen` snapshot — it never lives in the deliverable.

```bash
zenith workspace scratch new doc.zen --page page.hero --status draft \
  --notes "take A: dark, product-forward" --workspace-role scratch --cleanup-policy delete
```

- `--page <id>` is the page this candidate captures (default `*` = whole document).
- Repeat for each take. `--promotion-target` records where it is meant to land.
- Keep all takes on the same tokens so a palette change is one edit.

## 3. Render-preview and self-critique

```bash
zenith validate doc.zen --json              # hard diagnostics must be empty
zenith render doc.zen --all-pages preview/  # one PNG per page
```

- Treat every Error as blocking.
- Look at the PNGs: headline legible over the motif? product safe area clear? texture too noisy?
  Revise nodes by id, re-snapshot.

## 4. Review and set lifecycle

```bash
zenith workspace scratch list doc.zen            # enumerate candidates (cand0, cand1, …)
zenith workspace scratch show doc.zen cand0      # detail for one (add --json)
zenith workspace candidate doc.zen cand0 selected   # or: rejected
```

Lifecycle is `draft → selected | rejected`. Only a `selected` candidate can be promoted.

## 5. Promote the chosen candidate

```bash
zenith workspace promote doc.zen cand0 --into page.export
# keep cloned ids unique with a custom suffix:
zenith workspace promote doc.zen cand0 --into page.export --id-suffix .v2
```

Fetches the candidate's stored snapshot, deep-copies its source page into the target page
(suffixing all ids, default `.promoted`), validates, writes the document back in place, and records
the promote in version history. Then `validate` + `render` the deliverable.

## 6. Finalize and clean up

```bash
zenith workspace finalize doc.zen            # add --json for a machine-readable report
```

Removes candidates with `status = rejected` and `cleanup-policy = delete` from the scratch index
(snapshot objects are left for a future GC pass); all other candidates are preserved. Then check
`zenith tokens doc.zen` for unused-token advisories; final source must validate + render clean.

## 7. History and portability

```bash
zenith history doc.zen                      # list versions
zenith version doc.zen "v1-pre-promote"     # name a checkpoint
zenith undo doc.zen  /  zenith redo doc.zen
zenith restore doc.zen <rev>                # <rev> grammar: zenith restore --help
zenith sync doc.zen                         # capture an external/hand edit
zenith workspace bundle doc.zen --out doc.zenithbundle   # pack the whole store (history + scratch)
zenith workspace unbundle doc.zenithbundle               # restore it on another machine/clone
```

Name a checkpoint before risky steps (e.g. promotion). The `doc-id` in the `.zen` is the key that
reattaches a bundle to its file.

## 8. Later semantic edits

Stable ids + tokens + `semantic-role` groups make edits precise transactions:

- "Reduce the grunge" → `set_opacity` on `bg.grunge`.
- "Stronger glow" → update the shadow token it references.
- "Remove honeycomb near the headline" → delete/clip nodes in `bg.honeycomb`.

## Not implemented (don't assume these)

Brush/stamp definitions; an automated critique report (self-critique by reading the render, step 3);
recording an agent-run/preview log via the CLI (the store has the schema, but no command writes to
it yet).
