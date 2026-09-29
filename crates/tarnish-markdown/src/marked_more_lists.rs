//! `marked-more-lists` 1.0.1: a list tokenizer that also reads letters and roman numerals as
//! ordered list markers.

use std::borrow::Cow;
use std::sync::LazyLock;

use tarnish::stack;

use crate::marked::{Lexer, List, ListTokenizer, Token, TokenData, Tokens, hr};
use tarnish_js::JsError;
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::utf16;

const ROMAN_UPPER: &str = "(?:C|XC|L?X{0,3}(?:IX|IV|V?I{0,3}))";
const ROMAN_LOWER: &str = "(?:c|xc|l?x{0,3}(?:ix|iv|v?i{0,3}))";

static RULE: LazyLock<RegExp> = LazyLock::new(|| {
    let bullet_pattern =
        format!(r"(?:[*+-]|(?:\d{{1,9}}|[a-zA-Z]|{ROMAN_UPPER}|{ROMAN_LOWER})[.)]))");
    RegExp::new(
        &format!(r"^( {{0,3}}{bullet_pattern}([ \t][^\n]+?)?(?:\n|$)"),
        "",
    )
});
static ROMAN_UPPER_BULLET: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(&format!("^{ROMAN_UPPER}[.)]$"), ""));
static ROMAN_LOWER_BULLET: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(&format!("^{ROMAN_LOWER}[.)]$"), ""));
static LOWER_BULLET: LazyLock<RegExp> = LazyLock::new(|| RegExp::new("^[a-z][.)]$", ""));
static UPPER_BULLET: LazyLock<RegExp> = LazyLock::new(|| RegExp::new("^[A-Z][.)]$", ""));

const SPACE: u16 = utf16::unit(b' ');
const TAB: u16 = utf16::unit(b'\t');
const NEWLINE: u16 = utf16::unit(b'\n');

/// The regexes and patterns that end an item, for each of the four indents
/// `Math.min(3, indent - 1)` gives: a line may start with that many spaces and still end it.
struct Enders {
    spaces: usize,
    next_bullet: RegExp,
    hr: RegExp,
}

static ENDERS: LazyLock<Vec<Enders>> = LazyLock::new(|| {
    (0..=3)
        .map(|spaces| Enders {
            spaces,
            next_bullet: RegExp::new(
                &format!(
                    r"^ {{0,{spaces}}}(?:[*+-]|(?:\d{{1,9}}|[a-zA-Z]|{ROMAN_UPPER}|{ROMAN_LOWER})[.)])((?:[ \t][^\n]*)?(?:\n|$))"
                ),
                "",
            ),
            hr: RegExp::new(
                &format!(r"^ {{0,{spaces}}}((?:- *){{3,}}|(?:_ *){{3,}}|(?:\* *){{3,}})(?:\n+|$)"),
                "",
            ),
        })
        .collect()
});

impl Enders {
    /// The line past its spaces, if it has no more than may start an ender. None of the
    /// enders goes on with a space.
    fn after_spaces<'l>(&self, line: &'l [u16]) -> Option<&'l [u16]> {
        let spaces = line.iter().take_while(|&&unit| unit == SPACE).count();
        (spaces <= self.spaces).then(|| &line[spaces..])
    }

    /// `fencesBeginRegex`, `^ {0,n}(?:```|~~~)`.
    fn fences_begin(&self, line: &[u16]) -> bool {
        self.after_spaces(line)
            .is_some_and(|rest| utf16::starts_with(rest, "```") || utf16::starts_with(rest, "~~~"))
    }

    /// `headingBeginRegex`, `^ {0,n}#`.
    fn heading_begin(&self, line: &[u16]) -> bool {
        self.after_spaces(line)
            .is_some_and(|rest| rest.first() == Some(&utf16::unit(b'#')))
    }

    /// `htmlBeginRegex`, `^ {0,n}<[a-z].*>` ignoring case, which without the `u` flag folds
    /// only ASCII letters to `[a-z]`.
    fn html_begin(&self, line: &[u16]) -> bool {
        let Some([open, letter, rest @ ..]) = self.after_spaces(line) else {
            return false;
        };
        *open == utf16::unit(b'<')
            && u8::try_from(*letter).is_ok_and(|letter| letter.is_ascii_alphabetic())
            && rest
                .iter()
                .take_while(|&&unit| !utf16::is_line_terminator(unit))
                .any(|&unit| unit == utf16::unit(b'>'))
    }

    /// `nextBulletRegex`, which only a line with a marker after its spaces may match.
    fn next_bullet(&self, line: &[u16]) -> bool {
        self.after_spaces(line).is_some_and(marker_follows) && self.next_bullet.test(line)
    }

    /// `hrRegex`, which only a line with a `-`, `_` or `*` after its spaces may match.
    fn hr(&self, line: &[u16]) -> bool {
        self.after_spaces(line).is_some_and(
            |rest| matches!(rest.first(), Some(&unit) if b"-_*".map(utf16::unit).contains(&unit)),
        ) && self.hr.test(line)
    }
}

