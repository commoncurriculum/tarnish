//! `tiptap-extension-flat-list`: `FlatListCore` and the three flat list items, each a block of
//! inline content that holds its indent in place of nesting, with their `li` parse rules and
//! their Markdown.

use tarnish::Node;
use tarnish::chunk::ValueRef;
use tarnish_js::json::{Map, Value, json, object};
use tarnish_js::{self as js, Error, value};
use tarnish_markdown::marked::{List, Token, TokenData};

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::{AttributeDefault, DomSpec, ExtensionAttribute, HtmlNode, NodeExtension};
use crate::{ParseHtml, PlainExtension, SpecAttrs, node_attr};

const ORDERED: &str = "flatListItemOrdered";
const TASK: &str = "flatListItemTask";
const UNORDERED: &str = "flatListItemUnordered";

/// Four spaces a level: a nested item has to reach its parent's content column, which is 3 for
/// `1. ` and 2 for `- `, and stay within three columns of it, or CommonMark reads it as a
/// sibling or as code.
const INDENT_PER_LEVEL: &str = "    ";

pub fn flat_list_core() -> PlainExtension {
    PlainExtension::create("flatListCore").priority(200)
}

pub fn flat_list_ordered() -> NodeExtension {
    flat_list_item(ORDERED, 230)
        .add_attributes(vec![
            indent(),
            ExtensionAttribute::new("counter", json!(1)).not_rendered(),
        ])
        .parse_html([ParseHtml::tag("li")
            .get_attrs(ordered_attrs)
            .content_element(item_content)])
        .render_html(|node, _| {
            let list = SpecAttrs::from([
                ("start", node_attr(node, "counter").into()),
                ("style", style(node, "decimal")?.into()),
            ]);
            let item = DomSpec::wrapping("li", item_attrs(node));
            Ok(DomSpec::element("ol", list, vec![item]))
        })
        .parse_markdown(|token, helpers| {
            let mut nodes = Vec::new();
            if let Some(list) = token.list().filter(|list| list.ordered) {
                for (counter, item) in (1..).zip(&list.items) {
                    let attrs = object!({ "counter": counter });
                    push_item(&mut nodes, ORDERED, Some(attrs), item, helpers)?;
                }
            }
            Ok(Parsed::Nodes(nodes))
        })
}

pub fn flat_list_unordered() -> NodeExtension {
    flat_list_item(UNORDERED, 210)
        .add_attributes(vec![indent()])
        .parse_html([ParseHtml::tag("li")
            .get_attrs(|element| indent_attrs(element).map(Some))
            .content_element(item_content)])
        .render_html(|node, _| {
            let list = SpecAttrs::from([("style", style(node, "disc")?.into())]);
            let item = DomSpec::wrapping("li", item_attrs(node));
            Ok(DomSpec::element("ul", list, vec![item]))
        })
        .parse_markdown(|token, helpers| {
            let mut nodes = Vec::new();
            for item in items(token) {
                push_item(&mut nodes, UNORDERED, None, item, helpers)?;
            }
            Ok(Parsed::Nodes(nodes))
        })
}

pub fn flat_list_task() -> NodeExtension {
    flat_list_item(TASK, 220)
        .add_attributes(vec![
            indent(),
            ExtensionAttribute::new("checked", json!(false)).not_rendered(),
        ])
        .parse_html([ParseHtml::tag("li")
            .get_attrs(task_attrs)
            .content_element(|element| {
                let content = task_content_element(element)?;
                replace_paragraphs_with_breaks(&content)?;
                Ok(content)
            })])
        .render_html(|node, _| render_task(node))
        .parse_markdown(parse_task)
}

/// What the three items share: their schema, and the Markdown they parse from and render to.
fn flat_list_item(name: &'static str, priority: i32) -> NodeExtension {
    NodeExtension::create(name)
        .group("block")
        .content("inline*")
        .priority(priority)
        .defining()
        .markdown_token_name("list")
        .render_markdown(render_flat_list_markdown)
}

fn indent() -> ExtensionAttribute {
    ExtensionAttribute::new("indent", AttributeDefault::Undefined).not_rendered()
}

