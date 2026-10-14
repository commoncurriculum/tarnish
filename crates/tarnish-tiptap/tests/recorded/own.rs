//! The replay of a fixture the Tiptap recorders write: each record of each section held to what
//! tarnish-tiptap makes of its inputs, with the extensions of the configuration it names in
//! `extensions`, and inputs nested far deeper than the recorded ones held to what those fold to.

use std::collections::HashMap;
use std::sync::Arc;

use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Map, Value, json, object};
use tarnish::model::{AttributeDefault, AttributeSpec, Whitespace};
use tarnish::{Node, Schema};
use tarnish_fixtures::{expect, outcome, read, same};
use tarnish_js::Error;
use tarnish_markdown::marked::Marked;
use tarnish_markdown::marked_more_lists::more_lists;
use tarnish_tiptap::extensions::bold::bold;
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::hard_break::hard_break;
use tarnish_tiptap::extensions::heading::heading;
use tarnish_tiptap::extensions::highlight::highlight;
use tarnish_tiptap::extensions::image::image;
use tarnish_tiptap::extensions::italic::italic;
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::strike::strike;
use tarnish_tiptap::extensions::subscript::subscript;
use tarnish_tiptap::extensions::superscript::superscript;
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::text_style::text_style;
use tarnish_tiptap::extensions::underline::underline;
use tarnish_tiptap::extensions::unique_id::{UniqueIdOptions, generate_unique_ids};
use tarnish_tiptap::markdown::{Indentation, MarkdownManager};
use tarnish_tiptap::{
    Extension, TextOptions, TextSerializer, TiptapSchema, generate_text, get_schema, html,
    node_attr,
};

/// Tiptap's own extensions, as `harness/tiptap.mjs` gives them.
pub fn extensions() -> Vec<Extension> {
    vec![
        document().into(),
        paragraph(Default::default()).into(),
        text().into(),
        heading(Default::default()).into(),
        image(Default::default()).into(),
        hard_break(Default::default()).into(),
        bold(Default::default()).into(),
        italic(Default::default()).into(),
        strike(Default::default()).into(),
        underline(Default::default()).into(),
        subscript(Default::default()).into(),
        superscript(Default::default()).into(),
        highlight(Default::default()).into(),
        text_style(Default::default()).into(),
    ]
}

/// `otherAttributes` of `harness/tiptap.mjs`, HTMLAttributes of values that aren't strings,
/// `undefined` as `null`.
pub fn other_attributes() -> Map {
    object!({
        "class": 5,
        "style": null,
        "data-null": null,
        "data-undefined": null,
        "data-zero": 0,
        "data-false": false,
        "data-true": true,
        "data-number": 1.5,
        "data-empty": "",
        "data-object": {},
        "data-array": [1, "a", null],
    })
}

/// The extensions of a configuration, and what else of it a section reads: its
/// `MarkdownManager`'s indentation, and the options `generateUniqueIds` reads of its UniqueID.
pub struct Configuration {
    pub extensions: Vec<Extension>,
    pub indentation: Indentation,
    pub unique_id: Option<UniqueIdOptions>,
}

impl From<Vec<Extension>> for Configuration {
    fn from(extensions: Vec<Extension>) -> Configuration {
        Configuration {
            extensions,
            indentation: Indentation::default(),
            unique_id: None,
        }
    }
}

/// A configuration, with its schema and its `MarkdownManager`, whose marked uses
/// marked-more-lists.
struct Converter {
    configuration: Configuration,
    schema: TiptapSchema,
    markdown: MarkdownManager,
}

impl Converter {
    fn new(configuration: Configuration) -> Converter {
        let extensions = &configuration.extensions;
        let markdown = MarkdownManager::new(extensions, Marked::new(more_lists()));
        Converter {
            schema: get_schema(extensions).expect("the schema"),
            markdown: markdown.with_indentation(configuration.indentation),
            configuration,
        }
    }

