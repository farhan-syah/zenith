//! Opt-in Knuth–Liang hyphenation and `overflow-wrap="break-word"` character
//! splitting. The embedded en-US `Standard` dictionary is loaded exactly ONCE
//! into a process-wide `OnceLock` (deterministic: a pure function of the embedded
//! patterns, no time/random/IO beyond the embedded blob). A [`HyphenationContext`]
//! bundles the dictionary with the shaping engine and node-level family so the
//! packer can re-shape a `fragment-` head and the remainder of a split word with
//! identical style.

use std::sync::OnceLock;

use hyphenation::{Hyphenator, Language, Load, Standard};

use zenith_core::FontProvider;
use zenith_layout::{RustybuzzEngine, ShapeRequest, TextDirection, TextLayoutEngine};

use super::pack::Line;
use super::shape::{WordSource, WordToken};

/// Process-wide cache for the embedded en-US hyphenation dictionary. Loaded at
/// most once; `None` only if the embedded blob fails to decode (it should not,
/// but we never panic). Subsequent calls reuse the same `Standard`.
static EN_US_HYPHENATOR: OnceLock<Option<Standard>> = OnceLock::new();

/// Return the cached en-US hyphenator, loading it on first use. Deterministic.
pub(in crate::compile) fn en_us_hyphenator() -> Option<&'static Standard> {
    EN_US_HYPHENATOR
        .get_or_init(|| Standard::from_embedded(Language::EnglishUS).ok())
        .as_ref()
}

/// Everything the packer needs to hyphenate + re-shape a word fragment: the
/// dictionary plus the shaping engine, fonts, and node family. The per-word
/// weight/style/size come from each [`WordToken::src`], so a chain or wrapped
/// node hyphenates with that word's exact style.
pub(in crate::compile) struct HyphenationContext<'a> {
    /// The en-US dictionary, or `None` when only break-word is requested (or the
    /// embedded blob failed to load). The hyphenation branch in
    /// [`super::pack::pack_lines_core`] runs only when this is `Some`; the
    /// break-word branch is independent of it, so a node that requests ONLY
    /// `overflow-wrap="break-word"` still gets a context (with `dict: None`).
    pub(in crate::compile) dict: Option<&'static Standard>,
    pub(in crate::compile) engine: &'a RustybuzzEngine,
    pub(in crate::compile) fonts: &'a dyn FontProvider,
    pub(in crate::compile) families: &'a [String],
    /// The hyphen glyph string shaped onto the head fragment.
    pub(in crate::compile) hyphen: &'a str,
    /// Base writing direction for re-shaping fragments (matches the node).
    pub(in crate::compile) direction: TextDirection,
    /// When `true`, the packer may break an unbreakable token that is wider than
    /// the line box at a CHARACTER boundary (`overflow-wrap="break-word"`). When
    /// `false`, the break-word branch never runs (byte-identical to before).
    pub(in crate::compile) break_word: bool,
}

/// A word split at a hyphenation point: the head (`fragment-`, including the
/// hyphen glyph) to place on the current line, and the tail to carry to the next.
pub(in crate::compile) struct HyphenSplit {
    pub(in crate::compile) head: WordToken,
    pub(in crate::compile) tail: WordToken,
}

/// Re-shape `text` into a [`WordToken`] inheriting `donor`'s visual attributes,
/// at `donor`'s weight/style/size. `hyphen_part` tags the result as a
/// hyphenation head/tail (or `None` for a plain reconstruction). Returns `None`
/// on a shaping failure so the caller falls back to NOT splitting. Deterministic.
fn reshape_fragment(
    text: &str,
    donor: &WordToken,
    hyphen_part: Option<(String, bool)>,
    ctx: &HyphenationContext,
) -> Option<WordToken> {
    let req = ShapeRequest {
        text,
        families: ctx.families,
        weight: donor.src.weight,
        style: donor.src.style,
        font_size: donor.src.font_size,
        direction: ctx.direction,
    };
    let result = ctx.engine.shape_with_fallback(&req, ctx.fonts).ok()?;
    let advance: f64 = result.runs.iter().map(|r| r.advance_width as f64).sum();
    Some(WordToken {
        runs: result.runs,
        advance,
        color: donor.color,
        underline: donor.underline,
        strikethrough: donor.strikethrough,
        highlight: donor.highlight,
        code: donor.code,
        link: donor.link.clone(),
        baseline_dy: donor.baseline_dy,
        src: WordSource {
            text: text.to_owned(),
            weight: donor.src.weight,
            style: donor.src.style,
            font_size: donor.src.font_size,
            paragraph: donor.src.paragraph,
            hyphen_part,
        },
    })
}

