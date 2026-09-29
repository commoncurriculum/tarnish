//! The operations the bindings expose. Specs and steps are ProseMirror's JSON; documents are
//! nodes, which a binding reads from JSON once and keeps.

mod ops;

use crate::Text;
use crate::js;
use crate::json::Value;
use crate::model::{Node, Schema, SchemaSpec};
use crate::transform::{Mappable, Mapping, Step, StepResult, TRANSFORM_ERROR, Transform};
use crate::{Error, Result};

/// A schema from its spec: `nodes` and `marks`, each an object of the types' specs in order or
/// an array of `[name, spec]` pairs, and `topNode`.
pub fn schema(spec: &Value) -> Result<Schema> {
    Schema::new(SchemaSpec::from_json(spec)?)
}

/// The document with the steps applied in order.
pub fn apply_steps<'a>(doc: &Node<'a>, steps: &Value) -> Result<Node<'a>> {
    let mut doc = doc.clone();
    for step in step_list(doc.schema(), steps)? {
        doc = match step.apply(doc)? {
            StepResult::Ok(doc) => doc,
            StepResult::Failed(message) => return Err(Error::Of(&TRANSFORM_ERROR, message)),
        };
    }
    Ok(doc)
}

/// The steps that undo the steps applied to the document, last first.
pub fn invert_steps(doc: &Node, steps: &Value) -> Result<Value> {
    let tr = stepped(doc, steps)?;
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

/// The document with the ops applied in order to one `Transform`, and the JSON of the steps
/// they made. An op is an object naming a `Transform` method in `"op"`, with the method's
/// arguments by name: nodes, fragments, slices, marks and steps as their JSON, and node and
/// mark types by name. For a `NodeRange`, an op gives `from`, `to` and optionally `depth`.
/// `lift` without a `target` and `wrap` without `wrappers` compute them with
/// [`lift_target`](crate::transform::lift_target) and
/// [`find_wrapping`](crate::transform::find_wrapping), `wrap` then taking the `nodeType` and
/// `attrs` to wrap in.
pub fn transform<'a>(doc: &Node<'a>, ops: &Value) -> Result<(Node<'a>, Value)> {
    let Value::Array(ops) = ops else {
        return Err(Error::Range("Ops must be an array".into()));
    };
    let mut tr = Transform::new(doc.clone());
    for op in ops {
        ops::apply(&mut tr, doc.schema(), op)?;
    }
    let steps = tr.steps().iter().map(Step::to_json).collect();
    Ok((tr.doc().clone(), Value::Array(steps)))
}

/// `textBetween`: the text between `from` and `to`, with `block_separator` between blocks and
/// `leaf_text` for leaves that aren't text.
pub fn text_between(
    doc: &Node,
    from: usize,
    to: usize,
    block_separator: Option<&str>,
    leaf_text: Option<&str>,
) -> Result<Text> {
    // ProseMirror reads a child past the last for a `to` past the content.
    if doc.text().is_none() && to > doc.content().size() {
        return Err(js::value::cannot_read(
            js::value::Nullish::Undefined,
            "nodeSize",
        ));
    }
    let separator = block_separator.map(Text::from);
    // An empty `leafText` is falsy, so the spec's `leafText` applies.
    match leaf_text.filter(|leaf| !leaf.is_empty()).map(Text::from) {
        Some(leaf) => doc.text_between(
            from,
            to,
            separator.as_ref(),
            Some(&mut |_: &Node| Ok(leaf.clone())),
        ),
        None => doc.text_between(from, to, separator.as_ref(), None),
    }
}

/// `textContent`: all the text in the node.
pub fn text_content(doc: &Node) -> Result<Text> {
    doc.text_content()
}

fn step_list(schema: &Schema, json: &Value) -> Result<Vec<Step<'static>>> {
    match json {
        Value::Array(steps) => steps
            .iter()
            .map(|step| Step::from_json(schema, step))
            .collect(),
        _ => Err(Error::Range("Steps must be an array".into())),
    }
}

fn stepped<'a>(doc: &Node<'a>, steps: &Value) -> Result<Transform<'a>> {
    let mut tr = Transform::new(doc.clone());
    for step in step_list(doc.schema(), steps)? {
        tr.step(step)?;
    }
    Ok(tr)
}
