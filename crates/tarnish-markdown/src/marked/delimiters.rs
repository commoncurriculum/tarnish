//! marked's rules for where emphasis and strikethrough close, `emStrongRDelimAst`,
//! `emStrongRDelimUnd` and `delRDelim`, matched directly instead of through the regex engine.
//! The lexer runs one from every opening delimiter to the next that closes it, so on text full
//! of delimiters it runs over the rest of the text again and again. Each gives what its regex in
//! `rules.rs` gives, which the tests below check against that regex on random input.
//!
//! Past what a rule skips, a match is a character, then a run of the delimiter, and the rule
//! tells what the run can do by what kind of character is on each side of it.

use std::ops::{Range, RangeInclusive};
use std::sync::LazyLock;

use super::matchers::run;
use tarnish_js as js;
use tarnish_js::utf16::{find, unit};

/// A match of one of the rules.
#[derive(Debug, PartialEq)]
pub struct Found {
    /// `match.index`.
    pub index: usize,
    /// `lastIndex` after the match.
    pub end: usize,
    /// The run of delimiters the match ends with, and what it can do; `None` where the rule
    /// matches text it skips on its way to one.
    pub run: Option<(Range<usize>, Flank)>,
}

/// What a run of delimiters can do, by the characters around it: the rules' groups 1 and 2,
/// 3 and 4, or 5 and 6.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flank {
    Close,
    Open,
    Either,
}

/// `emStrongRDelimAst.exec(src)` for `*`, or `emStrongRDelimUnd.exec(src)` for `_`, from
/// `lastIndex`.
pub fn em_strong(src: &[u16], last_index: usize, delimiter: u16) -> Option<Found> {
    let asterisk = delimiter == unit(b'*');
    // The asterisk's rule takes `~` as a letter, and the underscore's as the symbol it is.
    let kind_at = |at| match code_point_at(src, at) {
        Some(tilde) if asterisk && tilde == u32::from(b'~') => Kind::Other,
        code_point => kind(code_point),
    };
    let mut at = last_index;
    if at == 0
        && let Some(end) = orphan(src, delimiter)
    {
        return Some(Found {
            index: 0,
            end,
            run: None,
        });
    }
    while at < src.len() {
        if src[at] == delimiter {
            at += run(src, at, |each| each == delimiter);
            continue;
        }
        // `[^*]+(?=[^*])`: the text up to the last character before the next delimiter.
        let text_end = at + run(src, at, |each| each != delimiter);
        let last = last_code_point(src, at, text_end);
        if last > at {
            return Some(Found {
                index: at,
                end: last,
                run: None,
            });
        }
        if text_end == src.len() {
            return None;
        }
        let run_end = text_end + run(src, text_end, |each| each == delimiter);
        if let Some(flank) = flank(kind_at(at), kind_at(run_end), asterisk) {
            return Some(Found {
                index: at,
                end: run_end,
                run: Some((text_end..run_end, flank)),
            });
        }
        at = run_end;
    }
    None
}

/// `delRDelim.exec(src)` from `lastIndex`.
pub fn del(src: &[u16], last_index: usize) -> Option<Found> {
    let tilde = unit(b'~');
    if last_index > src.len() {
        return None;
    }
    if last_index == 0 {
        // `^[^~]+(?=[^~])`, which only the start tries.
        let last = last_code_point(src, 0, run(src, 0, |each| each != tilde));
        if last > 0 {
            return Some(Found {
                index: 0,
                end: last,
                run: None,
            });
        }
    }
    let mut at = last_index;
    loop {
        let run_start = at + find(&src[at..], tilde)?;
        let run_end = run_start + run(src, run_start, |each| each == tilde);
        // `~~?` takes at most two, and none of the rules lets a third follow.
        if run_start > at && run_end - run_start <= 2 {
            let before = last_code_point(src, at, run_start);
            let kinds = (
                kind(code_point_at(src, before)),
                kind(code_point_at(src, run_end)),
            );
            if let Some(flank) = flank(kinds.0, kinds.1, true) {
                return Some(Found {
                    index: before,
                    end: run_end,
                    run: Some((run_start..run_end, flank)),
                });
            }
        }
        at = run_end;
    }
}

/// `^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)` for `*`, and the same with the two swapped for `_`: a
/// delimiter alone between two pairs of the other, which the rule skips.
fn orphan(src: &[u16], delimiter: u16) -> Option<usize> {
    let other = if delimiter == unit(b'*') {
        unit(b'_')
    } else {
        unit(b'*')
    };
    let next = |from| from + run(src, from, |each| each != delimiter && each != other);
    let pair_at = |at: usize| src.get(at..at + 2) == Some(&[other, other][..]);
    let first = next(0);
    if !pair_at(first) {
        return None;
    }
    let single = next(first + 2);
    if src.get(single) != Some(&delimiter) {
        return None;
    }
    let end = next(single + 1);
    pair_at(end).then_some(end)
}

/// What a character next to a run of delimiters is to the rules: `\s`, `[\p{P}\p{S}]`, or
/// neither; or the end of the text after it.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Space,
    Punctuation,
    Other,
    End,
}

fn kind(code_point: Option<u32>) -> Kind {
    let Some(code_point) = code_point else {
        return Kind::End;
    };
    if char::from_u32(code_point).is_some_and(js::is_whitespace) {
        Kind::Space
    } else if PUNCTUATION_OR_SYMBOL.contains(code_point) {
        Kind::Punctuation
    } else {
        Kind::Other
    }
}