/// Attempt to hyphenate `word` so its head fragment (`fragment-`, including the
/// hyphen glyph) fits within `avail` pixels. Tries the LAST (longest) embedded
/// break point that fits; returns `None` when the word has no break point whose
/// head fits, leaving the caller to push the whole word to the next line.
///
/// Determinism: the break list is pattern-derived (deterministic), the chosen
/// point is a pure function of `avail`, and both fragments are re-shaped with the
/// same engine. Words containing non-letters (e.g. trailing punctuation) still
/// hyphenate on their letter run because the dictionary only proposes interior
/// letter breaks; the head/tail slices are taken on byte boundaries the
/// dictionary returns, which always fall between characters.
pub(in crate::compile) fn try_hyphenate(
    word: &WordToken,
    avail: f64,
    ctx: &HyphenationContext,
) -> Option<HyphenSplit> {
    // No dictionary (break-word-only context, or the blob failed to load) → no
    // hyphenation; the caller falls back to wrapping the whole word.
    let dict = ctx.dict?;
    let text = word.src.text.as_str();
    // A break shorter than this many bytes on either side is never useful.
    if text.len() < 4 {
        return None;
    }
    let breaks = dict.hyphenate(text).breaks;
    // Walk break points from LAST to FIRST: the longest head that still fits is
    // the most text we can place, minimizing wasted space.
    for &b in breaks.iter().rev() {
        // `b` is a byte offset within `text` strictly inside the word.
        let (Some(head_txt), Some(tail_txt)) = (text.get(..b), text.get(b..)) else {
            continue;
        };
        if head_txt.is_empty() || tail_txt.is_empty() {
            continue;
        }
        let head_with_hyphen = format!("{head_txt}{}", ctx.hyphen);
        let orig = text.to_owned();
        let Some(head) = reshape_fragment(&head_with_hyphen, word, Some((orig.clone(), true)), ctx)
        else {
            continue;
        };
        if head.advance > avail {
            continue;
        }
        let Some(tail) = reshape_fragment(tail_txt, word, Some((orig, false)), ctx) else {
            continue;
        };
        return Some(HyphenSplit { head, tail });
    }
    None
}

/// Split `word` at the LONGEST char-boundary prefix whose re-shaped advance fits
/// within `avail` pixels (`>= 1` char in the head), for `overflow-wrap="break-word"`.
/// Returns `(head, tail)` re-shaped as plain fragments (NO hyphen glyph), or
/// `None` when not even one character fits or a shaping failure occurs — in which
/// case the caller leaves the word whole (it overflows as today).
///
/// Determinism + safety: the candidate split points are the word's own
/// `char_indices` (so every slice falls on a UTF-8 char boundary — no panic, no
/// mojibake on multi-byte chars), walked in order; the chosen point is a pure
/// function of `avail`. O(n) in the word's char count.
pub(in crate::compile) fn try_break_word(
    word: &WordToken,
    avail: f64,
    ctx: &HyphenationContext,
) -> Option<(WordToken, WordToken)> {
    let text = word.src.text.as_str();
    // Track the largest fitting prefix found so far (byte offset of the split and
    // the re-shaped head). Grow from 1 char up, keeping the last that fits.
    let mut best: Option<(usize, WordToken)> = None;
    // Candidate split byte offsets are the START of each char AFTER the first, so
    // the head always has at least one char; the final candidate is `text.len()`
    // but a full-word "split" is useless (empty tail), so it is excluded.
    let mut boundaries: Vec<usize> = text.char_indices().map(|(b, _)| b).skip(1).collect();
    // `char_indices` never yields `text.len()`; an empty `boundaries` means the
    // word is a single char and cannot be split.
    if boundaries.is_empty() {
        return None;
    }
    // Walk boundaries in order; keep the LARGEST prefix whose head fits `avail`.
    // Once a prefix overflows, all longer prefixes also overflow (advance grows
    // monotonically), so we can stop at the first overflow.
    boundaries.push(text.len()); // sentinel guards the very last char block
    for &b in &boundaries {
        let Some(head_txt) = text.get(..b) else {
            continue;
        };
        if head_txt.is_empty() {
            continue;
        }
        let Some(head) = reshape_fragment(head_txt, word, None, ctx) else {
            // Shaping failure on this prefix: stop and use the best-so-far.
            break;
        };
        if head.advance > avail {
            break;
        }
        // The tail must be non-empty for this to be a real split.
        if b >= text.len() {
            break;
        }
        best = Some((b, head));
    }
    let (b, head) = best?;
    let tail_txt = text.get(b..)?;
    let tail = reshape_fragment(tail_txt, word, None, ctx)?;
    Some((head, tail))
}

