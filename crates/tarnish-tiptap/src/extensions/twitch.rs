//! `@tiptap/extension-twitch`: a video, clip or channel, whose `src` is the Twitch URL it was
//! given or an embed URL names, rendered as an iframe of Twitch's player or clip embed.

use std::sync::LazyLock;

use crate::attributes::option_default;
use crate::web_url::{WebUrl, with_search_params};
use crate::{
    AttrValue, DomSpec, ExtensionAttribute, HtmlNode, NodeExtension, ParseHtml, SpecAttrs,
    merge_attributes,
};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16, value};

pub struct TwitchOptions {
    pub allow_fullscreen: bool,
    pub autoplay: bool,
    pub muted: bool,
    /// `None` for `undefined`.
    pub time: Option<String>,
    pub parent: String,
    pub height: Value,
    pub width: Value,
    pub html_attributes: Map,
    pub inline: bool,
}

impl Default for TwitchOptions {
    fn default() -> Self {
        TwitchOptions {
            allow_fullscreen: true,
            autoplay: false,
            muted: false,
            time: None,
            parent: "localhost".into(),
            height: json!(480),
            width: json!(640),
            html_attributes: Map::new(),
            inline: false,
        }
    }
}

pub fn twitch(options: TwitchOptions) -> NodeExtension {
    let parsed =
        |key: &'static str| move |element: &HtmlNode| parsed_attributes(element)?.get(key).cloned();
    let time = ExtensionAttribute::new("time", option_default(&options.time));
    let attributes = vec![
        ExtensionAttribute::new("src", Value::Null).parse_html(parsed("src")),
        ExtensionAttribute::new("width", options.width.clone())
            .parse_html(|element| parsed_dimension(element.attribute("width"))),
        ExtensionAttribute::new("height", options.height.clone())
            .parse_html(|element| parsed_dimension(element.attribute("height"))),
        ExtensionAttribute::new("autoplay", json!(options.autoplay)).parse_html(parsed("autoplay")),
        ExtensionAttribute::new("muted", json!(options.muted)).parse_html(parsed("muted")),
        time.parse_html(parsed("time")),
    ];
    let inline = options.inline;
    NodeExtension::create("twitch")
        .inline(inline)
        .group(if inline { "inline" } else { "block" })
        .draggable()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("div[data-twitch-video] iframe")])
        .render_html(move |_, mut html| {
            let value = |key: &str| html.get(key).map(AttrValue::to_value);
            let or_option =
                |key: &str, option: Value| js::coalesce(value(key).as_ref(), &option).clone();
            let embed = Embed {
                allow_fullscreen: options.allow_fullscreen,
                autoplay: or_option("autoplay", json!(options.autoplay)),
                muted: or_option("muted", json!(options.muted)),
                time: or_option(
                    "time",
                    options.time.clone().map_or(Value::Null, Value::String),
                ),
                parent: &options.parent,
            };
            let Some(embed_url) = embed_url_from_twitch_url(value("src").as_ref(), &embed)? else {
                let invalid = DomSpec::Text("Invalid Twitch URL".into());
                return Ok(DomSpec::element("div", SpecAttrs::new(), vec![invalid]));
            };
            html.set("src", embed_url);
            let frame = SpecAttrs::from([
                ("width", options.width.clone().into()),
                ("height", options.height.clone().into()),
                ("allowfullscreen", json!(options.allow_fullscreen).into()),
                ("scrolling", "no".into()),
                ("frameborder", "0".into()),
            ]);
            let iframe = merge_attributes(SpecAttrs::from(&options.html_attributes), frame)?;
            Ok(DomSpec::element(
                "div",
                SpecAttrs::from([("data-twitch-video", "".into())]),
                vec![DomSpec::element(
                    "iframe",
                    merge_attributes(iframe, html)?,
                    Vec::new(),
                )],
            ))
        })
        .atom_block_markdown_spec(
            "twitch",
            &["src", "width", "height", "autoplay", "muted", "time"],
        )
}

static TWITCH_REGEX: LazyLock<RegExp> = LazyLock::new(|| {
    RegExp::new(
        r"^(https?:\/\/)?(www\.)?(twitch\.tv|clips\.twitch\.tv)\/(?:videos\/(\d+)|(\w+)\/clip\/([\w-]+)|([\w-]+)(?:\/)?)?(\?.*)?$",
        "",
    )
});

/// `isValidTwitchUrl(url)`.
pub fn is_valid_twitch_url(url: &str) -> bool {
    TWITCH_REGEX.test(&utf16::from(url))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Clip,
    Channel,
}

