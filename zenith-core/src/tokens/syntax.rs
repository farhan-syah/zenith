//! Built-in syntax-highlight palette: fallback colors keyed by `syntax.*` token ids.
//! Doc-declared `syntax.*` tokens override per-kind at compile time.

pub use oxidoc_highlight::token::TokenKind;

/// A built-in color theme for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyntaxTheme {
    #[default]
    Dark,
    Light,
}

impl SyntaxTheme {
    /// Parse a theme name case-insensitively. Returns `None` for unknown names.
    pub fn from_name(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else if s.eq_ignore_ascii_case("light") {
            Some(Self::Light)
        } else {
            None
        }
    }

    /// The canonical lowercase name, for formatting.
    pub fn as_str(self) -> &'static str {
        match self {
            SyntaxTheme::Dark => "dark",
            SyntaxTheme::Light => "light",
        }
    }
}

/// Returns the dotted token id for a given `TokenKind`.
///
/// The match is exhaustive: adding a new upstream variant becomes a compile error.
pub fn token_id_for_kind(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Keyword => "syntax.keyword",
        TokenKind::String => "syntax.string",
        TokenKind::Comment => "syntax.comment",
        TokenKind::Number => "syntax.number",
        TokenKind::Function => "syntax.function",
        TokenKind::Type => "syntax.type",
        TokenKind::Operator => "syntax.operator",
        TokenKind::Punctuation => "syntax.punctuation",
        TokenKind::Property => "syntax.property",
        TokenKind::Builtin => "syntax.builtin",
        TokenKind::Attr => "syntax.attr",
        TokenKind::Variable => "syntax.variable",
        TokenKind::Plain => "syntax.plain",
    }
}

/// Returns the built-in `#rrggbb` fallback color for a theme/kind pair.
///
/// Both arms are exhaustive over all 13 kinds (no wildcard).
pub fn builtin_color(theme: SyntaxTheme, kind: TokenKind) -> &'static str {
    match theme {
        SyntaxTheme::Dark => match kind {
            TokenKind::Keyword => "#c792ea",
            TokenKind::String => "#c3e88d",
            TokenKind::Comment => "#546e7a",
            TokenKind::Number => "#f78c6c",
            TokenKind::Function => "#82aaff",
            TokenKind::Type => "#ffcb6b",
            TokenKind::Operator => "#89ddff",
            TokenKind::Punctuation => "#89ddff",
            TokenKind::Property => "#f07178",
            TokenKind::Builtin => "#c792ea",
            TokenKind::Attr => "#ffcb6b",
            TokenKind::Variable => "#eeffff",
            TokenKind::Plain => "#eeffff",
        },
        SyntaxTheme::Light => match kind {
            TokenKind::Keyword => "#cf222e",
            TokenKind::String => "#0a3069",
            TokenKind::Comment => "#6e7781",
            TokenKind::Number => "#0550ae",
            TokenKind::Function => "#8250df",
            TokenKind::Type => "#953800",
            TokenKind::Operator => "#cf222e",
            TokenKind::Punctuation => "#1f2328",
            TokenKind::Property => "#0550ae",
            TokenKind::Builtin => "#8250df",
            TokenKind::Attr => "#116329",
            TokenKind::Variable => "#1f2328",
            TokenKind::Plain => "#1f2328",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [TokenKind; 13] = [
        TokenKind::Keyword,
        TokenKind::String,
        TokenKind::Comment,
        TokenKind::Number,
        TokenKind::Function,
        TokenKind::Type,
        TokenKind::Operator,
        TokenKind::Punctuation,
        TokenKind::Property,
        TokenKind::Builtin,
        TokenKind::Attr,
        TokenKind::Variable,
        TokenKind::Plain,
    ];

    fn is_hex_color(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn builtin_color_all_kinds_both_themes_are_valid_hex() {
        for kind in ALL {
            let dark = builtin_color(SyntaxTheme::Dark, kind);
            assert!(
                is_hex_color(dark),
                "Dark {:?} -> {:?} is not valid #rrggbb",
                kind,
                dark
            );
            let light = builtin_color(SyntaxTheme::Light, kind);
            assert!(
                is_hex_color(light),
                "Light {:?} -> {:?} is not valid #rrggbb",
                kind,
                light
            );
        }
    }

    #[test]
    fn token_id_for_kind_all_kinds_are_valid() {
        for kind in ALL {
            let id = token_id_for_kind(kind);
            assert!(
                id.starts_with("syntax."),
                "id {:?} does not start with 'syntax.'",
                id
            );
            assert!(
                !id.chars().any(|c| c.is_whitespace() || c.is_uppercase()),
                "id {:?} contains whitespace or uppercase",
                id
            );
        }
    }

    #[test]
    fn syntax_theme_default_is_dark() {
        assert_eq!(SyntaxTheme::default(), SyntaxTheme::Dark);
    }

    #[test]
    fn from_name_parses_known_and_unknown() {
        assert_eq!(SyntaxTheme::from_name("dark"), Some(SyntaxTheme::Dark));
        assert_eq!(SyntaxTheme::from_name("DARK"), Some(SyntaxTheme::Dark));
        assert_eq!(SyntaxTheme::from_name("light"), Some(SyntaxTheme::Light));
        assert_eq!(SyntaxTheme::from_name("nope"), None);
    }

    #[test]
    fn as_str_round_trips_for_both_variants() {
        for t in [SyntaxTheme::Dark, SyntaxTheme::Light] {
            assert_eq!(
                SyntaxTheme::from_name(t.as_str()),
                Some(t),
                "as_str/from_name round-trip failed for {:?}",
                t
            );
        }
    }

    #[test]
    fn dark_and_light_differ_for_keyword() {
        let dark = builtin_color(SyntaxTheme::Dark, TokenKind::Keyword);
        let light = builtin_color(SyntaxTheme::Light, TokenKind::Keyword);
        assert_ne!(dark, light, "Dark and Light keyword colors must differ");
    }
}
