//! What `@tiptap/extension-youtube` and `@tiptap/extension-twitch` each write for themselves: an
//! iframe in a `div[data-…-video]`, whose `src` an embed URL in an element's `src` gives back.

use crate::web_url::WebUrl;
use crate::{DomSpec, HtmlNode, SpecAttrs, merge_attributes};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{Error, value};

/// `getAttributesFrom…EmbedUrl(url)`: the attributes an embed URL names.
pub(crate) type EmbedAttributes = fn(&str) -> Option<Map>;

/// `parseHTML: element => getParsed…Attributes(element)?.[key]`: the attribute of those the
/// embed URL in the element's `src` names, or else of `{ src }`.
pub(crate) fn parsed(
    embed_attributes: EmbedAttributes,
    key: &'static str,
) -> impl Fn(&HtmlNode) -> Option<Value> + Send + Sync + 'static {
    move |element| {
        let src = element.attribute("src").filter(|src| !src.is_empty())?;
        let attributes = embed_attributes(&src);
        let attributes = attributes.unwrap_or_else(|| Map::from_iter([("src".into(), json!(src))]));
        attributes.get(key).cloned()
    }
}

/// `parsedUrl.hostname.replace(/^www\./, '')`.
pub(crate) fn hostname(url: &WebUrl) -> &str {
    let hostname = url.hostname();
    hostname.strip_prefix("www.").unwrap_or(hostname)
}

/// The string `url.match(regex)` is called on.
pub(crate) fn url_to_match(url: Option<&Value>) -> Result<&str, Error> {
    value::get(url, "match")?;
    url.and_then(Value::as_str)
        .ok_or_else(|| value::not_a_function("url.match"))
}

/// `['div', { [data]: '' }, ['iframe', mergeAttributes(this.options.HTMLAttributes, frame,
/// HTMLAttributes)]]`.
pub(crate) fn iframe<'a>(
    data: &'static str,
    html_attributes: &Map,
    frame: SpecAttrs<'a>,
    html: SpecAttrs<'a>,
) -> Result<DomSpec<'a>, Error> {
    let attrs = merge_attributes(SpecAttrs::from(html_attributes), frame)?;
    let iframe = DomSpec::element("iframe", merge_attributes(attrs, html)?, Vec::new());
    Ok(DomSpec::element(
        "div",
        SpecAttrs::from([(data, "".into())]),
        vec![iframe],
    ))
}
