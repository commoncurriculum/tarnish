//! `fixtures/tiptap-lists.json`, of @tiptap/extension-list's lists, and lists nested far deeper
//! than a small stack could recurse through.

use tarnish::json::{Map, Value, json, object};
use tarnish_tiptap::Extension;
use tarnish_tiptap::extensions::bullet_list::{BulletListOptions, bullet_list};
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::flat_list::{
    flat_list_core, flat_list_ordered, flat_list_task, flat_list_unordered,
};
use tarnish_tiptap::extensions::list_item::{ListItemOptions, list_item};
use tarnish_tiptap::extensions::ordered_list::{OrderedListOptions, ordered_list};
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::task_item::{TaskItemOptions, task_item};
use tarnish_tiptap::extensions::task_list::{TaskListOptions, task_list};
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::markdown::{IndentStyle, Indentation};

use crate::own::{self, Configuration, Nested, Replay};

fn configuration(name: &str) -> Configuration {
    match name {
        "lists" => with_own(lists()),
        "nested" => {
            let mut nested = lists();
            nested[4] = task_item(TaskItemOptions {
                nested: true,
                ..TaskItemOptions::default()
            })
            .into();
            with_own(nested)
        }
        "attributes" => with_attributes([
            object!({"class": "bullets", "data-list": "bullet"}),
            object!({"class": "numbers", "start": "9", "type": "A", "style": "color: red"}),
            object!({"class": "item"}),
            object!({"class": "tasks", "data-type": "mine"}),
            object!({"class": "task", "data-checked": "maybe", "style": "color: blue"}),
        ]),
        "attributeValues" => with_attributes(std::array::from_fn(|_| own::other_attributes())),
        "bullets" => with_own(vec![
            bullet_list(BulletListOptions::default()).into(),
            list_item(ListItemOptions::default()).into(),
        ]),
        "ordered" => with_own(vec![
            ordered_list(OrderedListOptions::default()).into(),
            list_item(ListItemOptions::default()).into(),
        ]),
        "tasks" => with_own(vec![
            task_list(TaskListOptions::default()).into(),
            task_item(TaskItemOptions::default()).into(),
        ]),
        "paragraphs" => with_own(vec![
            bullet_list(BulletListOptions {
                item_type_name: "paragraph",
                ..BulletListOptions::default()
            })
            .into(),
            ordered_list(OrderedListOptions {
                item_type_name: "paragraph",
                ..OrderedListOptions::default()
            })
            .into(),
            task_list(TaskListOptions {
                item_type_name: "paragraph",
                ..TaskListOptions::default()
            })
            .into(),
        ]),
        "flat" => {
            let mut flat: Vec<Extension> = vec![
                document().into(),
                paragraph(Default::default()).into(),
                text().into(),
                flat_list_core().into(),
                flat_list_ordered().into(),
                flat_list_unordered().into(),
                flat_list_task().into(),
            ];
            flat.extend(lists());
            flat
        }
        "tabs" => {
            let indentation = Indentation {
                style: IndentStyle::Tab,
                size: 3,
            };
            return Configuration {
                indentation,
                ..with_own(lists()).into()
            };
        }
        other => panic!("no configuration {other}"),
    }
    .into()
}

/// The five lists, with their default options.
fn lists() -> Vec<Extension> {
    vec![
        bullet_list(BulletListOptions::default()).into(),
        ordered_list(OrderedListOptions::default()).into(),
        list_item(ListItemOptions::default()).into(),
        task_list(TaskListOptions::default()).into(),
        task_item(TaskItemOptions::default()).into(),
    ]
}

/// Tiptap's own extensions and `more`.
fn with_own(more: Vec<Extension>) -> Vec<Extension> {
    let mut extensions = own::extensions();
    extensions.extend(more);
    extensions
}

/// Tiptap's own extensions and the five lists, each with its HTMLAttributes.
fn with_attributes([bullets, numbers, item, tasks, task]: [Map; 5]) -> Vec<Extension> {
    with_own(vec![
        bullet_list(BulletListOptions {
            html_attributes: bullets,
            ..BulletListOptions::default()
        })
        .into(),
        ordered_list(OrderedListOptions {
            html_attributes: numbers,
            ..OrderedListOptions::default()
        })
        .into(),
        list_item(ListItemOptions {
            html_attributes: item,
        })
        .into(),
        task_list(TaskListOptions {
            html_attributes: tasks,
            ..TaskListOptions::default()
        })
        .into(),
        task_item(TaskItemOptions {
            html_attributes: task,
            ..TaskItemOptions::default()
        })
        .into(),
    ])
}

fn replay() -> Replay {
    Replay::new("tiptap-lists", configuration)
}

#[test]
fn converts_as_tiptap_does() {
    replay().check();
}

#[test]
fn converts_lists_nested_as_deeply_as_memory_allows() {
    replay().check_nested(&NESTED);
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

/// A paragraph of `x`.
fn x() -> Value {
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
        let nested = (0..depth).fold(x(), |inner, _| self.list(vec![inner], attrs.clone()));
        json!({"type": "doc", "content": [nested]})
    }

    /// Lists `depth` deep, each item holding the next list after its paragraph.
    fn after(&self, depth: usize, attrs: Option<Value>) -> Value {
        let innermost = self.list(vec![x()], attrs.clone());
        let nested = (1..depth).fold(innermost, |inner, _| {
            self.list(vec![x(), inner], attrs.clone())
        });
        json!({"type": "doc", "content": [nested]})
    }

    /// `depth` lists, each item but the last's an empty paragraph, as the parser closes an item
    /// for a list it opens before any text.
    fn closed(&self, depth: usize, attrs: Option<Value>) -> Value {
        let mut content =
            vec![self.list(vec![json!({"type": "paragraph"})], attrs.clone()); depth - 1];
        content.push(self.list(vec![x()], attrs));
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
            let content = vec![TASKS.list(vec![x()], None); depth];
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
