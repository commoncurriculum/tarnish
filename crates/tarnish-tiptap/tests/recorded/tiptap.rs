//! `fixtures/tiptap.json`, of Tiptap's own extensions, alone, with an option set or beside an
//! application's, and of every group's together, and documents, HTML and Markdown nested far
//! deeper than a small stack could recurse through. Parsing takes time that grows with the square
//! of the depth (the cube, when a lazy continuation line re-lexes every nested blockquote), so
//! parses nest less deeply.

use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Map, Value, json, object};
use tarnish::model::Whitespace;
use tarnish_fixtures::{read, same, version};
use tarnish_tiptap::extensions::audio::audio;
use tarnish_tiptap::extensions::blockquote::blockquote;
use tarnish_tiptap::extensions::bold::{BoldOptions, bold};
use tarnish_tiptap::extensions::bullet_list::bullet_list;
use tarnish_tiptap::extensions::code::code;
use tarnish_tiptap::extensions::code_block::code_block;
use tarnish_tiptap::extensions::details::{details, details_content, details_summary};
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::emoji::emoji;
use tarnish_tiptap::extensions::hard_break::{HardBreakOptions, hard_break};
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::highlight::{HighlightOptions, highlight};
use tarnish_tiptap::extensions::horizontal_rule::horizontal_rule;
use tarnish_tiptap::extensions::image::{ImageOptions, image};
use tarnish_tiptap::extensions::italic::{ItalicOptions, italic};
use tarnish_tiptap::extensions::link::link;
use tarnish_tiptap::extensions::list_item::list_item;
use tarnish_tiptap::extensions::mathematics::mathematics;
use tarnish_tiptap::extensions::mention::mention;
use tarnish_tiptap::extensions::ordered_list::ordered_list;
use tarnish_tiptap::extensions::paragraph::{ParagraphOptions, paragraph};
use tarnish_tiptap::extensions::ruby_text::ruby_text;
use tarnish_tiptap::extensions::strike::{StrikeOptions, strike};
use tarnish_tiptap::extensions::subscript::{SubscriptOptions, subscript};
use tarnish_tiptap::extensions::superscript::{SuperscriptOptions, superscript};
use tarnish_tiptap::extensions::table::{table, table_cell, table_header, table_row};
use tarnish_tiptap::extensions::table_of_contents::table_of_contents;
use tarnish_tiptap::extensions::task_item::{TaskItemOptions, task_item};
use tarnish_tiptap::extensions::task_list::task_list;
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::text_align::{TextAlignOptions, text_align};
use tarnish_tiptap::extensions::text_style::{
    ColorOptions, TextStyleOptions, background_color, color, font_family, font_size, line_height,
    text_style,
};
use tarnish_tiptap::extensions::twitch::twitch;
use tarnish_tiptap::extensions::underline::{UnderlineOptions, underline};
use tarnish_tiptap::extensions::youtube::youtube;
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension,
    NodeExtension, ParseHtml, PlainExtension, get_schema, html,
};

use crate::own::{self, Configuration, Nested, Replay};

fn configuration(name: &str) -> Configuration {
    match name {
        "own" => own::extensions(),
        "colors" => colors(),
        "code" => application_code(),
        "htmlAttributes" => configured(
            strings(),
            &[
                (
                    "image",
                    object!({"alt": "option", "title": "option", "width": 5}),
                ),
                ("highlight", object!({"data-color": "option"})),
            ],
        ),
        "htmlAttributeValues" => configured(
            own::other_attributes(),
            &[
                ("subscript", object!({"style": 5})),
                ("superscript", object!({"class": 0, "data-given": null})),
            ],
        ),
        "levels" => replaced(heading(HeadingOptions {
            levels: &[4, 2],
            ..HeadingOptions::default()
        })),
        "noLevels" => replaced(heading(HeadingOptions {
            levels: &[],
            ..HeadingOptions::default()
        })),
        "multicolor" => replaced(highlight(HighlightOptions {
            multicolor: true,
            ..HighlightOptions::default()
        })),
        "inline" => replaced(image(ImageOptions {
            inline: true,
            ..ImageOptions::default()
        })),
        "base64" => replaced(image(ImageOptions {
            allow_base64: true,
            ..ImageOptions::default()
        })),
        "renderedValues" => {
            let mut extensions = replaced(paragraph(ParagraphOptions {
                html_attributes: object!({"class": "option", "style": "color: blue"}),
            }));
            extensions.push(renders().into());
            extensions
        }
        "nodeType" => node_type(),
        "application" => application(),
        "all" => all(),
        other => panic!("no configuration {other}"),
    }
    .into()
}

