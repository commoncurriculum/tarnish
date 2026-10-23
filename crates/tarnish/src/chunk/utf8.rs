//! A chunk's strings and text as UTF-8: checked as the chunk loads, or span by span as they
//! are read.

use std::borrow::Cow;

use super::{SECTIONS, STRINGS, TEXT, WIDTHS, corrupt};

/// A section of UTF-8, or `None` when it isn't. simdutf8 checks a section of a large document
/// several times as fast as `str::from_utf8` does, which loading a chunk spent most of its time
/// in.
fn utf8_section<'b>(
    bytes: &'b [u8],
    (starts, counts): (&[u32; SECTIONS], &[u32; SECTIONS]),
    section: usize,
) -> Option<&'b str> {
    let start = starts[section] as usize;
    let end = start + counts[section] as usize * WIDTHS[section];
    simdutf8::basic::from_utf8(bytes.get(start..end)?).ok()
}

/// A chunk's strings or text section: checked as UTF-8 once, so that a read only slices it,
/// or checked span by span as it is read, which costs less when a call reads a few spans of a
/// large chunk, as a step does.
pub(super) enum Utf8<'a> {
    Checked(Cow<'a, str>),
    OnRead,
}

/// The most bytes of strings and text a chunk loaded to be checked where it is read checks
/// as it loads all the same.
pub(super) const CHECKED_WHOLE: u32 = 4096;

/// When a chunk loaded checks its strings and text are UTF-8.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// As it loads, for a call that reads much of it.
    Whole,
    /// As each span is read, for a call that reads little of it. A span that isn't UTF-8 then
    /// panics, as a corrupt record does.
    OnRead,
}

/// The strings and text sections of a chunk's bytes, borrowed when the bytes are.
pub(super) fn utf8_sections<'a>(
    bytes: &Cow<'a, [u8]>,
    sections: (&[u32; SECTIONS], &[u32; SECTIONS]),
) -> Option<(Utf8<'a>, Utf8<'a>)> {
    Some(match bytes {
        Cow::Borrowed(bytes) => (
            Utf8::Checked(Cow::Borrowed(utf8_section(bytes, sections, STRINGS)?)),
            Utf8::Checked(Cow::Borrowed(utf8_section(bytes, sections, TEXT)?)),
        ),
        Cow::Owned(bytes) => (
            Utf8::Checked(Cow::Owned(
                utf8_section(bytes, sections, STRINGS)?.to_owned(),
            )),
            Utf8::Checked(Cow::Owned(utf8_section(bytes, sections, TEXT)?.to_owned())),
        ),
    })
}

/// A span of bytes as UTF-8, which panics when it isn't, as a corrupt record does.
#[inline]
pub(super) fn utf8(span: &[u8]) -> &str {
    std::str::from_utf8(span).unwrap_or_else(|_| corrupt())
}

/// `len` bytes of a section of UTF-8 from `start`.
#[inline]
pub(super) fn str_span(section: &str, start: u32, len: u32) -> &str {
    let start = start as usize;
    section
        .get(start..start + len as usize)
        .unwrap_or_else(|| corrupt())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::Check;
    use crate::chunk::{Chunk, STRINGS, TEXT, layout};
    use crate::{Error, Node, api, json};

    /// A chunk's strings are checked as it loads, so one whose strings aren't UTF-8 is refused
    /// then rather than on the read that would come across them.
    #[test]
    fn refuses_a_chunk_whose_strings_are_not_utf8() {
        let spec = r#"{"nodes": {"doc": {"content": "text*", "attrs": {"a": {"default": "x"}}},
                                 "text": {}}}"#;
        let schema = api::schema(&json::from_str(spec).expect("JSON")).expect("a schema");
        let doc = json::from_str(r#"{"type": "doc", "attrs": {"a": "é"}}"#).expect("JSON");
        let doc = Node::from_json(&schema, &doc).expect("a document");
        let mut bytes = doc.chunk().bytes().to_vec();
        let (starts, counts) = layout(&bytes).expect("a layout");
        assert!(counts[STRINGS] > 0);
        assert!(Chunk::load(Cow::Borrowed(&bytes), &schema, Vec::new()).is_ok());
        bytes[starts[STRINGS] as usize] = 0xff;
        let refused = Chunk::load(Cow::Owned(bytes), &schema, Vec::new()).err();
        assert!(refused == Some(Error::Range("Invalid document chunk".into())));
    }

    /// A chunk loaded to check its text where it is read loads with strings that aren't UTF-8,
    /// and the read that comes across them panics, as one of a corrupt record does. A small
    /// chunk is checked as it loads all the same, and refused.
    #[test]
    fn checks_strings_where_read_when_loaded_so() {
        let spec = r#"{"nodes": {"doc": {"content": "text*", "attrs": {"a": {"default": "x"}}},
                                 "text": {}}}"#;
        let schema = api::schema(&json::from_str(spec).expect("JSON")).expect("a schema");
        for (text, small) in [
            ("é".to_owned(), true),
            (format!("é{}", "x".repeat(5000)), false),
        ] {
            let doc = json::json!({ "type": "doc", "attrs": { "a": "é" }, "content": [{ "type": "text", "text": text }] });
            let doc = Node::from_json(&schema, &doc).expect("a document");
            let good = doc.chunk().bytes().to_vec();
            let loaded =
                Chunk::load_checking(Cow::Borrowed(&good), &schema, Vec::new(), Check::OnRead);
            let loaded = Node::root(std::sync::Arc::new(loaded.expect("a chunk"))).expect("a node");
            assert_eq!(loaded.to_json(), doc.to_json());
            for section in [STRINGS, TEXT] {
                let mut bytes = good.clone();
                let (starts, counts) = layout(&bytes).expect("a layout");
                assert!(counts[section] > 0);
                bytes[starts[section] as usize] = 0xff;
                let chunk =
                    Chunk::load_checking(Cow::Borrowed(&bytes), &schema, Vec::new(), Check::OnRead);
                assert!(Chunk::load(Cow::Borrowed(&bytes), &schema, Vec::new()).is_err());
                if small {
                    assert!(chunk.is_err());
                    continue;
                }
                let node =
                    Node::root(std::sync::Arc::new(chunk.expect("a chunk"))).expect("a node");
                let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    node.to_json().to_value()
                }));
                assert!(read.is_err());
            }
        }
    }
}
