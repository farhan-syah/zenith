# Contributing to Zenith

Thanks for your interest in Zenith. This guide covers contributing to the **engine** (this repository). For sharing finished designs, see the separate [zenith-showcase](https://github.com/zenitheditor/zenith-showcase) repository.

Zenith is pre-`0.1`. The format and APIs are still moving, so opening an issue to discuss a change before you write it is usually the fastest path.

> **Before writing any code, read [AGENTS.md](AGENTS.md).** It is the source of truth for the repository's conventions and hard rules (determinism, C-free dependencies, no `unwrap`/`panic` in library code, `BTreeMap`-only, exhaustive enum matches, lean `mod.rs`, files under ~1000 lines, and more). Changes that violate those invariants will be sent back.

## Ways to Contribute

- **Engine code** — parser, validation/diagnostics, scene compilation, text layout, the transaction ops, the PNG/PDF renderers, history, or the CLI.
- **Diagnostics** — new validation rules that catch real authoring mistakes (a fitting, high-value contribution; see "Adding a diagnostic" below).
- **Examples** — small, focused `.zen` files in `examples/` that demonstrate a feature. Keep them valid against the current parser.
- **Libraries** — your own reusable packs (`libraries/*.zen`), or a new built-in preset shipped with the engine (see "Libraries" below).
- **Conformance** — scenario coverage rendered into `conformance/<area>/`.
- **Bug reports & feature requests** — open an issue with a minimal `.zen` reproduction and the exact `zenith` command and output.
- **Designs** — reusable `.zen` source, recipes, and rendered output belong in [zenith-showcase](https://github.com/zenitheditor/zenith-showcase), not here.

## Getting Started

```bash
git clone --recurse-submodules https://github.com/zenitheditor/zenith
cd zenith
cargo build --release
cargo test --workspace
```

No C toolchain or system libraries are needed — the dependency graph is C-free.

The build output is at `target/release/zenith`; run it directly, or install it onto your `PATH`:

```bash
cargo install --path zenith-cli   # installs `zenith` to ~/.cargo/bin (already on PATH)
./scripts/install.sh --local      # builds from this checkout and installs to ~/.local/bin
```

## The Development Loop

Every change must leave the tree green. Before you commit:

```bash
cargo build                                            # compiles clean
cargo test --workspace                                 # all tests pass
cargo clippy --workspace --all-targets -- -D warnings  # no lints (warnings are errors)
cargo fmt --all                                        # canonical formatting applied
```

Land work in small, focused, bisect-safe commits — one coherent unit each, each one green on its own. Fix problems at the source as you find them rather than deferring them.

## Where Things Go

| You're changing…                       | Crate            |
| -------------------------------------- | ---------------- |
| `.zen` syntax, AST, tokens, validation | `zenith-core`    |
| Text shaping / fonts                   | `zenith-layout`  |
| Scene compilation / geometry           | `zenith-scene`   |
| PNG or PDF output                      | `zenith-render`  |
| Edit operations (`tx`)                 | `zenith-tx`      |
| Local history / versions               | `zenith-session` |
| The `zenith` command                   | `zenith-cli`     |

See [AGENTS.md](AGENTS.md) for the full module map and the testing layout (unit tests in-file, integration tests in each crate's `tests/`).

## Libraries

A **library pack** is a reusable bundle of components and tokens (flowchart shapes, filters,
masks, a brand kit) that `zenith library add` materializes into a document. A pack is itself a
`.zen` file:

- it declares its own identity in a `libraries` self-entry — `library id="@scope/name" version="…"`;
- it exposes items as `components` (and reusable tokens such as filters/masks);
- it carries the `tokens` / `styles` / `assets` those items reference, so a materialized item is
  self-contained;
- it includes a stub `document { page … {} }` (the parser requires one; the resolver ignores it).

`zenith-cli/assets/libraries/zenith-flowchart.zen` is the canonical worked example.

Resolution is deterministic: **project packs first, then embedded presets**, sorted by id. A
project pack _shadows_ an embedded preset of the same id. `library add` copies the chosen item
plus its dependencies (dedup by id, with a `library.dependency_conflict` warning on a clash) and
records `libraries` + `provenance` entries in the target.

### Create your own library (no engine change)

Drop a pack next to your document under a `libraries/` directory and use it immediately:

```text
my-project/
  libraries/
    my-pack.zen          # libraries { library id="@me/pack" version="0.1.0" } + components/tokens
  poster.zen
```

```bash
zenith library list my-project/                       # see @me/pack alongside the presets
zenith library add @me/pack#logo --into poster.zen --page p1 --at 40,40
```

The `path` for `library list` is a project directory, or a `.zen` file whose parent is the
project directory. No rebuild is needed — project packs are read from disk.

### Contribute a built-in preset (engine change)

Built-in presets are embedded in the binary. To add one:

1. Add `zenith-cli/assets/libraries/<name>.zen` (the pack), following the format above and the
   existing presets. Use a `@zenith/<name>` id.
2. Register it in `EMBEDDED_PACKS` in `zenith-cli/src/library/registry.rs` (an `include_str!` of
   your new file), keeping the list ordered.
3. Add a test that `resolve_packs(None)` exposes the new pack and that `library add` materializes
   one of its items cleanly (no diagnostics, ids namespaced).

Keep preset assets redistributable and the dependency graph C-free; bundled fonts/images must
carry a license that permits redistribution (see `zenith-core/assets/fonts/LICENSE.txt` for the Noto example).

## Adding a Node Kind, Op, or Diagnostic

- **Diagnostics** use stable dot-separated codes, `<namespace>.<snake_event>` (e.g. `token.cyclic_reference`, `font.glyph_missing`), at `Error` / `Warning` / `Advisory` severity. Emit them deterministically (dedupe, sorted), tie them to the offending node id, and cover them with a test.
- **Node kinds and transaction ops** are added to the relevant enum. Because matches over our enums are exhaustive (no `_` wildcard), the compiler will point you at every site that needs updating — handle them all rather than suppressing the error.
- **Keep additive changes byte-identical when the feature is absent.** A document that doesn't use your new property must render exactly as it did before. Add a regression test that proves it.

## Examples & Conformance

- Examples in `examples/` must parse and render against the current engine. If you change syntax, update the affected examples in the same change.
- Render conformance proof into `conformance/<area>/` (it is gitignored and regenerable — do not commit it).

## Commit & PR Conventions

- Conventional Commits with a crate scope: `feat(zenith-scene): …`, `fix(zenith-core): …`,`test(zenith-cli): …`. List every touched crate in the scope.
- No co-author trailers or tool-attribution lines.
- A PR should describe the change, list the verification commands you ran (build / test / clippy /fmt), and link any related issue. Include rendered output or diagnostic changes when relevant.
- Never stage `conformance/`, `resource/`, `zenith-showcase/`, or local-scratch directories.

## License

By contributing, you agree that your contributions are licensed under the repository's [Apache-2.0 License](LICENSE).
