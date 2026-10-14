//! What Tiptap's inline extensions, and the extensions that give other types attributes, make of
//! documents, recorded by `harness/record-tiptap-inline.mjs`: tarnish-tiptap's must make the same
//! schemas, documents, Markdown and HTML, or fail with the same error.

#[allow(dead_code)]
mod own;

use std::collections::HashMap;
use std::sync::Arc;

use own::{Converter, input_of};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Map, Value, json, object};
use tarnish_fixtures::{expect, outcome, read, records, same};
use tarnish_tiptap::extensions::link::{LinkOptions, link};
use tarnish_tiptap::extensions::mention::{MentionHtml, MentionOptions, mention};
use tarnish_tiptap::extensions::text_align::{TextAlignOptions, text_align};
use tarnish_tiptap::extensions::text_style::{
    StyleOptions, TextStyleKitOptions, TextStyleOptions, background_color, color, font_family,
    font_size, line_height, text_style, text_style_kit,
};
use tarnish_tiptap::extensions::unique_id::{
    UniqueIdOptions, UniqueIdTypes, generate_unique_ids, unique_id,
};
use tarnish_tiptap::extensions::{
    bold::bold, document::document, hard_break::hard_break, heading::HeadingOptions,
    heading::heading, italic::italic, paragraph::paragraph, text::text,
};
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension,
    NodeExtension, ParseHtml, PlainExtension, TextOptions, TextSerializer, TiptapSchema,
    generate_text, get_schema, node_attr,
};

/// Global attributes for each kind of `types`.
fn globals() -> PlainExtension {
    let global = |types, attributes| GlobalAttributes { types, attributes };
    PlainExtension::create("globals").add_global_attributes(vec![
        global(
            GlobalTypes::All,
            vec![ExtensionAttribute::new("every", json!("*"))],
        ),
        global(
            GlobalTypes::Nodes,
            vec![
                ExtensionAttribute::new("node", json!("n")),
                ExtensionAttribute::new("every", json!("node")),
            ],
        ),
        global(
            GlobalTypes::Marks,
            vec![ExtensionAttribute::new("mark", Value::Null)],
        ),
        global(
            GlobalTypes::Named(&["paragraph", "bold", "paragraph", "missing"]),
            vec![ExtensionAttribute::new("named", json!(1))],
        ),
        global(
            GlobalTypes::NodesBut(&["doc", "text"]),
            vec![ExtensionAttribute::new("made", json!(false)).not_rendered()],
        ),
    ])
}

/// Document, Paragraph and Text, and `others`.
fn with(others: impl IntoIterator<Item = Extension>) -> Vec<Extension> {
    let base: [Extension; 3] = [document().into(), paragraph().into(), text().into()];
    base.into_iter().chain(others).collect()
}

/// A node and marks that set the fields of a spec that only an editor reads.
fn specs() -> Vec<Extension> {
    let chip = NodeExtension::create("chip")
        .group("inline")
        .inline(true)
        .atom()
        .selectable(false)
        .draggable()
        .add_attributes(vec![ExtensionAttribute::new("label", json!("c"))])
        .render_text(|props| {
            let label = node_attr(props.node, "label").to_js_string()?;
            let parent = props.parent.node_type();
            let (pos, index, range) = (props.pos, props.index, &props.range);
            Ok(format!(
                "<{label} {pos} {} {index} {}-{}>",
                parent.name(),
                range.start,
                range.end
            ))
        });
    let rule = NodeExtension::create("rule")
        .group("block")
        .atom()
        .selectable(true);
    let tag = MarkExtension::create("tag").inclusive(false);
    let free = MarkExtension::create("free").inclusive(true);
    with([chip.into(), rule.into(), tag.into(), free.into()])
}

/// A block of blocks, which nests.
fn boxes() -> NodeExtension {
    NodeExtension::create("box")
        .group("block")
        .content("block+")
        .parse_html([ParseHtml::tag("div")])
        .render_html(|_, html| Ok(DomSpec::wrapping("div", html)))
}

/// Tiptap's own extensions, and `others`.
fn own_with(others: impl IntoIterator<Item = Extension>) -> Vec<Extension> {
    own::extensions().into_iter().chain(others).collect()
}

