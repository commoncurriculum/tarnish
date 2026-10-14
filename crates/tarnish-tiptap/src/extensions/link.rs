//! `@tiptap/extension-link`: the link mark, which parses and renders only the hrefs
//! `isAllowedUri` allows. Linking as one types or pastes, and opening links, are an editor's, as
//! is linkifyjs, which only they use.

use std::sync::Arc;

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::merge_html_attributes;
use crate::{AttrValue, DomSpec, ExtensionAttribute, MarkExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16, value};
use tarnish_markdown::marked::Token;

/// An `isAllowedUri` option: whether a link may have the href, `None` being `undefined`, given
/// the extension's protocols.
pub type IsAllowedUri =
    Arc<dyn Fn(Option<&Value>, &UriContext) -> Result<bool, Error> + Send + Sync>;

pub struct LinkOptions {
    /// `autolink`, which makes the mark inclusive.
    pub autolink: bool,
    /// The schemes of `protocols`, which `isAllowedUri` allows with its own. A protocol's
    /// `optionalSlashes` is only linkifyjs's.
    pub protocols: Vec<String>,
    pub default_protocol: String,
    pub html_attributes: Map,
    /// `None` for Tiptap's, which is [`UriContext::default_validate`].
    pub is_allowed_uri: Option<IsAllowedUri>,
}

impl Default for LinkOptions {
    fn default() -> Self {
        let mut html_attributes = Map::new();
        html_attributes.insert("target".into(), json!("_blank"));
        html_attributes.insert("rel".into(), json!("noopener noreferrer nofollow"));
        html_attributes.insert("class".into(), Value::Null);
        LinkOptions {
            autolink: true,
            protocols: Vec::new(),
            default_protocol: "http".to_owned(),
            html_attributes,
            is_allowed_uri: None,
        }
    }
}

/// What an `isAllowedUri` option is given besides the href.
pub struct UriContext<'a> {
    pub protocols: &'a [String],
    pub default_protocol: &'a str,
    allowed: &'a AllowedUris,
}

impl UriContext<'_> {
    /// `defaultValidate`: `!!isAllowedUri(url, protocols)`.
    pub fn default_validate(&self, url: Option<&Value>) -> Result<bool, Error> {
        self.allowed.allows(url)
    }
}

/// `isAllowedUri(uri, protocols)`, as a boolean: whether the URI is empty, or once the
/// whitespace DOMPurify strips is gone, starts with an allowed protocol or with what can't be one.
pub fn is_allowed_uri(uri: Option<&Value>, protocols: &[String]) -> Result<bool, Error> {
    AllowedUris::new(protocols).allows(uri)
}

/// The regex `isAllowedUri` matches a URI with.
struct AllowedUris(RegExp);

const ALLOWED_PROTOCOLS: [&str; 10] = [
    "http", "https", "ftp", "ftps", "mailto", "tel", "callto", "sms", "cid", "xmpp",
];

impl AllowedUris {
    fn new(protocols: &[String]) -> Self {
        let custom = protocols.iter().map(String::as_str);
        let allowed = ALLOWED_PROTOCOLS
            .into_iter()
            .chain(custom.filter(|protocol| !protocol.is_empty()));
        let escaped: Vec<String> = allowed.map(escape_for_regex).collect();
        let source = format!(
            r"^(?:(?:{}):|[^a-z]|[a-z0-9+.\-]+(?:[^a-z+.\-:]|$))",
            escaped.join("|")
        );
        AllowedUris(RegExp::new(&source, "i"))
    }

    fn allows(&self, uri: Option<&Value>) -> Result<bool, Error> {
        match uri {
            Some(uri) if js::truthy(Some(uri)) => {
                let Value::String(uri) = uri else {
                    return Err(value::not_a_function("uri.replace"));
                };
                let mut units = utf16::from(uri.as_str());
                units.retain(|&unit| !is_unicode_whitespace(unit));
                Ok(self.0.test(&units))
            }
            _ => Ok(true),
        }
    }
}

