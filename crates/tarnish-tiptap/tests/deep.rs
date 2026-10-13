//! Documents, HTML and Markdown nested far deeper than a small stack could recurse through,
//! converted with Tiptap's own extensions on a thread with such a stack: each recursion over
//! nesting has to grow the stack. Parsing takes time that grows with the square of the depth
//! (the cube, when a lazy continuation line re-lexes every nested blockquote), so parses nest
//! less deeply.

mod own;

use own::{Converter, input_of, named};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Value, json};
use tarnish_fixtures::{expect, read, records, same};
use tarnish_tiptap::extensions::text_style::color;
use tarnish_tiptap::{get_schema, html};

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
const INLINES: usize = 500;
const LAZY: usize = 500;
/// How deep blocks nest in a style's CSS, which the CSS engine reads in time that grows with
/// the depth.
const CSSES: usize = 100_000;

/// An input nested `depth` deep, and what the conversion a `section` of `tiptap.json` records
/// makes of it, which the test folds from the depths below and holds to Tiptap's at the depths
/// `harness/record-tiptap.mjs` records.
struct Nested {
    what: &'static str,
    section: &'static str,
    depth: usize,
    input: fn(usize) -> Value,
    output: fn(usize) -> Value,
}

/// `node_type` inside itself, `depth` deep, around a text node.
fn nested(node_type: &str, depth: usize) -> Value {
    let text = json!({"type": "text", "text": "x"});
    let nested = (0..depth).fold(
        text,
        |inner, _| json!({"type": node_type, "content": [inner]}),
    );
    json!({"type": "doc", "content": [nested]})
}

fn around(open: &str, close: &str, depth: usize) -> String {
    open.repeat(depth) + "x" + &close.repeat(depth)
}

fn markdown(open: &str, close: &str, depth: usize) -> Value {
    json!({"markdown": around(open, close, depth)})
}

fn doc(content: Vec<Value>) -> Value {
    json!({"doc": {"type": "doc", "content": content}})
}

fn paragraph(content: Value) -> Value {
    json!({"type": "paragraph", "content": [content]})
}

/// `>` four times as deep as `depth`, around Markdown nested `depth` deep.
fn quoted(inside: String, depth: usize) -> Value {
    Value::String(">".repeat(4 * depth) + " " + &inside)
}

/// The text inside a mark's delimiters `depth` deep, each part of it with a mark for each pair
/// of delimiters around it: @tiptap/markdown's `applyMarkToContent` adds a mark however many of
/// the type the text has.
fn marked(mark: &str, depth: usize) -> Vec<Value> {
    let marks = |count: usize| vec![json!({"type": mark}); count];
    let text = |text: &str, count| json!({"type": "text", "text": text, "marks": marks(count)});
    let opening = (1..depth).map(|count| text("a ", count));
    let closing = (1..depth).rev().map(|count| text(" a", count));
    let middle = text("a x a", depth);
    opening.chain([middle]).chain(closing).collect()
}

fn paragraph_of(content: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": content})
}

