//! The rules an extension's `parseHTML` returns, and `injectExtensionAttributesToParseRule`.

use std::sync::Arc;

use tarnish::dom::{
    Content, ContentElement, ElementRule, GetAttrsResult, Namespace, ParseRule, Rule,
    StyleRule as DomStyleRule, TagRule as DomTagRule,
};
use tarnish::{Map, Value, js};
use tarnish_html::HtmlNode;

use super::attributes::ExtensionAttribute;

/// A tag rule's `getAttrs`, given the element: `None` for `false`.
pub type TagAttrs = Arc<dyn Fn(&HtmlNode) -> Option<Map> + Send + Sync>;
/// A tag rule's `contentElement`, given the element.
pub type ContentHook = Arc<dyn Fn(&HtmlNode) -> HtmlNode + Send + Sync>;
/// A style rule's `getAttrs`, given the style's value.
pub type StyleAttrs = fn(&str) -> GetAttrs;

/// What a style rule's `getAttrs` returns.
pub enum GetAttrs {
    /// `false`: the rule doesn't match.
    False,
    /// `null`: it matches, with no attributes.
    Null,
    Attrs(Map),
}

#[derive(Clone)]
pub enum ParseHtml {
    Tag(TagRule),
    Style(StyleRule),
}

/// `{ tag, ... }`.
#[derive(Clone)]
pub struct TagRule {
    tag: &'static str,
    priority: i32,
    consuming: bool,
    attrs: Map,
    /// `None` for `false`, and empty attributes for `null`, as the attributes Tiptap spreads
    /// over it make of it.
    get_attrs: Option<TagAttrs>,
    content_element: Option<ContentHook>,
}

/// `{ style, ... }`.
#[derive(Clone)]
pub struct StyleRule {
    style: &'static str,
    consuming: bool,
    get_attrs: Option<StyleAttrs>,
    clear_mark: bool,
}

impl ParseHtml {
    pub fn tag(tag: &'static str) -> TagRule {
        TagRule {
            tag,
            priority: 50,
            consuming: true,
            attrs: Map::new(),
            get_attrs: None,
            content_element: None,
        }
    }

    pub fn style(style: &'static str) -> StyleRule {
        StyleRule {
            style,
            consuming: true,
            get_attrs: None,
            clear_mark: false,
        }
    }
}

impl TagRule {
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn not_consuming(mut self) -> Self {
        self.consuming = false;
        self
    }

    pub fn attrs(mut self, attrs: Map) -> Self {
        self.attrs = attrs;
        self
    }

    pub fn get_attrs(
        mut self,
        get_attrs: impl Fn(&HtmlNode) -> Option<Map> + Send + Sync + 'static,
    ) -> Self {
        self.get_attrs = Some(Arc::new(get_attrs));
        self
    }

    pub fn content_element(
        mut self,
        content_element: impl Fn(&HtmlNode) -> HtmlNode + Send + Sync + 'static,
    ) -> Self {
        self.content_element = Some(Arc::new(content_element));
        self
    }
}

impl StyleRule {
    pub fn not_consuming(mut self) -> Self {
        self.consuming = false;
        self
    }

    pub fn get_attrs(mut self, get_attrs: StyleAttrs) -> Self {
        self.get_attrs = Some(get_attrs);
        self
    }

    /// `clearMark: (mark) => mark.type.name === <this mark>`.
    pub fn clears_mark(mut self) -> Self {
        self.clear_mark = true;
        self
    }
}

impl From<TagRule> for ParseHtml {
    fn from(rule: TagRule) -> Self {
        ParseHtml::Tag(rule)
    }
}

impl From<StyleRule> for ParseHtml {
    fn from(rule: StyleRule) -> Self {
        ParseHtml::Style(rule)
    }
}

/// A type's parse rules, with `injectExtensionAttributesToParseRule` applied to each tag rule:
/// every attribute of the extension is read from the element and laid over the rule's own
/// attributes. `name` is the type's.
pub(crate) fn parse_rules(
    name: &'static str,
    rules: &[ParseHtml],
    attributes: &Arc<Vec<ExtensionAttribute>>,
) -> Vec<ParseRule<HtmlNode>> {
    rules
        .iter()
        .map(|rule| match rule.clone() {
            ParseHtml::Tag(TagRule {
                tag,
                priority,
                consuming,
                attrs,
                get_attrs,
                content_element,
            }) => {
                let attributes = Arc::clone(attributes);
                let injected = move |element: &HtmlNode| {
                    let mut result = match &get_attrs {
                        Some(get_attrs) => match get_attrs(element) {
                            Some(attrs) => attrs,
                            None => return Ok(GetAttrsResult::Reject),
                        },
                        None => attrs.clone(),
                    };
                    for attribute in attributes.iter() {
                        let value = match &attribute.parse_html {
                            Some(parse) => parse(element),
                            None => from_string(element.attribute(attribute.name)),
                        };
                        if let Some(value) = value.filter(|value| !value.is_null()) {
                            result.insert(attribute.name.into(), value);
                        }
                    }
                    Ok(GetAttrsResult::Attrs(result))
                };
                let content = match content_element {
                    Some(hook) => Content::Element(ContentElement::Hook(Arc::new(
                        move |element: &HtmlNode| Ok(hook(element)),
                    ))),
                    None => Content::Children,
                };
                ParseRule::Tag(Rule {
                    priority: Some(f64::from(priority)),
                    consuming,
                    ..Rule::new(DomTagRule {
                        tag: tag.into(),
                        namespace: Namespace::Any,
                        get_attrs: Some(Arc::new(injected)),
                        element: ElementRule {
                            content,
                            ..ElementRule::default()
                        },
                    })
                })
            }
            ParseHtml::Style(StyleRule {
                style,
                consuming,
                get_attrs,
                clear_mark,
            }) => ParseRule::Style(Rule {
                consuming,
                ..Rule::new(DomStyleRule {
                    style: style.into(),
                    get_attrs: get_attrs.map(|get_attrs| {
                        Arc::new(move |value: &str| {
                            Ok(match get_attrs(value) {
                                GetAttrs::False => GetAttrsResult::Reject,
                                GetAttrs::Null => GetAttrsResult::Defaults,
                                GetAttrs::Attrs(attrs) => GetAttrsResult::Attrs(attrs),
                            })
                        }) as _
                    }),
                    clear_mark: clear_mark.then(|| {
                        Arc::new(move |mark: &tarnish::Mark<'static>| {
                            Ok(mark.mark_type().name() == name)
                        }) as _
                    }),
                })
            }),
        })
        .collect()
}

/// `fromString(value)`: a number or boolean for a string that spells one.
fn from_string(value: Option<String>) -> Option<Value> {
    let value = value?;
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(&value);
    let (whole, fraction) = match unsigned.rsplit_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => ("", unsigned),
    };
    let numeric = !fraction.is_empty() && digits(fraction) && digits(whole);
    Some(match value.as_str() {
        _ if numeric => js::number(js::string_to_number(&value)),
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(value),
    })
}
