//! Rules the lexer runs at nearly every character, matched directly instead of through the
//! regex engine. Each gives what its regex in `rules.rs` gives, which the tests below check
//! against that regex on random input.
//!
//! Where a regex scans far before it fails, a guard runs first: a cheaper test the input must
//! pass for the regex to match. A guard can say yes to input the regex rejects, but never no to
//! input it matches, which the tests check too.

use std::ops::Range;

use super::rules::{BLOCK, INLINE};
use tarnish_js::regexp::{Match, RegExp};
use tarnish_js::utf16::{
    BACKSLASH, BACKTICK, CLOSE_BRACKET, CLOSE_PAREN, LESS_THAN, OPEN_BRACKET, OPEN_PAREN, find,
    is_line_terminator, is_whitespace, unit,
};

/// How many units from `at` on `each` holds for.
fn run(src: &[u16], at: usize, each: impl Fn(u16) -> bool) -> usize {
    src[at..].iter().take_while(|&&unit| each(unit)).count()
}

/// ` {0,3}`: the spaces, up to three, that `src` starts with.
pub(crate) fn indent(src: &[u16]) -> usize {
    src.iter()
        .take(3)
        .take_while(|&&each| each == unit(b' '))
        .count()
}

/// `regex.exec(src)`, where `may` says the regex may match.
fn guarded<'t>(regex: &RegExp, may: fn(&[u16]) -> bool, src: &'t [u16]) -> Option<Match<'t>> {
    if !may(src) {
        return None;
    }
    regex.exec(src)
}

/// `block.newline`'s match length, `^(?:[ \t]*(?:\n|$))+`: the blank lines at the start.
pub fn newline(src: &[u16]) -> Option<usize> {
    let mut matched = None;
    let mut index = 0;
    loop {
        let blank = index + run(src, index, |each| each == unit(b' ') || each == unit(b'\t'));
        match src.get(blank) {
            Some(&each) if each == unit(b'\n') => {
                index = blank + 1;
                matched = Some(index);
            }
            None => return Some(blank),
            Some(_) => return matched,
        }
    }
}

/// `block.hr`'s match length,
/// `^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)`: a line of three or more
/// of one of `-`, `_` or `*`, with blanks among them, and the newlines after it.
pub fn hr(src: &[u16]) -> Option<usize> {
    let spaces = indent(src);
    let &mark = src.get(spaces)?;
    if !b"-_*".map(unit).contains(&mark) {
        return None;
    }
    let blank = |each: u16| each == unit(b' ') || each == unit(b'\t');
    let line = spaces + run(src, spaces, |each| each == mark || blank(each));
    if src[spaces..line]
        .iter()
        .filter(|&&each| each == mark)
        .count()
        < 3
    {
        return None;
    }
    match src.get(line) {
        None => Some(line),
        Some(&each) if each == unit(b'\n') => {
            Some(line + run(src, line, |each| each == unit(b'\n')))
        }
        Some(_) => None,
    }
}

/// `block.heading`, `^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)`: the match's end, how many `#`s it
/// has, and where its text is.
pub struct Heading {
    pub end: usize,
    pub depth: usize,
    pub text: Range<usize>,
}

pub fn heading(src: &[u16]) -> Option<Heading> {
    let spaces = indent(src);
    let depth = run(src, spaces, |each| each == unit(b'#'));
    if !(1..=6).contains(&depth) {
        return None;
    }
    let text = spaces + depth;
    if src.get(text).is_some_and(|&each| !is_whitespace(each)) {
        return None;
    }
    // `.` stops at a line terminator, and only `\n` or the end may follow the text.
    let text_end = text + run(src, text, |each| !is_line_terminator(each));
    let end = match src.get(text_end) {
        None => text_end,
        Some(&each) if each == unit(b'\n') => {
            text_end + run(src, text_end, |each| each == unit(b'\n'))
        }
        Some(_) => return None,
    };
    Some(Heading {
        end,
        depth,
        text: text..text_end,
    })
}

/// `block.paragraph`'s match length. A first line no other follows is the whole match, as each
/// further line the paragraph takes has to hold something; past it, only the regex knows which
/// lines interrupt a paragraph.
pub fn paragraph(src: &[u16]) -> Option<usize> {
    let line = find(src, unit(b'\n')).unwrap_or(src.len());
    let alone = src.get(line + 1).is_none_or(|&each| each == unit(b'\n'));
    if line > 0 && alone {
        return Some(line);
    }
    BLOCK.paragraph.exec(src).map(|found| found.end())
}

