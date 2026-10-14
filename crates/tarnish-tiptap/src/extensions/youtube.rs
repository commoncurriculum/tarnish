//! `@tiptap/extension-youtube`: a video, whose `src` is the YouTube URL it was given or an
//! embed URL names, rendered as an iframe of the embed URL the options make of it.

use std::sync::LazyLock;

use crate::web_url::WebUrl;
use crate::{
    AttrValue, DomSpec, ExtensionAttribute, HtmlNode, NodeExtension, ParseHtml, SpecAttrs,
    merge_attributes,
};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16, value};

pub struct YoutubeOptions {
    pub allow_fullscreen: bool,
    pub autoplay: bool,
    pub cc_language: Option<String>,
    pub cc_load_policy: Option<bool>,
    pub controls: bool,
    pub disable_kb_controls: bool,
    pub enable_iframe_api: bool,
    pub end_time: f64,
    pub height: Value,
    pub interface_language: Option<String>,
    pub iv_load_policy: f64,
    pub r#loop: bool,
    pub modest_branding: bool,
    pub html_attributes: Map,
    pub inline: bool,
    pub nocookie: bool,
    pub origin: String,
    pub playlist: String,
    pub progress_bar_color: Option<String>,
    pub width: Value,
    pub rel: Option<f64>,
}

impl Default for YoutubeOptions {
    fn default() -> Self {
        YoutubeOptions {
            allow_fullscreen: true,
            autoplay: false,
            cc_language: None,
            cc_load_policy: None,
            controls: true,
            disable_kb_controls: false,
            enable_iframe_api: false,
            end_time: 0.0,
            height: json!(480),
            interface_language: None,
            iv_load_policy: 0.0,
            r#loop: false,
            modest_branding: false,
            html_attributes: Map::new(),
            inline: false,
            nocookie: false,
            origin: String::new(),
            playlist: String::new(),
            progress_bar_color: None,
            width: json!(640),
            rel: Some(1.0),
        }
    }
}

pub fn youtube(options: YoutubeOptions) -> NodeExtension {
    let attributes = vec![
        ExtensionAttribute::new("src", Value::Null)
            .parse_html(|element| parsed_attributes(element)?.get("src").cloned()),
        ExtensionAttribute::new("start", json!(0))
            .parse_html(|element| parsed_attributes(element)?.get("start").cloned()),
        ExtensionAttribute::new("width", options.width.clone())
            .parse_html(|element| parsed_dimension(element.attribute("width"))),
        ExtensionAttribute::new("height", options.height.clone())
            .parse_html(|element| parsed_dimension(element.attribute("height"))),
    ];
    let inline = options.inline;
    NodeExtension::create("youtube")
        .inline(inline)
        .group(if inline { "inline" } else { "block" })
        .draggable()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("div[data-youtube-video] iframe")])
        .render_html(move |_, mut html| {
            let start = html.get("start").filter(|start| start.truthy());
            let start = start.map_or(json!(0), AttrValue::to_value);
            let src = html.get("src").map(AttrValue::to_value);
            let embed_url = embed_url_from_youtube_url(src.as_ref(), &options, &start)?;
            html.set("src", embed_url.map_or(Value::Null, Value::String));
            let configured = SpecAttrs::from(&options.html_attributes);
            let iframe = merge_attributes(configured, iframe_options(&options))?;
            Ok(DomSpec::element(
                "div",
                SpecAttrs::from([("data-youtube-video", "".into())]),
                vec![DomSpec::element(
                    "iframe",
                    merge_attributes(iframe, html)?,
                    Vec::new(),
                )],
            ))
        })
        .atom_block_markdown_spec("youtube", &["src", "width", "height", "start"])
}

