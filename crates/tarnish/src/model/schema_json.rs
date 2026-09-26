//! Schema specs as data: what a spec written in JSON, or in another language's values, holds.

use super::schema::{AttributeSpec, MarkSpec, NodeSpec, SchemaSpec, Validate, Whitespace};
use crate::error::{Error, Result};
use crate::js;
use crate::json::{Map, Value};

fn invalid(what: &str) -> Error {
    Error::Range(format!("Invalid schema spec: {what}"))
}

/// A property that, when set, must be a string.
fn string(spec: &Map, key: &str) -> Result<Option<String>> {
    match spec.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(&format!("{key} must be a string"))),
    }
}

fn flag(spec: &Map, key: &str) -> bool {
    js::truthy(spec.get(key))
}

fn optional_flag(spec: &Map, key: &str) -> Option<bool> {
    spec.get(key).map(|value| js::truthy(Some(value)))
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map> {
    value
        .as_object()
        .ok_or_else(|| invalid(&format!("{what} must be an object")))
}

fn attributes(spec: &Map) -> Result<Vec<(String, AttributeSpec)>> {
    let attrs = match spec.get("attrs") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(attrs) => object(attrs, "attrs")?,
    };
    attrs
        .iter()
        .map(|(name, attr)| {
            let attr = object(attr, &format!("attribute {name}"))?;
            Ok((
                name.to_string(),
                AttributeSpec {
                    default: attr.get("default").cloned().map(Some),
                    validate: string(attr, "validate")?.map(Validate::Types),
                },
            ))
        })
        .collect()
}

fn node_spec(spec: &Map) -> Result<NodeSpec> {
    Ok(NodeSpec {
        content: string(spec, "content")?,
        marks: string(spec, "marks")?,
        group: string(spec, "group")?,
        inline: flag(spec, "inline"),
        atom: flag(spec, "atom"),
        attrs: attributes(spec)?,
        selectable: optional_flag(spec, "selectable"),
        draggable: flag(spec, "draggable"),
        code: flag(spec, "code"),
        whitespace: match string(spec, "whitespace")?.as_deref() {
            None => None,
            Some("pre") => Some(Whitespace::Pre),
            Some("normal") => Some(Whitespace::Normal),
            Some(_) => return Err(invalid("whitespace must be \"pre\" or \"normal\"")),
        },
        defining_as_context: flag(spec, "definingAsContext"),
        defining_for_content: flag(spec, "definingForContent"),
        defining: flag(spec, "defining"),
        isolating: flag(spec, "isolating"),
        linebreak_replacement: flag(spec, "linebreakReplacement"),
        leaf_text: None,
        to_debug_string: None,
    })
}

fn mark_spec(spec: &Map) -> Result<MarkSpec> {
    Ok(MarkSpec {
        attrs: attributes(spec)?,
        inclusive: match spec.get("inclusive") {
            Some(Value::Bool(inclusive)) => Some(*inclusive),
            _ => None,
        },
        excludes: string(spec, "excludes")?,
        group: string(spec, "group")?,
        spanning: optional_flag(spec, "spanning"),
        code: flag(spec, "code"),
    })
}

/// The node or mark types under `key`, in order: an object of each type's spec, or, where the
/// data can't keep an object's order, an array of `[name, spec]` pairs.
fn types<T>(spec: &Map, key: &str, read: fn(&Map) -> Result<T>) -> Result<Vec<(String, T)>> {
    let read_type =
        |name: &str, type_spec: &Value| Ok((name.to_owned(), read(object(type_spec, name)?)?));
    match spec.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(pairs)) => pairs
            .iter()
            .map(|pair| match pair.as_array().map(Vec::as_slice) {
                Some([Value::String(name), type_spec]) => read_type(name, type_spec),
                _ => Err(invalid(&format!("{key} must hold [name, spec] pairs"))),
            })
            .collect(),
        Some(types) => object(types, key)?
            .iter()
            .map(|(name, type_spec)| read_type(name, type_spec))
            .collect(),
    }
}

impl SchemaSpec {
    /// A spec from data: `nodes` and `marks`, each an object of the types' specs in order or an
    /// array of `[name, spec]` pairs, and `topNode`, as ProseMirror's `SchemaSpec` holds them.
    /// Functions a spec can hold in JavaScript, such as `toDOM` and `leafText`, have no place
    /// here, and other properties are ignored.
    pub fn from_json(json: &Value) -> Result<SchemaSpec> {
        let spec = object(json, "the spec")?;
        Ok(SchemaSpec {
            nodes: types(spec, "nodes", node_spec)?,
            marks: types(spec, "marks", mark_spec)?,
            top_node: string(spec, "topNode")?,
        })
    }
}
