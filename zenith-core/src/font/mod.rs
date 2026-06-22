//! Font sourcing layer: provider trait, data types, and the bundled default.

pub mod embedded;
mod provider;

pub use provider::{BytesFontProvider, FontData, FontProvider, FontStyle, default_provider};
