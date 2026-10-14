//! `@tiptap/extension-audio`: an audio element, whose source renders only when it's an http or
//! https URL, a path, or with `allow_base64` a base64 `data:audio` URL.

use std::sync::LazyLock;

use crate::attributes::option_default;
use crate::web_url::WebUrl;
use crate::{
    AttrValue, DomSpec, ExtensionAttribute, NodeExtension, ParseHtml, SpecAttrs, merge_attributes,
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
    let attributes = vec![
        ExtensionAttribute::new("src", Value::Null),
        ExtensionAttribute::new("controls", json!(options.controls)),
        ExtensionAttribute::new("autoplay", json!(options.autoplay)),
        ExtensionAttribute::new("loop", json!(options.r#loop)),
        ExtensionAttribute::new("muted", json!(options.muted)),
        ExtensionAttribute::new("preload", options.preload.clone()),
        ExtensionAttribute::new("controlslist", option_default(&options.controlslist)),
        ExtensionAttribute::new("crossorigin", option_default(&options.crossorigin)),
        ExtensionAttribute::new(
            "disableremoteplayback",
            json!(options.disable_remote_playback),
        ),
    ];
    let tag = match options.allow_base64 {
        true => "audio[src]",
        false => r#"audio[src]:not([src^="data:"])"#,
    };
    NodeExtension::create("audio")
        .inline_or_block(options.inline)
        .atom()
        .draggable()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag(tag)])
        .render_html(move |_, mut html| {
            let src = html.get("src").map(AttrValue::to_value);
            let src = sanitize_audio_src(src.as_ref(), options.allow_base64)?;
            html.set("src", Value::from(src));
            let defaults = [
                ("controls", json!(options.controls)),
                ("autoplay", json!(options.autoplay)),
                ("loop", json!(options.r#loop)),
                ("muted", json!(options.muted)),
                ("preload", options.preload.clone()),
                ("controlslist", options.controlslist.clone().into()),
                ("crossorigin", options.crossorigin.clone().into()),
                (
                    "disableremoteplayback",
                    json!(options.disable_remote_playback),
                ),
            ];
            let defaults = defaults.map(|(name, value)| (name, value.into()));
            let merged = merge_attributes(SpecAttrs::from(&options.html_attributes), defaults)?;
            let mut merged = merge_attributes(merged, html)?;
            merged.retain(|_, value| !matches!(value.to_value(), Value::Null | Value::Bool(false)));
            Ok(DomSpec::element("audio", merged, Vec::new()))
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