/// Every group's extensions, as the other modules give them: Tiptap's own, the lists, the
/// tables, the blocks and embeds, and the inline extensions, TextAlign on headings and paragraphs.
fn all() -> Vec<Extension> {
    let mut all = own::extensions();
    all.extend([
        bullet_list(Default::default()).into(),
        ordered_list(Default::default()).into(),
        list_item(Default::default()).into(),
        task_list(Default::default()).into(),
        task_item(Default::default()).into(),
        table(Default::default()).into(),
        table_row(Default::default()).into(),
        table_cell(Default::default()).into(),
        table_header(Default::default()).into(),
        blockquote(Default::default()).into(),
        code(Default::default()).into(),
        code_block(Default::default()).into(),
        horizontal_rule(Default::default()).into(),
        details(Default::default()).into(),
        details_summary(Default::default()).into(),
        details_content(Default::default()).into(),
    ]);
    all.extend(mathematics());
    all.extend([
        emoji(Default::default()).into(),
        youtube(Default::default()).into(),
        twitch(Default::default()).into(),
        audio(Default::default()).into(),
        table_of_contents(Default::default()).into(),
        ruby_text(Default::default()).into(),
        link(Default::default()).into(),
        mention(Default::default()).into(),
        background_color(Default::default()).into(),
        color(Default::default()).into(),
        font_family(Default::default()).into(),
        font_size(Default::default()).into(),
        line_height(Default::default()).into(),
        text_align(TextAlignOptions {
            types: &["heading", "paragraph"],
            ..TextAlignOptions::default()
        })
        .into(),
    ]);
    all
}

/// Tiptap's own extensions, with `extension` in place of the one of its name.
fn replaced(extension: impl Into<Extension>) -> Vec<Extension> {
    let extension = extension.into();
    let mut extensions = own::extensions();
    let replaced = extensions.iter().position(|own| own.name == extension.name);
    extensions[replaced.expect("one of Tiptap's own")] = extension;
    extensions
}

/// Tiptap's own extensions, Highlight with its colors and after TextStyle, and Color.
fn colors() -> Vec<Extension> {
    let mut colors = own::extensions();
    colors.retain(|extension| extension.name != "highlight");
    colors.push(
        highlight(HighlightOptions {
            multicolor: true,
            ..HighlightOptions::default()
        })
        .into(),
    );
    colors.push(color(ColorOptions::default()).into());
    colors
}

/// HTMLAttributes of strings, which Bold's JSX leaves `children` out of, `undefined` as `null`.
fn strings() -> Map {
    object!({
        "class": "option more",
        "style": "color: blue; font-size: 2px",
        "data-given": "option",
        "data-null": null,
        "data-undefined": null,
        "data-number": 1,
        "children": "child",
    })
}