/// Flatten packed lines back into a single ordered word stream for re-wrapping in
/// the NEXT chain box, MERGING any hyphenation head+tail pair back into the
/// original unsplit word so a fragment is never carried across a box (and never
/// hyphenated twice). When `hyph` is `None` (no hyphenation was performed) the
/// words pass through unchanged. A head whose tail does not immediately follow
/// (it should always, by construction) is passed through as-is rather than lost.
pub(in crate::compile) fn flatten_lines_to_tokens(
    lines: Vec<Line>,
    hyph: Option<&HyphenationContext>,
) -> Vec<WordToken> {
    let mut words: Vec<WordToken> = Vec::new();
    for line in lines {
        for w in line.words {
            words.push(w);
        }
    }
    let Some(ctx) = hyph else {
        return words;
    };
    // Merge adjacent (head, tail) pairs back into the original word.
    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut iter = words.into_iter().peekable();
    while let Some(w) = iter.next() {
        if let Some((orig, true)) = &w.src.hyphen_part {
            let is_tail_next = iter
                .peek()
                .and_then(|n| n.src.hyphen_part.as_ref())
                .is_some_and(|(o, head)| !head && o == orig);
            if is_tail_next {
                let orig = orig.clone();
                let tail = iter.next();
                match reshape_fragment(&orig, &w, None, ctx) {
                    Some(merged) => out.push(merged),
                    None => {
                        // Reshape failed: keep both fragments rather than drop text.
                        out.push(w);
                        if let Some(t) = tail {
                            out.push(t);
                        }
                    }
                }
                continue;
            }
        }
        out.push(w);
    }
    out
}

#[cfg(test)]
mod break_word_tests {
    use super::{HyphenationContext, try_break_word};
    use zenith_core::{FontProvider, FontStyle, default_provider};
    use zenith_layout::{RustybuzzEngine, TextDirection};

    use super::super::ctx::{NodeShape, ShapeEnv};
    use super::super::pack::pack_lines_reporting;
    use super::super::shape::{ResolvedSpan, WordToken, shape_words};
    use crate::ir::Color;

    /// Shape `word` into a single [`WordToken`] using the real engine + bundled
    /// fonts, so `try_break_word` exercises real glyph advances. The `src.text`
    /// carries the original word so the splitter can slice it.
    fn shape_word(word: &str, engine: &RustybuzzEngine, fonts: &dyn FontProvider) -> WordToken {
        let families = vec!["Noto Sans".to_owned()];
        let spans = [ResolvedSpan {
            text: word.to_owned(),
            color: Color::srgb(0, 0, 0, 255),
            underline: false,
            strikethrough: false,
            highlight: None,
            code: false,
            link: None,
            weight: 400,
            style: FontStyle::Normal,
            font_size: 16.0,
            baseline_dy: 0.0,
        }];
        let mut diags = Vec::new();
        let (mut tokens, _m) = shape_words(
            &spans,
            &families,
            NodeShape {
                font_size: 16.0,
                base_weight: 400,
                direction: TextDirection::Ltr,
            },
            ShapeEnv { engine, fonts },
            &mut diags,
            "t",
            None,
        );
        tokens.pop().expect("the word must shape to one token")
    }

