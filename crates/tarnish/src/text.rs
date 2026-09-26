use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

/// A string as JavaScript holds one. A document's positions count its UTF-16 units, so a step
/// from a browser lands where it did there, even when it splits a surrogate pair. Text is kept
/// as UTF-8, with its length in units, and as the units themselves only when it holds a lone
/// surrogate, which UTF-8 can't.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Text(Repr);

/// Which representation a text has depends only on its units, so equal texts have the same one.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Utf8 { text: Arc<str>, length: usize },
    Utf16(Arc<[u16]>),
}

impl Text {
    fn utf8(text: &str, length: usize) -> Text {
        Text(Repr::Utf8 {
            text: text.into(),
            length,
        })
    }

    /// The text of these units: UTF-8 unless they hold a lone surrogate.
    pub fn from_units(units: &[u16]) -> Text {
        match String::from_utf16(units) {
            Ok(text) => Text::utf8(&text, units.len()),
            Err(_) => Text(Repr::Utf16(units.into())),
        }
    }

    /// The length in UTF-16 units, JavaScript's `length`.
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Utf8 { length, .. } => *length,
            Repr::Utf16(units) => units.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn units(&self) -> Cow<'_, [u16]> {
        match &self.0 {
            Repr::Utf8 { text, .. } => Cow::Owned(text.encode_utf16().collect()),
            Repr::Utf16(units) => Cow::Borrowed(units),
        }
    }

    pub fn first_unit(&self) -> Option<u16> {
        match &self.0 {
            Repr::Utf8 { text, .. } => text.chars().next().map(|first| {
                let mut units = [0; 2];
                first.encode_utf16(&mut units)[0]
            }),
            Repr::Utf16(units) => units.first().copied(),
        }
    }

    pub fn last_unit(&self) -> Option<u16> {
        match &self.0 {
            Repr::Utf8 { text, .. } => text.chars().next_back().map(|last| {
                let mut units = [0; 2];
                let units = last.encode_utf16(&mut units);
                units[units.len() - 1]
            }),
            Repr::Utf16(units) => units.last().copied(),
        }
    }

    /// The units the text is held in, widened: its UTF-8 bytes, or its UTF-16 units when it has
    /// a lone surrogate. An ASCII character is one unit of either, and no unit of another
    /// character is ASCII, so a scan for ASCII characters can read these.
    pub(crate) fn held_units(&self) -> impl DoubleEndedIterator<Item = u16> + '_ {
        let (bytes, units): (&[u8], &[u16]) = match &self.0 {
            Repr::Utf8 { text, .. } => (text.as_bytes(), &[]),
            Repr::Utf16(units) => (&[], units),
        };
        let bytes = bytes.iter().map(|&byte| u16::from(byte));
        bytes.chain(units.iter().copied())
    }

    /// The text, when it holds no lone surrogate.
    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            Repr::Utf8 { text, .. } => Some(text),
            Repr::Utf16(_) => None,
        }
    }

    /// The text as Rust holds strings, with a lone surrogate as U+FFFD.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match &self.0 {
            Repr::Utf8 { text, .. } => Cow::Borrowed(text),
            Repr::Utf16(units) => Cow::Owned(String::from_utf16_lossy(units)),
        }
    }

    /// The units from `from` to `to`, clamped to the text, as `String.prototype.slice` takes
    /// them.
    pub fn slice(&self, from: usize, to: usize) -> Text {
        let to = to.min(self.len());
        let from = from.min(to);
        if from == 0 && to == self.len() {
            return self.clone();
        }
        if let Repr::Utf8 { text, .. } = &self.0
            && let (Some(start), Some(end)) = (byte_offset(text, from), byte_offset(text, to))
        {
            return Text::utf8(&text[start..end], to - from);
        }
        Text::from_units(&self.units()[from..to])
    }

    /// The part of the text between offsets into the units it is held in, which is `length`
    /// UTF-16 units long.
    fn part(&self, raw: Range<usize>, length: usize) -> Text {
        match &self.0 {
            Repr::Utf8 { text, .. } => Text::utf8(&text[raw], length),
            Repr::Utf16(units) => Text::from_units(&units[raw]),
        }
    }

    pub fn concat(&self, other: &Text) -> Text {
        match (&self.0, &other.0) {
            (
                Repr::Utf8 {
                    text: a,
                    length: la,
                },
                Repr::Utf8 {
                    text: b,
                    length: lb,
                },
            ) => {
                let mut text = String::with_capacity(a.len() + b.len());
                text.push_str(a);
                text.push_str(b);
                Text::utf8(&text, la + lb)
            }
            // Two lone surrogates may pair up.
            _ => Text::from_units(&[&*self.units(), &*other.units()].concat()),
        }
    }

    pub fn ptr_eq(&self, other: &Text) -> bool {
        match (&self.0, &other.0) {
            (Repr::Utf8 { text: a, .. }, Repr::Utf8 { text: b, .. }) => Arc::ptr_eq(a, b),
            (Repr::Utf16(a), Repr::Utf16(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `JSON.stringify(text)`.
    pub fn to_json_string(&self) -> String {
        let mut out = String::with_capacity(self.len() + 2);
        match &self.0 {
            Repr::Utf8 { text, .. } => crate::js::json::write_string(&mut out, text),
            Repr::Utf16(units) => {
                out.push('"');
                for decoded in char::decode_utf16(units.iter().copied()) {
                    match decoded {
                        Ok('"') => out.push_str("\\\""),
                        Ok('\\') => out.push_str("\\\\"),
                        Ok('\u{8}') => out.push_str("\\b"),
                        Ok('\u{c}') => out.push_str("\\f"),
                        Ok('\n') => out.push_str("\\n"),
                        Ok('\r') => out.push_str("\\r"),
                        Ok('\t') => out.push_str("\\t"),
                        Ok(control) if (control as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", control as u32));
                        }
                        Ok(character) => out.push(character),
                        Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
                    }
                }
                out.push('"');
            }
        }
        out
    }
}

/// The byte offset of UTF-16 offset `units` in `text`, or `None` when it falls between the two
/// units of a surrogate pair.
fn byte_offset(text: &str, units: usize) -> Option<usize> {
    // Through ASCII, a unit is a byte.
    let prefix = &text.as_bytes()[..units.min(text.len())];
    if prefix.is_ascii() && units <= text.len() {
        return Some(units);
    }
    let mut seen = 0;
    for (offset, character) in text.char_indices() {
        if seen >= units {
            return (seen == units).then_some(offset);
        }
        seen += character.len_utf16();
    }
    (seen == units).then_some(text.len())
}

/// Whether a UTF-16 unit is whitespace to JavaScript's `\s`.
pub(crate) fn is_js_space(unit: u16) -> bool {
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

/// Whether the text is all JavaScript whitespace: `!/\S/.test(text)`.
pub(crate) fn is_blank(text: &Text) -> bool {
    match &text.0 {
        Repr::Utf8 { text, .. } => text
            .chars()
            .all(|character| character.len_utf16() == 1 && is_js_space(character as u16)),
        Repr::Utf16(units) => units.iter().all(|&unit| is_js_space(unit)),
    }
}

/// The line breaks in a text, `\r\n`, `\r` and `\n`, each as its offset in UTF-16 units and its
/// length.
pub(crate) fn line_breaks(text: &Text) -> impl Iterator<Item = (usize, usize)> + '_ {
    LineBreaks::new(text).map(|found| (found.unit, found.len))
}

/// `text.split(/\r?\n|\r/)`.
pub(crate) fn split_lines(text: &Text) -> impl Iterator<Item = Text> + '_ {
    let mut breaks = LineBreaks::new(text);
    let mut line = Some((0, 0));
    std::iter::from_fn(move || {
        let (raw, unit) = line?;
        let found = breaks.next();
        line = found.map(|found| (found.raw + found.len, found.unit + found.len));
        Some(match found {
            Some(found) => text.part(raw..found.raw, found.unit - unit),
            None => text.part(raw..breaks.raw.len(), text.len() - unit),
        })
    })
}

/// `text.replace(/\r?\n|\r/g, with)`.
pub(crate) fn replace_line_breaks(text: &Text, with: char) -> Text {
    let mut breaks = LineBreaks::new(text).peekable();
    if breaks.peek().is_none() {
        return text.clone();
    }
    match &text.0 {
        Repr::Utf8 { text, length } => {
            let mut replaced = String::with_capacity(text.len());
            let (mut from, mut length) = (0, *length);
            for found in breaks {
                replaced.push_str(&text[from..found.raw]);
                replaced.push(with);
                from = found.raw + found.len;
                length = length - found.len + with.len_utf16();
            }
            replaced.push_str(&text[from..]);
            Text::utf8(&replaced, length)
        }
        Repr::Utf16(units) => {
            let (mut replaced, mut from) = (Vec::with_capacity(units.len()), 0);
            for found in breaks {
                replaced.extend_from_slice(&units[from..found.raw]);
                replaced.extend(with.encode_utf16(&mut [0; 2]).iter());
                from = found.raw + found.len;
            }
            replaced.extend_from_slice(&units[from..]);
            Text::from_units(&replaced)
        }
    }
}

/// A line break, where it starts in UTF-16 units and in the units the text is held in, and
/// its length, the same in both as line breaks are ASCII.
#[derive(Clone, Copy)]
struct LineBreak {
    unit: usize,
    raw: usize,
    len: usize,
}

/// The units a text is held in: UTF-8 bytes, or UTF-16 units for a text with a lone surrogate.
#[derive(Clone, Copy)]
enum Raw<'a> {
    Utf8(&'a [u8]),
    Utf16(&'a [u16]),
}

impl Raw<'_> {
    fn len(self) -> usize {
        match self {
            Raw::Utf8(bytes) => bytes.len(),
            Raw::Utf16(units) => units.len(),
        }
    }

    /// The unit at `index`, and how many UTF-16 units it starts: none for a byte that continues
    /// a UTF-8 character, and two for the first of a four-byte one.
    fn at(self, index: usize) -> (u16, usize) {
        match self {
            Raw::Utf8(bytes) => {
                let byte = bytes[index];
                let width = match byte {
                    0x80..=0xbf => 0,
                    0xf0.. => 2,
                    _ => 1,
                };
                (byte.into(), width)
            }
            Raw::Utf16(units) => (units[index], 1),
        }
    }
}

struct LineBreaks<'a> {
    raw: Raw<'a>,
    /// The next unit to scan, as held and in UTF-16 units.
    next: usize,
    unit: usize,
}

impl<'a> LineBreaks<'a> {
    fn new(text: &'a Text) -> Self {
        let raw = match &text.0 {
            Repr::Utf8 { text, .. } => Raw::Utf8(text.as_bytes()),
            Repr::Utf16(units) => Raw::Utf16(units),
        };
        let none = match raw {
            Raw::Utf8(bytes) => !bytes.iter().any(|&byte| byte == b'\r' || byte == b'\n'),
            Raw::Utf16(_) => false,
        };
        LineBreaks {
            raw,
            next: if none { raw.len() } else { 0 },
            unit: 0,
        }
    }
}

impl Iterator for LineBreaks<'_> {
    type Item = LineBreak;

    fn next(&mut self) -> Option<LineBreak> {
        while self.next < self.raw.len() {
            let (value, width) = self.raw.at(self.next);
            let (raw, unit) = (self.next, self.unit);
            self.next += 1;
            self.unit += width;
            let len = match value {
                0x0a => 1,
                0x0d if self.next < self.raw.len() && self.raw.at(self.next).0 == 0x0a => {
                    self.next += 1;
                    self.unit += 1;
                    2
                }
                0x0d => 1,
                _ => continue,
            };
            return Some(LineBreak { unit, raw, len });
        }
        None
    }
}

