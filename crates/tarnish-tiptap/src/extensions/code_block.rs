//! `@tiptap/extension-code-block`, and `@tiptap/extension-code-block-lowlight`, whose schema,
//! HTML and Markdown are the code block's: it highlights code with a plugin's decorations.

use tarnish::chunk::{Kind, ValueRef};

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::merge_html_attributes;
use crate::{
    DomSpec, ExtensionAttribute, HtmlNode, NodeExtension, ParseHtml, PreserveWhitespace, SpecAttrs,
    node_attr,
};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, utf16, value};
use tarnish_markdown::marked::Token;

pub struct CodeBlockOptions {
    /// `languageClassPrefix`, `None` for `null`.
    pub language_class_prefix: Option<&'static str>,
    pub default_language: Value,
    pub html_attributes: Map,
}

impl Default for CodeBlockOptions {
    fn default() -> Self {
        CodeBlockOptions {
            language_class_prefix: Some("language-"),
            default_language: Value::Null,
            html_attributes: Map::new(),
        }
    }
}

pub fn code_block(options: CodeBlockOptions) -> NodeExtension {
    let prefix = options.language_class_prefix;
    let html_attributes = options.html_attributes;
    NodeExtension::create("codeBlock")
        .content("text*")
        .marks("")
        .group("block")
        .code()
        .defining()
        .add_attributes(vec![
            ExtensionAttribute::new("language", options.default_language)
                .parse_html(move |element| parse_language(element, prefix))
                .not_rendered(),
        ])
        .parse_html([ParseHtml::tag("pre").preserve_whitespace(PreserveWhitespace::Full)])
        .render_html(move |node, html| {
            let mut code = SpecAttrs::new();
            if let Some(class) = language_class(prefix, node_attr(node, "language"))? {
                code.push("class", class.into());
            }
            Ok(DomSpec::element(
                "pre",
                merge_html_attributes(&html_attributes, html)?,
                vec![DomSpec::wrapping("code", code)],
            ))
        })
        .markdown_token_name("code")
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

/// `CodeBlockLowlight`, as the conversions read it.
pub fn code_block_lowlight(options: CodeBlockOptions) -> NodeExtension {
    code_block(options)
}

/// The language a class of the element's first child names after the prefix.
fn parse_language(element: &HtmlNode, prefix: Option<&str>) -> Option<Value> {
    let prefix = prefix.filter(|prefix| !prefix.is_empty())?;
    let first = element.children().into_iter().next()?;
    let classes = first.attribute("class").unwrap_or_default();
    let language = classes
        .split(['\t', '\n', '\u{C}', '\r', ' '])
        .find_map(|class| class.strip_prefix(prefix))?;
    (!language.is_empty()).then(|| Value::String(language.to_string()))
}

/// `node.attrs.language ? languageClassPrefix + node.attrs.language : null`.
fn language_class(prefix: Option<&str>, language: ValueRef) -> Result<Option<String>, Error> {
    if !language.truthy() {
        return Ok(None);
    }
    let class = match prefix {
        Some(prefix) => format!("{prefix}{}", language.to_js_string()?),
        // `null + language` adds a number or a boolean to 0.
        None => match language.kind() {
            Kind::Number(number) => js::number_to_string(number.as_f64()),
            Kind::Bool(_) => "1".to_string(),
            _ => format!("null{}", language.to_js_string()?),
        },
    };
    Ok(Some(class))
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let fenced = ["```", "~~~"]
        .iter()
        .any(|fence| utf16::starts_with(&token.raw, fence));
    if !fenced && token.code_block_style() != Some("indented") {
        return Ok(Parsed::Nodes(Vec::new()));
    }
    let language = token.lang().filter(|lang| !lang.is_empty());
    let language = Value::from(language.map(utf16::to_string));
    let content = match token.text().is_empty() {
        true => Vec::new(),
        false => vec![json!({ "type": "text", "text": utf16::to_string(token.text()) })],
    };
    Ok(Parsed::Node(helpers.create_node(
        "codeBlock",
        Some(Map::from_iter([("language".into(), language)])),
        Some(content),
    )))
}

/// The code between fences of three backticks, the first followed by the language, whatever
/// backticks the code holds.
fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    let language = js::string_or_empty(value::optional(node.get("attrs"), "language"))?;
    Ok(match node.get("content") {
        Some(content) if js::truthy(Some(content)) => {
            let code = helpers.render_children(content, "")?;
            [format!("```{language}"), code, "```".to_string()].join("\n")
        }
        _ => format!("```{language}\n\n```"),
    })
}