/// `block.def`.
pub fn def(src: &[u16]) -> Option<Match<'_>> {
    guarded(&BLOCK.def, may_be_def, src)
}

/// After up to 3 spaces, a `[`, a label with no bracket that isn't escaped, and `]:`.
fn may_be_def(src: &[u16]) -> bool {
    let spaces = indent(src);
    if src.get(spaces) != Some(&OPEN_BRACKET) {
        return false;
    }
    let mut index = spaces + 1;
    while let Some(&each) = src.get(index) {
        match each {
            BACKSLASH => index += 2,
            OPEN_BRACKET => return false,
            CLOSE_BRACKET => return src.get(index + 1) == Some(&unit(b':')),
            _ => index += 1,
        }
    }
    false
}

/// `block.table`.
pub fn table(src: &[u16]) -> Option<Match<'_>> {
    guarded(&BLOCK.table, may_be_table, src)
}

/// The line after the first starts a delimiter row: up to 3 spaces, an optional pipe and the
/// spaces after it, an optional colon, and a `-`.
fn may_be_table(src: &[u16]) -> bool {
    let Some(newline) = find(src, unit(b'\n')) else {
        return false;
    };
    let mut rest = &src[newline + 1..];
    rest = &rest[indent(rest)..];
    if rest.first() == Some(&unit(b'|')) {
        rest = &rest[1 + run(rest, 1, |each| each == unit(b' '))..];
    }
    if rest.first() == Some(&unit(b':')) {
        rest = &rest[1..];
    }
    rest.first() == Some(&unit(b'-'))
}

/// `block.lheading`.
pub fn lheading(src: &[u16]) -> Option<Match<'_>> {
    guarded(&BLOCK.lheading, may_be_lheading, src)
}

/// The underline, a line starting with up to 3 spaces and an `=` or `-`, comes before the first
/// blank line, as the text can't take a newline a blank line follows.
fn may_be_lheading(src: &[u16]) -> bool {
    let mut from = 0;
    while let Some(newline) = find(&src[from..], unit(b'\n')) {
        let next = &src[from + newline + 1..];
        if matches!(next.get(indent(next)), Some(&each) if each == unit(b'=') || each == unit(b'-'))
        {
            return true;
        }
        // `\n(?!\s*?\n)`
        let blank = next
            .iter()
            .find(|&&each| each == unit(b'\n') || !is_whitespace(each));
        if blank == Some(&unit(b'\n')) {
            return false;
        }
        from += newline + 1;
    }
    false
}

/// `inline.text`'s match length:
///
/// `^([`~]+|[^`~])(?:(?= {2,}\n)|(?=[E]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|[hH][tT][tT][pP][sS]?|
/// [fF][tT][pP]:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^E](?=[E]+@)))`
///
/// with `E` the characters of an email's local part. After its first character (or run of
/// backticks and tildes), text runs up to the first place one of the lookaheads holds: the
/// first character it stops at, or earlier, the spaces before a newline or an email's local
/// part, where something else comes before them.
pub fn inline_text(src: &[u16]) -> Option<usize> {
    let &first = src.first()?;
    let is_fence = |each| each == BACKTICK || each == unit(b'~');
    let start = if is_fence(first) {
        1 + run(src, 1, is_fence)
    } else {
        1
    };
    if spaces_then_newline(src, start) || email_then_at(src, start) {
        return Some(start);
    }
    // The first newline and `@` on the way, where the other lookaheads can first hold.
    let (mut stop, mut newline, mut at) = (start, None, None);
    while let Some(&current) = src.get(stop) {
        let class = class_of(current);
        if class & (STOP | SCHEME | LOOKAHEAD) != 0 {
            // `\b_` needs a `_` here, which the class already has.
            if class & STOP != 0 || class & SCHEME != 0 && starts_url(&src[stop..]) {
                break;
            }
            if current == unit(b'\n') {
                newline.get_or_insert(stop);
            } else if current == unit(b'@') {
                at.get_or_insert(stop);
            }
        }
        stop += 1;
    }
    let breaks = newline
        .and_then(|newline| spaces_before_newline(src, start, newline, stop))
        .unwrap_or(stop);
    let email = local_part_before_at(src, start, at.unwrap_or(stop), stop).unwrap_or(stop);
    Some(stop.min(breaks).min(email))
}

