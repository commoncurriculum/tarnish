//! What Tiptap's inline extensions, and the extensions that give other types attributes, make of
//! documents, recorded by `harness/record-tiptap-inline.mjs`: tarnish-tiptap's must make the same
//! schemas, documents, Markdown and HTML, or fail with the same error.

#[allow(dead_code)]
mod own;

use own::{Converter, input_of};
use tarnish::json::{Value, json};
use tarnish_fixtures::{read, records, same};
use tarnish_tiptap::extensions::{
    bold::bold, document::document, hard_break::hard_break, heading::HeadingOptions,
    heading::heading, italic::italic, paragraph::paragraph, text::text,
};
use tarnish_tiptap::{
    Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension, NodeExtension,
    PlainExtension, TiptapSchema, get_schema,
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
        .draggable();
    let rule = NodeExtension::create("rule")
        .group("block")
        .atom()
        .selectable(true);
    let tag = MarkExtension::create("tag").inclusive(false);
    let free = MarkExtension::create("free").inclusive(true);
    with([chip.into(), rule.into(), tag.into(), free.into()])
}

/// The extensions of each configuration the recorder names.
fn configuration(name: &str) -> Vec<Extension> {
    match name {
        "own" => own::extensions(),
        "specs" => specs(),
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

/// A configuration, and what converts with its extensions.
struct Configured {
    name: String,
    converter: Converter,
}

/// Each configuration the records of `section` name, in the order they first name it.
fn configured(records: &[Value]) -> Vec<Configured> {
    let mut configured: Vec<Configured> = Vec::new();
    for record in records {
        let name = record["extensions"].as_str().expect("a configuration");
        if configured.iter().all(|done| done.name != name) {
            let extensions = configuration(name);
            configured.push(Configured {
                name: name.to_owned(),
                converter: Converter::of(&extensions),
            });
        }
    }
    configured
}

/// Holds each record of `section` to what `run` makes of its input with the extensions it names,
/// as a record of the section holds it.
fn check(section: &str, run: impl Fn(&Configured, &Value) -> Value) {
    let fixtures = read("tiptap-inline");
    let records = records(&fixtures, section);
    let configured = configured(records);
    let mut failures = Vec::new();
    for record in records {
        let mut expected = record.clone();
        let fields = expected.as_object_mut().expect("a record");
        let name = fields.shift_remove("extensions").expect("a configuration");
        let input = fields.shift_remove(input_of(section)).expect("an input");
        let configuration = configured
            .iter()
            .find(|configured| configured.name == name)
            .expect("the configuration");
        let actual = run(configuration, &input);
        if !same(&expected, &actual) {
            failures.push(format!(
                "{name}: {input}\n  Tiptap: {expected}\n  tarnish-tiptap: {actual}"
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

#[test]
fn parses_markdown_as_tiptap_does() {
    check("parsesMarkdown", |configured, input| {
        configured.converter.convert("parsesMarkdown", input)
    });
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    check("serializesMarkdown", |configured, input| {
        configured.converter.convert("serializesMarkdown", input)
    });
}

#[test]
fn parses_html_as_tiptap_does() {
    check("parsesHTML", |configured, input| {
        configured.converter.convert("parsesHTML", input)
    });
}

#[test]
fn serializes_html_as_tiptap_does() {
    check("serializesHTML", |configured, input| {
        configured.converter.convert("serializesHTML", input)
    });
}
