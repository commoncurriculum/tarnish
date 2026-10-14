//! `@tiptap/extension-audio`: an audio element, whose source renders only when it's an http or
//! https URL, a path, or with `allow_base64` a base64 `data:audio` URL.

use std::sync::LazyLock;

use crate::web_url::WebUrl;
use crate::{
    DomSpec, ExtensionAttribute, NodeExtension, ParseHtml, element_attrs, merge_objects,
    rendered_object,
};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16, value};

pub struct AudioOptions {
    pub allow_base64: bool,
    pub autoplay: bool,
    pub controls: bool,
    pub r#loop: bool,
    pub muted: bool,
    /// `preload`: `auto`, `metadata`, `none` or `null`.
    pub preload: Value,
    /// `None` for `undefined`.
    pub controlslist: Option<String>,
    /// `None` for `undefined`.
    pub crossorigin: Option<String>,
    pub disable_remote_playback: bool,
    pub html_attributes: Map,
    pub inline: bool,
}

impl Default for AudioOptions {
    fn default() -> Self {
        AudioOptions {
            allow_base64: false,
            autoplay: false,
            controls: true,
            r#loop: false,
            muted: false,
            preload: json!("metadata"),
            controlslist: None,
            crossorigin: None,
            disable_remote_playback: false,
            html_attributes: Map::new(),
            inline: false,
        }
    }
}

pub fn audio(options: AudioOptions) -> NodeExtension {
    let optional = |name, value: &Option<String>| match value {
        Some(value) => ExtensionAttribute::new(name, json!(value)),
        None => ExtensionAttribute::undefined(name),
    };
    let attributes = vec![
        ExtensionAttribute::new("src", Value::Null),
        ExtensionAttribute::new("controls", json!(options.controls)),
        ExtensionAttribute::new("autoplay", json!(options.autoplay)),
        ExtensionAttribute::new("loop", json!(options.r#loop)),
        ExtensionAttribute::new("muted", json!(options.muted)),
        ExtensionAttribute::new("preload", options.preload.clone()),
        optional("controlslist", &options.controlslist),
        optional("crossorigin", &options.crossorigin),
        ExtensionAttribute::new(
            "disableremoteplayback",
            json!(options.disable_remote_playback),
        ),
    ];
    let inline = options.inline;
    let tag = match options.allow_base64 {
        true => "audio[src]",
        false => r#"audio[src]:not([src^="data:"])"#,
    };
    NodeExtension::create("audio")
        .inline(inline)
        .group(if inline { "inline" } else { "block" })
        .atom()
        .draggable()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag(tag)])
        .render_html(move |_, html| {
            let mut html = rendered_object(&html);
            let src = sanitize_audio_src(html.get("src"), options.allow_base64)?;
            html.insert("src".into(), src.map_or(Value::Null, Value::String));
            let optional =
                |value: &Option<String>| value.clone().map_or(Value::Null, Value::String);
            let defaults = Map::from_iter([
                ("controls".into(), json!(options.controls)),
                ("autoplay".into(), json!(options.autoplay)),
                ("loop".into(), json!(options.r#loop)),
                ("muted".into(), json!(options.muted)),
                ("preload".into(), options.preload.clone()),
                ("controlslist".into(), optional(&options.controlslist)),
                ("crossorigin".into(), optional(&options.crossorigin)),
                (
                    "disableremoteplayback".into(),
                    json!(options.disable_remote_playback),
                ),
            ]);
            let mut merged = merge_objects([&options.html_attributes, &defaults, &html])?;
            merged.retain(|_, value| !matches!(value, Value::Null | Value::Bool(false)));
            Ok(DomSpec::element(
                "audio",
                element_attrs(merged)?,
                Vec::new(),
            ))
        })
        .atom_block_markdown_spec(
            "audio",
            &[
                "src",
                "controls",
                "autoplay",
                "loop",
                "muted",
                "preload",
                "controlslist",
                "crossorigin",
                "disableremoteplayback",
            ],
        )
}

static AUDIO_DATA_URL: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"^data:audio\/[a-zA-Z0-9.+-]+;base64,", "i"));

/// `sanitizeAudioSrc(url, allowBase64)`: the URL, when it's one an audio element may load.
fn sanitize_audio_src(url: Option<&Value>, allow_base64: bool) -> Result<Option<String>, Error> {
    let Some(url) = url.filter(|url| js::truthy(Some(url))) else {
        return Ok(None);
    };
    if allow_base64 && AUDIO_DATA_URL.test(&utf16::from(&js::to_string(url)?)) {
        return Ok(Some(js::to_string(url)?));
    }
    let Value::String(url) = url else {
        return Err(value::not_a_function("url.startsWith"));
    };
    Ok(is_valid_audio_url(url, allow_base64).then(|| url.clone()))
}

/// `isValidAudioUrl(url, allowBase64)`.
pub fn is_valid_audio_url(url: &str, allow_base64: bool) -> bool {
    if url.is_empty() {
        return false;
    }
    if allow_base64 && AUDIO_DATA_URL.test(&utf16::from(url)) {
        return true;
    }
    if ["//", "/", "./", "../"]
        .iter()
        .any(|start| url.starts_with(start))
    {
        return true;
    }
    WebUrl::parse(url)
        .is_some_and(|parsed| matches!(parsed.protocol().as_str(), "http:" | "https:"))
}
