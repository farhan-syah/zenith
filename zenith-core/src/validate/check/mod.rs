//! Document-level semantic validation checks.
//!
//! This module is split into cohesive submodules; `validate/mod.rs` re-exports
//! only the public surface (`validate`, `ValidationReport`).
//!
//! Checks performed (in one document walk):
//!
//! 1. **Global ID uniqueness** — every id across tokens, styles, body, pages,
//!    and nodes must be unique. Duplicates → `id.duplicate` (Error).
//! 2. **Required geometry** — `page` requires non-`Unit::Unknown` `width`/
//!    `height`; `rect`/`text` require all four of `x`, `y`, `w`, `h` present
//!    and with known units. Missing → `node.missing_geometry` (Error);
//!    unknown unit → `node.invalid_geometry` (Error).
//! 3. **Token-reference integrity + type compatibility** — visual `TokenRef`
//!    properties that point at an unknown or wrong-type token →
//!    `token.unknown_reference` / `token.incompatible_property` (Error).
//! 4. **Raw visual literal** — a recognized visual property (fill, stroke,
//!    stroke-width, font-family, font-size, radius) whose value is a
//!    `Literal(...)` → `token.raw_visual_literal` (Error).
//! 5. **Unknown node kind** → `node.unknown_kind` (Warning).
//!    **Unknown property** → `node.unknown_property` (Warning).
//! 6. **Unused token** — a token defined but never referenced by any node
//!    visual property or style → `token.unused` (Advisory).
//!
//! Submodules:
//! - [`visual`] — visual-property token type/existence/raw-literal checks.
//! - [`nodes`] — the recursive node walk and geometry helpers.
//! - [`contrast`] — the WCAG 2.2 contrast advisory.
//! - [`safezone`] — safe-zone exclusion/required overlap advisories.
//! - [`fold`] — fold-line content-crossing advisories.
//! - [`margin`] — book live-area (mirrored-margin) violation advisories.
//! - [`agent_runs`] — `agent-runs` block checks (duplicate run/step ids, empty
//!   actions, unresolved parent-step references, unknown affected-node ids).
//! - [`driver`] — the `validate` entry point and its document walk.
//! - [`passes`] — the orchestration helpers the driver calls (id collection,
//!   footnote-ref resolution, per-declaration and styles-block checks).
//! - [`report`] — the [`ValidationReport`] outcome type.

mod agent_runs;
mod contrast;
mod driver;
mod fold;
mod margin;
mod nodes;
mod passes;
mod recipes;
mod report;
mod safezone;
mod variants;
mod visual;

// ── Public surface ────────────────────────────────────────────────────────────
// `validate` and `ValidationReport` are the crate's public validate API,
// re-exported up through `validate/mod.rs` → `lib.rs`. `register_id` lives in
// `passes` but is called by the node submodules via
// `crate::validate::check::register_id`, so it is re-exported here to keep that
// path resolving.
pub use driver::validate;
pub(in crate::validate::check) use passes::register_id;
pub use report::ValidationReport;