const NESTED: [Nested; 17] = [
    Nested {
        what: "HTML of paragraphs",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| nested("paragraph", depth),
        output: |depth| json!({"html": around("<p>", "</p>", depth)}),
    },
    // The manager renders a node of any type it has a renderer for, a mark's among them.
    Nested {
        what: "Markdown of docs",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("doc", depth),
        output: |_| json!({"markdown": "x"}),
    },
    Nested {
        what: "Markdown of paragraphs",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("paragraph", depth),
        output: |_| json!({"markdown": "x"}),
    },
    Nested {
        what: "Markdown of bolds",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("bold", depth),
        output: |depth| markdown("**", "**", depth),
    },
    Nested {
        what: "Markdown of italics",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("italic", depth),
        output: |depth| markdown("*", "*", depth),
    },
    Nested {
        what: "Markdown of strikes",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("strike", depth),
        output: |depth| markdown("~~", "~~", depth),
    },
    Nested {
        what: "<div>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String("<div>".repeat(depth) + "x"),
        output: |_| doc(vec![paragraph(json!({"type": "text", "text": "x"}))]),
    },
    // Each paragraph opened inside one closes it.
    Nested {
        what: "<p>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String("<p>".repeat(depth) + "x"),
        output: |depth| {
            let mut content = vec![json!({"type": "paragraph"}); depth - 1];
            content.push(paragraph(json!({"type": "text", "text": "x"})));
            doc(content)
        },
    },
    Nested {
        what: "<b>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String("<b>".repeat(depth) + "x"),
        output: |_| {
            let text = json!({"type": "text", "marks": [{"type": "bold"}], "text": "x"});
            doc(vec![paragraph(text)])
        },
    },
    Nested {
        what: "<span style=color:red>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String("<span style=color:red>".repeat(depth) + "x"),
        output: |_| {
            let text = json!({"type": "text", "marks": [{"type": "textStyle"}], "text": "x"});
            doc(vec![paragraph(text)])
        },
    },
    // Without an extension for blockquotes or lists, the manager parses a blockquote's content
    // and drops a list.
    Nested {
        what: "blockquotes",
        section: "parsesMarkdown",
        depth: PARSES / 4,
        input: |depth| quoted("x".into(), depth),
        output: |_| doc(vec![paragraph(json!({"type": "text", "text": "x"}))]),
    },
    Nested {
        what: "lazy continuations",
        section: "parsesMarkdown",
        depth: LAZY,
        input: |depth| Value::String("> ".repeat(depth) + "x\ny"),
        output: |_| doc(vec![paragraph(json!({"type": "text", "text": "x\ny"}))]),
    },
    Nested {
        what: "list items",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String("1. ".repeat(depth) + "x"),
        output: |_| doc(Vec::new()),
    },
    // Marks inside blockquotes run their recursion on top of the blockquotes'.
    Nested {
        what: "strong",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("**a ", " a**", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("bold", depth))]),
    },
    Nested {
        what: "em",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("*a ", " a*", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("italic", depth))]),
    },
    Nested {
        what: "del",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("~~a ", " a~~", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("strike", depth))]),
    },
    // A heading's blank lines become a token of their own, the heading copied, its tokens with
    // it.
    Nested {
        what: "a heading's blank lines",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| Value::String("# ".to_owned() + &around("*a ", " a*", depth) + "\n\ny"),
        output: |depth| {
            let heading = json!({"type": "heading", "attrs": {"level": 1},
                "content": marked("italic", depth)});
            doc(vec![
                heading,
                paragraph(json!({"type": "text", "text": "y"})),
            ])
        },
    },
];

#[test]
fn converts_what_nests_as_deeply_as_memory_allows() {
    let fixtures = read("tiptap");
    for nested in &NESTED {
        let input = input_of(nested.section);
        for depth in 1..=3 {
            let given = (nested.input)(depth);
            let record = records(&fixtures, nested.section)
                .iter()
                .find(|record| named(record).is_none() && same(&record[input], &given));
            let record = record.unwrap_or_else(|| panic!("no record of {given}"));
            expect(record, &[input], (nested.output)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        let converter = Converter::new(None);
        for nested in &NESTED {
            let converted = converter.convert(nested.section, &(nested.input)(nested.depth));
            let expected = (nested.output)(nested.depth);
            assert!(same(&expected, &converted), "{}", nested.what);
        }
    });
}

/// A color whose CSS nests far deeper than the HTML does: the CSS engine reads the span's style
/// to parse the color, and writes the style the color renders.
#[test]
fn colors_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let mut extensions = own::extensions();
        extensions.push(color().into());
        let schema = get_schema(&extensions).expect("the schema");
        let parens = "(".repeat(CSSES) + &")".repeat(CSSES);
        let color = format!("var(--a,{parens})");
        let html = format!("<p><span style=\"color: {color};\">x</span></p>");

        let doc = html::parse(&schema, &html).expect("a document");
        let text = json!({"type": "text", "marks": [
            {"type": "textStyle", "attrs": {"color": color}},
        ], "text": "x"});
        let paragraph = json!({"type": "paragraph", "content": [text]});
        let expected = json!({"type": "doc", "content": [paragraph]});
        assert!(same(&expected, &doc.to_json()));
        assert!(html::serialize(&schema, &doc).expect("HTML") == html);
    });
}
