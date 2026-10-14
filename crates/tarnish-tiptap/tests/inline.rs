//! What Tiptap's inline extensions, and the extensions that give other types attributes, make of
//! documents, recorded by `harness/record-tiptap-inline.mjs`: tarnish-tiptap's must make the same
//! schemas, documents, Markdown and HTML, or fail with the same error.

#[allow(dead_code)]
mod own;

use std::collections::HashMap;
use std::sync::Arc;

use own::{Converter, input_of};
use tarnish::json::{Map, Value, json, object};
use tarnish_fixtures::{outcome, read, records, same};
use tarnish_tiptap::extensions::link::{LinkOptions, link};
use tarnish_tiptap::extensions::mention::{MentionHtml, MentionOptions, mention};
use tarnish_tiptap::extensions::text_align::{TextAlignOptions, text_align};
use tarnish_tiptap::extensions::text_style::{
    StyleOptions, TextStyleKitOptions, TextStyleOptions, background_color, color, font_family,
    font_size, line_height, text_style_kit,
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
    NodeExtension, PlainExtension, TextOptions, TextSerializer, TiptapSchema, generate_text,
    get_schema, node_attr,
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

/// Holds each record of `section` to what `run` makes of its inputs, those of the fields `inputs`
/// names that it has, with the extensions it names, as a record of the section holds it.
fn check(section: &str, inputs: &[&str], run: impl Fn(&Configured, &Map) -> Value) {
    let fixtures = read("tiptap-inline");
    let records = records(&fixtures, section);
    let configured = configured(records);
    let mut failures = Vec::new();
    for record in records {
        let mut expected = record.clone();
        let fields = expected.as_object_mut().expect("a record");
        let name = fields.shift_remove("extensions").expect("a configuration");
        let given: Map = inputs
            .iter()
            .filter_map(|input| Some(((*input).into(), fields.shift_remove(input)?)))
            .collect();
        let configuration = configured
            .iter()
            .find(|configured| configured.name == name)
            .expect("the configuration");
        let actual = run(configuration, &given);
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
    let inputs = ["doc", "blockSeparator", "serializers"];
    check("generatesText", &inputs, |configured, given| {
        let mut options = TextOptions::default();
        if let Some(separator) = given.get("blockSeparator") {
            options.block_separator = separator.as_str().expect("a separator").to_owned();
        }
        if let Some(named) = given.get("serializers") {
            options.text_serializers = serializers(named.as_str().expect("a name"));
        }
        let text = generate_text(&given["doc"], &configured.extensions, &options);
        outcome(text.map(|text| json!({"text": text})))
    });
}

#[test]
fn generates_unique_ids_as_tiptap_does() {
    check("generatesUniqueIds", &["doc"], |configured, given| {
        let options = ids(&configured.name);
        let doc = generate_unique_ids(&given["doc"], &configured.extensions, &options);
        outcome(doc.map(|doc| json!({"ids": doc})))
    });
}

/// Holds each record of a section `own::Converter` converts.
fn converts(section: &str) {
    check(section, &[input_of(section)], |configured, given| {
        configured
            .converter
            .convert(section, &given[input_of(section)])
    });
}

#[test]
fn parses_markdown_as_tiptap_does() {
    converts("parsesMarkdown");
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    converts("serializesMarkdown");
}

#[test]
fn parses_html_as_tiptap_does() {
    converts("parsesHTML");
}

#[test]
fn serializes_html_as_tiptap_does() {
    converts("serializesHTML");
}