/// Tiptap's own extensions, each with `attributes` and what `extra` adds for it, Highlight with
/// its colors, and [`given`].
fn configured(attributes: Map, extra: &[(&str, Map)]) -> Vec<Extension> {
    let html = |name: &str| {
        let mut html = attributes.clone();
        if let Some((_, added)) = extra.iter().find(|(extended, _)| *extended == name) {
            html.extend(added.clone());
        }
        html
    };
    vec![
        document().into(),
        paragraph(ParagraphOptions {
            html_attributes: html("paragraph"),
        })
        .into(),
        text().into(),
        heading(HeadingOptions {
            html_attributes: html("heading"),
            ..HeadingOptions::default()
        })
        .into(),
        image(ImageOptions {
            html_attributes: html("image"),
            ..ImageOptions::default()
        })
        .into(),
        hard_break(HardBreakOptions {
            html_attributes: html("hardBreak"),
        })
        .into(),
        bold(BoldOptions {
            html_attributes: html("bold"),
        })
        .into(),
        italic(ItalicOptions {
            html_attributes: html("italic"),
        })
        .into(),
        strike(StrikeOptions {
            html_attributes: html("strike"),
        })
        .into(),
        underline(UnderlineOptions {
            html_attributes: html("underline"),
        })
        .into(),
        subscript(SubscriptOptions {
            html_attributes: html("subscript"),
        })
        .into(),
        superscript(SuperscriptOptions {
            html_attributes: html("superscript"),
        })
        .into(),
        highlight(HighlightOptions {
            multicolor: true,
            html_attributes: html("highlight"),
        })
        .into(),
        text_style(TextStyleOptions::default()).into(),
        given().into(),
    ]
}

/// An application's attribute on each type with HTMLAttributes, which renders a class, a style and
/// an attribute of the options'.
fn given() -> PlainExtension {
    let given =
        ExtensionAttribute::new("given", Value::Null).render_html(|attributes, rendered| {
            match attributes.get("given") {
                Some(given) if given.truthy() => {
                    rendered.merge("class", given)?;
                    rendered.merge("style", format!("color: {}", given.to_js_string()?))?;
                    rendered.merge("data-given", given)
                }
                _ => Ok(()),
            }
        });
    let types = [
        "paragraph",
        "heading",
        "image",
        "hardBreak",
        "bold",
        "italic",
        "strike",
        "underline",
        "subscript",
        "superscript",
        "highlight",
    ];
    PlainExtension::create("given").add_global_attributes(move |_| {
        vec![GlobalAttributes {
            types: GlobalTypes::Named(types.to_vec()),
            attributes: vec![given.clone()],
        }]
    })
}

/// An application's two attributes on paragraphs and bold, each rendering the object a document
/// gives it.
fn renders() -> PlainExtension {
    let attribute = |name: &'static str| {
        ExtensionAttribute::new(name, Value::Null).render_html(move |attributes, rendered| {
            if let Some(object) = attributes.get(name).filter(|object| object.is_object()) {
                for (key, value) in object.entries() {
                    rendered.merge(key, value)?;
                }
            }
            Ok(())
        })
    };
    let attributes = vec![attribute("first"), attribute("second")];
    PlainExtension::create("renders").add_global_attributes(move |_| {
        vec![GlobalAttributes {
            types: GlobalTypes::Named(vec!["paragraph", "bold"]),
            attributes: attributes.clone(),
        }]
    })
}

/// An application's code mark and code block, whose text stays as it is, and a figure whose
/// attribute renders under a name it makes.
fn application_code() -> Vec<Extension> {
    let code = MarkExtension::create("code")
        .code()
        .render_markdown(|node, helpers, _| {
            Ok(["`", &helpers.render_children(node, "")?, "`"].concat())
        });
    let block = NodeExtension::create("codeBlock")
        .code()
        .group("block")
        .content("text*")
        .parse_html([ParseHtml::tag("pre")])
        .render_markdown(|node, helpers, _| {
            Ok(["```\n", &helpers.render_children(node, "")?, "\n```"].concat())
        });
    let name = ["data", "made"].join("-");
    let figure = NodeExtension::create("figure")
        .group("block")
        .parse_html([ParseHtml::tag("figure")])
        .render_html(|_, html| Ok(DomSpec::element("figure", html, Vec::new())))
        .add_attributes(vec![
            ExtensionAttribute::new("made", Value::Null).render_html(move |attrs, rendered| {
                rendered.merge(name.clone(), attrs.get("made").expect("its attribute"))
            }),
        ]);
    vec![
        document().into(),
        paragraph(ParagraphOptions::default()).into(),
        text().into(),
        code.into(),
        block.into(),
        figure.into(),
    ]
}

/// HTMLAttributes holding a `nodeType`.
fn node_typed(node_type: Value) -> Map {
    object!({"data-x": "y", "nodeType": node_type})
}

