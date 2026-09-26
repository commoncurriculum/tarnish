use std::fmt;
use std::sync::{Arc, LazyLock};

/// A string of UTF-16 code units, as JavaScript holds one. A document's positions count these
/// units, so text is kept in them: a step from a browser lands where it did there, even when it
/// splits a surrogate pair.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Text(Arc<[u16]>);

impl Text {
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The units from `from` to `to`, clamped to the text, as `String.prototype.slice` takes
    /// them.
    pub fn slice(&self, from: usize, to: usize) -> Text {
        let to = to.min(self.len());
        let from = from.min(to);
        if from == 0 && to == self.len() {
            return self.clone();
        }
        Text(Arc::from(&self.0[from..to]))
    }

    pub fn concat(&self, other: &Text) -> Text {
        let mut units = Vec::with_capacity(self.len() + other.len());
        units.extend_from_slice(&self.0);
        units.extend_from_slice(&other.0);
        Text(units.into())
    }

    pub fn ptr_eq(&self, other: &Text) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The text as Rust holds strings, with a lone surrogate as U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    /// `JSON.stringify(text)`.
    pub fn to_json_string(&self) -> String {
        let mut out = String::with_capacity(self.len() + 2);
        out.push('"');
        for decoded in char::decode_utf16(self.0.iter().copied()) {
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
        out
    }
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
        static EMPTY: LazyLock<Text> = LazyLock::new(|| Text(Arc::from(Vec::new())));
        EMPTY.clone()
    }
}

impl From<&str> for Text {
    fn from(string: &str) -> Self {
        Text(string.encode_utf16().collect::<Vec<_>>().into())
    }
}

impl From<String> for Text {
    fn from(string: String) -> Self {
        Text::from(string.as_str())
    }
}

impl From<Vec<u16>> for Text {
    fn from(units: Vec<u16>) -> Self {
        Text(units.into())
    }
}

impl From<&[u16]> for Text {
    fn from(units: &[u16]) -> Self {
        Text(units.into())
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
