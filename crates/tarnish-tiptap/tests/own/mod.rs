//! The extensions `harness/record-tiptap.mjs` gives the `MarkdownManager` and `getSchema`: Tiptap's
//! own, with their options or with an option set, and the others a record names, and the
//! conversions it records with them. [`lists`] has the configurations
//! `harness/record-tiptap-lists.mjs` names.

pub mod lists;

use tarnish::json::{Map, Value, json, object};
use tarnish::model::{AttributeDefault, AttributeSpec, Whitespace};
use tarnish::{Node, Schema};
use tarnish_fixtures::{outcome, read, records, same};
use tarnish_js::Error;
use tarnish_markdown::marked::Marked;
use tarnish_markdown::marked_more_lists::more_lists;
use tarnish_tiptap::extensions::bold::{BoldOptions, bold};
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::hard_break::{HardBreakOptions, hard_break};
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::highlight::{HighlightOptions, highlight};
use tarnish_tiptap::extensions::image::{ImageOptions, image};
use tarnish_tiptap::extensions::italic::{ItalicOptions, italic};
use tarnish_tiptap::extensions::paragraph::{ParagraphOptions, paragraph};
use tarnish_tiptap::extensions::strike::{StrikeOptions, strike};
use tarnish_tiptap::extensions::subscript::{SubscriptOptions, subscript};
use tarnish_tiptap::extensions::superscript::{SuperscriptOptions, superscript};
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::text_style::{ColorOptions, TextStyleOptions, color, text_style};
use tarnish_tiptap::extensions::underline::{UnderlineOptions, underline};
use tarnish_tiptap::markdown::MarkdownManager;
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension,
    NodeExtension, ParseHtml, PlainExtension, TiptapSchema, get_schema, html,
};

/// Tiptap's own extensions, but Highlight, with Heading's and Image's options.
fn own_but_highlight(
    heading_options: HeadingOptions,
    image_options: ImageOptions,
) -> Vec<Extension> {
    vec![
        document().into(),
        paragraph(ParagraphOptions::default()).into(),
        text().into(),
        heading(heading_options).into(),
        image(image_options).into(),
        hard_break(HardBreakOptions::default()).into(),
        bold(BoldOptions::default()).into(),
        italic(ItalicOptions::default()).into(),
        strike(StrikeOptions::default()).into(),
        underline(UnderlineOptions::default()).into(),
        subscript(SubscriptOptions::default()).into(),
        superscript(SuperscriptOptions::default()).into(),
    ]
}

/// Tiptap's own extensions, with Heading's, Image's and Highlight's options.
fn own(
    heading_options: HeadingOptions,
    image_options: ImageOptions,
    highlight_options: HighlightOptions,
) -> Vec<Extension> {
    let mut own = own_but_highlight(heading_options, image_options);
    own.push(highlight(highlight_options).into());
    own.push(text_style(TextStyleOptions::default()).into());
    own
}

pub fn extensions() -> Vec<Extension> {
    own(
        HeadingOptions::default(),
        ImageOptions::default(),
        HighlightOptions::default(),
    )
}