/// The options the iframe is given ahead of the node's attributes.
fn iframe_options<'a>(options: &YoutubeOptions) -> SpecAttrs<'a> {
    let optional = |value: &Option<String>| value.clone().map_or(Value::Null, Value::String);
    let entries = [
        ("width", options.width.clone()),
        ("height", options.height.clone()),
        ("allowfullscreen", json!(options.allow_fullscreen)),
        ("autoplay", json!(options.autoplay)),
        ("ccLanguage", optional(&options.cc_language)),
        (
            "ccLoadPolicy",
            options.cc_load_policy.map_or(Value::Null, Value::Bool),
        ),
        ("disableKBcontrols", json!(options.disable_kb_controls)),
        ("enableIFrameApi", json!(options.enable_iframe_api)),
        ("endTime", js::number(options.end_time)),
        ("interfaceLanguage", optional(&options.interface_language)),
        ("ivLoadPolicy", js::number(options.iv_load_policy)),
        ("loop", json!(options.r#loop)),
        ("modestBranding", json!(options.modest_branding)),
        ("origin", json!(options.origin.clone())),
        ("playlist", json!(options.playlist.clone())),
        ("progressBarColor", optional(&options.progress_bar_color)),
        ("rel", options.rel.map_or(Value::Null, js::number)),
    ];
    SpecAttrs::from(entries.map(|(name, value)| (name, value.into())))
}

static YOUTUBE_REGEX: LazyLock<RegExp> = LazyLock::new(|| {
    RegExp::new(
        r"^((?:https?:)?\/\/)?((?:www|m|music)\.)?((?:youtube\.com|youtu\.be|youtube-nocookie\.com))(\/(?:[\w-]+\?v=|embed\/|v\/)?)([\w-]+)(\S+)?$",
        "",
    )
});

/// `isValidYoutubeUrl(url)`.
pub fn is_valid_youtube_url(url: &str) -> bool {
    YOUTUBE_REGEX.test(&utf16::from(url))
}

/// `getYoutubeEmbedUrl(nocookie, isPlaylist)`.
fn youtube_embed_url(nocookie: bool, is_playlist: bool) -> &'static str {
    match (is_playlist, nocookie) {
        (true, _) => "https://www.youtube-nocookie.com/embed/videoseries?list=",
        (false, true) => "https://www.youtube-nocookie.com/embed/",
        (false, false) => "https://www.youtube.com/embed/",
    }
}

static VIDEO_ID: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"(?:(v|list)=|shorts\/)([-\w]+)", "gm"));

/// `getEmbedUrlFromYoutubeUrl({ url, ...options, startAt })`, of the node's `src`.
fn embed_url_from_youtube_url(
    url: Option<&Value>,
    options: &YoutubeOptions,
    start_at: &Value,
) -> Result<Option<String>, Error> {
    let url = match url {
        Some(Value::String(url)) => url,
        Some(Value::Null) => return Err(value::cannot_read(value::Nullish::Null, "match")),
        None => return Err(value::cannot_read(value::Nullish::Undefined, "match")),
        Some(_) => return Err(value::not_a_function("url.match")),
    };
    if !is_valid_youtube_url(url) {
        return Ok(None);
    }
    if url.contains("/embed/") {
        return Ok(Some(url.clone()));
    }
    if url.contains("youtu.be") {
        let id = url.rsplit('/').next().unwrap_or_default();
        if id.is_empty() {
            return Ok(None);
        }
        return Ok(Some(format!(
            "{}{id}",
            youtube_embed_url(options.nocookie, false)
        )));
    }
    let units = utf16::from(url);
    let Some(matches) = VIDEO_ID.exec(&units) else {
        return Ok(None);
    };
    let Some(id) = matches.get(2).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let is_list = matches.get(1).is_some_and(|kind| utf16::is(kind, "list"));
    let mut output_url = format!(
        "{}{}",
        youtube_embed_url(options.nocookie, is_list),
        utf16::to_string(id)
    );
    let mut params: Vec<String> = Vec::new();
    let number = js::number_to_string;
    if !options.allow_fullscreen {
        params.push("fs=0".into());
    }
    if options.autoplay {
        params.push("autoplay=1".into());
    }
    if let Some(language) = options.cc_language.as_deref().filter(|it| !it.is_empty()) {
        params.push(format!("cc_lang_pref={language}"));
    }
    if options.cc_load_policy == Some(true) {
        params.push("cc_load_policy=1".into());
    }
    if !options.controls {
        params.push("controls=0".into());
    }
    if options.disable_kb_controls {
        params.push("disablekb=1".into());
    }
    if options.enable_iframe_api {
        params.push("enablejsapi=1".into());
    }
    if js::number_to_boolean(options.end_time) {
        params.push(format!("end={}", number(options.end_time)));
    }
    if let Some(language) = options
        .interface_language
        .as_deref()
        .filter(|it| !it.is_empty())
    {
        params.push(format!("hl={language}"));
    }
    if js::number_to_boolean(options.iv_load_policy) {
        params.push(format!("iv_load_policy={}", number(options.iv_load_policy)));
    }
    if options.r#loop {
        params.push("loop=1".into());
    }
    if options.modest_branding {
        params.push("modestbranding=1".into());
    }
    if !options.origin.is_empty() {
        params.push(format!("origin={}", options.origin));
    }
    if !options.playlist.is_empty() {
        params.push(format!("playlist={}", options.playlist));
    }
    if js::truthy(Some(start_at)) {
        params.push(format!("start={}", js::to_string(start_at)?));
    }
    if let Some(color) = options
        .progress_bar_color
        .as_deref()
        .filter(|it| !it.is_empty())
    {
        params.push(format!("color={color}"));
    }
    if let Some(rel) = options.rel {
        params.push(format!("rel={}", number(rel)));
    }
    if !params.is_empty() {
        output_url += if is_list { "&" } else { "?" };
        output_url += &params.join("&");
    }
    Ok(Some(output_url))
}

