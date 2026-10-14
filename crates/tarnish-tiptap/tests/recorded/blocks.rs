//! `fixtures/tiptap-blocks.json`, of Tiptap's block and embed extensions, and blockquotes and
//! details nested far deeper than a small stack could recurse through.

use tarnish::json::{Map, Value, json, object};
use tarnish_fixtures::outcome;
use tarnish_js::utf16;
use tarnish_tiptap::Extension;
use tarnish_tiptap::extensions::audio::{AudioOptions, audio};
use tarnish_tiptap::extensions::blockquote::{BlockquoteOptions, blockquote};
use tarnish_tiptap::extensions::bold::bold;
use tarnish_tiptap::extensions::code::{CodeOptions, code};
use tarnish_tiptap::extensions::code_block::{CodeBlockOptions, code_block, code_block_lowlight};
use tarnish_tiptap::extensions::details::{
    DetailsContentOptions, DetailsOptions, DetailsSummaryOptions, details, details_content,
    details_summary,
};
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::emoji::{EmojiItem, EmojiOptions, emoji};
use tarnish_tiptap::extensions::hard_break::hard_break;
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::horizontal_rule::{HorizontalRuleOptions, horizontal_rule};
use tarnish_tiptap::extensions::italic::italic;
use tarnish_tiptap::extensions::mathematics::mathematics;
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::ruby_text::{RubyTextOptions, ruby_text};
use tarnish_tiptap::extensions::table_of_contents::{TableOfContentsOptions, table_of_contents};
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::twitch::{TwitchOptions, twitch};
use tarnish_tiptap::extensions::youtube::{YoutubeOptions, youtube};
use tarnish_tiptap::markdown::specs::{parse_attributes, serialize_attributes};

use crate::own::{self, Configuration, Nested, Replay};

/// Document, Paragraph, Text, Heading, HardBreak, Bold and Italic.
fn base() -> Vec<Extension> {
    vec![
        document().into(),
        paragraph(Default::default()).into(),
        text().into(),
        heading(HeadingOptions::default()).into(),
        hard_break(Default::default()).into(),
        bold(Default::default()).into(),
        italic(Default::default()).into(),
    ]
}

/// The `HTMLAttributes` the recorder gives the extensions whose options it varies.
fn given() -> Map {
    object!({"class": "given more", "style": "color: red", "data-given": "yes"})
}

fn emojis() -> Vec<EmojiItem> {
    let item =
        |name: &str, emoji: Option<&str>, shortcodes: &[&str], image: Option<&str>| EmojiItem {
            name: name.into(),
            emoji: emoji.map(Into::into),
            shortcodes: shortcodes.iter().map(|code| (*code).into()).collect(),
            fallback_image: image.map(Into::into),
        };
    vec![
        item(
            "smile",
            Some("😄"),
            &["smile", "happy"],
            Some("https://e.test/smile.png"),
        ),
        item("star", Some("★"), &["star", "glow"], None),
        item(
            "custom",
            None,
            &["custom_code"],
            Some("https://e.test/custom.png"),
        ),
        item("bare", None, &[], None),
        item("empty", Some(""), &["empty_code"], Some("")),
    ]
}

fn details_trio(options: DetailsOptions, summary: Map, content: Map) -> Vec<Extension> {
    vec![
        details(options).into(),
        details_summary(DetailsSummaryOptions {
            html_attributes: summary,
        })
        .into(),
        details_content(DetailsContentOptions {
            html_attributes: content,
        })
        .into(),
    ]
}