#[derive(Clone, Copy)]
enum ListType {
    Unordered,
    UpperRoman,
    LowerRoman,
    LowerLetter,
    UpperLetter,
    Decimal,
}

impl ListType {
    const ALL: [ListType; 6] = [
        ListType::Unordered,
        ListType::UpperRoman,
        ListType::LowerRoman,
        ListType::LowerLetter,
        ListType::UpperLetter,
        ListType::Decimal,
    ];

    /// `bull`: the pattern of the list's markers, which for an ordered list ends with the
    /// delimiter its first marker has.
    fn bull(self, delimiter: char) -> String {
        let marker = match self {
            ListType::Unordered => return "[*+-]".to_string(),
            ListType::UpperRoman => ROMAN_UPPER,
            ListType::LowerRoman => ROMAN_LOWER,
            ListType::LowerLetter => "[a-z]",
            ListType::UpperLetter => "[A-Z]",
            ListType::Decimal => r"\d{1,9}",
        };
        format!(r"{marker}\{delimiter}")
    }
}

/// `itemRegex` for each list type, for a `.` delimiter and then a `)`.
static ITEM_REGEXES: LazyLock<[[RegExp; 2]; 6]> = LazyLock::new(|| {
    ListType::ALL.map(|list_type| {
        ['.', ')'].map(|delimiter| {
            let bull = list_type.bull(delimiter);
            RegExp::new(&format!(r"^( {{0,3}}{bull})((?:[\t ][^\n]*)?(?:\n|$))"), "")
        })
    })
});

/// `markedMoreLists()`.
pub fn more_lists() -> ListTokenizer {
    list
}

fn letter_to_int(letter: &[u16]) -> f64 {
    match utf16::to_lower_case(letter).first() {
        Some(&unit) => f64::from(unit) - 96.0,
        None => f64::NAN,
    }
}

fn roman_to_int(roman: &[u16]) -> f64 {
    let value = |unit: Option<&u16>| -> Option<f64> {
        Some(
            match char::from_u32(u32::from(*unit?))?.to_ascii_uppercase() {
                'I' => 1.0,
                'V' => 5.0,
                'X' => 10.0,
                'L' => 50.0,
                'C' => 100.0,
                'D' => 500.0,
                'M' => 1000.0,
                _ => return None,
            },
        )
    };
    let roman = utf16::from(&utf16::to_string(roman).to_uppercase());
    let mut total = 0.0;
    for index in 0..roman.len() {
        let current = value(roman.get(index)).unwrap_or(f64::NAN);
        match value(roman.get(index + 1)) {
            Some(next) if current < next => total -= current,
            _ => total += current,
        }
    }
    total
}

/// `parseInt(string, 10)`.
fn parse_int(digits: &[u16]) -> f64 {
    tarnish_js::parse_int(&utf16::to_string(digits))
}

/// Whether `RULE` may match: after up to 3 spaces, a marker.
fn may_be_list(src: &[u16]) -> bool {
    marker_follows(&src[crate::marked::indent(src)..])
}

/// Whether `rest` starts with a bullet, or any digits or letters and a `.` or `)`, and then a
/// space, a tab, a newline or the end, as a list marker does. Most blocks start with a word,
/// which takes a regex further to rule out.
fn marker_follows(rest: &[u16]) -> bool {
    let byte = |at: usize| rest.get(at).and_then(|&unit| u8::try_from(unit).ok());
    let marker = match byte(0) {
        Some(b'*' | b'+' | b'-') => 1,
        _ => {
            let letters = (0..)
                .take_while(|&at| byte(at).is_some_and(|byte| byte.is_ascii_alphanumeric()))
                .count();
            // The roman numerals' pattern also matches nothing, so `.` alone is a marker.
            if !matches!(byte(letters), Some(b'.' | b')')) {
                return false;
            }
            letters + 1
        }
    };
    rest.get(marker).is_none() || matches!(byte(marker), Some(b' ' | b'\t' | b'\n'))
}

/// `line.replace(/\t/g, "    ")`.
fn replace_tabs(line: &[u16]) -> Cow<'_, [u16]> {
    if !line.contains(&TAB) {
        return Cow::Borrowed(line);
    }
    let mut replaced = Vec::with_capacity(line.len() + 8);
    for &unit in line {
        if unit == TAB {
            replaced.extend([SPACE; 4]);
        } else {
            replaced.push(unit);
        }
    }
    Cow::Owned(replaced)
}

