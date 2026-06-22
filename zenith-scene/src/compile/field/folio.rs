//! Folio formatting: turn a 1-based page/section index into its display string
//! according to the requested numbering style (decimal or Roman).

/// Format a folio number according to the requested style.
///
/// `"lower-roman"` → standard subtractive lower-case Roman numerals.
/// `"upper-roman"` → same, upper-cased.
/// `"decimal"`, `None`, or any unrecognised value → decimal string.
pub(in crate::compile) fn format_folio(n: usize, style: Option<&str>) -> String {
    match style {
        Some("lower-roman") => to_roman(n, false),
        Some("upper-roman") => to_roman(n, true),
        // "decimal", None, or any unknown style → decimal
        _ => n.to_string(),
    }
}

/// Convert a positive integer to a Roman numeral string.
///
/// Uses the standard subtractive-pairs table (i, iv, v, ix, x, xl, l, xc,
/// c, cd, d, cm, m). `upper` controls whether the result is upper- or
/// lower-case. For `n == 0` (not a valid folio but defensive), returns `"0"`.
fn to_roman(n: usize, upper: bool) -> String {
    if n == 0 {
        return "0".to_owned();
    }
    const PAIRS: &[(usize, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut result = String::new();
    let mut remaining = n;
    for &(value, symbol) in PAIRS {
        while remaining >= value {
            result.push_str(symbol);
            remaining -= value;
        }
    }
    if upper { result.to_uppercase() } else { result }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_roman ───────────────────────────────────────────────────────────────

    #[test]
    fn to_roman_table() {
        let cases: &[(usize, &str)] = &[
            (1, "i"),
            (3, "iii"),
            (4, "iv"),
            (9, "ix"),
            (14, "xiv"),
            (40, "xl"),
            (49, "xlix"),
            (90, "xc"),
            (2024, "mmxxiv"),
        ];
        for &(n, expected) in cases {
            assert_eq!(
                to_roman(n, false),
                expected,
                "to_roman({n}, false) should be {expected:?}"
            );
        }
    }

    #[test]
    fn to_roman_upper_case() {
        assert_eq!(
            to_roman(4, true),
            "IV",
            "upper=true must upper-case the result"
        );
    }

    #[test]
    fn to_roman_zero_returns_decimal_zero() {
        assert_eq!(
            to_roman(0, false),
            "0",
            "n=0 must return \"0\" (no Roman zero)"
        );
    }

    // ── format_folio ───────────────────────────────────────────────────────────

    #[test]
    fn format_folio_decimal_default() {
        assert_eq!(format_folio(5, None), "5");
        assert_eq!(format_folio(5, Some("decimal")), "5");
    }

    #[test]
    fn format_folio_lower_roman() {
        assert_eq!(format_folio(3, Some("lower-roman")), "iii");
    }

    #[test]
    fn format_folio_upper_roman() {
        assert_eq!(format_folio(4, Some("upper-roman")), "IV");
    }

    #[test]
    fn format_folio_unknown_style_falls_back_to_decimal() {
        assert_eq!(format_folio(7, Some("klingon")), "7");
    }
}