/// `[^ ](?= {2,}\n)` between `start` and `stop`, looking from the newline at `from`: where the
/// first two or more spaces before a newline start, after something else. A run of spaces is
/// next to its newline, so it ends before `stop`, which isn't a space.
fn spaces_before_newline(src: &[u16], start: usize, from: usize, stop: usize) -> Option<usize> {
    let mut from = from;
    while let Some(newline) = find(&src[from..stop], unit(b'\n')).map(|index| from + index) {
        let spaces = src[start..newline]
            .iter()
            .rev()
            .take_while(|&&each| each == unit(b' '))
            .count();
        if spaces >= 2 && newline - spaces > start {
            return Some(newline - spaces);
        }
        from = newline + 1;
    }
    None
}

/// `[^E](?=[E]+@)` before `stop`, looking from the `@` at `from` or else from `stop`: where the
/// first local part followed by an `@` starts, after something else. Past `stop`, the local
/// part runs through its email characters.
fn local_part_before_at(src: &[u16], start: usize, from: usize, stop: usize) -> Option<usize> {
    let reach = (stop + run(src, stop, is_email_character)).min(src.len().saturating_sub(1));
    let mut from = from;
    while from <= reach {
        let at = from + find(&src[from..=reach], unit(b'@'))?;
        let local = src[start..at]
            .iter()
            .rev()
            .take_while(|&&each| is_email_character(each))
            .count();
        // A local part reaching back to `start` would have matched there.
        if local > 0 && at - local > start {
            return Some(at - local);
        }
        from = at + 1;
    }
    None
}

fn class_of(unit: u16) -> u8 {
    CLASSES.get(usize::from(unit)).copied().unwrap_or(0)
}

/// What `inline_text` asks of an ASCII character.
const STOP: u8 = 1;
const SCHEME: u8 = 2;
const EMAIL: u8 = 4;
/// `\n` and `@`, which the lookaheads for a hard break and an email end at.
const LOOKAHEAD: u8 = 8;

static CLASSES: [u8; 128] = {
    let mut classes = [0; 128];
    let mut byte = 0;
    while byte < 128 {
        let character = byte as u8;
        if matches!(
            character,
            b'\\' | b'<' | b'!' | b'[' | b'`' | b'*' | b'~' | b'_'
        ) {
            classes[byte] |= STOP;
        }
        if matches!(character, b'h' | b'H' | b'f' | b'F' | b'w') {
            classes[byte] |= SCHEME;
        }
        if character.is_ascii_alphanumeric() || is_email_symbol(character) {
            classes[byte] |= EMAIL;
        }
        if character == b'\n' || character == b'@' {
            classes[byte] |= LOOKAHEAD;
        }
        byte += 1;
    }
    classes
};

/// `[hH][tT][tT][pP][sS]?|[fF][tT][pP]:\/\/|www\.` at the start of `src`.
fn starts_url(src: &[u16]) -> bool {
    starts_with_ignoring_case(src, b"http")
        || starts_with_ignoring_case(src, b"ftp")
            && src[3..].starts_with(&[unit(b':'), unit(b'/'), unit(b'/')])
        || src.starts_with(&[unit(b'w'), unit(b'w'), unit(b'w'), unit(b'.')])
}

/// `(?= {2,}\n)` at `at`.
fn spaces_then_newline(src: &[u16], at: usize) -> bool {
    let spaces = run(src, at, |each| each == unit(b' '));
    spaces >= 2 && src.get(at + spaces) == Some(&unit(b'\n'))
}

/// `(?=[E]+@)` at `at`.
fn email_then_at(src: &[u16], at: usize) -> bool {
    let local = run(src, at, is_email_character);
    local >= 1 && src.get(at + local) == Some(&unit(b'@'))
}

/// `[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]`.
fn is_email_character(each: u16) -> bool {
    class_of(each) & EMAIL != 0
}

const fn is_email_symbol(byte: u8) -> bool {
    matches!(
        byte,
        b'.' | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'/'
            | b'='
            | b'?'
            | b'_'
            | b'`'
            | b'{'
            | b'|'
            | b'}'
            | b'~'
            | b'-'
    )
}

/// Whether `src` starts with the ASCII letters `word`, in either case.
fn starts_with_ignoring_case(src: &[u16], word: &[u8]) -> bool {
    src.len() >= word.len()
        && src.iter().zip(word).all(|(&each, &letter)| {
            u8::try_from(each).is_ok_and(|byte| byte.eq_ignore_ascii_case(&letter))
        })
}

/// `inline.url`.
pub fn url(src: &[u16]) -> Option<Match<'_>> {
    guarded(&INLINE.url, may_be_url, src)
}

