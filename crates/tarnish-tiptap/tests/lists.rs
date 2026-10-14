//! What @tiptap/markdown's `MarkdownManager` and ProseMirror's `DOMParser` and `DOMSerializer` do
//! with @tiptap/extension-list's lists, recorded by `harness/record-tiptap-lists.mjs` in each of
//! its configurations: tarnish-tiptap's must make the same schemas, documents, Markdown and HTML,
//! or fail with the same error, and lists nested far deeper than a small stack could recurse
//! through convert as the recorded ones fold to.

#[allow(dead_code)]
mod own;

use own::{Converter, input_of, named};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Value, json};
use tarnish_fixtures::{expect, read, records, same};

const CONFIGURATIONS: [&str; 10] = [
    "lists",
    "nested",
    "attributes",
    "attributeValues",
    "bullets",
    "ordered",
    "tasks",
    "paragraphs",
    "flat",
    "tabs",
];

/// Holds each record of `section` to what its conversion makes of the record's input, with the
/// configuration the record names.
fn check(section: &str) {
    let fixtures = read("tiptap-lists");
    let converters = CONFIGURATIONS.map(|name| (name, Converter::new(Some(name))));
    let mut failures = Vec::new();
    for mut record in records(&fixtures, section).to_vec() {
        let (_, converter) = converters
            .iter()
            .find(|(name, _)| Some(*name) == named(&record))
            .expect("the configuration");
        let fields = record.as_object_mut().expect("a record");
        let input = fields.remove(input_of(section)).expect("an input");
        let configuration = fields.remove("extensions").expect("a configuration");
        let actual = converter.convert(section, &input);
        if !same(&record, &actual) {
            failures.push(format!(
                "{configuration} {input}\n  Tiptap: {record}\n  tarnish-tiptap: {actual}"
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

#[test]
fn builds_the_schemas_tiptap_does() {
    own::check_schemas("tiptap-lists", Converter::new);
}

const DOCUMENTS: usize = 10_000;
/// How deep items nest after a paragraph, whose Markdown indents the lines of every level inside
/// each level again.
const AFTER: usize = 1_000;
/// Parsing takes time that grows with the square of the depth.
const PARSES: usize = 1_000;
/// How deep Markdown nests by indentation, which marked and TaskList's tokenizer lex again for
/// each level with the levels inside it, in time that grows with the cube of the depth.
const INDENTED: usize = 300;

/// A list of each kind: its type, its items' type, the Markdown before an item's paragraph,
/// and its items' HTML.
struct Kind {
    list: &'static str,
    item: &'static str,
    marker: &'static str,
    open: &'static str,
    close: &'static str,
}

const BULLETS: Kind = Kind {
    list: "bulletList",
    item: "listItem",
    marker: "- ",
    open: "<ul><li>",
    close: "</li></ul>",
};
const NUMBERS: Kind = Kind {
    list: "orderedList",
    item: "listItem",
    marker: "1. ",
    open: "<ol><li>",
    close: "</li></ol>",
};
const TASKS: Kind = Kind {
    list: "taskList",
    item: "taskItem",
    marker: "- [ ] ",
    open: r#"<ul data-type="taskList"><li data-checked="false" data-type="taskItem"><label><input type="checkbox"><span></span></label><div>"#,
    close: "</div></li></ul>",
};

fn paragraph() -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]})
}

impl Kind {
    /// A list of one item of `content`, with the attributes a parse gives `attrs`.
    fn list(&self, content: Vec<Value>, attrs: Option<Value>) -> Value {
        let item = match self.item {
            "taskItem" => {
                json!({"type": "taskItem", "attrs": {"checked": false}, "content": content})
            }
            item => json!({"type": item, "content": content}),
        };
        match attrs {
            Some(attrs) => json!({"type": self.list, "attrs": attrs, "content": [item]}),
            None => json!({"type": self.list, "content": [item]}),
        }
    }

    /// Lists `depth` deep, each item holding the next list first.
    fn first(&self, depth: usize, attrs: Option<Value>) -> Value {
        let nested = (0..depth).fold(paragraph(), |inner, _| {
            self.list(vec![inner], attrs.clone())
        });
        json!({"type": "doc", "content": [nested]})
    }

    /// Lists `depth` deep, each item holding the next list after its paragraph.
    fn after(&self, depth: usize, attrs: Option<Value>) -> Value {
        let innermost = self.list(vec![paragraph()], attrs.clone());
        let nested = (1..depth).fold(innermost, |inner, _| {
            self.list(vec![paragraph(), inner], attrs.clone())
        });
        json!({"type": "doc", "content": [nested]})
    }

    /// `depth` lists, each item but the last's an empty paragraph, as the parser closes an item
    /// for a list it opens before any text.
    fn closed(&self, depth: usize, attrs: Option<Value>) -> Value {
        let mut content =
            vec![self.list(vec![json!({"type": "paragraph"})], attrs.clone()); depth - 1];
        content.push(self.list(vec![paragraph()], attrs));
        json!({"type": "doc", "content": content})
    }

    /// The Markdown of lists nested after paragraphs, each level indented by `indent` more.
    fn indented(&self, indent: &str, depth: usize) -> String {
        let lines: Vec<String> = (0..depth)
            .map(|level| indent.repeat(level) + self.marker + "x")
            .collect();
        lines.join("\n")
    }
}

fn markdown(markdown: String) -> Value {
    json!({ "markdown": markdown })
}

fn html(html: String) -> Value {
    json!({ "html": html })
}

fn doc(doc: Value) -> Value {
    json!({ "doc": doc })
}

/// What `parseHTML` gives an ordered list without a start or a type.
fn numbered() -> Option<Value> {
    Some(json!({"start": 1, "type": null}))
}

/// An input nested `depth` deep, and what the conversion a `section` of `tiptap-lists.json`
/// records with `configuration` makes of it, which the test folds from the depths below and
/// holds to Tiptap's at the depths the recorder records.
struct Nested {
    what: &'static str,
    section: &'static str,
    configuration: &'static str,
    depth: usize,
    input: fn(usize) -> Value,
    output: fn(usize) -> Value,
}

const NESTED: [Nested; 19] = [
    Nested {
        what: "Markdown of bullets inside bullets",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| BULLETS.first(depth, None),
        output: |depth| markdown(BULLETS.marker.repeat(depth) + "x"),
    },
    Nested {
        what: "Markdown of numbers inside numbers",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| NUMBERS.first(depth, None),
        output: |depth| markdown(NUMBERS.marker.repeat(depth) + "x"),
    },
    Nested {
        what: "Markdown of tasks inside tasks",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| TASKS.first(depth, None),
        output: |depth| markdown(TASKS.marker.repeat(depth) + "x"),
    },
    Nested {
        what: "Markdown of bullets after paragraphs",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: AFTER,
        input: |depth| BULLETS.after(depth, None),
        output: |depth| markdown(BULLETS.indented("  ", depth)),
    },
    Nested {
        what: "Markdown of numbers after paragraphs",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: AFTER,
        input: |depth| NUMBERS.after(depth, None),
        output: |depth| markdown(NUMBERS.indented("  ", depth)),
    },
    Nested {
        what: "Markdown of tasks after paragraphs",
        section: "serializesMarkdown",
        configuration: "lists",
        depth: AFTER,
        input: |depth| TASKS.after(depth, None),
        output: |depth| markdown(TASKS.indented("  ", depth)),
    },
    Nested {
        what: "Markdown of bullets after paragraphs, indented with tabs",
        section: "serializesMarkdown",
        configuration: "tabs",
        depth: AFTER,
        input: |depth| BULLETS.after(depth, None),
        output: |depth| markdown(BULLETS.indented("\t\t\t", depth)),
    },
    Nested {
        what: "HTML of bullets inside bullets",
        section: "serializesHTML",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| BULLETS.first(depth, None),
        output: |depth| {
            html(BULLETS.open.repeat(depth) + "<p>x</p>" + &BULLETS.close.repeat(depth))
        },
    },
    Nested {
        what: "HTML of numbers after paragraphs",
        section: "serializesHTML",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| NUMBERS.after(depth, None),
        output: |depth| {
            html(
                (NUMBERS.open.to_owned() + "<p>x</p>").repeat(depth) + &NUMBERS.close.repeat(depth),
            )
        },
    },
    Nested {
        what: "HTML of tasks inside tasks",
        section: "serializesHTML",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| TASKS.first(depth, None),
        output: |depth| html(TASKS.open.repeat(depth) + "<p>x</p>" + &TASKS.close.repeat(depth)),
    },
    Nested {
        what: "HTML of tasks after paragraphs",
        section: "serializesHTML",
        configuration: "lists",
        depth: DOCUMENTS,
        input: |depth| TASKS.after(depth, None),
        output: |depth| {
            html((TASKS.open.to_owned() + "<p>x</p>").repeat(depth) + &TASKS.close.repeat(depth))
        },
    },
    Nested {
        what: "`- ` inside `- `",
        section: "parsesMarkdown",
        configuration: "lists",
        depth: PARSES,
        input: |depth| Value::String(BULLETS.marker.repeat(depth) + "x"),
        output: |depth| doc(BULLETS.first(depth, None)),
    },
    Nested {
        what: "indented bullets",
        section: "parsesMarkdown",
        configuration: "lists",
        depth: INDENTED,
        input: |depth| Value::String(BULLETS.indented("  ", depth)),
        output: |depth| doc(BULLETS.after(depth, None)),
    },
    Nested {
        what: "indented numbers",
        section: "parsesMarkdown",
        configuration: "lists",
        depth: PARSES,
        input: |depth| Value::String(NUMBERS.indented("   ", depth)),
        output: |depth| doc(NUMBERS.after(depth, None)),
    },
    Nested {
        what: "indented tasks",
        section: "parsesMarkdown",
        configuration: "lists",
        depth: INDENTED,
        input: |depth| Value::String(TASKS.indented("  ", depth)),
        output: |depth| doc(TASKS.after(depth, None)),
    },
    Nested {
        what: "<ol><li>",
        section: "parsesHTML",
        configuration: "lists",
        depth: PARSES,
        input: |depth| Value::String("<ol><li>".repeat(depth) + "x"),
        output: |depth| doc(NUMBERS.closed(depth, numbered())),
    },
    Nested {
        what: "<ul><li>x",
        section: "parsesHTML",
        configuration: "lists",
        depth: PARSES,
        input: |depth| Value::String("<ul><li>x".repeat(depth)),
        output: |depth| doc(BULLETS.after(depth, None)),
    },
    // A task item holds only paragraphs, so each task list in one closes it.
    Nested {
        what: "task items after text",
        section: "parsesHTML",
        configuration: "lists",
        depth: PARSES,
        input: |depth| Value::String((TASK_ITEM.to_owned() + "x").repeat(depth)),
        output: |depth| {
            let content = vec![TASKS.list(vec![paragraph()], None); depth];
            doc(json!({"type": "doc", "content": content}))
        },
    },
    Nested {
        what: "nested task items after text",
        section: "parsesHTML",
        configuration: "nested",
        depth: PARSES,
        input: |depth| Value::String((TASK_ITEM.to_owned() + "x").repeat(depth)),
        output: |depth| doc(TASKS.after(depth, None)),
    },
];

const TASK_ITEM: &str = r#"<ul data-type="taskList"><li data-type="taskItem">"#;

#[test]
fn converts_lists_nested_as_deeply_as_memory_allows() {
    let fixtures = read("tiptap-lists");
    for nested in &NESTED {
        let input = input_of(nested.section);
        for depth in 1..=3 {
            let given = (nested.input)(depth);
            let record = records(&fixtures, nested.section).iter().find(|record| {
                named(record) == Some(nested.configuration) && same(&record[input], &given)
            });
            let record = record.unwrap_or_else(|| panic!("no record of {given}"));
            expect(record, &[input, "extensions"], (nested.output)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        for nested in &NESTED {
            let converter = Converter::new(Some(nested.configuration));
            let converted = converter.convert(nested.section, &(nested.input)(nested.depth));
            let expected = (nested.output)(nested.depth);
            assert!(same(&expected, &converted), "{}", nested.what);
        }
    });
}