/// `getAttributesFromYoutubeEmbedUrl(url)`: the video or playlist an embed URL shows, and
/// where it starts.
pub fn attributes_from_youtube_embed_url(url: &str) -> Option<Map> {
    let parsed = WebUrl::parse(url)?;
    let hostname = parsed.hostname();
    let hostname = hostname.strip_prefix("www.").unwrap_or(hostname);
    if hostname != "youtube.com" && hostname != "youtube-nocookie.com" {
        return None;
    }
    let src = match parsed.pathname() {
        "/embed/videoseries" => {
            let list = parsed
                .search_param("list")
                .filter(|list| !list.is_empty())?;
            format!("https://www.youtube.com/playlist?list={list}")
        }
        pathname => {
            static EMBED: LazyLock<RegExp> =
                LazyLock::new(|| RegExp::new(r"^\/embed\/([\w-]+)$", ""));
            let pathname = utf16::from(pathname);
            let found = EMBED.exec(&pathname)?;
            let id = found.get(1).filter(|id| !id.is_empty())?;
            format!("https://www.youtube.com/watch?v={}", utf16::to_string(id))
        }
    };
    if !is_valid_youtube_url(&src) {
        return None;
    }
    let mut attributes = Map::from_iter([("src".into(), json!(src))]);
    if let Some(start) = parsed
        .search_param("start")
        .filter(|start| !start.is_empty())
    {
        let parsed_start = js::parse_int_radix(&start, 10);
        if !parsed_start.is_nan() {
            attributes.insert("start".into(), js::number(parsed_start));
        }
    }
    Some(attributes)
}

/// `getParsedYoutubeAttributes(element)`.
fn parsed_attributes(element: &HtmlNode) -> Option<Map> {
    let src = element.attribute("src").filter(|src| !src.is_empty())?;
    attributes_from_youtube_embed_url(&src)
        .or_else(|| Some(Map::from_iter([("src".into(), json!(src))])))
}

/// `getParsedDimension(value)`: the number the value spells, or else the value trimmed.
fn parsed_dimension(value: Option<String>) -> Option<Value> {
    let value = value.filter(|value| !value.is_empty())?;
    let trimmed = js::trim(&value);
    if trimmed.is_empty() {
        return None;
    }
    let parsed = js::string_to_number(trimmed);
    Some(match parsed.is_nan() {
        true => json!(trimmed),
        false => js::number(parsed),
    })
}
