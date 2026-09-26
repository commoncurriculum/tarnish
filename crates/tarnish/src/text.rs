use std::borrow::Cow;
use std::fmt;
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
    use super::Text;

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
