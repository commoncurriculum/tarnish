//! The operations the bindings expose. Specs and steps are ProseMirror's JSON; documents are
//! nodes, which a binding reads from JSON once and keeps.

use crate::error::{Error, Result};
use crate::json::Value;
use crate::model::{Node, Schema, SchemaSpec};
use crate::transform::{Mappable, Mapping, Step, Transform};

/// A schema from its spec: `nodes` and `marks`, each an object of the types' specs in order or
/// an array of `[name, spec]` pairs, and `topNode`.
pub fn schema(spec: &Value) -> Result<Schema> {
    Schema::new(SchemaSpec::from_json(spec)?)
}

/// The document with the steps applied in order.
pub fn apply_steps(doc: &Node, steps: &Value) -> Result<Node> {
    Ok(transform(doc, steps)?.doc().clone())
}

/// The steps that undo the steps applied to the document, last first.
pub fn invert_steps(doc: &Node, steps: &Value) -> Result<Value> {
    let tr = transform(doc, steps)?;
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

fn transform(doc: &Node, steps: &Value) -> Result<Transform> {
    let mut tr = Transform::new(doc.clone());
    for step in step_list(doc.node_type().schema(), steps)? {
        tr.step(step)?;
    }
    Ok(tr)
}