/// `indentAttr(indent)`: `None` for the `undefined` an item holds for 0.
fn indent_attr(indent: f64) -> Result<Option<f64>, Error> {
    if indent < 0.0 {
        let indent = js::number_to_string(indent);
        return Err(Error::Other(format!("Invalid indent: {indent}")));
    }
    Ok((indent != 0.0).then_some(indent))
}

/// `getIndent(node)`, `None` for the 0 of an item that holds none.
fn get_indent<'a>(node: &'a Node) -> Option<ValueRef<'a>> {
    node.attrs_view()
        .get("indent")
        .filter(|indent| !indent.is_null())
}

fn style(node: &Node, list_style_type: &str) -> Result<String, Error> {
    let indent = get_indent(node).map_or(Ok(0.0), ValueRef::to_number)?;
    let margin = js::number_to_string(20.0 * indent);
    Ok([
        "margin-bottom: 0; margin-left: ",
        &margin,
        "px; list-style-type: ",
        list_style_type,
        ";",
    ]
    .concat())
}

/// The `li`'s `data-list-indent`, left out for an indent of 0.
fn item_attrs<'a>(node: &'a Node) -> SpecAttrs<'a> {
    let mut attrs = SpecAttrs::new();
    if let Some(indent) = get_indent(node).filter(|indent| indent.truthy()) {
        attrs.push("data-list-indent", indent.into());
    }
    attrs
}

fn render_task<'a>(node: &'a Node<'static>) -> Result<DomSpec<'a>, Error> {
    let list = SpecAttrs::from([
        ("data-task-list", "".into()),
        ("style", style(node, "none")?.into()),
    ]);
    let checked = node_attr(node, "checked").truthy();
    let mut item = item_attrs(node);
    let mut checkbox = SpecAttrs::from([("type", "checkbox".into()), ("disabled", "".into())]);
    if checked {
        item.push("data-checked", "".into());
        checkbox.push("checked", "".into());
    }
    item.push("style", "position: relative;".into());
    let text = node.text_content()?;
    let text = text.to_string_lossy();
    let text = if text.is_empty() {
        "empty task item"
    } else {
        &text
    };
    checkbox.push(
        "aria-label",
        format!("Task item checkbox for {text}").into(),
    );
    let label = DomSpec::element(
        "label",
        SpecAttrs::from([(
            "style",
            "position: absolute; left: -20px; top: 0; user-select: none;".into(),
        )]),
        vec![
            DomSpec::element("input", checkbox, Vec::new()),
            DomSpec::element("span", SpecAttrs::new(), Vec::new()),
        ],
    );
    let content = DomSpec::wrapping("div", SpecAttrs::new());
    let item = DomSpec::element("li", item, vec![label, content]);
    Ok(DomSpec::element("ul", list, vec![item]))
}

/// The ordered item's `getAttrs`: an item of an `ol`, counted from 1 among its `li`s.
fn ordered_attrs(element: &HtmlNode) -> Result<Option<Map>, Error> {
    let Some(list) = element
        .parent_element()
        .filter(|parent| parent.is_tag("OL"))
    else {
        return Ok(None);
    };
    let mut items = list
        .children()
        .into_iter()
        .filter(|child| child.is_tag("LI"));
    let index = items.position(|item| item == *element);
    let mut attrs = indent_attrs(element)?;
    attrs.insert("counter".into(), json!(index.map_or(0, |index| index + 1)));
    Ok(Some(attrs))
}

/// The task item's `getAttrs`: an item of a list marked `data-task-list`.
fn task_attrs(element: &HtmlNode) -> Result<Option<Map>, Error> {
    let list = element.parent_element();
    if !list.is_some_and(|list| get_boolean_attribute(&list, "data-task-list")) {
        return Ok(None);
    }
    let mut attrs = indent_attrs(element)?;
    let checked = get_boolean_attribute(element, "data-checked");
    attrs.insert("checked".into(), Value::Bool(checked));
    Ok(Some(attrs))
}

/// `{ indent: indentAttr(computeIndent(element)) }`.
fn indent_attrs(element: &HtmlNode) -> Result<Map, Error> {
    let mut attrs = Map::new();
    if let Some(indent) = indent_attr(compute_indent(element))? {
        attrs.push("indent".into(), js::number(indent));
    }
    Ok(attrs)
}