/// What the rules make of a run of delimiters between these kinds of characters. Only the
/// asterisk's rule lets a run between two letters close.
fn flank(before: Kind, after: Kind, between_others: bool) -> Option<Flank> {
    use Kind::{End, Other, Punctuation, Space};
    match (before, after) {
        (Punctuation, Space | End) | (Other, Space | Punctuation | End) => Some(Flank::Close),
        (Space | Punctuation, Other) | (Space, Punctuation) => Some(Flank::Open),
        (Punctuation, Punctuation) => Some(Flank::Either),
        (Other, Other) if between_others => Some(Flank::Either),
        _ => None,
    }
}

/// The code point at `at`, as a regex with the `u` flag reads it: a surrogate pair's, or a
/// lone surrogate's own.
fn code_point_at(src: &[u16], at: usize) -> Option<u32> {
    char::decode_utf16(src.get(at..)?.iter().copied())
        .next()
        .map(|decoded| decoded.map_or_else(|lone| lone.unpaired_surrogate().into(), u32::from))
}

/// Where the last code point of `src[start..end]` starts.
fn last_code_point(src: &[u16], start: usize, end: usize) -> usize {
    let pair = end >= start + 2
        && (0xDC00..0xE000).contains(&src[end - 1])
        && (0xD800..0xDC00).contains(&src[end - 2]);
    if pair {
        end - 2
    } else {
        end.saturating_sub(1).max(start)
    }
}

/// `[\p{P}\p{S}]` with the `u` flag, from the regex engine's own tables.
static PUNCTUATION_OR_SYMBOL: LazyLock<CodePoints> =
    LazyLock::new(|| CodePoints::new(["P", "S"].into_iter().flat_map(general_category)));

fn general_category(value: &'static str) -> impl Iterator<Item = RangeInclusive<u32>> {
    regress::general_category(value).expect("a General_Category value")
}

/// A set of code points: a bit for each in the Basic Multilingual Plane, and ranges past it.
struct CodePoints {
    basic: Box<[u64; 1 << 10]>,
    astral: Vec<RangeInclusive<u32>>,
}

impl CodePoints {
    fn new(ranges: impl Iterator<Item = RangeInclusive<u32>>) -> Self {
        let mut set = CodePoints {
            basic: Box::new([0; 1 << 10]),
            astral: Vec::new(),
        };
        for range in ranges {
            for code_point in *range.start()..=(*range.end()).min(0xFFFF) {
                set.basic[code_point as usize / 64] |= 1 << (code_point % 64);
            }
            if *range.end() > 0xFFFF {
                set.astral
                    .push((*range.start()).max(0x10000)..=*range.end());
            }
        }
        set.astral.sort_by_key(|range| *range.start());
        set
    }

    fn contains(&self, code_point: u32) -> bool {
        match self.basic.get(code_point as usize / 64) {
            Some(bits) => bits & (1 << (code_point % 64)) != 0,
            None => self
                .astral
                .binary_search_by(|range| {
                    if *range.end() < code_point {
                        std::cmp::Ordering::Less
                    } else if *range.start() > code_point {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .is_ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::rules::INLINE;
    use super::{Flank, Found};
    use tarnish_js::random::check_same;
    use tarnish_js::regexp::RegExp;
    use tarnish_js::utf16::unit;

    /// Characters of each kind around each delimiter, pairs of each, and letters and marks
    /// outside ASCII.
    const ALPHABET: &[&str] = &[
        "*",
        "*",
        "**",
        "***",
        "_",
        "_",
        "__",
        "___",
        "~",
        "~",
        "~~",
        "~~~",
        "a",
        "b",
        "Z",
        "0",
        " ",
        " ",
        "\n",
        "\t",
        "\u{A0}",
        "\u{2028}",
        "\u{3000}",
        "\u{FEFF}",
        "\u{B}",
        ".",
        ",",
        "!",
        "#",
        "$",
        "+",
        "<",
        "=",
        "^",
        "`",
        "|",
        "(",
        ")",
        "[",
        "]",
        "\\",
        "é",
        "\u{301}",
        "。",
        "€",
        "«",
        "😀",
        "𝔸",
        "\u{10FFFD}",
        "\u{E000}",
    ];

    /// Every match the rule makes going through `src` as marked's loop does, from each match's
    /// end to the next.
    fn matches(src: &[u16], mut next: impl FnMut(&[u16], usize) -> Option<Found>) -> Vec<Found> {
        let mut found = Vec::new();
        let mut last_index = 0;
        while let Some(each) = next(src, last_index) {
            last_index = each.end;
            found.push(each);
        }
        found
    }

    /// A match of the regex as the matchers give it. Each group is the run the match ends with.
    fn found(regex: &RegExp, src: &[u16], last_index: usize) -> Option<Found> {
        let found = regex.exec_at(src, last_index)?;
        let run = (1..=6).find_map(|group| {
            let flank = [Flank::Close, Flank::Open, Flank::Either][(group - 1) / 2];
            let run = found.truthy(group)?;
            Some((found.end() - run.len()..found.end(), flank))
        });
        Some(Found {
            index: found.index(),
            end: found.end(),
            run,
        })
    }

    #[test]
    fn em_strong_matches_its_regexes() {
        for (delimiter, regex) in [
            (b'*', &INLINE.em_strong_r_delim_ast),
            (b'_', &INLINE.em_strong_r_delim_und),
        ] {
            check_same(
                ALPHABET,
                1_000_000,
                |src| matches(src, |src, at| super::em_strong(src, at, unit(delimiter))),
                |src| matches(src, |src, at| found(regex, src, at)),
            );
        }
    }

    #[test]
    fn del_matches_its_regex() {
        check_same(
            ALPHABET,
            1_000_000,
            |src| matches(src, super::del),
            |src| matches(src, |src, at| found(&INLINE.del_r_delim, src, at)),
        );
    }
}
