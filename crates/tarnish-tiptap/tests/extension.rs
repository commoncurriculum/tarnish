//! What an application builds its extensions with: attributes changed in place and rendered
//! under names they make, parse rules whose hooks fail, and Markdown hooks that hold what they
//! need.

use tarnish::json::{Map, Value, json};
use tarnish_markdown::marked::Marked;
use tarnish_markdown::marked_more_lists::more_lists;
use tarnish_tiptap::extensions::{document::document, paragraph::paragraph, text::text};
use tarnish_tiptap::markdown::{MarkdownManager, Parsed};
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, NodeExtension, ParseHtml, get_schema, html,
};

/// A document of paragraphs and `figure`s.
fn with(figure: NodeExtension) -> Vec<Extension> {
    vec![
        document().into(),
        paragraph().into(),
        text().into(),
        figure.group("block").into(),
    ]
}

fn figure() -> NodeExtension {
    NodeExtension::create("figure")
        .parse_html([ParseHtml::tag("figure")])
        .render_html(|_, html| Ok(DomSpec::element("figure", html, Vec::new())))
}

#[test]
fn updates_an_attribute_where_it_is() {
    let figure = figure()
        .add_attributes(vec![
            ExtensionAttribute::new("a", json!(1)),
            ExtensionAttribute::new("b", json!(2)),
        ])
        .update_attribute("a", |a| a.default(json!(3)).not_rendered());
    let attributes: Vec<_> = figure
        .attributes
        .iter()
        .map(|attribute| {
            (
                attribute.name,
                attribute.default.clone(),
                attribute.rendered,
            )
        })
        .collect();
    assert_eq!(
        attributes,
        [("a", Some(json!(3)), false), ("b", Some(json!(2)), true)]
    );
}

#[test]
#[should_panic(expected = "figure has no attribute c")]
fn updates_only_an_attribute_it_has() {
    let _ = figure().update_attribute("c", |c| c);
}

#[test]
fn an_attribute_without_a_default_is_required() -> tarnish::Result<()> {
    for (default, required) in [(None, true), (Some(Value::Null), false)] {
        let figure = figure().add_attributes(vec![ExtensionAttribute::new("src", default)]);
        let schema = get_schema(&with(figure))?.schema;
        let figure = schema.node_type("figure").expect("the figure type");
        assert_eq!(figure.has_required_attrs(), required);
    }
    Ok(())
}

#[test]
fn renders_an_attribute_under_a_name_it_makes() -> tarnish::Result<()> {
    let name = ["data", "made"].join("-");
    let figure = figure().add_attributes(vec![
        ExtensionAttribute::new("made", Value::Null).render_html(move |attrs, rendered| {
            rendered.merge(name.clone(), attrs.get("made").expect("its attribute"))
        }),
    ]);
    let schema = get_schema(&with(figure))?;
    let doc = html::parse(&schema, r#"<figure made="x"></figure>"#)?;
    assert_eq!(
        html::serialize(&schema, &doc)?,
        r#"<figure data-made="x"></figure>"#
    );
    Ok(())
}

#[test]
fn fails_to_parse_where_a_hook_fails() -> tarnish::Result<()> {
    let attrs = NodeExtension::create("figure").parse_html([ParseHtml::tag("figure").get_attrs(
        |figure| {
            figure.query_selector("[")?;
            Ok(Some(Map::new()))
        },
    )]);
    let content = NodeExtension::create("figure")
        .content("text*")
        .parse_html([ParseHtml::tag("figure").content_element(|figure| {
            figure.query_selector("[")?;
            Ok(figure.clone())
        })]);
    for figure in [attrs, content] {
        let schema = get_schema(&with(figure))?;
        assert!(html::parse(&schema, "<figure>x</figure>").is_err());
    }
    Ok(())
}

#[test]
fn markdown_hooks_hold_what_they_need() -> tarnish::Result<()> {
    let (node, marker) = (json!({ "type": "figure" }), String::from("* * *"));
    let figure = figure()
        .markdown_token_name("hr")
        .parse_markdown(move |_, _| Ok(Parsed::Node(node.clone())))
        .render_markdown(move |_, _, _| Ok(marker.clone()));
    let manager = MarkdownManager::new(&with(figure), Marked::new(more_lists()));
    let doc = json!({ "type": "doc", "content": [{ "type": "figure" }] });
    assert_eq!(manager.parse("---")?, doc);
    assert_eq!(manager.serialize(&doc)?, "* * *");
    Ok(())
}
