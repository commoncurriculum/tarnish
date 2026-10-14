//! What @tiptap/markdown's `MarkdownManager` and ProseMirror's `DOMParser` and `DOMSerializer` do
//! with Tiptap's extensions, recorded by `harness/record-tiptap.mjs`: tarnish-tiptap's must make
//! the same schemas, documents, Markdown and HTML, or fail with the same error.

mod own;

use own::{Converter, OPTIONED, input_of, named};
use tarnish_fixtures::{read, records, same, version};

/// Holds each record of `section` to what its conversion makes of the record's input, with the
/// extensions the record names.
fn check(section: &str) {
    let fixtures = read("tiptap");
    let names = [None, Some("colors"), Some("code"), Some("renderedValues")];
    let names = names.into_iter().chain(OPTIONED.map(Some));
    let converters: Vec<_> = names.map(|name| (name, Converter::new(name))).collect();
    let mut failures = Vec::new();
    for mut record in records(&fixtures, section).to_vec() {
        let (_, converter) = converters
            .iter()
            .find(|(name, _)| *name == named(&record))
            .expect("the extensions");
        let fields = record.as_object_mut().expect("a record");
        let input = fields.remove(input_of(section)).expect("an input");
        fields.remove("extensions");
        let actual = converter.convert(section, &input);
        if !same(&record, &actual) {
            failures.push(format!(
                "{input}\n  Tiptap: {record}\n  tarnish-tiptap: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Each Tiptap package installed, the extensions among them, is the version ported.
#[test]
fn ports_the_installed_tiptap() {
    let versions = read("versions");
    let tiptap = versions.as_object().expect("versions").keys();
    let tiptap: Vec<_> = tiptap
        .filter(|key| key.starts_with("@tiptap/") && !key.contains(" > "))
        .collect();
    assert!(tiptap.len() > 1, "{tiptap:?}");
    for package in tiptap {
        assert_eq!(version(package), tarnish_tiptap::TIPTAP, "{package}");
    }
    let flat_list = version("tiptap-extension-flat-list");
    assert_eq!(flat_list, tarnish_tiptap::FLAT_LIST);
}

#[test]
fn builds_the_schemas_tiptap_does() {
    own::check_schemas("tiptap", Converter::new);
}

#[test]
fn parses_markdown_as_tiptap_does() {
    check("parsesMarkdown");
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    check("serializesMarkdown");
}

#[test]
fn parses_html_as_tiptap_does() {
    check("parsesHTML");
}

#[test]
fn serializes_html_as_tiptap_does() {
    check("serializesHTML");
}