/// The options of the UniqueID of each configuration that has one.
fn ids(name: &str) -> UniqueIdOptions {
    match name {
        "uniqueIdNone" => UniqueIdOptions::default(),
        "uniqueId" => UniqueIdOptions {
            types: UniqueIdTypes::Named(&["paragraph", "heading"]),
            generate_id: Arc::new(|node, pos| {
                Ok(json!(format!("{}-{pos}", node.node_type().name())))
            }),
            ..UniqueIdOptions::default()
        },
        "uniqueIdAll" => UniqueIdOptions {
            types: UniqueIdTypes::All,
            generate_id: Arc::new(|node, pos| {
                Ok(if node.is_inline() {
                    json!(pos)
                } else {
                    json!({"at": pos})
                })
            }),
            ..UniqueIdOptions::default()
        },
        "uniqueIdName" => UniqueIdOptions {
            attribute_name: "uid",
            types: UniqueIdTypes::Named(&["paragraph"]),
            generate_id: Arc::new(|_, _| Ok(json!(""))),
        },
        "deep" => UniqueIdOptions {
            types: UniqueIdTypes::Named(&["box"]),
            generate_id: Arc::new(|node, pos| {
                Ok(json!(format!("{}-{pos}", node.node_type().name())))
            }),
            ..UniqueIdOptions::default()
        },
        other => panic!("no UniqueID in {other}"),
    }
}

/// Document, Paragraph, Text, Heading and Bold, and the extensions of a `TextStyleKit`.
fn kit(options: TextStyleKitOptions) -> Vec<Extension> {
    let base = with([heading(HeadingOptions::default()).into(), bold().into()]);
    base.into_iter().chain(text_style_kit(options)).collect()
}