/// Paragraph, Heading, HardBreak, Bold and Italic with a `nodeType` among their HTMLAttributes,
/// and a node whose attribute renders under the name `nodeType`.
fn node_type() -> Vec<Extension> {
    let typed = NodeExtension::create("typed")
        .group("block")
        .atom()
        .add_attributes(vec![ExtensionAttribute::new("nodeType", Value::Null)])
        .render_html(|_, html| Ok(DomSpec::element("div", html, Vec::new())));
    vec![
        document().into(),
        paragraph(ParagraphOptions {
            html_attributes: node_typed(Value::Null),
        })
        .into(),
        text().into(),
        heading(HeadingOptions {
            html_attributes: node_typed(json!(1)),
            ..HeadingOptions::default()
        })
        .into(),
        hard_break(HardBreakOptions {
            html_attributes: node_typed(json!(0)),
        })
        .into(),
        bold(BoldOptions {
            html_attributes: node_typed(json!("1")),
        })
        .into(),
        italic(ItalicOptions {
            html_attributes: node_typed(json!("x")),
        })
        .into(),
        typed.into(),
    ]
}

/// The lists, TaskItem nested, and an application's blocks: one whose Markdown is the level its
/// renderer is given, and one whose whitespace is `pre`.
fn application() -> Vec<Extension> {
    let leveled = NodeExtension::create("leveled")
        .group("block")
        .content("inline*")
        .render_markdown(|node, helpers, context| {
            Ok(format!(
                "{}> {}",
                context.level,
                helpers.render_children(node, "")?
            ))
        });
    let preformatted = NodeExtension::create("preformatted")
        .group("block")
        .content("text*")
        .whitespace(Whitespace::Pre)
        .parse_html([ParseHtml::tag("div[data-pre]")]);
    vec![
        document().into(),
        paragraph(ParagraphOptions::default()).into(),
        text().into(),
        bullet_list(Default::default()).into(),
        ordered_list(Default::default()).into(),
        list_item(Default::default()).into(),
        task_list(Default::default()).into(),
        task_item(TaskItemOptions {
            nested: true,
            ..TaskItemOptions::default()
        })
        .into(),
        leveled.into(),
        preformatted.into(),
    ]
}

fn replay() -> Replay {
    Replay::new("tiptap", configuration)
}

#[test]
fn converts_as_tiptap_does() {
    replay().check();
}

#[test]
fn converts_what_nests_as_deeply_as_memory_allows() {
    replay().check_nested(&NESTED);
}

/// Each Tiptap package installed, the extensions among them, is the version ported.
#[test]
fn ports_the_installed_tiptap() {
    let versions = read("versions");
    let tiptap = versions.as_object().expect("versions").keys();
    let tiptap: Vec<_> = tiptap
        .filter(|key| key.starts_with("@tiptap/") && !key.contains(" > "))
        .collect();
    assert!(tiptap.len() > 1, "{tiptap:?}");
    for package in tiptap {
        assert_eq!(version(package), tarnish_tiptap::TIPTAP, "{package}");
    }
    let flat_list = version("tiptap-extension-flat-list");
    assert_eq!(flat_list, tarnish_tiptap::FLAT_LIST);
}

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
const INLINES: usize = 500;
const LAZY: usize = 500;
/// Blockquotes nest tables less deeply, as each level lexes the rest of its source again.
const QUOTES: usize = 500;
/// How deep blocks nest in a style's CSS, which the CSS engine reads in time that grows with
/// the depth.
const CSSES: usize = 100_000;

/// `node_type` inside itself, `depth` deep, around a text node.
fn nested(node_type: &str, depth: usize) -> Value {
    let text = json!({"type": "text", "text": "x"});
    let nested = (0..depth).fold(
        text,
        |inner, _| json!({"type": node_type, "content": [inner]}),
    );
    json!({"type": "doc", "content": [nested]})
}

fn around(open: &str, close: &str, depth: usize) -> String {
    open.repeat(depth) + "x" + &close.repeat(depth)
}

fn markdown(open: &str, close: &str, depth: usize) -> Value {
    json!({"markdown": around(open, close, depth)})
}