    /// What the conversion a `section` records makes of a record's inputs, as the record holds it.
    fn convert(&self, section: &str, inputs: &Map) -> Value {
        fn record<T: Into<Value>>(to: &str, result: Result<T, Error>) -> Value {
            outcome(result.map(|output| json!({ to: output.into() })))
        }
        let doc = || &inputs["doc"];
        let extensions = &self.configuration.extensions;
        match section {
            "parsesMarkdown" => record("doc", self.markdown.parse(string(&inputs["markdown"]))),
            "serializesMarkdown" => record("markdown", self.markdown.serialize(doc())),
            "parsesHTML" => record(
                "doc",
                html::parse(&self.schema, string(&inputs["html"])).map(|doc| doc.to_json()),
            ),
            "serializesHTML" => record(
                "html",
                Node::from_json(&self.schema.schema, doc())
                    .and_then(|doc| html::serialize(&self.schema, &doc)),
            ),
            "generatesText" => {
                let mut options = TextOptions::default();
                if let Some(separator) = inputs.get("blockSeparator") {
                    options.block_separator = string(separator).to_owned();
                }
                if let Some(serializers) = inputs.get("serializers") {
                    options.text_serializers = text_serializers(string(serializers));
                }
                record("text", generate_text(doc(), extensions, &options))
            }
            "generatesUniqueIds" => {
                let options = self.configuration.unique_id.as_ref();
                let options = options.expect("the configuration's UniqueID");
                record("ids", generate_unique_ids(doc(), extensions, options))
            }
            _ => panic!("no section {section}"),
        }
    }
}

fn string(value: &Value) -> &str {
    value.as_str().expect("text")
}

/// The fields of a record of a section that converts with a configuration that are its inputs.
fn inputs(section: &str) -> Option<&'static [&'static str]> {
    Some(match section {
        "parsesMarkdown" => &["markdown"],
        "parsesHTML" => &["html"],
        "serializesMarkdown" | "serializesHTML" | "generatesUniqueIds" => &["doc"],
        "generatesText" => &["doc", "blockSeparator", "serializers"],
        _ => return None,
    })
}

