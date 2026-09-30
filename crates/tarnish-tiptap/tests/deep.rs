//! Documents, HTML and Markdown nested far deeper than a small stack could recurse through,
//! converted with Tiptap's own extensions on a thread with such a stack: each recursion over
//! nesting has to grow the stack. Parsing takes time that grows with the square of the depth,
//! so parses nest less deeply.

mod own;

use tarnish::Node;
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{self, Value};
use tarnish_js::Error;
use tarnish_tiptap::html;

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
const INLINES: usize = 500;

/// `node_type` inside itself, `levels` deep, around a text node.
fn nested(node_type: &str, levels: usize) -> Value {
    let text = json::json!({"type": "text", "text": "x"});
    let nested = (0..levels).fold(
        text,
        |inner, _| json::json!({"type": node_type, "content": [inner]}),
    );
    json::json!({"type": "doc", "content": [nested]})
}

fn converts<T>(what: &str, result: Result<T, Error>) {
    if let Err(error) = result {
        panic!("{what}: {}", error.message());
    }
}

#[test]
fn serializes_documents_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let schema = own::schema();
        let doc = Node::from_json(&schema.schema, &nested("paragraph", DOCUMENTS));
        converts(
            "HTML of paragraphs",
            doc.and_then(|doc| html::serialize(&schema, &doc)),
        );
        let markdown = own::markdown();
        // The manager renders a node of any type it has a renderer for, a mark's among them.
        for node_type in ["doc", "paragraph", "bold", "italic", "strike"] {
            converts(
                &format!("Markdown of {node_type}"),
                markdown.serialize(&nested(node_type, DOCUMENTS)),
            );
        }
    });
}

#[test]
fn parses_html_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let schema = own::schema();
        for tag in ["div", "p", "b", "span style=color:red"] {
            let html = format!("<{tag}>").repeat(PARSES) + "x";
            converts(&format!("<{tag}>"), html::parse(&schema, &html));
        }
    });
}

/// Marks inside blockquotes run their recursion on top of the blockquotes'.
#[test]
fn parses_markdown_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let markdown = own::markdown();
        let quoted = |inside: String| ">".repeat(PARSES) + " " + &inside;
        let delimited =
            |open: &str, close: &str| open.repeat(INLINES) + "x" + &close.repeat(INLINES);
        let nested = [
            ("blockquotes", quoted("x".into())),
            ("list items", "1. ".repeat(PARSES) + "x"),
            ("strong", quoted(delimited("**a ", " a**"))),
            ("em", quoted(delimited("*a ", " a*"))),
            ("del", quoted(delimited("~~a ", " a~~"))),
        ];
        for (what, input) in nested {
            converts(what, markdown.parse(&input));
        }
    });
}
