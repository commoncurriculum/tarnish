//! What @tiptap/markdown's `MarkdownManager` and ProseMirror's `DOMParser` and `DOMSerializer` do
//! with Tiptap's block and embed extensions, recorded by `harness/record-tiptap-blocks.mjs`:
//! tarnish-tiptap's must make the same schemas, documents, Markdown and HTML, or fail with the
//! same error, in each configuration a record names, and at any depth blockquotes and details
//! nest.

#[allow(dead_code)]
mod own;

use own::{Converter, input_of};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Map, Value, json, object};
use tarnish_fixtures::{expect, outcome, read, records, same};
use tarnish_js::utf16;
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
use tarnish_tiptap::{Extension, TextOptions, generate_text};

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
        "all" => {
            let mut all = vec![
                blockquote(BlockquoteOptions::default()).into(),
                code(CodeOptions::default()).into(),
                code_block(CodeBlockOptions::default()).into(),
                horizontal_rule(HorizontalRuleOptions::default()).into(),
            ];
            all.extend(details_trio(
                DetailsOptions::default(),
                Map::new(),
                Map::new(),
            ));
            all.extend(mathematics());
            all.extend([
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
            all
        }
        "blockquote" => vec![blockquote(BlockquoteOptions::default()).into()],
        "blockquoteOptions" => vec![
            blockquote(BlockquoteOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        "code" => vec![
            code(CodeOptions::default()).into(),
            code_block(CodeBlockOptions::default()).into(),
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
        "codeNullPrefix" | "codeEmptyPrefix" => vec![
            code(CodeOptions::default()).into(),
            code_block(CodeBlockOptions {
                language_class_prefix: (name == "codeEmptyPrefix").then_some(""),
                ..CodeBlockOptions::default()
            })
            .into(),
        ],
        "lowlight" => vec![
            code(CodeOptions::default()).into(),
            code_block_lowlight(CodeBlockOptions::default()).into(),
        ],
        "horizontalRule" => vec![horizontal_rule(HorizontalRuleOptions::default()).into()],
        "horizontalRuleOptions" => vec![
            horizontal_rule(HorizontalRuleOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        "details" => details_trio(DetailsOptions::default(), Map::new(), Map::new()),
        "detailsOptions" => details_trio(
            DetailsOptions {
                persist: true,
                html_attributes: given(),
            },
            object!({"class": "summary"}),
            object!({"class": "content", "data-type": "given"}),
        ),
        "mathematics" => mathematics(),
        "emoji" | "emojiOptions" => vec![
            emoji(EmojiOptions {
                emojis: emojis(),
                force_fallback_images: name == "emojiOptions",
                html_attributes: match name {
                    "emojiOptions" => given(),
                    _ => Map::new(),
                },
            })
            .into(),
        ],
        "youtube" => vec![youtube(YoutubeOptions::default()).into()],
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
        "twitch" => vec![twitch(TwitchOptions::default()).into()],
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
        "audio" => vec![audio(AudioOptions::default()).into()],
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
        "tableOfContents" => vec![table_of_contents(TableOfContentsOptions::default()).into()],
        "tableOfContentsAnchors" => vec![
            table_of_contents(TableOfContentsOptions {
                anchor_types: &["paragraph", "heading"],
            })
            .into(),
        ],
        "rubyText" => vec![ruby_text(RubyTextOptions::default()).into()],
        "rubyTextOptions" => vec![
            ruby_text(RubyTextOptions {
                html_attributes: given(),
            })
            .into(),
        ],
        other => panic!("no configuration {other}"),
    }
}

fn extensions(name: &str) -> Vec<Extension> {
    let mut extensions = base();
    extensions.extend(added(name));
    extensions
}

fn converter(name: &str) -> Converter {
    Converter::of(&extensions(name))
}

/// The configuration a record names.
fn named(record: &Value) -> &str {
    record["extensions"].as_str().expect("a configuration")
}

/// Holds each record of `section` to what its conversion makes of the record's input, in the
/// configuration the record names.
fn check(section: &str) {
    let fixtures = read("tiptap-blocks");
    let mut converters: Vec<(String, Converter)> = Vec::new();
    let mut failures = Vec::new();
    for mut record in records(&fixtures, section).to_vec() {
        let name = named(&record).to_owned();
        if !converters.iter().any(|(known, _)| *known == name) {
            converters.push((name.clone(), converter(&name)));
        }
        let (_, converter) = converters
            .iter()
            .find(|(known, _)| *known == name)
            .expect("the converter");
        let fields = record.as_object_mut().expect("a record");
        let input = fields.remove(input_of(section)).expect("an input");
        fields.remove("extensions");
        let actual = converter.convert(section, &input);
        if !same(&record, &actual) {
            failures.push(format!(
                "{name}: {input}\n  Tiptap: {record}\n  tarnish-tiptap: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn builds_the_schemas_tiptap_does() {
    own::check_schemas("tiptap-blocks", |name| {
        converter(name.expect("a configuration"))
    });
}

#[test]
fn parses_markdown_as_tiptap_does() {
    check("parsesMarkdown");
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    check("serializesMarkdown");
}

#[test]
fn parses_html_as_tiptap_does() {
    check("parsesHTML");
}

#[test]
fn serializes_html_as_tiptap_does() {
    check("serializesHTML");
}

/// The text `generateText` makes of each document, with the extensions the record names.
#[test]
fn generates_text_as_tiptap_does() {
    let fixtures = read("tiptap-blocks");
    let mut failures = Vec::new();
    for record in records(&fixtures, "generatesText") {
        let (name, doc) = (named(record), &record["doc"]);
        let text = generate_text(doc, &extensions(name), &TextOptions::default());
        let actual = outcome(text.map(|text| json!({ "text": text })));
        let mut expected = record.clone();
        let fields = expected.as_object_mut().expect("a record");
        fields.shift_remove("extensions");
        fields.shift_remove("doc");
        if !same(&expected, &actual) {
            failures.push(format!(
                "{name} {doc}\n  Tiptap: {expected}\n  tarnish-tiptap: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// `parseAttributes` of each string, and `serializeAttributes` of each value.
#[test]
fn reads_and_writes_attributes_as_tiptap_does() {
    let fixtures = read("tiptap-blocks");
    let mut failures = Vec::new();
    for mut record in records(&fixtures, "attributes").to_vec() {
        let fields = record.as_object_mut().expect("a record");
        let (parse, serialize) = (fields.remove("parse"), fields.remove("serialize"));
        let actual = match (&parse, serialize) {
            (Some(Value::String(input)), None) => {
                let attributes = parse_attributes(&utf16::from(input));
                outcome(Ok::<_, tarnish::Error>(
                    json!({ "attributes": Value::Object(attributes) }),
                ))
            }
            (None, Some(input)) => {
                outcome(serialize_attributes(&input).map(|string| json!({ "string": string })))
            }
            _ => panic!("a parse or a serialize"),
        };
        if !same(&record, &actual) {
            failures.push(format!("Tiptap: {record}\n  tarnish-tiptap: {actual}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// How deep the conversions are held at. Blockquotes' Markdown is written and read in time that
/// grows with the square of the depth, as each level puts its `> ` on all the lines inside it
/// and marked reads them again, and so is details' Markdown, which each level copies and each
/// level's tokenizer reads to the `:::` that closes it.
const DOCUMENTS: usize = 10_000;
const PARSES: usize = 2_000;
const DETAILS: usize = 1_000;

/// An input nested `depth` deep, and what the conversion a `section` records makes of it, which
/// the test folds from the depths below and holds to Tiptap's at the depths the recorder records.
struct Nested {
    what: &'static str,
    section: &'static str,
    depth: usize,
    input: fn(usize) -> Value,
    output: fn(usize) -> Value,
}

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
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| doc(nested_details(depth)),
        output: |depth| json!({ "html": details_html(depth) }),
    },
    // Each blockquote puts `> ` ahead of each line its child renders.
    Nested {
        what: "Markdown of blockquotes",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| doc(quoted(depth)),
        output: |depth| json!({ "markdown": "> ".repeat(depth) + "x" }),
    },
    Nested {
        what: "Markdown of details",
        section: "serializesMarkdown",
        depth: DOCUMENTS / 5,
        input: |depth| doc(nested_details(depth)),
        output: |depth| json!({ "markdown": details_markdown(depth) }),
    },
    Nested {
        what: "<blockquote>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String("<blockquote>".repeat(depth) + "x"),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "<details>",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(details_html(depth)),
        output: |depth| json!({ "doc": doc(nested_details(depth)) }),
    },
    Nested {
        what: "spaced blockquotes",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String("> ".repeat(depth) + "x"),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "blockquotes",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String(">".repeat(depth) + " x"),
        output: |depth| json!({ "doc": doc(quoted(depth)) }),
    },
    Nested {
        what: "Markdown details",
        section: "parsesMarkdown",
        depth: DETAILS,
        input: |depth| Value::String(details_markdown(depth)),
        output: |depth| json!({ "doc": doc(nested_details(depth)) }),
    },
];

#[test]
fn converts_what_nests_as_deeply_as_memory_allows() {
    let fixtures = read("tiptap-blocks");
    for nested in &NESTED {
        let input = input_of(nested.section);
        for depth in 1..=3 {
            let given = (nested.input)(depth);
            let record = records(&fixtures, nested.section)
                .iter()
                .find(|record| named(record) == "all" && same(&record[input], &given));
            let record = record.unwrap_or_else(|| panic!("no record of {given}"));
            expect(record, &[input, "extensions"], (nested.output)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        let converter = converter("all");
        for nested in &NESTED {
            let converted = converter.convert(nested.section, &(nested.input)(nested.depth));
            let expected = (nested.output)(nested.depth);
            assert!(same(&expected, &converted), "{}", nested.what);
        }
    });
}
