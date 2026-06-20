//! Backend-neutral scene IR and scene compilation for Zenith.
//!
//! Owns the display-list primitives (`FillRect`, `DrawGlyphRun`, `PushClip`,
//! etc.), scene compilation from a validated AST into a z-ordered display list,
//! opacity/visibility/clip resolution, and exclusion of non-printing nodes.
//!
//! This crate is the stable contract boundary that every future backend —
//! GPU, PDF, SVG export — will consume.
//!
//! # Module layout
//!
//! - `ir`      — scene IR types (`Scene`, `SceneCommand`, `Color`, `SceneGlyph`).
//! - `color`   — sRGB hex parsing → `Color`.
//! - `compile` — `compile(&Document, &dyn FontProvider) -> CompileResult`.
//!
//! # Quick start
//!
//! ```rust
//! use zenith_scene::{Scene, SceneCommand, Color};
//!
//! let mut scene = Scene::new(640.0, 360.0);
//! scene.commands.push(SceneCommand::FillRect {
//!     x: 0.0, y: 0.0, w: 640.0, h: 360.0,
//!     color: Color::srgb(248, 250, 252, 255),
//! });
//! let json = scene.to_json().expect("serializes");
//! assert!(json.contains("zenith-scene-v1"));
//! ```
//!
//! Compile a parsed document into a scene with
//! [`compile`](crate::compile::compile).

pub mod color;
pub mod compile;
pub mod ir;

// Curated flat re-exports.
pub use compile::{CompileResult, compile, compile_page};
pub use ir::{
    Color, FitMode, GradientPaint, GradientStop, ImageClip, Scene, SceneCommand, SceneGlyph,
    ShadowSpec,
};