/// `computeIndent(element)`: the indent the item stores, or else how many lists it's in past
/// the first.
fn compute_indent(element: &HtmlNode) -> f64 {
    if let Some(stored) = parse_integer_attr(element.attribute("data-list-indent")) {
        return stored;
    }
    let mut count = -1.0;
    let mut ancestor = element.parent_element();
    while let Some(parent) = ancestor {
        if parent.is_tag("UL") || parent.is_tag("OL") {
            count += 1.0;
        }
        ancestor = parent.parent_element();
    }
    f64::max(count, 0.0)
}

/// `parseIntegerAttr(attr)`: the integer `parseInt` reads, if it is one.
fn parse_integer_attr(attr: Option<String>) -> Option<f64> {
    let integer = js::parse_int(&attr?);
    (integer.is_finite() && integer.trunc() == integer).then_some(integer)
}

fn get_boolean_attribute(element: &HtmlNode, attribute: &str) -> bool {
    element
        .attribute(attribute)
        .is_some_and(|value| value != "false")
}

/// The ordered and unordered items' `contentElement`: the item itself.
fn item_content(element: &HtmlNode) -> Result<HtmlNode, Error> {
    replace_paragraphs_with_breaks(element)?;
    Ok(element.clone())
}

/// `getContentElement("flatListItemTask", li)`: the last element after a first `label`, as the
/// task renders it, or else the item.
fn task_content_element(li: &HtmlNode) -> Result<HtmlNode, Error> {
    let children = li.children();
    if let (Some(first), Some(last)) = (children.first(), children.last())
        && first.is_tag("LABEL")
    {
        return Ok(last.clone());
    }
    Ok(li.clone())
}

/// `replaceParagraphsWithBreaks(element)`: each paragraph in the element gives way to its
/// children and a break, but where it's last or a list or an item follows it, whose break the
/// parser would read as a line of the item.
fn replace_paragraphs_with_breaks(element: &HtmlNode) -> Result<(), Error> {
    if !element.children().iter().any(|child| child.is_tag("P")) {
        return Ok(());
    }
    let children = element.child_nodes();
    for (index, child) in children.iter().enumerate() {
        if !child.is_tag("P") {
            continue;
        }
        for grandchild in child.child_nodes() {
            child.before(&grandchild)?;
        }
        if let Some(next) = children.get(index + 1) {
            if !["OL", "UL", "LI"].iter().any(|tag| next.is_tag(tag)) {
                child.before(&element.dom().create_html_element("br")?)?;
            }
        }
        child.remove();
    }
    Ok(())
}

/// `token.items ?? []`.
fn items(token: &Token) -> &[Token] {
    token.list().map_or(&[], |list| &list.items)
}

fn is_task(item: &Token) -> bool {
    matches!(item.data, TokenData::ListItem { task: true, .. })
}

/// The task item's `parseMarkdown`: a list's task items, with each run of its other items
/// parsed again as a list of its own, or nothing for a list of no tasks.
fn parse_task(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let mut nodes = Vec::new();
    let Some(list) = token.list().filter(|list| list.items.iter().any(is_task)) else {
        return Ok(Parsed::Nodes(nodes));
    };
    let mut others = Vec::new();
    for item in &list.items {
        if !is_task(item) {
            others.push(item.clone());
            continue;
        }
        parse_others(token, list, &mut others, helpers, &mut nodes)?;
        let checked = matches!(
            item.data,
            TokenData::ListItem {
                checked: Some(true),
                ..
            }
        );
        let attrs = object!({ "checked": checked });
        push_item(&mut nodes, TASK, Some(attrs), item, helpers)?;
    }
    parse_others(token, list, &mut others, helpers, &mut nodes)?;
    Ok(Parsed::Nodes(nodes))
}

