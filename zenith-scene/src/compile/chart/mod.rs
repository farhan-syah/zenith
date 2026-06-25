//! `chart` node compilation — axis frame, scale, plot area, bar/line/area/sparkline.
//!
//! Wiring only: submodule declarations and the single public re-export.
//! No business logic lives here (AGENTS.md: module-root files are wiring only).

mod axis;
mod bar;
mod entry;
mod frame;
mod legend;
mod line;
mod palette;
mod pie;
mod scale;

pub(in crate::compile) use entry::compile_chart;