/// `line.replace(/^\t+/, (tabs) => " ".repeat(3 * tabs.length))`.
fn leading_tabs_to_spaces(line: &[u16]) -> Vec<u16> {
    let tabs = line.iter().take_while(|&&unit| unit == TAB).count();
    let mut replaced = vec![SPACE; 3 * tabs];
    replaced.extend_from_slice(&line[tabs..]);
    replaced
}

/// `line.search(/[^ ]/)`.
fn first_non_space(line: &[u16]) -> Option<usize> {
    line.iter().position(|&unit| unit != SPACE)
}

/// `/^[ \t]*$/.test(line)`.
fn is_blank(line: &[u16]) -> bool {
    line.iter().all(|&unit| unit == SPACE || unit == TAB)
}

/// `/\n[ \t]*\n[ \t]*$/.test(raw)`.
fn ends_in_blank_line(raw: &[u16]) -> bool {
    fn without_blanks(units: &[u16]) -> &[u16] {
        let kept = units.iter().rposition(|&unit| unit != SPACE && unit != TAB);
        &units[..kept.map_or(0, |last| last + 1)]
    }
    match without_blanks(raw) {
        [rest @ .., last] if *last == NEWLINE => {
            matches!(without_blanks(rest), [.., last] if *last == NEWLINE)
        }
        _ => false,
    }
}

/// `/^\[[ xX]\] /.exec(item)`'s match.
fn task(item: &[u16]) -> Option<&[u16]> {
    match item {
        [open, mark, close, space, ..]
            if *open == utf16::unit(b'[')
                && b" xX".map(utf16::unit).contains(mark)
                && *close == utf16::unit(b']')
                && *space == SPACE =>
        {
            Some(&item[..4])
        }
        _ => None,
    }
}

/// `item.replace(/^\[[ xX]\] +/, "")`, for an item `task` found a task in.
fn without_task(item: &[u16]) -> Vec<u16> {
    let spaces = item[3..].iter().take_while(|&&unit| unit == SPACE).count();
    item[3 + spaces..].to_vec()
}

/// `/\n.*\n/.test(raw)`: two newlines with no other line terminator between them.
fn has_newlines_in_a_row(raw: &[u16]) -> bool {
    let mut after_newline = false;
    for &unit in raw {
        if utf16::is_line_terminator(unit) {
            if unit == NEWLINE && after_newline {
                return true;
            }
            after_newline = unit == NEWLINE;
        }
    }
    false
}

