//! The operations the bindings expose, on ProseMirror's JSON: each binding only turns its own
//! values into [`Value`]s and back.

use crate::error::{Error, Result};
use crate::json::Value;
use crate::model::{Node, Schema, SchemaSpec};
use crate::transform::{Mappable, Mapping, Step, Transform};

/// A schema from its spec: `nodes` and `marks`, each an object of the types' specs in order or
/// an array of `[name, spec]` pairs, and `topNode`.
pub fn schema(spec: &Value) -> Result<Schema> {
    Schema::new(SchemaSpec::from_json(spec)?)
}

/// Checks that the document conforms to the schema.
pub fn check(schema: &Schema, doc: &Value) -> Result<()> {
    Node::from_json(schema, doc)?.check()
}

/// The document with the steps applied in order.
pub fn apply_steps(schema: &Schema, doc: &Value, steps: &Value) -> Result<Value> {
    Ok(transform(schema, doc, steps)?.doc().to_json())
}

/// The steps that undo the steps applied to the document, last first.
pub fn invert_steps(schema: &Schema, doc: &Value, steps: &Value) -> Result<Value> {
    let tr = transform(schema, doc, steps)?;
    let inverted = tr
        .steps()
        .iter()
        .zip(tr.docs())
        .rev()
        .map(|(step, before)| Ok(step.invert(before)?.to_json()))
        .collect::<Result<Vec<_>>>()?;
    Ok(Value::Array(inverted))
}

/// A position mapped through the changes the steps make. With `assoc` below zero, a position
/// where content is inserted stays before it.
pub fn map_position(schema: &Schema, steps: &Value, pos: usize, assoc: i32) -> Result<usize> {
    let mut mapping = Mapping::new();
    for step in step_list(schema, steps)? {
        mapping.append_map(step.get_map(), None);
    }
    Ok(mapping.map(pos, assoc))
}

fn step_list(schema: &Schema, json: &Value) -> Result<Vec<Step>> {
    match json {
        Value::Array(steps) => steps
            .iter()
            .map(|step| Step::from_json(schema, step))
            .collect(),
        _ => Err(Error::Range("Steps must be an array".into())),
    }
}

fn transform(schema: &Schema, doc: &Value, steps: &Value) -> Result<Transform> {
    let mut tr = Transform::new(Node::from_json(schema, doc)?);
    for step in step_list(schema, steps)? {
        tr.step(step)?;
    }
    Ok(tr)
}