/// `[\u0000-   ᠎ -  　]`, DOMPurify's whitespace.
fn is_unicode_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0000..=0x0020 | 0x00A0 | 0x1680 | 0x180E | 0x2000..=0x2029 | 0x205F | 0x3000
    )
}

/// `protocol.replace(/[-/\\^$*+?.()|[\]{}]/g, '\\$&')`.
fn escape_for_regex(protocol: &str) -> String {
    let mut escaped = String::with_capacity(protocol.len());
    for character in protocol.chars() {
        if "-/\\^$*+?.()|[]{}".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The extension's options, and the URIs its protocols allow, as its parse rule and rendering
/// read them.
struct Configured {
    options: LinkOptions,
    allowed: AllowedUris,
}

impl Configured {
    /// `this.options.isAllowedUri(uri, { defaultValidate, protocols, defaultProtocol })`.
    fn is_allowed_uri(&self, uri: Option<&Value>) -> Result<bool, Error> {
        let context = UriContext {
            protocols: &self.options.protocols,
            default_protocol: &self.options.default_protocol,
            allowed: &self.allowed,
        };
        match &self.options.is_allowed_uri {
            Some(is_allowed_uri) => is_allowed_uri(uri, &context),
            None => context.default_validate(uri),
        }
    }
}

pub fn link(options: LinkOptions) -> MarkExtension {
    let default_of = |name: &str| options.html_attributes.get(name).cloned();
    let attributes = vec![
        ExtensionAttribute::new("href", Value::Null)
            .parse_html(|element| element.attribute("href").map(Value::String)),
        ExtensionAttribute::new("target", default_of("target").unwrap_or(Value::Null)),
        ExtensionAttribute::new("rel", default_of("rel").unwrap_or(Value::Null)),
        ExtensionAttribute::new("class", default_of("class").unwrap_or(Value::Null)),
        ExtensionAttribute::new("title", Value::Null),
    ];
    let autolink = options.autolink;
    let configured = Arc::new(Configured {
        allowed: AllowedUris::new(&options.protocols),
        options,
    });
    let parsing = Arc::clone(&configured);
    MarkExtension::create("link")
        .priority(1000)
        .inclusive(autolink)
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("a[href]").get_attrs(move |element| {
            let href = element.attribute("href").filter(|href| !href.is_empty());
            let allowed = match href {
                Some(href) => parsing.is_allowed_uri(Some(&Value::String(href)))?,
                None => false,
            };
            Ok(allowed.then(Map::new))
        })])
        .render_html(move |_, mut html| {
            let href = html.get("href").map(AttrValue::to_value);
            if !configured.is_allowed_uri(href.as_ref())? {
                html.set("href", "");
            }
            let html = merge_html_attributes(&configured.options.html_attributes, html)?;
            Ok(DomSpec::wrapping("a", html))
        })
        .markdown_token_name("link")
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
    // `{ href: token.href, title: token.title || null }`
    let mut attrs = Map::with_capacity(2);
    if let Some(href) = token.href() {
        attrs.push("href".into(), json!(utf16::to_string(href)));
    }
    let title = token.title().filter(|title| !title.is_empty());
    let title = title.map_or(Value::Null, |title| json!(utf16::to_string(title)));
    attrs.push("title".into(), title);
    Ok(helpers.apply_mark("link", content, Some(attrs)))
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    // `node.attrs?.[key] ?? ''`
    let attr = |key| js::coalesce(value::optional(node.get("attrs"), key), &js::EMPTY_STRING);
    let (href, title) = (attr("href"), attr("title"));
    let text = helpers.render_children(node, "")?;
    let href = js::to_string(href)?;
    Ok(if js::truthy(Some(title)) {
        format!("[{text}]({href} \"{}\")", js::to_string(title)?)
    } else {
        format!("[{text}]({href})")
    })
}
