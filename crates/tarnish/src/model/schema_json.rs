//! Schema specs as data: what a spec written in JSON, or in another language's values, holds,
//! read as ProseMirror reads a spec object.

use std::sync::Arc;

use super::attrs::{AttributeDefault, AttributeSpec, Validate};
use super::schema::{MarkSpec, NodeSpec, SchemaSpec, Whitespace};
use crate::error::{Error, Result};
use crate::js;
use crate::json::{self, Map, Value};

fn invalid(what: &str) -> Error {
    Error::Range(format!("Invalid schema spec: {what}"))
}

/// JavaScript's `value == text`, for a `text` that as a number is `0` when empty and `NaN`
/// otherwise.
fn loosely_equals(value: &Value, text: &str) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => !value && text.is_empty(),
        Value::Number(number) => text.is_empty() && number.as_f64() == Some(0.0),
        Value::String(value) => value == text,
        Value::Array(_) | Value::Object(_) => js::to_string(value) == text,
    }
}

/// A property that ProseMirror splits or parses as a string when it is truthy, and passes over
/// when it isn't.
fn truthy_string(spec: &Map, key: &str) -> Result<Option<String>> {
    match spec.get(key).filter(|value| js::truthy(Some(value))) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(&format!("{key} must be a string"))),
    }
}

/// `marks`, which ProseMirror compares with `"_"`, then splits when truthy, and otherwise
/// compares with `""`.
fn marks(spec: &Map) -> Result<Option<String>> {
    match spec.get("marks") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(expr)) => Ok(Some(expr.clone())),
        Some(value) if loosely_equals(value, "_") => Ok(Some("_".into())),
        Some(value) if !js::truthy(Some(value)) => Ok(Some(String::new())),
        Some(_) => Err(invalid("marks must be a string")),
    }
}

/// `excludes`, which ProseMirror compares with `null` and then `""` before it splits it.
fn excludes(spec: &Map) -> Result<Option<String>> {
    match spec.get("excludes") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(expr)) => Ok(Some(expr.clone())),
        Some(value) if loosely_equals(value, "") => Ok(Some(String::new())),
        Some(_) => Err(invalid("excludes must be a string")),
    }
}

fn flag(spec: &Map, key: &str) -> bool {
    js::truthy(spec.get(key))
}

/// A flag that only `false` turns off, as ProseMirror compares it with `=== false`.
fn unless_false(spec: &Map, key: &str) -> Option<bool> {
    match spec.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

/// The properties of a type's or an attribute's spec. ProseMirror finds none it reads in a
/// value that isn't an object, and throws on `null`.
fn properties<'a>(value: &'a Value, what: &str) -> Result<&'a Map> {
    match value {
        Value::Object(properties) => Ok(properties),
        Value::Null => Err(invalid(&format!("{what} must not be null"))),
        _ => Ok(&json::EMPTY),
    }
}

/// The object under `key`, whose properties ProseMirror visits with `for...in`. That finds none
/// in other values but a string's or an array's indexes, which a spec can't mean as names, so
/// those are refused.
fn entries<'a>(spec: &'a Map, key: &str) -> Result<Option<&'a Map>> {
    match spec.get(key) {
        Some(Value::Object(entries)) => Ok(Some(entries)),
        Some(Value::String(_) | Value::Array(_)) => {
            Err(invalid(&format!("{key} must be an object")))
        }
        _ => Ok(None),
    }
}

fn validate(attr: &Map, name: &str) -> Option<Validate> {
    match attr.get("validate") {
        Some(Value::String(types)) => Some(Validate::Types(types.clone())),
        // ProseMirror calls any other truthy value when it checks the attribute, which throws.
        Some(value) if js::truthy(Some(value)) => {
            let message = format!("The validate of attribute {name} is not a function");
            Some(Validate::Hook(Arc::new(move |_| {
                Err(Error::Other(message.clone()))
            })))
        }
        _ => None,
    }
}

fn attributes(spec: &Map) -> Result<Vec<(String, AttributeSpec)>> {
    let Some(attrs) = entries(spec, "attrs")? else {
        return Ok(Vec::new());
    };
    attrs
        .iter()
        .map(|(name, attr)| {
            let attr = properties(attr, &format!("attribute {name}"))?;
            Ok((
                name.to_string(),
                AttributeSpec {
                    default: attr
                        .get("default")
                        .cloned()
                        .map_or(AttributeDefault::Required, AttributeDefault::Value),
                    validate: validate(attr, name),
                },
            ))
        })
        .collect()
}