/// `src` starts with a scheme the rule takes or `www.`, or with an email's local part and an
/// `@`.
fn may_be_url(src: &[u16]) -> bool {
    let local = run(src, 0, |each| {
        u8::try_from(each).is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
    });
    if local > 0 && src.get(local) == Some(&unit(b'@')) {
        return true;
    }
    let scheme = |word: &[u8]| {
        starts_with_ignoring_case(src, word)
            && src[word.len()..].starts_with(&[unit(b':'), unit(b'/'), unit(b'/')])
    };
    scheme(b"http")
        || scheme(b"https")
        || scheme(b"ftp")
        || src.starts_with(&[unit(b'w'), unit(b'w'), unit(b'w'), unit(b'.')])
}

/// `blockSkip`'s next match from `from` on, as its search from `lastIndex` finds it: a link, a
/// code span or a tag, whose text inline tokenizing masks. Each alternative settles without
/// backtracking: no way back through a label, a code span's run or a destination turns up
/// another place for the match to end.
pub fn block_skip(src: &[u16], from: usize) -> Option<Range<usize>> {
    let mut at = from;
    while at < src.len() {
        at += src[at..]
            .iter()
            .position(|&each| matches!(each, OPEN_BRACKET | BACKTICK | LESS_THAN))?;
        let end = match src[at] {
            // `\[(?:[^\[\]`]|(?<a>`+)[^`]+\k<a>(?!`))*?\]\(…\)`
            OPEN_BRACKET => link_label(src, at + 1),
            // `(?<!`)()(?<b>`+)[^`]+\k<b>(?!`)`
            BACKTICK if at == 0 || src[at - 1] != BACKTICK => code_span(src, at),
            // A backtick comes before it, which the lookbehind rules out.
            BACKTICK => None,
            // `<(?! )[^<>]*?>`
            _ => angle(src, at + 1),
        };
        if let Some(end) = end {
            return Some(at..end);
        }
        at += 1;
    }
    None
}

/// The end of a link from past its `[`: a label of anything but brackets and backticks, or
/// whole code spans, then `](`, then a destination.
fn link_label(src: &[u16], mut at: usize) -> Option<usize> {
    loop {
        match *src.get(at)? {
            CLOSE_BRACKET if src.get(at + 1) == Some(&OPEN_PAREN) => {
                return destination(src, at + 2);
            }
            OPEN_BRACKET | CLOSE_BRACKET => return None,
            BACKTICK => at = code_span(src, at)?,
            _ => at += 1,
        }
    }
}

/// The end of a code span at `at`: a run of backticks, something other than backticks, and a
/// run exactly as long.
fn code_span(src: &[u16], at: usize) -> Option<usize> {
    let is_backtick = |each| each == BACKTICK;
    let ticks = run(src, at, is_backtick);
    let content = at + ticks;
    let close = content + find(&src[content..], BACKTICK)?;
    (close > content && run(src, close, is_backtick) == ticks).then_some(close + ticks)
}

/// The end of `(?:\\[\s\S]|[^\\\(\)]|\((?:\\[\s\S]|[^\\\(\)])*\))*\)` from `at`, past the `(`:
/// escapes, anything but parentheses, and parentheses holding neither, up to a `)`.
fn destination(src: &[u16], mut at: usize) -> Option<usize> {
    let mut nested = false;
    loop {
        match *src.get(at)? {
            BACKSLASH if at + 1 < src.len() => at += 2,
            BACKSLASH => return None,
            CLOSE_PAREN if nested => {
                nested = false;
                at += 1;
            }
            CLOSE_PAREN => return Some(at + 1),
            OPEN_PAREN if nested => return None,
            OPEN_PAREN => {
                nested = true;
                at += 1;
            }
            _ => at += 1,
        }
    }
}

/// The end of a tag from past its `<`: no space first, and no `<` before the `>`.
fn angle(src: &[u16], at: usize) -> Option<usize> {
    if src.get(at) == Some(&unit(b' ')) {
        return None;
    }
    let close = at
        + src[at..]
            .iter()
            .position(|&each| each == unit(b'>') || each == LESS_THAN)?;
    (src[close] == unit(b'>')).then_some(close + 1)
}

#[cfg(test)]
mod tests {
    use super::super::rules::{BLOCK, INLINE};
    use tarnish_js::random::{check_may, check_same};
    use tarnish_js::regexp::RegExp;