/// The extensions of each configuration the recorder names.
fn configuration(name: &str) -> Vec<Extension> {
    match name {
        "own" => own::extensions(),
        "specs" => specs(),
        "styles" => {
            let styles = [background_color, color, font_family, font_size, line_height];
            own_with(styles.map(|style| style(StyleOptions::default()).into()))
        }
        "textAlignNone" => own_with([text_align(TextAlignOptions::default()).into()]),
        "textAlign" => own_with([text_align(TextAlignOptions {
            types: &["heading", "paragraph"],
            ..TextAlignOptions::default()
        })
        .into()]),
        "textAlignOptions" => own_with([
            font_size(StyleOptions {
                types: &["paragraph"],
            })
            .into(),
            text_align(TextAlignOptions {
                types: &["paragraph", "missing"],
                alignments: &["left", "right", "start"],
                default_alignment: Some("left"),
            })
            .into(),
        ]),
        "link" => own_with([link(LinkOptions::default()).into()]),
        "linkOptions" => own_with([link(LinkOptions {
            autolink: false,
            protocols: ["ftp", "git", "web+x", "", "", "a|b", "C.D"]
                .map(String::from)
                .into(),
            html_attributes: object!({
                "target": null,
                "rel": "noopener",
                "class": "link",
                "data-x": "y",
            }),
            ..LinkOptions::default()
        })
        .into()]),
        "linkAllowed" => own_with([link(LinkOptions {
            default_protocol: "https".into(),
            protocols: vec!["web+y".into()],
            is_allowed_uri: Some(Arc::new(|url, context| {
                let url_is = |text: &str| url.and_then(Value::as_str) == Some(text);
                let dotted = url
                    .and_then(Value::as_str)
                    .is_some_and(|url| url.starts_with("./"));
                let counted = format!("{}:{}", context.default_protocol, context.protocols.len());
                Ok(dotted
                    || url_is(&counted)
                    || (!url_is("https://rejected.example") && context.default_validate(url)?))
            })),
            ..LinkOptions::default()
        })
        .into()]),
        "linkClass" => {
            let mut options = LinkOptions::default();
            options.html_attributes.insert("class".into(), json!(5));
            own_with([link(options).into()])
        }
        "mention" => own_with([mention(MentionOptions::default()).into()]),
        "mentionOptions" => own_with([mention(MentionOptions {
            html_attributes: object!({"class": "mention", "style": "color: red", "data-x": 1}),
            suggestions: ["#", "@", "@", "++"].map(String::from).into(),
            ..MentionOptions::default()
        })
        .into()]),
        "mentionSuggestion" => own_with([mention(MentionOptions {
            suggestion: "+".into(),
            ..MentionOptions::default()
        })
        .into()]),
        "mentionRender" => own_with([mention(MentionOptions {
            render_text: Some(Arc::new(|node, char| {
                let id = node_attr(node, "id").to_js_string()?;
                Ok(format!("{char}text:{id}"))
            })),
            render_html: Some(Arc::new(|node, char, html| {
                let label = node_attr(node, "label").to_js_string()?;
                let text = DomSpec::Text(format!("{char}{label}").into());
                Ok(MentionHtml::Spec(DomSpec::element("b", html, vec![text])))
            })),
            ..MentionOptions::default()
        })
        .into()]),
        "mentionString" => own_with([mention(MentionOptions {
            render_html: Some(Arc::new(|node, _, _| {
                let id = node_attr(node, "id").to_js_string()?;
                Ok(MentionHtml::Text(format!("<{id}>")))
            })),
            ..MentionOptions::default()
        })
        .into()]),
        "mentionLabel" => own_with([mention(MentionOptions {
            html_attributes: object!({"class": "l"}),
            render_label: Some(Arc::new(|node, char| {
                let label = node_attr(node, "label").to_js_string()?;
                Ok(format!("{char}{label}!"))
            })),
            ..MentionOptions::default()
        })
        .into()]),
        "uniqueIdNone" => own_with([unique_id(UniqueIdOptions::default()).into()]),
        "uniqueId" | "uniqueIdAll" | "uniqueIdName" => {
            let options = ids(name);
            let mut extensions = own::extensions();
            if name == "uniqueIdAll" {
                extensions.push(mention(MentionOptions::default()).into());
            }
            extensions.push(unique_id(options).into());
            extensions
        }
        "deep" => {
            let others: [Extension; 8] = [
                bold().into(),
                boxes().into(),
                text_style(TextStyleOptions::default()).into(),
                font_size(StyleOptions::default()).into(),
                link(LinkOptions::default()).into(),
                mention(MentionOptions::default()).into(),
                text_align(TextAlignOptions {
                    types: &["box", "paragraph"],
                    ..TextAlignOptions::default()
                })
                .into(),
                unique_id(ids("deep")).into(),
            ];
            with(others)
        }
        "kit" => kit(TextStyleKitOptions::default()),
        "kitOptions" => kit(TextStyleKitOptions {
            background_color: None,
            line_height: None,
            color: Some(StyleOptions {
                types: &["textStyle", "heading"],
            }),
            font_size: Some(StyleOptions {
                types: &["paragraph"],
            }),
            text_style: Some(TextStyleOptions {
                html_attributes: object!({
                    "class": "styled",
                    "style": "font-weight: bold; color: black",
                    "data-x": null,
                    "data-n": 0,
                }),
                merge_nested_span_styles: false,
            }),
            ..TextStyleKitOptions::default()
        }),
        "globals" => vec![
            document().into(),
            paragraph().into(),
            text().into(),
            heading(HeadingOptions::default()).into(),
            hard_break().into(),
            bold().into(),
            italic().into(),
            globals().into(),
        ],
        other => panic!("no configuration {other}"),
    }
}

/// A configuration's extensions, and what converts with them.
struct Configured {
    name: String,
    extensions: Vec<Extension>,
    converter: Converter,
}

/// Each configuration the records of `section` name, in the order they first name it.
fn configured(records: &[Value]) -> Vec<Configured> {
    let mut configured: Vec<Configured> = Vec::new();
    for record in records {
        let name = record["extensions"].as_str().expect("a configuration");
        if configured.iter().all(|done| done.name != name) {
            let extensions = configuration(name);
            let converter = Converter::of(&extensions);
            configured.push(Configured {
                name: name.to_owned(),
                extensions,
                converter,
            });
        }
    }
    configured
}

/// The fields of a record of `section` that are inputs.
fn inputs(section: &str) -> &'static [&'static str] {
    match section {
        "generatesText" => &["doc", "blockSeparator", "serializers"],
        "parsesMarkdown" => &["markdown"],
        "parsesHTML" => &["html"],
        _ => &["doc"],
    }
}

