//! The configurations `harness/record-tiptap-lists.mjs` records the lists with, by the name a
//! record gives in `extensions`.

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
    let mut extensions = super::extensions();
    extensions.extend(more);
    extensions
}

fn attributes(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The extensions of the configuration `name`, and the indentation its manager has where it
/// isn't the default.
pub fn configuration(name: &str) -> Option<(Vec<Extension>, Option<Indentation>)> {
    let configured = match name {
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
        "attributes" => with_own(vec![
            bullet_list(BulletListOptions {
                html_attributes: attributes(&[("class", "bullets"), ("data-list", "bullet")]),
                ..BulletListOptions::default()
            })
            .into(),
            ordered_list(OrderedListOptions {
                html_attributes: attributes(&[
                    ("class", "numbers"),
                    ("start", "9"),
                    ("type", "A"),
                    ("style", "color: red"),
                ]),
                ..OrderedListOptions::default()
            })
            .into(),
            list_item(ListItemOptions {
                html_attributes: attributes(&[("class", "item")]),
            })
            .into(),
            task_list(TaskListOptions {
                html_attributes: attributes(&[("class", "tasks"), ("data-type", "mine")]),
                ..TaskListOptions::default()
            })
            .into(),
            task_item(TaskItemOptions {
                html_attributes: attributes(&[
                    ("class", "task"),
                    ("data-checked", "maybe"),
                    ("style", "color: blue"),
                ]),
                ..TaskItemOptions::default()
            })
            .into(),
        ]),
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
            let tabs = Indentation {
                style: IndentStyle::Tab,
                size: 3,
            };
            return Some((with_own(lists()), Some(tabs)));
        }
        _ => return None,
    };
    Some((configured, None))
}
