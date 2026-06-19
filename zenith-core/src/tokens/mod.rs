//! Token resolution for `zenith-token-v1`.
//!
//! This module owns the public resolution API. All logic lives in
//! [`resolve`]; this file is declarations and re-exports only.

mod resolve;
mod syntax;

pub use oxidoc_highlight::token::Token as HighlightToken;
pub use oxidoc_highlight::{is_supported, scan};
pub use resolve::{ResolvedToken, ResolvedValue, TokenResolution, resolve_tokens};
pub use syntax::{SyntaxTheme, TokenKind, builtin_color, token_id_for_kind};