fn list(lexer: &mut Lexer, src: &Units) -> Result<Option<Token>, JsError> {
    let mut src: &[u16] = src;
    if !may_be_list(src) {
        return Ok(None);
    }
    let Some(cap) = RULE.exec(src) else {
        return Ok(None);
    };
    let mut bullet = utf16::trim(cap.get(1).unwrap_or_default());
    let is_ordered = !["*", "-", "+"]
        .iter()
        .any(|unordered| utf16::is(bullet, unordered));
    let list_type = if !is_ordered {
        ListType::Unordered
    } else if ROMAN_UPPER_BULLET.test(bullet) {
        ListType::UpperRoman
    } else if ROMAN_LOWER_BULLET.test(bullet) {
        ListType::LowerRoman
    } else if LOWER_BULLET.test(bullet) {
        ListType::LowerLetter
    } else if UPPER_BULLET.test(bullet) {
        ListType::UpperLetter
    } else {
        ListType::Decimal
    };
    let paren = bullet.last() == Some(&utf16::unit(b')'));
    let item_regex = &ITEM_REGEXES[list_type as usize][usize::from(paren)];
    let mut list = List {
        ordered: is_ordered,
        ..List::default()
    };
    let mut list_raw: Vec<u16> = Vec::new();
    let mut ends_with_blank_line = false;
    while !src.is_empty() {
        let mut end_early = false;
        let mut item_contents: Vec<u16> = Vec::new();
        let Some(cap) = item_regex.exec(src) else {
            break;
        };
        // End the list if the bullet was actually a thematic break.
        if hr(src).is_some() {
            break;
        }
        let mut raw = cap.all().to_vec();
        bullet = utf16::trim(cap.get(1).unwrap_or_default());
        let cap1_length = cap.get(1).map_or(0, <[u16]>::len);
        let cap2 = cap.get(2).unwrap_or_default();
        src = utf16::substring(src, raw.len());

        let mut line = leading_tabs_to_spaces(utf16::first_line(cap2));
        let mut next_line = utf16::first_line(src);
        let mut blank_line = utf16::trim(&line).is_empty();

        let indent;
        if blank_line {
            indent = cap1_length + 1;
        } else {
            let first = first_non_space(cap2).map_or(-1, |index| index as isize);
            // Treat indented code blocks (more than 4 spaces) as having only 1 indent.
            let first = if first > 4 { 1 } else { first };
            item_contents = utf16::slice(&line, first, None).to_vec();
            indent = (first + cap1_length as isize).max(0) as usize;
        }

        // Items begin with at most one blank line.
        if blank_line && is_blank(next_line) {
            raw.extend_from_slice(next_line);
            raw.push(utf16::unit(b'\n'));
            src = utf16::substring(src, next_line.len() + 1);
            end_early = true;
        }

        if !end_early {
            let enders = &ENDERS[indent.saturating_sub(1).min(3)];
            while !src.is_empty() {
                let raw_line = utf16::first_line(src);
                next_line = raw_line;
                let next_line_without_tabs = replace_tabs(next_line);

                if enders.fences_begin(next_line)
                    || enders.heading_begin(next_line)
                    || enders.html_begin(next_line)
                    || enders.next_bullet(next_line)
                    || enders.hr(next_line)
                {
                    break;
                }

                let dedents =
                    first_non_space(&next_line_without_tabs).is_some_and(|first| first >= indent);
                if dedents || utf16::trim(next_line).is_empty() {
                    item_contents.push(utf16::unit(b'\n'));
                    item_contents
                        .extend_from_slice(utf16::substring(&next_line_without_tabs, indent));
                } else {
                    if blank_line {
                        break;
                    }
                    // A paragraph continues unless the last line was another kind of block.
                    let line_without_tabs = replace_tabs(&line);
                    if first_non_space(&line_without_tabs).is_some_and(|first| first >= 4)
                        || enders.fences_begin(&line)
                        || enders.heading_begin(&line)
                        || enders.hr(&line)
                    {
                        break;
                    }
                    item_contents.push(utf16::unit(b'\n'));
                    item_contents.extend_from_slice(next_line);
                }

                if !blank_line && utf16::trim(next_line).is_empty() {
                    blank_line = true;
                }

                raw.extend_from_slice(raw_line);
                raw.push(utf16::unit(b'\n'));
                src = utf16::substring(src, raw_line.len() + 1);
                line = utf16::substring(&next_line_without_tabs, indent).to_vec();
            }
        }

        if !list.loose {
            // If the previous item ended with a blank line, the list is loose.
            if ends_with_blank_line {
                list.loose = true;
            } else if ends_in_blank_line(&raw) {
                ends_with_blank_line = true;
            }
        }

        let mut checked = None;
        let task = task(&item_contents).map(<[u16]>::to_vec);
        if let Some(task) = &task {
            checked = Some(!utf16::is(task, "[ ] "));
            item_contents = without_task(&item_contents);
        }

        let marker = utf16::slice(bullet, 0, Some(-1));
        let value = match list_type {
            ListType::Unordered => None,
            ListType::Decimal => Some(parse_int(marker)),
            ListType::LowerLetter | ListType::UpperLetter => Some(letter_to_int(marker)),
            ListType::LowerRoman | ListType::UpperRoman => Some(roman_to_int(marker)),
        };

        if list.start.is_none() && is_ordered {
            list.start = value;
        }

        list_raw.extend_from_slice(&raw);
        list.items.push(Token {
            data: TokenData::ListItem {
                task: task.is_some(),
                checked,
                loose: false,
            },
            text: Some(Units::from(item_contents)),
            tokens: Some(Tokens::default()),
            ..Token::new("list_item", raw)
        });
    }

    // Don't consume the newlines at the end of the last item.
    let Some(last) = list.items.last_mut() else {
        return Err(JsError::type_error(
            "undefined is not an object (evaluating 'list.items[list.items.length - 1].raw')",
        ));
    };
    last.raw = last.raw.slice_of(utf16::trim_end(&last.raw));
    let text = last.text.clone().unwrap_or_default();
    last.text = Some(text.slice_of(utf16::trim_end(&text)));
    let list_raw = utf16::trim_end(&list_raw).to_vec();

    // The items' tokens come last, since the last item had to be trimmed first.
    for item in &mut list.items {
        lexer.state.top = false;
        let mut tokens = Vec::new();
        let text = item.text.clone().unwrap_or_default();
        stack::grow(|| lexer.block_tokens(&text, &mut tokens, false))?;
        if !list.loose {
            // The list is loose if an item holds a blank line.
            list.loose = tokens
                .iter()
                .any(|token| token.kind == "space" && has_newlines_in_a_row(&token.raw));
        }
        item.tokens = Some(tokens.into());
    }

    if list.loose {
        for item in &mut list.items {
            if let TokenData::ListItem { loose, .. } = &mut item.data {
                *loose = true;
            }
        }
    }

    Ok(Some(Token {
        data: TokenData::List(Box::new(list)),
        ..Token::new("list", list_raw)
    }))
}

