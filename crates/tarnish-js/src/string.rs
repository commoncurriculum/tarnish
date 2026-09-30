use memchr::memmem;

use crate::{Error, Result, deadline, number_to_string};

/// What JavaScript throws building a string longer than it holds, as tarnish throws building one
/// longer than memory holds.
pub fn invalid_string_length() -> Error {
    Error::Range("Invalid string length".into())
}

/// A string's `length` in JavaScript, which counts UTF-16 code units.
pub fn utf16_len(string: &str) -> usize {
    str_indices::utf16::count(string)
}

/// The byte offset of UTF-16 offset `units` in `string`, clamped to the string, and rounded up
/// past a character that the offset would split.
pub fn byte_offset(string: &str, units: usize) -> usize {
    let byte = str_indices::utf16::to_byte_idx(string, units);
    match splits_pair(string, units, byte) {
        true => byte + 4,
        false => byte,
    }
}

/// The byte offset of UTF-16 offset `units` in `string`, or `None` when it is past the end or
/// falls between the two units of a surrogate pair.
pub fn exact_byte_offset(string: &str, units: usize) -> Option<usize> {
    let byte = str_indices::utf16::to_byte_idx(string, units);
    if byte == string.len() {
        // The offset is the end, or past it, where the one before it is the end too.
        let before_end =
            units == 0 || str_indices::utf16::to_byte_idx(string, units - 1) < string.len();
        return before_end.then_some(byte);
    }
    (!splits_pair(string, units, byte)).then_some(byte)
}

/// Whether UTF-16 offset `units`, which str_indices put at `byte`, the start of the character
/// it falls in, is the second unit of that character's surrogate pair.
fn splits_pair(string: &str, units: usize, byte: usize) -> bool {
    string
        .as_bytes()
        .get(byte)
        .is_some_and(|&lead| lead >= 0xF0)
        && units > 0
        && str_indices::utf16::to_byte_idx(string, units - 1) == byte
}

/// Whether JavaScript takes `character` for whitespace, as `trim` strips it and `\s` matches it:
/// WhiteSpace and LineTerminator, which differ from [`char::is_whitespace`].
pub fn is_whitespace(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_whitespace_unit)
}

