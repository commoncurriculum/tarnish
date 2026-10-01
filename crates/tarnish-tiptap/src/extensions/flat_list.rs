//! `tiptap-extension-flat-list`: `FlatListCore` and the three flat list item nodes. Their `li`
//! parse rules, which prop up an item through the page's global `document`, are left to the
//! application.

use tarnish::chunk::ValueRef;
use tarnish::json::json;
use tarnish::{Error, Node, js};

use crate::{DomSpec, SpecAttrs, node_attr};
use crate::{ExtensionAttribute, NodeExtension, PlainExtension};

pub fn flat_list_core() -> PlainExtension {
    PlainExtension::create("flatListCore").priority(200)
}

pub fn flat_list_ordered() -> NodeExtension {
    NodeExtension::create("flatListItemOrdered")
        .group("block")
        .content("inline*")
        .priority(230)
        .add_attributes(attributes(Some("counter"), None))
        .render_html(|node, _| {
            Ok(DomSpec::element(
                "ol",
                SpecAttrs::from([
                    ("start", node_attr(node, "counter").into()),
                    ("style", style(node, "decimal")?.into()),
                ]),
                vec![item(node)],
            ))
        })
}

pub fn flat_list_unordered() -> NodeExtension {
    NodeExtension::create("flatListItemUnordered")
        .group("block")
        .content("inline*")
        .priority(210)
        .add_attributes(attributes(None, None))
        .render_html(|node, _| {
            Ok(DomSpec::element(
                "ul",
                SpecAttrs::from([("style", style(node, "disc")?.into())]),
                vec![item(node)],
            ))
        })
}

pub fn flat_list_task() -> NodeExtension {
    NodeExtension::create("flatListItemTask")
        .group("block")
        .content("inline*")
        .priority(220)
        .add_attributes(attributes(None, Some("checked")))
        .render_html(|node, _| render_task(node))
}

fn attributes(
    counter: Option<&'static str>,
    checked: Option<&'static str>,
) -> Vec<ExtensionAttribute> {
    let mut attributes = vec![ExtensionAttribute::new("indent", json!(0)).not_rendered()];
    if let Some(counter) = counter {
        attributes.push(ExtensionAttribute::new(counter, json!(1)).not_rendered());
    }
    if let Some(checked) = checked {
        attributes.push(ExtensionAttribute::new(checked, json!(false)));
    }
    attributes.push(ExtensionAttribute::new("_isTempPropped", json!(false)).not_rendered());
    attributes
}

fn style(node: &Node, list_style_type: &str) -> Result<String, Error> {
    let indent = node_attr(node, "indent").to_value();
    let margin = js::number_to_string(20.0 * js::to_number(Some(&indent))?);
    Ok([
        "margin-bottom: 0; margin-left: ",
        &margin,
        "px; list-style-type: ",
        list_style_type,
        ";",
    ]
    .concat())
}

fn item<'a>(node: &'a Node) -> DomSpec<'a> {
    DomSpec::wrapping(
        "li",
        SpecAttrs::from([("data-list-indent", node_attr(node, "indent").into())]),
    )
}

fn render_task<'a>(node: &'a Node<'static>) -> Result<DomSpec<'a>, Error> {
    let checked: ValueRef<'a> = node_attr(node, "checked");
    let text = node.text_content()?;
    let text = text.to_string_lossy();
    let label = if text.is_empty() {
        "empty task item"
    } else {
        &text
    };
    let mut checkbox = SpecAttrs::from([("type", "checkbox".into()), ("disabled", "true".into())]);
    // `checked: attrs.checked ? "checked" : null`, where a null attribute isn't set.
    if checked.truthy() {
        checkbox.push("checked", "checked".into());
    }
    // The JS sets `ariaLabel`, which `setAttribute` lowercases.
    checkbox.push(
        "arialabel",
        format!("Task item checkbox for {label}").into(),
    );
    Ok(DomSpec::element(
        "ul",
        SpecAttrs::from([
            ("data-task-list", "".into()),
            ("style", style(node, "none")?.into()),
        ]),
        vec![DomSpec::element(
            "li",
            SpecAttrs::from([
                ("data-list-indent", node_attr(node, "indent").into()),
                ("data-checked", checked.into()),
                ("style", "position: relative;".into()),
            ]),
            vec![
                DomSpec::element(
                    "label",
                    SpecAttrs::from([(
                        "style",
                        "position: absolute; left: -20px; top: 0; user-select: none;".into(),
                    )]),
                    vec![
                        DomSpec::element("input", checkbox, Vec::new()),
                        DomSpec::element("span", SpecAttrs::new(), Vec::new()),
                    ],
                ),
                DomSpec::wrapping("div", SpecAttrs::new()),
            ],
        )],
    ))
}