/// The text serializers a record of `generatesText` names: `places`, which names each paragraph
/// and chip and where it is.
fn text_serializers(name: &str) -> HashMap<String, TextSerializer> {
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

/// A schema as `harness/fixture.mjs`'s `schemaRecord` writes it: its top node's type, and each
/// node and mark type with each field of its spec, one left out written as ProseMirror reads it,
/// then the fields an extension adds, and each attribute: whether it's required, and its default,
/// but an `undefined` one.
fn schema_record(schema: &Schema) -> Value {
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

/// The converter of each configuration a fixture names, each built the first time it's asked
/// for.
struct Converters {
    configuration: fn(&str) -> Configuration,
    built: HashMap<String, Converter>,
}

impl Converters {
    fn new(configuration: fn(&str) -> Configuration) -> Converters {
        Converters {
            configuration,
            built: HashMap::new(),
        }
    }

    fn of(&mut self, name: &str) -> &Converter {
        let configuration = self.configuration;
        let built = self.built.entry(name.to_owned());
        built.or_insert_with(|| Converter::new(configuration(name)))
    }
}

/// An input nested `depth` deep, and what the conversion a `section` records with the
/// configuration `configuration` makes of it, which [`Replay::check_nested`] holds to the
/// records at depths 1 to 3 and folds from them to `depth`.
pub struct Nested {
    pub what: &'static str,
    pub configuration: &'static str,
    pub section: &'static str,
    pub depth: usize,
    pub input: fn(usize) -> Value,
    pub output: fn(usize) -> Value,
}

impl Nested {
    /// The record's inputs `depth` deep, the input of its section `input` gives.
    fn inputs(&self, depth: usize) -> Map {
        let input = inputs(self.section).expect("a section of a configuration")[0];
        Map::from_iter([(input.into(), (self.input)(depth))])
    }
}

/// A section of no configuration only one fixture records: the fields of a record that are its
/// inputs, and what makes the record's others of them.
struct Other {
    section: &'static str,
    inputs: &'static [&'static str],
    run: fn(&Map) -> Value,
}

/// A fixture, with what builds each configuration its records name.
pub struct Replay {
    name: &'static str,
    fixture: Value,
    configuration: fn(&str) -> Configuration,
    others: Vec<Other>,
}

impl Replay {
    /// `fixtures/<name>.json`, whose configurations `configuration` builds.
    pub fn new(name: &'static str, configuration: fn(&str) -> Configuration) -> Replay {
        Replay {
            name,
            fixture: read(name),
            configuration,
            others: Vec::new(),
        }
    }

    /// With a section of no configuration, whose records hold the inputs `inputs` names and what
    /// `run` makes of them.
    pub fn with(
        mut self,
        section: &'static str,
        inputs: &'static [&'static str],
        run: fn(&Map) -> Value,
    ) -> Replay {
        let other = Other {
            section,
            inputs,
            run,
        };
        self.others.push(other);
        self
    }

    /// Holds each record of each section of the fixture to what its conversion makes of the
    /// record's inputs, and each schema to the one its configuration builds, as [`schema_record`]
    /// writes it, naming each type that differs. Fails at a section it has no conversion for.
    #[track_caller]
    pub fn check(&self) {
        let mut converters = Converters::new(self.configuration);
        let mut failures = Vec::new();
        for (section, records) in self.fixture.as_object().expect("sections") {
            let records = records.as_array().expect("records");
            assert!(
                !records.is_empty(),
                "{}: no records in {section}",
                self.name
            );
            if section == "schemas" {
                for record in records {
                    failures.extend(schema_failures(record, &mut converters));
                }
                continue;
            }
            let other = self.others.iter().find(|other| other.section == section);
            let fields = match (inputs(section), other) {
                (Some(fields), _) => fields,
                (None, Some(other)) => other.inputs,
                (None, None) => panic!("{}: nothing checks {section}", self.name),
            };
            for record in records {
                let mut outputs = record.as_object().expect("a record").clone();
                let name = outputs.shift_remove("extensions");
                let given: Map = fields
                    .iter()
                    .filter_map(|field| Some(((*field).into(), outputs.shift_remove(field)?)))
                    .collect();
                let made = match (&name, other) {
                    (Some(name), None) => converters.of(string(name)).convert(section, &given),
                    (None, Some(other)) => (other.run)(&given),
                    _ => panic!("{section}: {record}"),
                };
                let recorded = Value::Object(outputs);
                if !same(&recorded, &made) {
                    let name = name.map_or(String::new(), |name| format!("{name} "));
                    failures.push(format!(
                        "{section} {name}{}\n  Tiptap: {recorded}\n  tarnish-tiptap: {made}",
                        Value::Object(given)
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Holds each of `nested` at depths 1 to 3 to the record of its conversion, then converts it
    /// `depth` deep on a stack too small to recurse through it without growing.
    #[track_caller]
    pub fn check_nested(&self, nested: &'static [Nested]) {
        for nested in nested {
            let fields = inputs(nested.section).expect("a section of a configuration");
            let records = self.fixture[nested.section].as_array().expect("records");
            for depth in 1..=3 {
                let given = nested.inputs(depth);
                let record = records.iter().find(|record| {
                    record["extensions"] == nested.configuration
                        && fields
                            .iter()
                            .all(|field| match (record.get(field), given.get(field)) {
                                (Some(recorded), Some(given)) => same(recorded, given),
                                (recorded, given) => recorded.is_none() && given.is_none(),
                            })
                });
                let record = record.unwrap_or_else(|| panic!("no record of {}", nested.what));
                expect(
                    record,
                    &[fields, &["extensions"]].concat(),
                    (nested.output)(depth),
                );
            }
        }
        let configuration = self.configuration;
        on_dirty_scheduler_stack(move || {
            let mut converters = Converters::new(configuration);
            for nested in nested {
                let converter = converters.of(nested.configuration);
                let made = converter.convert(nested.section, &nested.inputs(nested.depth));
                let expected = (nested.output)(nested.depth);
                assert!(same(&expected, &made), "{}", nested.what);
            }
        });
    }
}

/// How a record of `schemas` differs from the schema its configuration builds: in each type that
/// differs, or in the whole schema where no one type does.
fn schema_failures(record: &Value, converters: &mut Converters) -> Vec<String> {
    let mut expected = record.clone();
    let fields = expected.as_object_mut().expect("a record");
    let name = fields
        .shift_remove("extensions")
        .expect("the configuration it's of");
    let actual = schema_record(&converters.of(string(&name)).schema.schema);
    if same(&expected, &actual) {
        return Vec::new();
    }
    let write = |record: Option<&Value>| record.map_or("none".into(), Value::to_string);
    let mut differ = Vec::new();
    for kind in ["nodes", "marks"] {
        let types = |record: &Value| record[kind].as_array().cloned().unwrap_or_default();
        let (recorded, made) = (types(&expected), types(&actual));
        for index in 0..recorded.len().max(made.len()) {
            let (recorded, made) = (write(recorded.get(index)), write(made.get(index)));
            if recorded != made {
                differ.push(format!(
                    "schema {name} {kind}[{index}]\n  Tiptap: {recorded}\n  tarnish-tiptap: {made}"
                ));
            }
        }
    }
    if differ.is_empty() {
        differ.push(format!(
            "schema {name}\n  Tiptap: {expected}\n  tarnish-tiptap: {actual}"
        ));
    }
    differ
}