    fn ctx<'a>(
        engine: &'a RustybuzzEngine,
        fonts: &'a dyn FontProvider,
        families: &'a [String],
    ) -> HyphenationContext<'a> {
        HyphenationContext {
            dict: None,
            engine,
            fonts,
            families,
            hyphen: "-",
            direction: TextDirection::Ltr,
            break_word: true,
        }
    }

    /// `try_break_word` splits a long token so the head fits `avail`, the head has
    /// at least one char, and head+tail char text reconstructs the original.
    #[test]
    fn splits_and_reconstructs_original_text() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let original = "https://very-long.example.com/some/deep/path";
        let word = shape_word(original, &engine, &provider);
        let c = ctx(&engine, &provider, &families);

        // Pick an `avail` far smaller than the whole word's advance.
        let avail = word.advance / 3.0;
        let (head, tail) = try_break_word(&word, avail, &c).expect("a prefix must fit");

        assert!(head.advance <= avail, "head must fit avail");
        assert!(
            !head.src.text.is_empty() && head.src.text.chars().count() >= 1,
            "head needs at least one char"
        );
        assert!(!tail.src.text.is_empty(), "tail must be non-empty");
        assert_eq!(
            format!("{}{}", head.src.text, tail.src.text),
            original,
            "head+tail must reconstruct the original token exactly"
        );
    }

    /// A token containing multi-byte UTF-8 chars (an em-dash and an accented
    /// char) is split only on char boundaries — no panic, no lost/mojibake bytes.
    #[test]
    fn respects_multibyte_char_boundaries() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let original = "café—über—straße—long—compound—word";
        let word = shape_word(original, &engine, &provider);
        let c = ctx(&engine, &provider, &families);

        let avail = word.advance / 2.0;
        let (head, tail) = try_break_word(&word, avail, &c).expect("a prefix must fit");
        // Reconstruction proves no byte was lost or duplicated and that both
        // slices are valid UTF-8 (otherwise `src.text` would not equal a slice).
        assert_eq!(format!("{}{}", head.src.text, tail.src.text), original);
        // The split point is a valid char boundary of the original string.
        assert!(
            original.is_char_boundary(head.src.text.len()),
            "split must land on a char boundary"
        );
    }

    /// A box too narrow for even one character yields `None`, leaving the caller
    /// to keep the word whole (it overflows as today).
    #[test]
    fn returns_none_when_no_char_fits() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let word = shape_word("wide", &engine, &provider);
        let c = ctx(&engine, &provider, &families);
        assert!(
            try_break_word(&word, 0.0, &c).is_none(),
            "zero avail fits no char → None"
        );
    }

    /// A single-character token cannot be split (no useful interior boundary).
    #[test]
    fn single_char_token_is_not_split() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let word = shape_word("W", &engine, &provider);
        let c = ctx(&engine, &provider, &families);
        assert!(try_break_word(&word, 1000.0, &c).is_none());
    }

    /// Regression: under `break-word`, an ORDINARY word that fits a line by
    /// itself but not the REMAINING space on a non-empty line must wrap WHOLE to
    /// the next line — it must NOT be split mid-word into the leftover space.
    /// Only a token wider than the whole box may break (covered elsewhere).
    #[test]
    fn ordinary_word_wraps_whole_not_broken_into_remaining_space() {
        let engine = RustybuzzEngine::new();
        let provider = default_provider();
        let families = vec!["Noto Sans".to_owned()];
        let c = ctx(&engine, &provider, &families);

        let alpha = shape_word("alpha", &engine, &provider);
        let betagamma = shape_word("betagamma", &engine, &provider);
        let space_advance = 6.0;
        // Box fits "betagamma" alone (with slack) but NOT "alpha" + space +
        // "betagamma": the second word overflows the remainder yet fits a fresh
        // line, so it must wrap whole.
        let box_w = betagamma.advance + 5.0;
        assert!(
            alpha.advance + space_advance + betagamma.advance > box_w,
            "test setup: the pair must overflow one line"
        );

        let mut forced = false;
        let lines = pack_lines_reporting(
            vec![alpha, betagamma],
            box_w,
            space_advance,
            Some(&c),
            &mut forced,
        );

        assert!(!forced, "no forced break: the word fits a line by itself");
        assert_eq!(lines.len(), 2, "the second word wraps to its own line");
        assert_eq!(
            lines[1]
                .words
                .iter()
                .map(|w| w.src.text.as_str())
                .collect::<String>(),
            "betagamma",
            "the wrapped word stays intact (not split mid-word)"
        );
    }
}