#[cfg(test)]
mod tests {
    use super::ENDERS;
    use tarnish_js::random::check_same;
    use tarnish_js::regexp::RegExp;

    #[test]
    fn may_be_list_passes_its_matches() {
        tarnish_js::random::check_may(
            &super::RULE,
            super::may_be_list,
            &[
                " ",
                "  ",
                "*",
                "-",
                "+",
                "1",
                "12",
                "1234567890",
                "a",
                "Z",
                "I",
                "iv",
                "XC",
                "The",
                ".",
                ")",
                "\t",
                "\n",
                "é",
                "😀",
                "\u{A0}",
                "#",
            ],
        );
    }

    const LINE: &[&str] = &[
        " ", "  ", "\t", "\n", "\r", "\u{2028}", "a", "[", "]", "x", "X", "[x] ", "[ ] ", "```",
        "~~~", "#", "<", ">", "<a", "<B", "<1", "-", "_", "*", "+", "1.", "a)", "é", "&",
    ];

    fn matched(source: &str, flags: &str) -> impl Fn(&[u16]) -> Option<Vec<u16>> + use<> {
        let regex = RegExp::new(source, flags);
        move |units| regex.exec(units).map(|found| found.all().to_vec())
    }

    #[test]
    fn tabs_and_spaces_match_their_regexes() {
        let tab = RegExp::new(r"\t", "g");
        check_same(
            LINE,
            20_000,
            |line| super::replace_tabs(line).to_vec(),
            |line| tab.replace(line, "    "),
        );
        let tabs = RegExp::new(r"^\t+", "");
        check_same(LINE, 20_000, super::leading_tabs_to_spaces, |line| {
            tabs.replace_with(line, |tabs| vec![super::SPACE; 3 * tabs.all().len()])
        });
        let non_space = RegExp::new("[^ ]", "");
        check_same(LINE, 20_000, super::first_non_space, |line| {
            non_space.exec(line).map(|found| found.index())
        });
        let blank = matched(r"^[ \t]*$", "");
        check_same(LINE, 20_000, super::is_blank, |line| blank(line).is_some());
    }

    #[test]
    fn items_match_their_regexes() {
        let blank_line = matched(r"\n[ \t]*\n[ \t]*$", "");
        check_same(LINE, 50_000, super::ends_in_blank_line, |raw| {
            blank_line(raw).is_some()
        });
        let task = matched(r"^\[[ xX]] ", "");
        check_same(
            LINE,
            20_000,
            |item| super::task(item).map(<[u16]>::to_vec),
            task,
        );
        let marker = RegExp::new(r"^\[[ xX]] +", "");
        check_same(
            &["[x] ", "[ ] ", " ", "a", "\n"],
            20_000,
            |item| super::task(item).map(|_| super::without_task(item)),
            |item| super::task(item).map(|_| marker.replace(item, "")),
        );
        let lines = matched(r"\n.*\n", "");
        check_same(LINE, 50_000, super::has_newlines_in_a_row, |raw| {
            lines(raw).is_some()
        });
    }

    #[test]
    fn enders_match_their_regexes() {
        for (spaces, enders) in ENDERS.iter().enumerate() {
            let fences = matched(&format!("^ {{0,{spaces}}}(?:```|~~~)"), "");
            let heading = matched(&format!("^ {{0,{spaces}}}#"), "");
            let html = matched(&format!("^ {{0,{spaces}}}<[a-z].*>"), "i");
            let is_fences = |line: &[u16]| fences(line).is_some();
            check_same(LINE, 20_000, |line| enders.fences_begin(line), is_fences);
            let is_heading = |line: &[u16]| heading(line).is_some();
            check_same(LINE, 20_000, |line| enders.heading_begin(line), is_heading);
            let is_html = |line: &[u16]| html(line).is_some();
            check_same(LINE, 20_000, |line| enders.html_begin(line), is_html);
            check_same(
                LINE,
                20_000,
                |line| enders.next_bullet(line),
                |line| enders.next_bullet.test(line),
            );
            check_same(
                LINE,
                20_000,
                |line| enders.hr(line),
                |line| enders.hr.test(line),
            );
        }
    }
}
