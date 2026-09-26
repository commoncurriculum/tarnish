//! Schema specs as data: what a spec written in JSON, or in another language's values, holds.

use super::schema::{AttributeSpec, MarkSpec, NodeSpec, SchemaSpec, Validate, Whitespace};
use crate::error::{Error, Result};
use crate::value::{Object, Value};

fn invalid(what: &str) -> Error {
    Error::Range(format!("Invalid schema spec: {what}"))
}

/// A property that, when set, must be a string.
fn string(spec: &Object, key: &str) -> Result<Option<String>> {
    match spec.get(key) {
        None | Some(Value::Undefined | Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.to_string())),
        Some(_) => Err(invalid(&format!("{key} must be a string"))),
    }
}

fn flag(spec: &Object, key: &str) -> bool {
    spec.get(key).is_some_and(Value::is_truthy)
}

fn optional_flag(spec: &Object, key: &str) -> Option<bool> {
    match spec.get(key) {
        None | Some(Value::Undefined) => None,
        Some(value) => Some(value.is_truthy()),
    }
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Object> {
    value
        .as_object()
        .ok_or_else(|| invalid(&format!("{what} must be an object")))
}

fn attributes(spec: &Object) -> Result<Vec<(String, AttributeSpec)>> {
    let Some(attrs) = spec.get("attrs") else {
        return Ok(Vec::new());
    };
    if matches!(attrs, Value::Undefined | Value::Null) {
        return Ok(Vec::new());
    }
    object(attrs, "attrs")?
        .iter()
        .map(|(name, attr)| {
            let attr = object(attr, &format!("attribute {name}"))?;
            Ok((
                name.to_string(),
                AttributeSpec {
                    default: attr.get("default").cloned(),
                    validate: string(attr, "validate")?.map(Validate::Types),
                },
            ))
        })
        .collect()
}

fn node_spec(spec: &Object) -> Result<NodeSpec> {
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

fn mark_spec(spec: &Object) -> Result<MarkSpec> {
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

fn types<T>(spec: &Object, key: &str, read: fn(&Object) -> Result<T>) -> Result<Vec<(String, T)>> {
    match spec.get(key) {
        None | Some(Value::Undefined | Value::Null) => Ok(Vec::new()),
        Some(types) => object(types, key)?
            .iter()
            .map(|(name, type_spec)| Ok((name.to_string(), read(object(type_spec, name)?)?)))
            .collect(),
    }
}

impl SchemaSpec {
    /// A spec from data: `nodes` and `marks` objects of each type's spec, in order, and
    /// `topNode`, as ProseMirror's `SchemaSpec` holds them. Functions a spec can hold in
    /// JavaScript, such as `toDOM` and `leafText`, have no place here, and other properties are
    /// ignored.
    pub fn from_json(json: &Value) -> Result<SchemaSpec> {
        let spec = object(json, "the spec")?;
        Ok(SchemaSpec {
            nodes: types(spec, "nodes", node_spec)?,
            marks: types(spec, "marks", mark_spec)?,
            top_node: string(spec, "topNode")?,
        })
    }
}