/// [`is_whitespace`] of a UTF-16 unit.
pub fn is_whitespace_unit(unit: u16) -> bool {
    matches!(
        unit,
        0x09..=0x0D
            | 0x20
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

pub fn trim(string: &str) -> &str {
    trim_end(trim_start(string))
}

/// Whether an ASCII byte is JavaScript whitespace or a line terminator.
fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
}

pub fn trim_start(string: &str) -> &str {
    let bytes = string.as_bytes();
    let start = bytes
        .iter()
        .position(|&byte| !is_ascii_whitespace(byte))
        .unwrap_or(bytes.len());
    // Past ASCII, whitespace takes decoding.
    match bytes.get(start) {
        Some(&byte) if !byte.is_ascii() => string[start..].trim_start_matches(is_whitespace),
        _ => &string[start..],
    }
}

pub fn trim_end(string: &str) -> &str {
    let bytes = string.as_bytes();
    let end = bytes
        .iter()
        .rposition(|&byte| !is_ascii_whitespace(byte))
        .map_or(0, |last| last + 1);
    match end.checked_sub(1).map(|last| bytes[last]) {
        Some(byte) if !byte.is_ascii() => string[..end].trim_end_matches(is_whitespace),
        _ => &string[..end],
    }
}

/// Makes room in `string` for `additional` more bytes of a string built in one go, as long as
/// memory allows, or fails before any of them is built.
pub fn reserve(string: &mut String, additional: usize) -> Result<()> {
    deadline::build(additional)?;
    string
        .try_reserve_exact(additional)
        .map_err(|_| invalid_string_length())
}

/// `strings.join("")`, as long as memory allows.
pub fn concat(strings: &[&str]) -> Result<String> {
    let length = strings
        .iter()
        .try_fold(0usize, |length, string| length.checked_add(string.len()))
        .ok_or_else(invalid_string_length)?;
    let mut joined = String::new();
    reserve(&mut joined, length)?;
    strings.iter().for_each(|string| joined.push_str(string));
    Ok(joined)
}

/// `string.replaceAll(pattern, replacement)`, as long as memory allows, of a `pattern` that
/// isn't empty: JavaScript matches an empty one between the halves of a surrogate pair too, which
/// a `str` can't hold apart. The replacement's `$$`, `$&`, `` $` `` and `$'` stand for what they
/// do in JavaScript, and, with no groups to name, any other `$` for itself.
pub fn replace_all(string: &str, pattern: &str, replacement: &str) -> Result<String> {
    assert!(!pattern.is_empty(), "replace_all of an empty pattern");
    let pieces = substitution(replacement);
    let matches = memmem::Finder::new(pattern);
    let mut length = string.len();
    for at in matches.find_iter(string.as_bytes()) {
        length = pieces
            .iter()
            .try_fold(length - pattern.len(), |length, piece| {
                length.checked_add(piece.at(string, pattern, at).len())
            })
            .ok_or_else(invalid_string_length)?;
    }
    let mut replaced = String::new();
    reserve(&mut replaced, length)?;
    let mut copied = 0;
    for at in matches.find_iter(string.as_bytes()) {
        replaced.push_str(&string[copied..at]);
        for piece in &pieces {
            replaced.push_str(piece.at(string, pattern, at));
        }
        copied = at + pattern.len();
    }
    replaced.push_str(&string[copied..]);
    Ok(replaced)
}

/// A piece of a replacement as `GetSubstitution` reads it for a match of a string.
enum Piece<'r> {
    Text(&'r str),
    /// `$&`.
    Match,
    /// `` $` ``.
    Before,
    /// `$'`.
    After,
}

impl<'r> Piece<'r> {
    /// What the piece stands for at a match of `pattern` at `at` in `string`.
    fn at<'a>(&self, string: &'a str, pattern: &'a str, at: usize) -> &'a str
    where
        'r: 'a,
    {
        match *self {
            Piece::Text(text) => text,
            Piece::Match => pattern,
            Piece::Before => &string[..at],
            Piece::After => &string[at + pattern.len()..],
        }
    }
}

fn substitution(replacement: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut rest = replacement;
    while let Some(dollar) = rest.find('$') {
        let (piece, skip) = match rest.as_bytes().get(dollar + 1) {
            Some(b'$') => (Piece::Text("$"), 2),
            Some(b'&') => (Piece::Match, 2),
            Some(b'`') => (Piece::Before, 2),
            Some(b'\'') => (Piece::After, 2),
            _ => (Piece::Text("$"), 1),
        };
        pieces.push(Piece::Text(&rest[..dollar]));
        pieces.push(piece);
        rest = &rest[dollar + skip..];
    }
    pieces.push(Piece::Text(rest));
    pieces.retain(|piece| !matches!(piece, Piece::Text("")));
    pieces
}

/// `string.split(/\r?\n/)`, which leaves a `\r` that no `\n` follows.
pub fn split_crlf_lines(string: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(string);
    std::iter::from_fn(move || {
        let current = rest?;
        Some(match memchr::memchr(b'\n', current.as_bytes()) {
            Some(end) => {
                rest = Some(&current[end + 1..]);
                let line = &current[..end];
                line.strip_suffix('\r').unwrap_or(line)
            }
            None => {
                rest = None;
                current
            }
        })
    })
}

/// `string.repeat(count)`, as long as memory allows.
pub fn repeat(string: &str, count: f64) -> Result<String> {
    let times = if count.is_nan() { 0.0 } else { count.trunc() };
    if times < 0.0 || times.is_infinite() {
        return Err(Error::Range(format!(
            "Invalid count value: {}",
            number_to_string(count)
        )));
    }
    if times == 0.0 || string.is_empty() {
        return Ok(String::new());
    }
    let length = (times as usize)
        .checked_mul(string.len())
        .ok_or_else(invalid_string_length)?;
    let mut repeated = String::new();
    reserve(&mut repeated, length)?;
    repeated.push_str(string);
    while repeated.len() < length {
        repeated.extend_from_within(..repeated.len().min(length - repeated.len()));
    }
    Ok(repeated)
}

/// `a.localeCompare(b)`, which collates as ICU's root locale does, as Node's en-US does.
#[cfg(feature = "collation")]
pub fn locale_compare(a: &str, b: &str) -> f64 {
    static COLLATOR: std::sync::LazyLock<icu_collator::CollatorBorrowed<'static>> =
        std::sync::LazyLock::new(|| {
            icu_collator::Collator::try_new(Default::default(), Default::default())
                .expect("the collation data is compiled in")
        });
    match COLLATOR.compare(a, b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_utf16_offsets() {
        let text = "a😀é";
        assert_eq!(utf16_len(text), 4);
        let rounded: Vec<usize> = (0..6).map(|units| byte_offset(text, units)).collect();
        assert_eq!(rounded, [0, 1, 5, 5, 7, 7]);
        let exact: Vec<Option<usize>> =
            (0..6).map(|units| exact_byte_offset(text, units)).collect();
        assert_eq!(exact, [Some(0), Some(1), None, Some(5), Some(7), None]);
        assert_eq!(exact_byte_offset("", 0), Some(0));
        assert_eq!(exact_byte_offset("", 1), None);
        assert_eq!(exact_byte_offset("😀", 2), Some(4));
    }

    // Each as Node throws it.
    #[test]
    fn repeats_as_v8_does() {
        let thrown = |string: &str, count: f64| repeat(string, count).unwrap_err().to_string();
        assert_eq!(thrown("ab", -1.0), "RangeError: Invalid count value: -1");
        assert_eq!(
            thrown("ab", f64::INFINITY),
            "RangeError: Invalid count value: Infinity"
        );
        assert_eq!(thrown("", -1.0), "RangeError: Invalid count value: -1");
        assert_eq!(
            thrown("ab", 2f64.powi(63)),
            "RangeError: Invalid string length"
        );
        assert_eq!(
            thrown("a", 2f64.powi(63)),
            "RangeError: Invalid string length"
        );
        assert_eq!(repeat("ab", f64::NAN).unwrap(), "");
        assert_eq!(repeat("ab", -0.5).unwrap(), "");
        assert_eq!(repeat("", 2f64.powi(40)).unwrap(), "");
        assert_eq!(repeat("ab", 2.9).unwrap(), "abab");
    }

    #[test]
    fn reserves_what_memory_and_a_deadline_allow() {
        let thrown = reserve(&mut String::new(), usize::MAX).unwrap_err();
        assert_eq!(thrown.to_string(), "RangeError: Invalid string length");
        let late = crate::deadline::within(std::time::Duration::from_secs(60), || {
            reserve(&mut String::new(), 2 << 20)
        });
        assert!(late.is_none());
    }

    #[test]
    fn builds_in_one_go_what_a_deadline_allows() {
        let within = |build: &dyn Fn() -> Result<String>| {
            crate::deadline::within(std::time::Duration::from_secs(60), build)
        };
        let half = "\n".repeat(1 << 19);
        assert!(within(&|| concat(&[&half, &half])).is_some());
        assert!(within(&|| concat(&[&half, &half, "x"])).is_none());
        assert!(within(&|| replace_all(&half, "\n", "ab")).is_some());
        assert!(within(&|| replace_all(&half, "\n", "abc")).is_none());
        assert_eq!(replace_all(&half, "\n", "abc").unwrap().len(), 3 << 19);
    }

    // As V8 answers it.
    #[test]
    fn replaces_all_as_v8_does() {
        assert_eq!(
            replace_all("abcb", "b", "[$$|$&|$`|$'|$1|$<|$]").unwrap(),
            "a[$|b|a|cb|$1|$<|$]c[$|b|abc||$1|$<|$]"
        );
        assert_eq!(replace_all("aaa", "aa", "b").unwrap(), "ba");
        assert_eq!(concat(&["a", "", "bc"]).unwrap(), "abc");
    }

    #[cfg(feature = "regexp")]
    #[test]
    fn replaces_all_as_a_global_regex_does() {
        use crate::random::{check_same, strings};
        use crate::regexp::RegExp;
        // RegExp's replacements don't read `` $` `` and `$'`.
        let alphabet = &["a", "b", "\n", "\r\n", "$", "$$", "$&", "$1", "é", "😀"];
        let lossy = |units: &[u16]| String::from_utf16_lossy(units);
        for (pattern, source) in [("\n", r"\n"), ("ab", "ab"), ("$", r"\$")] {
            let regex = RegExp::new(source, "g");
            for replacement in strings(alphabet, 100) {
                let replacement = lossy(&replacement);
                check_same(
                    alphabet,
                    500,
                    |src| replace_all(&lossy(src), pattern, &replacement).unwrap(),
                    |src| {
                        let src = crate::utf16::from(&lossy(src));
                        lossy(&regex.replace(&src, &replacement))
                    },
                );
            }
        }
    }

    #[cfg(feature = "regexp")]
    #[test]
    fn splits_lines_as_the_regex_does() {
        let regex = crate::regexp::RegExp::new(r"\r?\n", "g");
        let lossy = |units: &[u16]| String::from_utf16_lossy(units);
        crate::random::check_same(
            &["a", "\r", "\n", "\r\n", "é", "\u{2028}"],
            100_000,
            |src| {
                split_crlf_lines(&lossy(src))
                    .map(String::from)
                    .collect::<Vec<_>>()
            },
            |src| {
                let src = crate::utf16::from(&lossy(src));
                regex.split(&src).into_iter().map(lossy).collect::<Vec<_>>()
            },
        );
    }
}