/// What the conversion a `section` records makes of a record's inputs, those of its fields
/// `inputs` names, with a configuration's extensions, as a record of it holds it.
fn run(section: &str, configured: &Configured, given: &Map) -> Value {
    match section {
        "generatesText" => {
            let mut options = TextOptions::default();
            if let Some(separator) = given.get("blockSeparator") {
                options.block_separator = separator.as_str().expect("a separator").to_owned();
            }
            if let Some(named) = given.get("serializers") {
                options.text_serializers = serializers(named.as_str().expect("a name"));
            }
            let text = generate_text(&given["doc"], &configured.extensions, &options);
            outcome(text.map(|text| json!({"text": text})))
        }
        "generatesUniqueIds" => {
            let options = ids(&configured.name);
            let doc = generate_unique_ids(&given["doc"], &configured.extensions, &options);
            outcome(doc.map(|doc| json!({"ids": doc})))
        }
        _ => configured
            .converter
            .convert(section, &given[input_of(section)]),
    }
}

/// Holds each record of `section` to what its conversion makes of its inputs with the extensions
/// it names.
fn check(section: &str) {
    let fixtures = read("tiptap-inline");
    let records = records(&fixtures, section);
    let configured = configured(records);
    let mut failures = Vec::new();
    for record in records {
        let mut expected = record.clone();
        let fields = expected.as_object_mut().expect("a record");
        let name = fields.shift_remove("extensions").expect("a configuration");
        let given: Map = inputs(section)
            .iter()
            .filter_map(|input| Some(((*input).into(), fields.shift_remove(input)?)))
            .collect();
        let configuration = configured
            .iter()
            .find(|configured| configured.name == name)
            .expect("the configuration");
        let actual = run(section, configuration, &given);
        if !same(&expected, &actual) {
            failures.push(format!(
                "{name}: {}\n  Tiptap: {expected}\n  tarnish-tiptap: {actual}",
                Value::Object(given)
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

/// Each node and mark type of a schema, as the recorder reads their specs.
fn types(schema: &TiptapSchema) -> Value {
    let schema = &schema.schema;
    let attrs =
        |attrs: Option<&tarnish::json::Map>| attrs.cloned().map_or(Value::Null, Value::Object);
    let nodes: Vec<Value> = schema
        .node_types()
        .map(|node| {
            json!([node.name(), {
                "atom": node.is_atom(),
                "inline": node.is_inline(),
                "selectable": node.spec().selectable != Some(false),
                "draggable": node.spec().draggable,
                "groups": node.groups(),
                "attrs": attrs(node.default_attrs()),
            }])
        })
        .collect();
    let marks: Vec<Value> = schema
        .mark_types()
        .map(|mark| {
            json!([mark.name(), {
                "inclusive": mark.spec().inclusive != Some(false),
                "attrs": attrs(mark.default_attrs()),
            }])
        })
        .collect();
    json!({"nodes": nodes, "marks": marks})
}

#[test]
fn makes_the_schemas_tiptap_does() {
    let fixtures = read("tiptap-inline");
    for record in records(&fixtures, "schemas") {
        let name = record["extensions"].as_str().expect("a configuration");
        let schema = get_schema(&configuration(name)).expect("the schema");
        let mut expected = record.clone();
        expected
            .as_object_mut()
            .expect("a record")
            .shift_remove("extensions");
        let actual = types(&schema);
        assert!(
            same(&expected, &actual),
            "{name}: {actual} where {expected}"
        );
    }
}

/// The text serializers a record names.
fn serializers(name: &str) -> HashMap<String, TextSerializer> {
    assert_eq!(name, "places", "no serializers {name}");
    let paragraph: TextSerializer = Arc::new(|props| {
        let name = props.node.node_type().name();
        Ok(format!("[{name} {} {}]", props.pos, props.index))
    });
    let chip: TextSerializer = Arc::new(|props| {
        let label = node_attr(props.node, "label").to_js_string()?;
        Ok(format!("({label})"))
    });
    HashMap::from([("paragraph".into(), paragraph), ("chip".into(), chip)])
}

#[test]
fn generates_text_as_tiptap_does() {
    check("generatesText");
}

#[test]
fn generates_unique_ids_as_tiptap_does() {
    check("generatesUniqueIds");
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

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
const INLINES: usize = 500;

/// An input nested `depth` deep, and what the conversion a `section` records makes of it, which
/// the test folds from the depths below and holds to Tiptap's at the depths
/// `harness/record-tiptap-inline.mjs` records, with the `deep` extensions.
struct Nested {
    what: &'static str,
    section: &'static str,
    depth: usize,
    input: fn(usize) -> Value,
    output: fn(usize) -> Value,
}

/// The alignment of the box `level` deep, from 1.
fn alignment(level: usize) -> &'static str {
    ["left", "center", "right"][level % 3]
}

/// A paragraph of text and a mention inside boxes `depth` deep, with the attributes `attrs`
/// gives each level from 1, around `paragraph`.
fn boxed(depth: usize, attrs: impl Fn(usize) -> Option<Value>, paragraph: Value) -> Value {
    let nested = (1..=depth).rev().fold(paragraph, |inner, level| {
        let mut node = json!({"type": "box"});
        if let Some(attrs) = attrs(level) {
            node["attrs"] = attrs;
        }
        node["content"] = json!([inner]);
        node
    });
    json!({"type": "doc", "content": [nested]})
}

fn mentioned(text: Value, mention: Value) -> Value {
    json!({"type": "paragraph", "content": [text, mention]})
}

fn x() -> Value {
    json!({"type": "text", "text": "x"})
}

/// Each box's id and alignment.
fn identified(level: usize) -> Option<Value> {
    Some(json!({"id": format!("b{level}"), "textAlign": alignment(level)}))
}

fn the_link() -> Value {
    json!({"type": "link", "attrs": {
        "href": "u", "target": "_blank", "rel": "noopener noreferrer nofollow",
        "class": null, "title": null,
    }})
}

/// The text inside bold delimiters `depth` deep inside a link: a bold mark for each pair of
/// delimiters around each part of it, and the link's after them.
fn bold_in_link(depth: usize) -> Vec<Value> {
    let link = json!({"type": "link", "attrs": {"href": "u", "title": null}});
    let marks = |count: usize| {
        let mut marks = vec![json!({"type": "bold"}); count];
        marks.push(link.clone());
        marks
    };
    let text = |text: &str, count| json!({"type": "text", "text": text, "marks": marks(count)});
    let opening = (1..depth).map(|count| text("a ", count));
    let closing = (1..depth).rev().map(|count| text(" a", count));
    let middle = text("a x a", depth);
    opening.chain([middle]).chain(closing).collect()
}

const NESTED: [Nested; 8] = [
    Nested {
        what: "spans with a font size in a link",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| {
            let spans: String = (1..=depth)
                .map(|level| format!("<span style=\"font-size: {level}px\">"))
                .collect();
            json!(format!("<p><a href=\"u\">{spans}x</a></p>"))
        },
        output: |depth| {
            let style = json!({"type": "textStyle", "attrs": {"fontSize": format!("{depth}px")}});
            let text = json!({"type": "text", "marks": [the_link(), style], "text": "x"});
            let paragraph = json!({"type": "paragraph", "attrs": {"textAlign": null},
                "content": [text]});
            json!({"doc": {"type": "doc", "content": [paragraph]}})
        },
    },
    Nested {
        what: "divs with an id and an alignment",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| {
            let divs: String = (1..=depth)
                .map(|level| {
                    let align = alignment(level);
                    format!("<div data-id=\"b{level}\" style=\"text-align: {align}\">")
                })
                .collect();
            json!(divs + "<p>x</p>")
        },
        output: |depth| {
            let paragraph = json!({"type": "paragraph", "attrs": {"textAlign": null},
                "content": [x()]});
            json!({"doc": boxed(depth, identified, paragraph)})
        },
    },
    Nested {
        what: "HTML of boxes with an id and an alignment",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: |depth| {
            let mention = json!({"type": "mention", "attrs": {"id": "m"}});
            boxed(depth, identified, mentioned(x(), mention))
        },
        output: |depth| {
            let divs: String = (1..=depth)
                .map(|level| {
                    let align = alignment(level);
                    format!("<div data-id=\"b{level}\" style=\"text-align: {align};\">")
                })
                .collect();
            let mention = r#"<span data-type="mention" data-id="m" data-mention-suggestion-char="@">@m</span>"#;
            json!({"html": format!("{divs}<p>x{mention}</p>{}", "</div>".repeat(depth))})
        },
    },
    Nested {
        what: "text of boxes",
        section: "generatesText",
        depth: DOCUMENTS,
        input: |depth| {
            let mention = json!({"type": "mention", "attrs": {"id": "m"}});
            boxed(depth, |_| None, mentioned(x(), mention))
        },
        output: |depth| json!({"text": "\n\n".repeat(depth) + "x@m"}),
    },
    Nested {
        what: "ids of boxes",
        section: "generatesUniqueIds",
        depth: PARSES,
        input: |depth| {
            let mention = json!({"type": "mention", "attrs": {"id": "m"}});
            boxed(depth, |_| None, mentioned(x(), mention))
        },
        output: |depth| {
            let mention = json!({"type": "mention", "attrs": {
                "id": "m", "label": null, "mentionSuggestionChar": "@",
            }});
            let paragraph = json!({"type": "paragraph", "attrs": {"textAlign": null},
                "content": [x(), mention]});
            let ids = |level: usize| {
                Some(json!({"id": format!("box-{}", level - 1),
                "textAlign": null}))
            };
            json!({"ids": boxed(depth, ids, paragraph)})
        },
    },
    // The manager renders a node of any type it has a renderer for, a mark's among them.
    Nested {
        what: "Markdown of links",
        section: "serializesMarkdown",
        depth: DOCUMENTS,
        input: |depth| {
            let nested = (0..depth).fold(
                x(),
                |inner, _| json!({"type": "link", "attrs": {"href": "u"}, "content": [inner]}),
            );
            json!({"type": "doc", "content": [nested]})
        },
        output: |depth| json!({"markdown": "[".repeat(depth) + "x" + &"](u)".repeat(depth)}),
    },
    Nested {
        what: "strong in a link",
        section: "parsesMarkdown",
        depth: INLINES,
        input: |depth| {
            let strong = "**a ".repeat(depth) + "x" + &" a**".repeat(depth);
            json!(format!("[{strong}](u)"))
        },
        output: |depth| {
            let paragraph = json!({"type": "paragraph", "content": bold_in_link(depth)});
            json!({"doc": {"type": "doc", "content": [paragraph]}})
        },
    },
    // Without an extension for blockquotes, the manager parses a blockquote's content.
    Nested {
        what: "a mention in blockquotes",
        section: "parsesMarkdown",
        depth: PARSES,
        input: |depth| json!(">".repeat(depth) + " [@ id=\"x\"]"),
        output: |_| {
            let mention = json!({"type": "mention", "attrs": {"id": "x"}});
            let paragraph = json!({"type": "paragraph", "content": [mention]});
            json!({"doc": {"type": "doc", "content": [paragraph]}})
        },
    },
];

/// The input of a record that `nested` converts.
fn given(section: &str, input: Value) -> Map {
    let mut given = Map::new();
    given.insert(inputs(section)[0].into(), input);
    given
}

#[test]
fn converts_what_nests_as_deeply_as_memory_allows() {
    let fixtures = read("tiptap-inline");
    for nested in &NESTED {
        for depth in 1..=3 {
            let given = given(nested.section, (nested.input)(depth));
            let record = records(&fixtures, nested.section).iter().find(|record| {
                let inputs = inputs(nested.section);
                let input = |fields: &Map, input| fields.get(input).cloned();
                let fields = record.as_object().expect("a record");
                record["extensions"] == "deep"
                    && inputs.iter().all(|name| {
                        let (recorded, given) = (input(fields, name), input(&given, name));
                        recorded.is_none() && given.is_none()
                            || recorded.zip(given).is_some_and(|(a, b)| same(&a, &b))
                    })
            });
            let record = record.unwrap_or_else(|| panic!("no record of {}", nested.what));
            let mut skipped = inputs(nested.section).to_vec();
            skipped.push("extensions");
            expect(record, &skipped, (nested.output)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        let extensions = configuration("deep");
        let converter = Converter::of(&extensions);
        let configured = Configured {
            name: "deep".into(),
            extensions,
            converter,
        };
        for nested in &NESTED {
            let given = given(nested.section, (nested.input)(nested.depth));
            let converted = run(nested.section, &configured, &given);
            let expected = (nested.output)(nested.depth);
            assert!(same(&expected, &converted), "{}", nested.what);
        }
    });
}
