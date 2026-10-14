//! `@tiptap/extension-ruby-text`: a mark whose `rt` annotates its text, rendered as a `ruby`
//! element with the text in an `rb` and the annotation in an `rt`.

use crate::merge_attributes::merge_html_attributes;
use crate::{DomSpec, MarkExtension, ParseHtml, SpecAttrs, mark_attr};
use crate::{ExtensionAttribute, HtmlAttributesOptions, HtmlNode};
use tarnish_js::Result;
use tarnish_js::json::{Map, Value, json};

pub type RubyTextOptions = HtmlAttributesOptions;

pub fn ruby_text(options: RubyTextOptions) -> MarkExtension {
    MarkExtension::create("rubyText")
        .inclusive(false)
        .add_attributes(vec![ExtensionAttribute::new("rt", Value::Null)])
        .parse_html([ParseHtml::tag("ruby")
            .content_element(content_element)
            .get_attrs(|ruby| {
                let Some(rt) = ruby.query_selector("rt")? else {
                    return Ok(None);
                };
                Ok(Some(Map::from_iter([(
                    "rt".into(),
                    json!(rt.text_content()),
                )])))
            })])
        .render_html(move |mark, html| {
            let html = merge_html_attributes(&options.html_attributes, html)?;
            let mut children = vec![DomSpec::wrapping("rb", SpecAttrs::new())];
            let rt = mark_attr(mark, "rt");
            if !rt.is_null() {
                let editable = SpecAttrs::from([("contenteditable", "false".into())]);
                children.push(DomSpec::element("rt", editable, vec![DomSpec::Attr(rt)]));
            }
            Ok(DomSpec::element("ruby", html, children))
        })
}

/// The `rb` inside the ruby, or else a span of its children but the `rt`s and `rp`s. The JS
/// gives the span copies of them; this moves them there, which the parser reads the same.
fn content_element(ruby: &HtmlNode) -> Result<HtmlNode> {
    if let Some(rb) = ruby.query_selector("rb")? {
        return Ok(rb);
    }
    let surrogate = ruby.dom().create_html_element("span")?;
    for child in ruby.child_nodes() {
        let name = child.node_name();
        if name != "RT" && name != "RP" {
            surrogate.append_child(&child)?;
        }
    }
    Ok(surrogate)
}