impl Default for Text {
    fn default() -> Self {
        static EMPTY: LazyLock<Text> = LazyLock::new(|| Text::utf8("", 0));
        EMPTY.clone()
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        let length = if text.is_ascii() {
            text.len()
        } else {
            text.chars().map(char::len_utf16).sum()
        };
        Text::utf8(text, length)
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text::from(text.as_str())
    }
}

impl From<Vec<u16>> for Text {
    fn from(units: Vec<u16>) -> Self {
        Text::from_units(&units)
    }
}

impl From<&[u16]> for Text {
    fn from(units: &[u16]) -> Self {
        Text::from_units(units)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_json_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{Text, line_breaks, replace_line_breaks, split_lines};

    /// Finding line breaks, and splitting and replacing at them, agree with a scan of the units,
    /// for text held as UTF-8 and for text with a lone surrogate, held as units.
    #[test]
    fn finds_line_breaks_as_units() {
        let texts = [
            Text::from("a\r\nb\rc\n\n😀é\r"),
            Text::from("\n"),
            Text::from("é😀"),
            Text::from(""),
            Text::from_units(&[0xd800, 0x0d, 0x0a, 0x61, 0x0d]),
        ];
        for text in texts {
            let units = text.units().into_owned();
            let mut breaks = Vec::new();
            let mut index = 0;
            while index < units.len() {
                let len = match (units[index], units.get(index + 1)) {
                    (0x0d, Some(0x0a)) => 2,
                    (0x0d | 0x0a, _) => 1,
                    _ => 0,
                };
                if len > 0 {
                    breaks.push((index, len));
                }
                index += len.max(1);
            }
            assert_eq!(line_breaks(&text).collect::<Vec<_>>(), breaks, "{text:?}");
            let mut lines = Vec::new();
            let mut from = 0;
            for &(at, len) in &breaks {
                lines.push(Text::from_units(&units[from..at]));
                from = at + len;
            }
            lines.push(Text::from_units(&units[from..]));
            assert_eq!(split_lines(&text).collect::<Vec<_>>(), lines, "{text:?}");
            let joined = lines.iter().skip(1).fold(lines[0].clone(), |joined, line| {
                joined.concat(&Text::from(" ")).concat(line)
            });
            assert_eq!(replace_line_breaks(&text, ' '), joined, "{text:?}");
            assert_eq!(text.first_unit(), units.first().copied());
            assert_eq!(text.last_unit(), units.last().copied());
        }
    }

    /// Slicing and joining text as UTF-8 agrees with doing it on the units, wherever the cuts
    /// fall, surrogate pairs included.
    #[test]
    fn slices_and_joins_as_units() {
        let text = Text::from("aé😀b😀");
        let units = text.units().into_owned();
        for from in 0..=units.len() {
            for to in from..=units.len() {
                let slice = text.slice(from, to);
                assert_eq!(slice, Text::from_units(&units[from..to]), "{from}..{to}");
                assert_eq!(slice.len(), to - from);
                for cut in 0..=slice.len() {
                    let joined = slice.slice(0, cut).concat(&slice.slice(cut, slice.len()));
                    assert_eq!(joined, slice);
                }
            }
        }
    }
}