fn node_spec(spec: &Map) -> Result<NodeSpec> {
    Ok(NodeSpec {
        content: truthy_string(spec, "content")?,
        marks: marks(spec)?,
        group: truthy_string(spec, "group")?,
        inline: flag(spec, "inline"),
        atom: flag(spec, "atom"),
        attrs: attributes(spec)?,
        selectable: unless_false(spec, "selectable"),
        draggable: flag(spec, "draggable"),
        code: flag(spec, "code"),
        // ProseMirror keeps any truthy value, which parses as `"normal"` does unless it is
        // `"pre"`.
        whitespace: match spec.get("whitespace") {
            value if !js::truthy(value) => None,
            Some(value) if loosely_equals(value, "pre") => Some(Whitespace::Pre),
            _ => Some(Whitespace::Normal),
        },
        defining_as_context: flag(spec, "definingAsContext"),
        defining_for_content: flag(spec, "definingForContent"),
        defining: flag(spec, "defining"),
        isolating: flag(spec, "isolating"),
        linebreak_replacement: flag(spec, "linebreakReplacement"),
        leaf_text: None,
        to_debug_string: None,
        extra: rest(
            spec,
            &[
                "content",
                "marks",
                "group",
                "inline",
                "atom",
                "attrs",
                "selectable",
                "draggable",
                "code",
                "whitespace",
                "definingAsContext",
                "definingForContent",
                "defining",
                "isolating",
                "linebreakReplacement",
            ],
        ),
    })
}

fn mark_spec(spec: &Map) -> Result<MarkSpec> {
    Ok(MarkSpec {
        attrs: attributes(spec)?,
        inclusive: unless_false(spec, "inclusive"),
        excludes: excludes(spec)?,
        group: truthy_string(spec, "group")?,
        spanning: unless_false(spec, "spanning"),
        code: flag(spec, "code"),
        extra: rest(
            spec,
            &[
                "attrs",
                "inclusive",
                "excludes",
                "group",
                "spanning",
                "code",
            ],
        ),
    })
}

/// The properties of the spec other than those read.
fn rest(spec: &Map, read: &[&str]) -> Map {
    spec.iter()
        .filter(|(key, _)| !read.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The node or mark types under `key`, in order: an object of each type's spec, or, where the
/// data can't keep an object's order, an array of `[name, spec]` pairs.
fn types<T>(spec: &Map, key: &str, read: fn(&Map) -> Result<T>) -> Result<Vec<(String, T)>> {
    let read_type =
        |name: &str, type_spec: &Value| Ok((name.to_owned(), read(properties(type_spec, name)?)?));
    match spec.get(key) {
        Some(Value::Array(pairs)) => pairs
            .iter()
            .map(|pair| match pair.as_array().map(Vec::as_slice) {
                Some([Value::String(name), type_spec]) => read_type(name, type_spec),
                _ => Err(invalid(&format!("{key} must hold [name, spec] pairs"))),
            })
            .collect(),
        _ => {
            let Some(types) = entries(spec, key)? else {
                return Ok(Vec::new());
            };
            types
                .iter()
                .map(|(name, type_spec)| read_type(name, type_spec))
                .collect()
        }
    }
}

impl SchemaSpec {
    /// A spec from data: `nodes` and `marks`, each an object of the types' specs in order or an
    /// array of `[name, spec]` pairs, and `topNode`, as ProseMirror's `SchemaSpec` holds them.
    /// Functions a spec can hold in JavaScript, such as `toDOM` and `leafText`, have no place
    /// here, and properties ProseMirror doesn't read are kept in each spec's `extra`.
    pub fn from_json(json: &Value) -> Result<SchemaSpec> {
        let spec = json
            .as_object()
            .ok_or_else(|| invalid("the spec must be an object"))?;
        Ok(SchemaSpec {
            nodes: types(spec, "nodes", node_spec)?,
            marks: types(spec, "marks", mark_spec)?,
            top_node: spec
                .get("topNode")
                .filter(|name| js::truthy(Some(name)))
                .map(js::to_string),
        })
    }
}