fn doc(content: Vec<Value>) -> Value {
    json!({"doc": {"type": "doc", "content": content}})
}

fn in_paragraph(content: Value) -> Value {
    json!({"type": "paragraph", "content": [content]})
}

/// `>` four times as deep as `depth`, around Markdown nested `depth` deep.
fn quoted(inside: String, depth: usize) -> Value {
    Value::String((">".repeat(4 * depth) + " " + &inside).into())
}

/// The text inside a mark's delimiters `depth` deep, each part of it with a mark for each pair
/// of delimiters around it: @tiptap/markdown's `applyMarkToContent` adds a mark however many of
/// the type the text has.
fn marked(mark: &str, depth: usize) -> Vec<Value> {
    let marks = |count: usize| vec![json!({"type": mark}); count];
    let text = |text: &str, count| json!({"type": "text", "text": text, "marks": marks(count)});
    let opening = (1..depth).map(|count| text("a ", count));
    let closing = (1..depth).rev().map(|count| text(" a", count));
    let middle = text("a x a", depth);
    opening.chain([middle]).chain(closing).collect()
}

fn paragraph_of(content: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": content})
}

/// `node` in blockquotes `depth` deep.
fn quoted_doc(node: Value, depth: usize) -> Value {
    let quoted = (0..depth).fold(
        node,
        |inner, _| json!({"type": "blockquote", "content": [inner]}),
    );
    doc(vec![quoted])
}

/// A table of one header cell holding `content`, as Markdown parses to it.
fn header_table(content: Value) -> Value {
    let header = json!({"type": "tableHeader", "content": [in_paragraph(content)]});
    json!({"type": "table", "content": [{"type": "tableRow", "content": [header]}]})
}

