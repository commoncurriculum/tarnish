//! What @tiptap/markdown's `MarkdownManager` and ProseMirror's `DOMParser` and `DOMSerializer` do
//! with Tiptap's own extensions, recorded by `harness/record-tiptap.mjs`: tarnish-tiptap's must
//! make the same documents, Markdown and HTML, or fail with the same error.

mod own;

use tarnish::Node;
use tarnish::json::{Value, json};
use tarnish_fixtures::{outcome, read};
use tarnish_js::Error;
use tarnish_tiptap::html;

/// Holds each record of `section` to what `convert` makes of its input, the record's `from`,
/// as the record has it under `to`, or as its error.
fn check<T: Into<Value>>(
    section: &str,
    from: &str,
    to: &str,
    convert: impl Fn(&Value) -> Result<T, Error>,
) {
    let mut fixtures = read("tiptap");
    let mut failures = Vec::new();
    for mut record in fixtures[section].take().into_array().expect("the records") {
        let input = record.as_object_mut().expect("a record").remove(from);
        let input = input.expect("an input");
        let actual = outcome(convert(&input).map(|output| json!({ to: output.into() })));
        if actual != record {
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

fn text(input: &Value) -> &str {
    input.as_str().expect("text")
}

#[test]
fn ports_the_installed_tiptap() {
    assert_eq!(
        read("tiptap")["tiptap"].as_str(),
        Some(tarnish_tiptap::TIPTAP)
    );
}

#[test]
fn parses_markdown_as_tiptap_does() {
    let markdown = own::markdown();
    check("parsesMarkdown", "markdown", "doc", |input| {
        markdown.parse(text(input))
    });
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    let markdown = own::markdown();
    check("serializesMarkdown", "doc", "markdown", |doc| {
        markdown.serialize(doc)
    });
}

#[test]
fn parses_html_as_tiptap_does() {
    let schema = own::schema();
    check("parsesHTML", "html", "doc", |input| {
        html::parse(&schema, text(input)).map(|doc| doc.to_json())
    });
}

#[test]
fn serializes_html_as_tiptap_does() {
    let schema = own::schema();
    check("serializesHTML", "doc", "html", |doc| {
        Node::from_json(&schema.schema, doc).and_then(|doc| html::serialize(&schema, &doc))
    });
}