/// `helpers.parseChildren([{ ...token, items: otherItems }])`, of the items not yet parsed.
fn parse_others(
    token: &Token,
    list: &List,
    others: &mut Vec<Token>,
    helpers: &ParseHelpers,
    nodes: &mut Vec<Value>,
) -> Result<(), Error> {
    if others.is_empty() {
        return Ok(());
    }
    let items = std::mem::take(others);
    let token = Token {
        kind: token.kind,
        raw: token.raw.clone(),
        text: token.text.clone(),
        tokens: token.tokens.clone(),
        data: TokenData::List(Box::new(List { items, ..*list })),
        extra: token.extra.clone(),
        child_tokens: token.child_tokens.clone(),
    };
    nodes.extend(helpers.parse_children(&[token])?);
    Ok(())
}

/// The item's node, then the items of the lists in it.
fn push_item(
    nodes: &mut Vec<Value>,
    name: &str,
    attrs: Option<Map>,
    item: &Token,
    helpers: &ParseHelpers,
) -> Result<(), Error> {
    let content = parse_item_content(item, helpers)?;
    nodes.push(helpers.create_node(name, attrs, Some(content)));
    nodes.extend(parse_nested_lists(item, helpers)?);
    Ok(())
}

/// `parseItemContent(item, helpers)`: the inline content of the item's first block.
fn parse_item_content(item: &Token, helpers: &ParseHelpers) -> Result<Vec<Value>, Error> {
    let tokens = item.tokens.as_deref().unwrap_or_default();
    let text_token = tokens
        .iter()
        .find(|child| !matches!(child.kind, "list" | "space" | "checkbox"));
    match text_token.and_then(|token| token.tokens.as_deref()) {
        Some(inline) => helpers.parse_inline(inline),
        None => {
            let text = text_token.map_or(&[][..], Token::text);
            helpers.parse_inline(&helpers.tokenize_inline(text)?)
        }
    }
}

/// `parseNestedLists(item, helpers)`: the lists in the item, parsed as any list is, each flat
/// item a level deeper.
fn parse_nested_lists(item: &Token, helpers: &ParseHelpers) -> Result<Vec<Value>, Error> {
    let tokens = item.tokens.as_deref().unwrap_or_default();
    let mut nodes = Vec::new();
    for list in tokens.iter().filter(|child| child.kind == "list") {
        nodes.extend(helpers.parse_children(std::slice::from_ref(list))?);
    }
    for node in &mut nodes {
        let Value::Object(node) = node else { continue };
        let flat = node.get("type").and_then(Value::as_str);
        if !matches!(flat, Some(ORDERED | TASK | UNORDERED)) {
            continue;
        }
        let indent = js::to_number(value::optional(node.get("attrs"), "indent"))?;
        let indent = indent_attr(or_zero(indent) + 1.0)?;
        let attrs = node.entry("attrs").or_insert_with(|| json!({}));
        if let (Value::Object(attrs), Some(indent)) = (attrs, indent) {
            attrs.insert("indent".into(), js::number(indent));
        }
    }
    Ok(nodes)
}

/// `number || 0`.
fn or_zero(number: f64) -> f64 {
    if js::number_to_boolean(number) {
        number
    } else {
        0.0
    }
}

/// `renderFlatListMarkdown(node, helpers)`: the item as a list of one, indented four spaces a
/// level, and every ordered item written `1.`, as Markdown numbers its items itself.
fn render_flat_list_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    let kind = node.get("type").and_then(Value::as_str);
    let attrs = node.get("attrs");
    let ordered = kind == Some(ORDERED);
    let marker = match kind {
        _ if ordered => "1.",
        Some(TASK) if js::truthy(value::optional(attrs, "checked")) => "- [x]",
        Some(TASK) => "- [ ]",
        _ => "-",
    };
    let level = js::to_number(value::optional(attrs, "indent"))?;
    let indent = js::repeat(INDENT_PER_LEVEL, or_zero(level))?;
    // A task's checkbox is content, not marker, so its lines go on at the bullet's column.
    let marker_width = if ordered { marker.len() + 1 } else { 2 };
    let continuation = ["\n", &indent, &" ".repeat(marker_width)].concat();
    let empty = Value::Array(Vec::new());
    let content = node.get("content").filter(|content| !content.is_null());
    let content = helpers.render_children(content.unwrap_or(&empty), "")?;
    let content = js::replace_all(&content, "\n", &continuation)?;
    Ok([&indent, marker, " ", &content].concat())
}
