//! `@tiptap/extension-mention`: the mention node, and its Markdown, which
//! `createInlineMarkdownSpec` makes, `[@ id="…" label="…" char="…"]`. Its suggestion popups,
//! and the trigger Backspace leaves, are an editor's.

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use tarnish::Node;
use tarnish::chunk::ValueRef;
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16};

use crate::markdown::specs::AllowedAttribute;
use crate::{
    DomSpec, ExtensionAttribute, NodeExtension, ParseHtml, SpecAttrs, merge_attributes, node_attr,
};

/// A `renderText` or `renderLabel` option, given the node and the `char` of its suggestion.
pub type RenderMentionText =
    Arc<dyn Fn(&Node<'static>, &str) -> Result<String, Error> + Send + Sync>;

/// A `renderHTML` option, given the node, the `char` of its suggestion, and
/// `options.HTMLAttributes`: the option's, merged with `data-type` and the node's attributes.
pub type RenderMentionHtml = Arc<
    dyn for<'a> Fn(&'a Node<'static>, &str, SpecAttrs<'a>) -> Result<MentionHtml<'a>, Error>
        + Send
        + Sync,
>;

/// What a `renderHTML` option returns: a spec, or the text of a span.
pub enum MentionHtml<'a> {
    Spec(DomSpec<'a>),
    Text(String),
}

pub struct MentionOptions {
    pub html_attributes: Map,
    /// The `char` of each of `suggestions`, `@` for one that sets none.
    pub suggestions: Vec<String>,
    /// The `char` of `suggestion`, the one suggestion when there are no `suggestions`.
    pub suggestion: String,
    /// `None` for Tiptap's: the char, then the label, or else the id.
    pub render_text: Option<RenderMentionText>,
    /// `None` for Tiptap's: a span of the text Tiptap's `render_text` gives.
    pub render_html: Option<RenderMentionHtml>,
    /// `renderLabel`, which Tiptap deprecates: the text of the node and of its span, in place of
    /// `render_text` and `render_html`.
    pub render_label: Option<RenderMentionText>,
}

impl Default for MentionOptions {
    fn default() -> Self {
        MentionOptions {
            html_attributes: Map::new(),
            suggestions: Vec::new(),
            suggestion: "@".to_owned(),
            render_text: None,
            render_html: None,
            render_label: None,
        }
    }
}

impl MentionOptions {
    /// `getSuggestionFromChar(this, node.attrs.mentionSuggestionChar).char`: the char of the
    /// suggestion whose char is the node's, or else of the first.
    fn char_of(&self, node: &Node) -> &str {
        let char = node_attr(node, "mentionSuggestionChar").as_str();
        match self.suggestions.first() {
            None => &self.suggestion,
            Some(first) => self
                .suggestions
                .iter()
                .find(|suggestion| Some(suggestion.as_str()) == char)
                .unwrap_or(first),
        }
    }

    /// `renderText`, or `renderLabel` where there is one.
    fn text(&self, node: &Node<'static>) -> Result<String, Error> {
        let char = self.char_of(node);
        match (&self.render_label, &self.render_text) {
            (Some(render), _) | (None, Some(render)) => render(node, char),
            (None, None) => label_text(node, char),
        }
    }
}

/// `${suggestion?.char ?? '@'}${node.attrs.label ?? node.attrs.id}`.
fn label_text(node: &Node, char: &str) -> Result<String, Error> {
    let label = ValueRef::coalesce(node_attr(node, "label"), node_attr(node, "id"));
    Ok(format!("{char}{}", label.to_js_string()?))
}

pub fn mention(options: MentionOptions) -> NodeExtension {
    let options = Arc::new(options);
    let texts = Arc::clone(&options);
    NodeExtension::create("mention")
        .priority(101)
        .group("inline")
        .inline(true)
        .selectable(false)
        .atom()
        .add_attributes(vec![
            ExtensionAttribute::new("id", Value::Null).html_attribute_when_truthy("data-id"),
            ExtensionAttribute::new("label", Value::Null).html_attribute_when_truthy("data-label"),
            ExtensionAttribute::new("mentionSuggestionChar", json!("@"))
                .html_attribute("data-mention-suggestion-char"),
        ])
        .parse_html([ParseHtml::tag(r#"span[data-type="mention"]"#)])
        .render_html(move |node, html| {
            let char = options.char_of(node);
            let configured = SpecAttrs::from(&options.html_attributes);
            let typed = SpecAttrs::from([("data-type", "mention".into())]);
            let merged = merge_attributes(merge_attributes(typed, configured.clone())?, html)?;
            if let Some(render_label) = &options.render_label {
                let label = render_label(node, char)?;
                return Ok(DomSpec::element("span", merged, vec![text(label)]));
            }
            let html = match &options.render_html {
                Some(render_html) => render_html(node, char, merged.clone())?,
                None => {
                    let attrs = merge_attributes(configured, merged)?;
                    let label = label_text(node, char)?;
                    return Ok(DomSpec::element("span", attrs, vec![text(label)]));
                }
            };
            Ok(match html {
                MentionHtml::Spec(spec) => spec,
                MentionHtml::Text(html) => DomSpec::element("span", merged, vec![text(html)]),
            })
        })
        .render_text(move |props| texts.text(props.node))
        .self_closing_inline_markdown_spec(
            "mention",
            "@",
            vec![
                "id".into(),
                "label".into(),
                AllowedAttribute {
                    name: "mentionSuggestionChar",
                    skip_if_default: Some(json!("@")),
                },
            ],
            parse_attributes,
            serialize_attributes,
        )
}

fn text<'a>(text: String) -> DomSpec<'a> {
    DomSpec::Text(Cow::Owned(text))
}

static ATTRIBUTE: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r#"(\w+)=(?:"([^"]*)"|'([^']*)')"#, "g"));

/// Mention's `parseAttributes`: each `key="value"` or `key='value'`, `char` being
/// `mentionSuggestionChar`. Setting `__proto__` to a string sets nothing.
fn parse_attributes(attributes: &[u16]) -> Map {
    let mut parsed = Map::new();
    let mut last_index = 0;
    while let Some(found) = ATTRIBUTE.exec_at(attributes, last_index) {
        last_index = found.end();
        let key = utf16::to_string(found.get(1).unwrap_or_default());
        let value = found.get(2).or(found.get(3)).unwrap_or_default();
        let key = if key == "char" {
            "mentionSuggestionChar".to_owned()
        } else {
            key
        };
        if key != "__proto__" {
            parsed.insert(key.into(), json!(utf16::to_string(value)));
        }
    }
    parsed
}

/// Mention's `serializeAttributes`: each attribute but a `null` one as `key="value"`, `char`
/// being `mentionSuggestionChar`.
fn serialize_attributes(attrs: &Map) -> Result<String, Error> {
    let mut serialized = Vec::new();
    for (key, value) in attrs.iter().filter(|(_, value)| !value.is_null()) {
        let key = match key.as_str() {
            "mentionSuggestionChar" => "char",
            key => key,
        };
        serialized.push(format!("{key}=\"{}\"", js::to_string(value)?));
    }
    Ok(serialized.join(" "))
}