const NESTED: [Nested; 20] = [
    Nested {
        what: "HTML of paragraphs",
        configuration: "own",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| nested("paragraph", depth),
        output: |depth| json!({"html": around("<p>", "</p>", depth)}),
    },
    // The manager renders a node of any type it has a renderer for, a mark's among them.
    Nested {
        what: "Markdown of docs",
        configuration: "own",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("doc", depth),
        output: |_| json!({"markdown": "x"}),
    },
    Nested {
        what: "Markdown of paragraphs",
        configuration: "own",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("paragraph", depth),
        output: |_| json!({"markdown": "x"}),
    },
    Nested {
        what: "Markdown of bolds",
        configuration: "own",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("bold", depth),
        output: |depth| markdown("**", "**", depth),
    },
    Nested {
        what: "Markdown of italics",
        configuration: "own",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("italic", depth),
        output: |depth| markdown("*", "*", depth),
    },
    Nested {
        what: "Markdown of strikes",
        configuration: "own",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| nested("strike", depth),
        output: |depth| markdown("~~", "~~", depth),
    },
    Nested {
        what: "<div>",
        configuration: "own",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(("<div>".repeat(depth) + "x").into()),
        output: |_| doc(vec![in_paragraph(json!({"type": "text", "text": "x"}))]),
    },
    // Each paragraph opened inside one closes it.
    Nested {
        what: "<p>",
        configuration: "own",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(("<p>".repeat(depth) + "x").into()),
        output: |depth| {
            let mut content = vec![json!({"type": "paragraph"}); depth - 1];
            content.push(in_paragraph(json!({"type": "text", "text": "x"})));
            doc(content)
        },
    },
    Nested {
        what: "<b>",
        configuration: "own",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(("<b>".repeat(depth) + "x").into()),
        output: |_| {
            let text = json!({"type": "text", "marks": [{"type": "bold"}], "text": "x"});
            doc(vec![in_paragraph(text)])
        },
    },
    Nested {
        what: "<span style=color:red>",
        configuration: "own",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| Value::String(("<span style=color:red>".repeat(depth) + "x").into()),
        output: |_| {
            let text = json!({"type": "text", "marks": [{"type": "textStyle"}], "text": "x"});
            doc(vec![in_paragraph(text)])
        },
    },
    // Without an extension for blockquotes or lists, the manager parses a blockquote's content
    // and drops a list.
    Nested {
        what: "blockquotes",
        configuration: "own",
        section: "parsesMarkdown",
        depth: PARSES / 4,
        input: |depth| quoted("x".into(), depth),
        output: |_| doc(vec![in_paragraph(json!({"type": "text", "text": "x"}))]),
    },
    Nested {
        what: "lazy continuations",
        configuration: "own",
        section: "parsesMarkdown",
        depth: LAZY,
        input: |depth| Value::String(("> ".repeat(depth) + "x\ny").into()),
        output: |_| doc(vec![in_paragraph(json!({"type": "text", "text": "x\ny"}))]),
    },
    Nested {
        what: "list items",
        configuration: "own",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| Value::String(("1. ".repeat(depth) + "x").into()),
        output: |_| doc(Vec::new()),
    },
    // Marks inside blockquotes run their recursion on top of the blockquotes'.
    Nested {
        what: "strong",
        configuration: "own",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("**a ", " a**", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("bold", depth))]),
    },
    Nested {
        what: "em",
        configuration: "own",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("*a ", " a*", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("italic", depth))]),
    },
    Nested {
        what: "del",
        configuration: "own",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| quoted(around("~~a ", " a~~", depth), depth),
        output: |depth| doc(vec![paragraph_of(marked("strike", depth))]),
    },
    // A heading's blank lines become a token of their own, the heading copied, its tokens with
    // it.
    Nested {
        what: "a heading's blank lines",
        configuration: "own",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| {
            Value::String(("# ".to_owned() + &around("*a ", " a*", depth) + "\n\ny").into())
        },
        output: |depth| {
            let heading = json!({"type": "heading", "attrs": {"level": 1},
                "content": marked("italic", depth)});
            doc(vec![
                heading,
                in_paragraph(json!({"type": "text", "text": "y"})),
            ])
        },
    },
    Nested {
        what: "tables in blockquotes",
        configuration: "all",
        section: "parsesMarkdown",
        depth: QUOTES,
        input: |depth| {
            Value::String(("> ".repeat(depth) + "| a |\n" + &"> ".repeat(depth) + "| - |").into())
        },
        output: |depth| quoted_doc(header_table(json!({"type": "text", "text": "a"})), depth),
    },
    // Table's tokenizer lexes the innermost table again, its code span's pipe escaped.
    Nested {
        what: "tables with code spans in blockquotes",
        configuration: "all",
        section: "parsesMarkdown",
        depth: QUOTES,
        input: |depth| {
            let quotes = "> ".repeat(depth);
            Value::String(format!("{quotes}| `a|b` |\n{quotes}| - |").into())
        },
        output: |depth| {
            let code = json!({"type": "text", "text": "a|b", "marks": [{"type": "code"}]});
            quoted_doc(header_table(code), depth)
        },
    },
    Nested {
        what: "a mention in blockquotes",
        configuration: "all",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| json!(">".repeat(depth) + " [@ id=\"x\"]"),
        output: |depth| {
            let mention = json!({"type": "mention", "attrs": {"id": "x"}});
            quoted_doc(in_paragraph(mention), depth)
        },
    },
];

/// A color whose CSS nests far deeper than the HTML does: the CSS engine reads the span's style
/// to parse the color, and writes the style the color renders.
#[test]
fn colors_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let mut extensions = own::extensions();
        extensions.push(color(ColorOptions::default()).into());
        let schema = get_schema(&extensions).expect("the schema");
        let parens = "(".repeat(CSSES) + &")".repeat(CSSES);
        let color = format!("var(--a,{parens})");
        let html = format!("<p><span style=\"color: {color};\">x</span></p>");

        let doc = html::parse(&schema, &html).expect("a document");
        let text = json!({"type": "text", "marks": [
            {"type": "textStyle", "attrs": {"color": color}},
        ], "text": "x"});
        let paragraph = json!({"type": "paragraph", "content": [text]});
        let expected = json!({"type": "doc", "content": [paragraph]});
        assert!(same(&expected, &doc.to_json().to_value()));
        assert!(html::serialize(&schema, &doc).expect("HTML") == html);
    });
}