/// The extensions of the configuration the recorder names, after the base ones.
fn added(name: &str) -> Vec<Extension> {
    match name {
        "blocks" => {
            let mut blocks = vec![
                blockquote(BlockquoteOptions::default()).into(),
                code(CodeOptions::default()).into(),
                code_block(CodeBlockOptions::default()).into(),
                horizontal_rule(HorizontalRuleOptions::default()).into(),
            ];
            blocks.extend(details_trio(
                DetailsOptions::default(),
                Map::new(),
                Map::new(),
            ));
            blocks.extend(mathematics());
            blocks.extend([
                emoji(EmojiOptions {
                    emojis: emojis(),
                    ..EmojiOptions::default()
                })
                .into(),
                youtube(YoutubeOptions::default()).into(),
                twitch(TwitchOptions::default()).into(),
                audio(AudioOptions::default()).into(),
                table_of_contents(TableOfContentsOptions::default()).into(),
                ruby_text(RubyTextOptions::default()).into(),
            ]);
            blocks
        }
        "blockquoteOptions" => vec![
            blockquote(BlockquoteOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        "codeOptions" => vec![
            code(CodeOptions {
                html_attributes: given(),
            })
            .into(),
            code_block(CodeBlockOptions {
                language_class_prefix: Some("lang-"),
                default_language: json!("plain"),
                html_attributes: given(),
            })
            .into(),
        ],
        "codeNullPrefix" => vec![
            code(CodeOptions::default()).into(),
            code_block(CodeBlockOptions {
                language_class_prefix: None,
                ..CodeBlockOptions::default()
            })
            .into(),
        ],
        "codeEmptyPrefix" => vec![
            code(CodeOptions::default()).into(),
            code_block(CodeBlockOptions {
                language_class_prefix: Some(""),
                ..CodeBlockOptions::default()
            })
            .into(),
        ],
        "lowlight" => vec![
            code(CodeOptions::default()).into(),
            code_block_lowlight(CodeBlockOptions::default()).into(),
        ],
        "horizontalRuleOptions" => vec![
            horizontal_rule(HorizontalRuleOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        "detailsOptions" => details_trio(
            DetailsOptions {
                persist: true,
                html_attributes: given(),
            },
            object!({"class": "summary"}),
            object!({"class": "content", "data-type": "given"}),
        ),
        "emojiOptions" => vec![
            emoji(EmojiOptions {
                emojis: emojis(),
                force_fallback_images: true,
                html_attributes: given(),
            })
            .into(),
        ],
        "youtubeOptions" => vec![
            youtube(YoutubeOptions {
                allow_fullscreen: false,
                autoplay: true,
                cc_language: Some("es".into()),
                cc_load_policy: Some(true),
                controls: false,
                disable_kb_controls: true,
                enable_iframe_api: true,
                end_time: 90.0,
                height: json!("50%"),
                interface_language: Some("fr".into()),
                iv_load_policy: 3.0,
                r#loop: true,
                modest_branding: true,
                html_attributes: given(),
                inline: false,
                nocookie: true,
                origin: "https://o.test".into(),
                playlist: "a,b".into(),
                progress_bar_color: Some("white".into()),
                width: json!(320.5),
                rel: Some(0.0),
            })
            .into(),
        ],
        "youtubeInline" => vec![
            youtube(YoutubeOptions {
                inline: true,
                rel: None,
                ..YoutubeOptions::default()
            })
            .into(),
        ],
        "twitchOptions" => vec![
            twitch(TwitchOptions {
                allow_fullscreen: false,
                autoplay: true,
                muted: true,
                time: Some("1h2m3s".into()),
                parent: "a b&c=d.test".into(),
                height: json!("50%"),
                width: json!(320),
                html_attributes: given(),
                inline: false,
            })
            .into(),
        ],
        "twitchInline" => vec![
            twitch(TwitchOptions {
                inline: true,
                parent: String::new(),
                ..TwitchOptions::default()
            })
            .into(),
        ],
        "audioOptions" => vec![
            audio(AudioOptions {
                allow_base64: true,
                autoplay: true,
                controls: false,
                r#loop: true,
                muted: true,
                preload: Value::Null,
                controlslist: Some("nodownload".into()),
                crossorigin: Some("anonymous".into()),
                disable_remote_playback: true,
                html_attributes: given(),
                inline: false,
            })
            .into(),
        ],
        "audioInline" => vec![
            audio(AudioOptions {
                inline: true,
                preload: json!("auto"),
                ..AudioOptions::default()
            })
            .into(),
        ],
        "tableOfContentsAnchors" => vec![
            table_of_contents(TableOfContentsOptions {
                anchor_types: &["paragraph", "heading"],
            })
            .into(),
        ],
        "rubyTextOptions" => vec![
            ruby_text(RubyTextOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        "attributeValues" => {
            let other = own::other_attributes;
            let mut configured = vec![
                blockquote(BlockquoteOptions {
                    html_attributes: other(),
                })
                .into(),
                code(CodeOptions {
                    html_attributes: other(),
                })
                .into(),
                code_block(CodeBlockOptions {
                    html_attributes: other(),
                    ..CodeBlockOptions::default()
                })
                .into(),
                horizontal_rule(HorizontalRuleOptions {
                    html_attributes: other(),
                })
                .into(),
            ];
            let details_options = DetailsOptions {
                html_attributes: other(),
                ..DetailsOptions::default()
            };
            configured.extend(details_trio(details_options, other(), other()));
            configured.extend([
                youtube(YoutubeOptions {
                    html_attributes: other(),
                    ..YoutubeOptions::default()
                })
                .into(),
                twitch(TwitchOptions {
                    html_attributes: other(),
                    ..TwitchOptions::default()
                })
                .into(),
                audio(AudioOptions {
                    html_attributes: other(),
                    ..AudioOptions::default()
                })
                .into(),
                ruby_text(RubyTextOptions {
                    html_attributes: other(),
                })
                .into(),
                emoji(EmojiOptions {
                    emojis: emojis(),
                    html_attributes: other(),
                    ..EmojiOptions::default()
                })
                .into(),
            ]);
            configured
        }
        other => panic!("no configuration {other}"),
    }
}

fn configuration(name: &str) -> Configuration {
    let mut extensions = base();
    extensions.extend(added(name));
    extensions.into()
}

/// `parseAttributes` of a string, or `serializeAttributes` of a value.
fn attributes(inputs: &Map) -> Value {
    match (inputs.get("parse"), inputs.get("serialize")) {
        (Some(Value::String(input)), None) => {
            let attributes = parse_attributes(&utf16::from(input));
            json!({ "attributes": Value::Object(attributes) })
        }
        (None, Some(input)) => {
            outcome(serialize_attributes(input).map(|string| json!({ "string": string })))
        }
        _ => panic!("a parse or a serialize"),
    }
}

fn replay() -> Replay {
    let replay = Replay::new("tiptap-blocks", configuration);
    replay.with("attributes", &["parse", "serialize"], attributes)
}

#[test]
fn converts_as_tiptap_does() {
    replay().check();
}

#[test]
fn converts_what_nests_as_deeply_as_memory_allows() {
    replay().check_nested(&NESTED);
}

/// How deep the conversions are held at. Blockquotes' Markdown is written and read in time that
/// grows with the square of the depth, as each level puts its `> ` on all the lines inside it
/// and marked reads them again, and so is details' Markdown, which each level copies and each
/// level's tokenizer reads to the `:::` that closes it.
const DOCUMENTS: usize = 10_000;
const PARSES: usize = 2_000;
const DETAILS: usize = 1_000;

fn x() -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]})
}

fn quoted(depth: usize) -> Value {
    (0..depth).fold(
        x(),
        |inner, _| json!({"type": "blockquote", "content": [inner]}),
    )
}

fn nested_details(depth: usize) -> Value {
    (0..depth).fold(x(), |inner, _| {
        let summary = json!({"type": "detailsSummary", "content": [{"type": "text", "text": "s"}]});
        let content = json!({"type": "detailsContent", "content": [inner]});
        json!({"type": "details", "content": [summary, content]})
    })
}

fn details_markdown(depth: usize) -> String {
    let opening = ":::details\n\n:::detailsSummary\n\ns\n\n:::\n\n:::detailsContent\n\n";
    opening.repeat(depth) + "x" + &"\n\n:::\n\n:::".repeat(depth)
}

fn details_html(depth: usize) -> String {
    let opening = r#"<details><summary>s</summary><div data-type="detailsContent">"#;
    opening.repeat(depth) + "<p>x</p>" + &"</div></details>".repeat(depth)
}

fn doc(node: Value) -> Value {
    json!({"type": "doc", "content": [node]})
}

const NESTED: [Nested; 9] = [
    Nested {
        what: "HTML of blockquotes",
        configuration: "blocks",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| doc(quoted(depth)),
        output: |depth| {
            let html = "<blockquote>".repeat(depth) + "<p>x</p>" + &"</blockquote>".repeat(depth);
            json!({ "html": html })
        },
    },
    Nested {
        what: "HTML of details",
        configuration: "blocks",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| doc(nested_details(depth)),
        output: |depth| json!({ "html": details_html(depth) }),
    },
    // Each blockquote puts `> ` ahead of each line its child renders.
    Nested {
        what: "Markdown of blockquotes",
        configuration: "blocks",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| doc(quoted(depth)),
        output: |depth| json!({ "markdown": "> ".repeat(depth) + "x" }),
    },
    Nested {
        what: "Markdown of details",
        configuration: "blocks",
        section: "serializesMarkdown",
        depth: DOCUMENTS / 5,
        input: |depth| doc(nested_details(depth)),
        output: |depth| json!({ "markdown": details_markdown(depth) }),
    },
    Nested {
        what: "<blockquote>",
        configuration: "blocks",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(("<blockquote>".repeat(depth) + "x").into()),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "<details>",
        configuration: "blocks",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(details_html(depth).into()),
        output: |depth| json!({ "doc": doc(nested_details(depth)) }),
    },
    Nested {
        what: "spaced blockquotes",
        configuration: "blocks",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String(("> ".repeat(depth) + "x").into()),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "blockquotes",
        configuration: "blocks",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String((">".repeat(depth) + " x").into()),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "Markdown details",
        configuration: "blocks",
        section: "parsesMarkdown",
        depth: DETAILS,
        input: |depth| Value::String(details_markdown(depth).into()),
        output: |depth| json!({ "doc": doc(nested_details(depth)) }),
    },
];