    /// A rule's regex, its guard, and the pieces of the strings to check the guard on.
    type Guard = (&'static RegExp, fn(&[u16]) -> bool, &'static [&'static str]);

    #[test]
    fn guards_pass_their_matches() {
        let guards: [Guard; 4] = [
            (
                &BLOCK.lheading,
                super::may_be_lheading,
                &[
                    "a", "b", " ", "  ", "   ", "    ", "\n", "\n", "\n", "\t", "=", "==", "-",
                    "--", "*", "+", "1.", "1)", "#", ">", "`", "```", "~~~", "<a>", "|", ":",
                    "\u{A0}", "\u{2028}", "\r",
                ],
            ),
            (
                &BLOCK.table,
                super::may_be_table,
                &[
                    "a", "b", " ", "  ", "\n", "\n", "|", "|", ":", "-", "--", "\t", ">", "#", "\r",
                ],
            ),
            (
                &INLINE.url,
                super::may_be_url,
                &[
                    "h", "H", "t", "T", "p", "P", "s", "S", "f", "F", "w", "www.", "wWw.", "http",
                    "https", "ftp", "hTtPs", "://", "://", ":", "/", "@", "@", "a", "Z", "0", ".",
                    "_", "+", "-", " ", "<", "é", "\n",
                ],
            ),
            (
                &BLOCK.def,
                super::may_be_def,
                &[
                    "[", "[", "]", "]:", ":", "\\", "\\]", "a", " ", "  ", "\n", "\t", "<", ">",
                    "\"", "'", "(", ")", "é",
                ],
            ),
        ];
        for (regex, may, alphabet) in guards {
            check_may(regex, may, alphabet);
        }
    }

    #[test]
    fn paragraph_matches_its_regex() {
        check_same(
            &[
                "a", "b", " ", "\n", "\n", "#", ">", "-", "*", "1.", "```", "<div>", "|", "\t", "é",
            ],
            1_000_000,
            super::paragraph,
            |src| BLOCK.paragraph.exec(src).map(|found| found.end()),
        );
    }

    #[test]
    fn block_skip_matches_its_regex() {
        let froms = |src: &[u16]| [0, src.len() / 3, src.len() / 2];
        check_same(
            &[
                "[", "[", "]", "](", "(", ")", "\\", "`", "`", "``", "<", "<", ">", " ", "a", "b",
                "\n",
            ],
            500_000,
            |src| froms(src).map(|from| super::block_skip(src, from)),
            |src| {
                froms(src).map(|from| {
                    INLINE.block_skip.exec_at(src, from).map(|found| {
                        found.index() + found.get(2).map_or(0, <[u16]>::len)..found.end()
                    })
                })
            },
        );
    }

    #[test]
    fn hr_matches_its_regex() {
        check_same(
            &[
                " ", "  ", "\t", "-", "-", "_", "_", "*", "*", "\n", "\n", "a", "\r", "\u{A0}",
            ],
            1_000_000,
            super::hr,
            |src| BLOCK.hr.exec(src).map(|found| found.end()),
        );
    }

    #[test]
    fn newline_matches_its_regex() {
        check_same(
            &[" ", "  ", "\t", "\n", "\n", "\r", "a", "\u{A0}", "\u{2028}"],
            1_000_000,
            super::newline,
            |src| BLOCK.newline.exec(src).map(|found| found.end()),
        );
    }

    #[test]
    fn heading_matches_its_regex() {
        check_same(
            &[
                " ", "  ", "#", "#", "##", "###", "\n", "\n", "\t", "\r", "a", "\u{A0}",
                "\u{2028}", "\u{2029}", "\u{FEFF}", "😀",
            ],
            2_000_000,
            |src| {
                super::heading(src).map(|heading| (heading.end, heading.depth, heading.text.len()))
            },
            |src| {
                BLOCK.heading.exec(src).map(|found| {
                    let group = |index| found.get(index).map_or(0, <[u16]>::len);
                    (found.end(), group(1), group(2))
                })
            },
        );
    }

    #[test]
    fn inline_text_matches_its_regex() {
        check_same(
            &[
                " ", "  ", "\n", "\t", "`", "~", "a", "Z", "0", ".", "!", "#", "$", "%", "&", "'",
                "*", "+", "/", "=", "?", "_", "{", "|", "}", "-", "@", "\\", "<", "[", "]", ",",
                ";", "\"", "(", ":", "h", "H", "t", "T", "p", "P", "s", "f", "F", "w", "www.",
                "http", "HTTPS", "ftp://", "Ftp:/", "é", "😀", "\u{A0}",
            ],
            2_000_000,
            super::inline_text,
            |src| INLINE.text.exec(src).map(|found| found.end()),
        );
    }
}