/// `getTwitchIdentifier(url)`.
fn twitch_identifier(url: &str) -> Option<(Kind, String)> {
    static CLIP: LazyLock<RegExp> =
        LazyLock::new(|| RegExp::new(r"clips\.twitch\.tv\/([\w-]+)", ""));
    static VIDEO: LazyLock<RegExp> =
        LazyLock::new(|| RegExp::new(r"twitch\.tv\/videos\/(\d+)", ""));
    static CHANNEL_CLIP: LazyLock<RegExp> =
        LazyLock::new(|| RegExp::new(r"twitch\.tv\/([\w-]+)\/clip\/([\w-]+)", ""));
    static CHANNEL: LazyLock<RegExp> =
        LazyLock::new(|| RegExp::new(r"twitch\.tv\/([\w-]+)(?:\/)?$", ""));
    if !is_valid_twitch_url(url) {
        return None;
    }
    let clean_url = url.split('?').next().unwrap_or_default();
    let units = utf16::from(clean_url);
    let group = |regex: &RegExp, group: usize| {
        regex
            .exec(&units)
            .map(|found| utf16::to_string(found.get(group).unwrap_or_default()))
    };
    if clean_url.contains("clips.twitch.tv/") {
        return group(&CLIP, 1).map(|id| (Kind::Clip, id));
    }
    if clean_url.contains("twitch.tv/") {
        if let Some(id) = group(&VIDEO, 1) {
            return Some((Kind::Video, id));
        }
        if let Some(id) = group(&CHANNEL_CLIP, 2) {
            return Some((Kind::Clip, id));
        }
        if let Some(id) = group(&CHANNEL, 1) {
            return Some((Kind::Channel, id));
        }
    }
    None
}

/// The options of `getEmbedUrlFromTwitchUrl` besides the URL.
struct Embed<'o> {
    allow_fullscreen: bool,
    autoplay: Value,
    muted: Value,
    time: Value,
    parent: &'o str,
}

/// `getEmbedUrlFromTwitchUrl({ url, ...embed })`, of the node's `src`.
fn embed_url_from_twitch_url(url: Option<&Value>, embed: &Embed) -> Result<Option<String>, Error> {
    let url = match url {
        Some(Value::String(url)) => url,
        Some(Value::Null) => return Err(value::cannot_read(value::Nullish::Null, "match")),
        None => return Err(value::cannot_read(value::Nullish::Undefined, "match")),
        Some(_) => return Err(value::not_a_function("url.match")),
    };
    let Some((kind, id)) = twitch_identifier(url) else {
        return Ok(None);
    };
    let parent = match embed.parent.is_empty() {
        true => "localhost",
        false => embed.parent,
    };
    let time = match kind == Kind::Video && js::truthy(Some(&embed.time)) {
        true => Some(js::to_string(&embed.time)?),
        false => None,
    };
    let mut params: Vec<(&str, &str)> = Vec::new();
    let (base, name) = match kind {
        Kind::Clip => ("https://clips.twitch.tv/embed", "clip"),
        Kind::Video => ("https://player.twitch.tv/", "video"),
        Kind::Channel => ("https://player.twitch.tv/", "channel"),
    };
    params.push((name, &id));
    params.push(("parent", parent));
    if kind != Kind::Clip && embed.allow_fullscreen {
        params.push(("allowfullscreen", "true"));
    }
    if js::truthy(Some(&embed.autoplay)) {
        params.push(("autoplay", "true"));
    }
    if js::truthy(Some(&embed.muted)) {
        params.push(("muted", "true"));
    }
    if let Some(time) = &time {
        params.push(("time", time));
    }
    Ok(Some(with_search_params(base, &params)))
}

/// `getAttributesFromTwitchEmbedUrl(url)`: the video, channel or clip an embed URL shows, and
/// how it plays.
pub fn attributes_from_twitch_embed_url(url: &str) -> Option<Map> {
    let parsed = WebUrl::parse(url)?;
    let hostname = parsed.hostname();
    let hostname = hostname.strip_prefix("www.").unwrap_or(hostname);
    let filled = |name: &str| parsed.search_param(name).filter(|value| !value.is_empty());
    let src = if hostname == "player.twitch.tv" {
        match (filled("video"), filled("channel")) {
            (Some(video), _) => format!("https://www.twitch.tv/videos/{video}"),
            (None, Some(channel)) => format!("https://www.twitch.tv/{channel}"),
            (None, None) => return None,
        }
    } else if hostname == "clips.twitch.tv" && parsed.pathname() == "/embed" {
        format!("https://clips.twitch.tv/{}", filled("clip")?)
    } else {
        return None;
    };
    let mut result = Map::from_iter([("src".into(), json!(src.clone()))]);
    if parsed.search_param("autoplay").as_deref() == Some("true") {
        result.push("autoplay".into(), Value::Bool(true));
    }
    if parsed.search_param("muted").as_deref() == Some("true") {
        result.push("muted".into(), Value::Bool(true));
    }
    if let Some(time) = filled("time") {
        result.push("time".into(), Value::String(time));
    }
    is_valid_twitch_url(&src).then_some(result)
}

/// `getParsedTwitchAttributes(element)`.
fn parsed_attributes(element: &HtmlNode) -> Option<Map> {
    let src = element.attribute("src").filter(|src| !src.is_empty())?;
    attributes_from_twitch_embed_url(&src)
        .or_else(|| Some(Map::from_iter([("src".into(), json!(src))])))
}

/// `getParsedDimension(value)`: the integer the value starts with.
fn parsed_dimension(value: Option<String>) -> Option<Value> {
    let value = value.filter(|value| !value.is_empty())?;
    let parsed = js::parse_int_radix(&value, 10);
    (!parsed.is_nan()).then(|| js::number(parsed))
}