/// Tiptap's own extensions, Highlight with its colors and after TextStyle, and Color.
fn colors() -> Vec<Extension> {
    let mut colors = own_but_highlight(HeadingOptions::default(), ImageOptions::default());
    colors.push(text_style(TextStyleOptions::default()).into());
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

/// The configurations of Tiptap's own extensions with an option set, which record the inputs of
/// Tiptap's own.
pub const OPTIONED: [&str; 7] = [
    "htmlAttributes",
    "htmlAttributeValues",
    "levels",
    "noLevels",
    "multicolor",
    "inline",
    "base64",
];

/// The extensions of a configuration of [`OPTIONED`].
fn optioned(name: &str) -> Option<Vec<Extension>> {
    let heading_levels = |levels| HeadingOptions {
        levels,
        ..HeadingOptions::default()
    };
    Some(match name {
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
            others(),
            &[
                ("subscript", object!({"style": 5})),
                ("superscript", object!({"class": 5})),
            ],
        ),
        "levels" => own(
            heading_levels(&[4, 2]),
            ImageOptions::default(),
            HighlightOptions::default(),
        ),
        "noLevels" => own(
            heading_levels(&[]),
            ImageOptions::default(),
            HighlightOptions::default(),
        ),
        "multicolor" => own(
            HeadingOptions::default(),
            ImageOptions::default(),
            HighlightOptions {
                multicolor: true,
                ..HighlightOptions::default()
            },
        ),
        "inline" => own(
            HeadingOptions::default(),
            ImageOptions {
                inline: true,
                ..ImageOptions::default()
            },
            HighlightOptions::default(),
        ),
        "base64" => own(
            HeadingOptions::default(),
            ImageOptions {
                allow_base64: true,
                ..ImageOptions::default()
            },
            HighlightOptions::default(),
        ),
        _ => return None,
    })
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

/// HTMLAttributes of values other than strings, `undefined` as `null`.
fn others() -> Map {
    object!({
        "class": 0,
        "style": null,
        "data-given": null,
        "data-true": true,
        "data-object": {},
        "data-array": [1, "a", null],
        "data-empty": "",
    })
}

/// `otherAttributes` of harness/fixture.mjs, which the group recorders give each extension's
/// HTMLAttributes, `undefined` as `null`.
pub fn other_attributes() -> Map {
    object!({
        "class": 5,
        "data-null": null,
        "data-undefined": null,
        "data-zero": 0,
        "data-false": false,
        "data-number": 1.5,
        "data-object": {},
        "data-array": [1, "a", null],
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
    let types = &[
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
    PlainExtension::create("given").add_global_attributes(vec![GlobalAttributes {
        types: GlobalTypes::Named(types),
        attributes: vec![given],
    }])
}

/// An application's code mark and code block, whose text stays as it is, and a figure whose
/// attribute renders under a name it makes.
fn code() -> Vec<Extension> {
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

/// The schema and the Markdown manager, `new MarkdownManager({ extensions, marked })` with a
/// marked that uses marked-more-lists, of the extensions a record names.
pub struct Converter {
    schema: TiptapSchema,
    markdown: MarkdownManager,
}

/// The extensions a record names in `extensions`, `None` for Tiptap's own.
pub fn named(record: &Value) -> Option<&str> {
    record.get("extensions").and_then(Value::as_str)
}

impl Converter {
    pub fn new(extensions: Option<&str>) -> Converter {
        let (extensions, indentation) = match extensions {
            None => (self::extensions(), None),
            Some("colors") => (colors(), None),
            Some("code") => (code(), None),
            Some(other) => match optioned(other) {
                Some(extensions) => (extensions, None),
                None => {
                    lists::configuration(other).unwrap_or_else(|| panic!("no extensions {other}"))
                }
            },
        };
        let converter = Converter::of(&extensions);
        match indentation {
            Some(indentation) => Converter {
                markdown: converter.markdown.with_indentation(indentation),
                ..converter
            },
            None => converter,
        }
    }

    /// The converter of `extensions`.
    pub fn of(extensions: &[Extension]) -> Converter {
        Converter {
            schema: get_schema(extensions).expect("the schema"),
            markdown: MarkdownManager::new(extensions, Marked::new(more_lists())),
        }
    }

    pub fn schema(&self) -> &TiptapSchema {
        &self.schema
    }

    /// What the conversion a `section` records makes of an input, as a record of it holds it.
    pub fn convert(&self, section: &str, input: &Value) -> Value {
        fn record<T: Into<Value>>(to: &str, result: Result<T, Error>) -> Value {
            outcome(result.map(|output| json!({ to: output.into() })))
        }
        let text = || input.as_str().expect("text");
        match section {
            "parsesMarkdown" => record("doc", self.markdown.parse(text())),
            "serializesMarkdown" => record("markdown", self.markdown.serialize(input)),
            "parsesHTML" => record(
                "doc",
                html::parse(&self.schema, text()).map(|doc| doc.to_json()),
            ),
            "serializesHTML" => record(
                "html",
                Node::from_json(&self.schema.schema, input)
                    .and_then(|doc| html::serialize(&self.schema, &doc)),
            ),
            _ => panic!("no section {section}"),
        }
    }
}

/// Each section's input field.
pub fn input_of(section: &str) -> &'static str {
    match section {
        "parsesMarkdown" => "markdown",
        "parsesHTML" => "html",
        _ => "doc",
    }
}

/// A schema as `harness/fixture.mjs`'s `schemaRecord` writes it: its top node's type, and each
/// node and mark type with each field of its spec, one left out written as ProseMirror reads it,
/// then the fields an extension adds, and each attribute: whether it's required, and its default,
/// but an `undefined` one.
pub fn schema_record(schema: &Schema) -> Value {
    fn typed(name: &str, fields: Map, extra: &Map, attrs: &[(String, AttributeSpec)]) -> Value {
        let attrs = attrs.iter().map(|(name, attr)| {
            let required = matches!(attr.default, AttributeDefault::Required);
            match &attr.default {
                AttributeDefault::Value(default) => {
                    json!({ "name": name, "required": required, "default": default.clone() })
                }
                _ => json!({ "name": name, "required": required }),
            }
        });
        let mut record = Map::from_iter([("name".into(), json!(name))]);
        record.extend(fields);
        record.extend(extra.clone());
        record.insert("attrs".into(), Value::Array(attrs.collect()));
        Value::Object(record)
    }
    let nodes = schema.node_types().map(|node| {
        let spec = node.spec();
        let whitespace = spec.whitespace.map(|whitespace| match whitespace {
            Whitespace::Pre => "pre",
            Whitespace::Normal => "normal",
        });
        let fields = object!({
            "content": spec.content.as_deref(),
            "marks": spec.marks.as_deref(),
            "group": spec.group.as_deref(),
            "inline": spec.inline,
            "atom": spec.atom,
            "selectable": spec.selectable.unwrap_or(true),
            "draggable": spec.draggable,
            "code": spec.code,
            "whitespace": whitespace,
            "definingAsContext": spec.defining_as_context,
            "definingForContent": spec.defining_for_content,
            "defining": spec.defining,
            "isolating": spec.isolating,
            "linebreakReplacement": spec.linebreak_replacement,
        });
        typed(node.name(), fields, &spec.extra, &spec.attrs)
    });
    let marks = schema.mark_types().map(|mark| {
        let spec = mark.spec();
        let fields = object!({
            "inclusive": spec.inclusive.unwrap_or(true),
            "excludes": spec.excludes.as_deref(),
            "group": spec.group.as_deref(),
            "spanning": spec.spanning.unwrap_or(true),
            "code": spec.code,
        });
        typed(mark.name(), fields, &spec.extra, &spec.attrs)
    });
    json!({
        "topNode": schema.top_node_type().name(),
        "nodes": Value::Array(nodes.collect()),
        "marks": Value::Array(marks.collect()),
    })
}

/// Holds each record of a fixture's `schemas` to the schema `converter` builds of the
/// configuration it names, as [`schema_record`] writes it, naming each type that differs.
#[track_caller]
pub fn check_schemas(fixture: &str, converter: impl Fn(Option<&str>) -> Converter) {
    let fixtures = read(fixture);
    let mut failures = Vec::new();
    for record in records(&fixtures, "schemas") {
        let name = named(record);
        let actual = schema_record(&converter(name).schema().schema);
        let mut expected = record.clone();
        expected
            .as_object_mut()
            .expect("a record")
            .shift_remove("extensions");
        if same(&expected, &actual) {
            continue;
        }
        let configuration = name.unwrap_or("own");
        let write = |record: Option<&Value>| record.map_or("none".into(), Value::to_string);
        let mut differ = Vec::new();
        for kind in ["nodes", "marks"] {
            let types = |record: &Value| record[kind].as_array().cloned().unwrap_or_default();
            let (recorded, made) = (types(&expected), types(&actual));
            for index in 0..recorded.len().max(made.len()) {
                let (recorded, made) = (write(recorded.get(index)), write(made.get(index)));
                if recorded != made {
                    differ.push(format!(
                        "{configuration} {kind}[{index}]\n  Tiptap: {recorded}\n  tarnish-tiptap: {made}"
                    ));
                }
            }
        }
        if differ.is_empty() {
            differ.push(format!(
                "{configuration}\n  Tiptap: {expected}\n  tarnish-tiptap: {actual}"
            ));
        }
        failures.extend(differ);
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
