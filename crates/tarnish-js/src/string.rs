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
}
