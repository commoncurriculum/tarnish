//! The ops of [`transform`](super::transform), which `harness/record-ops.mjs` reads the same
//! way against the real `Transform`.

use std::borrow::Cow;

use crate::error::{Error, Result};
use crate::js;
use crate::json::{Map, NULL, Value};
use crate::model::{Fragment, Mark, MarkType, Node, NodeRange, NodeType, Schema, Slice};
use crate::transform::{
    BlockAttrs, MarkMatch, Step, Transform, Wrapper, find_wrapping, lift_target,
};

/// JavaScript's `Number.MAX_SAFE_INTEGER`: `record-ops.mjs` takes no position past it.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub(super) fn apply<'a>(tr: &mut Transform<'a>, schema: &Schema, json: &Value) -> Result<()> {
    let op = Op {
        json,
        name: js::string(json.get("op")),
        schema,
    };
    match op.name.as_ref() {
        "replace" => {
            let (from, to) = op.range(true)?;
            tr.replace(from, to, &op.slice("slice")?)?;
        }
        "replaceWith" => {
            let (from, to) = op.range(false)?;
            tr.replace_with(from, to, op.content("content")?)?;
        }
        "delete" => {
            let (from, to) = op.range(false)?;
            tr.delete(from, to)?;
        }
        "insert" => {
            let pos = op.whole("pos")?;
            tr.insert(pos, op.content("content")?)?;
        }
        "replaceRange" => {
            let (from, to) = op.range(false)?;
            tr.replace_range(from, to, &op.slice("slice")?)?;
        }
        "replaceRangeWith" => {
            let (from, to) = op.range(false)?;
            tr.replace_range_with(from, to, op.node("node")?)?;
        }
        "deleteRange" => {
            let (from, to) = op.range(false)?;
            tr.delete_range(from, to)?;
        }
        "addMark" => {
            let (from, to) = op.range(false)?;
            tr.add_mark(from, to, &op.mark("mark")?)?;
        }
        "removeMark" => {
            let (from, to) = op.range(false)?;
            let mark = op.given("mark").map(|_| op.mark_or_type("mark"));
            let mark = mark.transpose()?;
            tr.remove_mark(from, to, mark.as_ref().map(|mark| mark.matching()))?;
        }
        "addNodeMark" => {
            let pos = op.whole("pos")?;
            tr.add_node_mark(pos, op.mark("mark")?)?;
        }
        "removeNodeMark" => {
            let pos = op.whole("pos")?;
            tr.remove_node_mark(pos, op.mark_or_type("mark")?.matching())?;
        }
        "setNodeMarkup" => {
            let pos = op.whole("pos")?;
            let node_type = op.given("type").map(|_| op.node_type("type"));
            let node_type = node_type.transpose()?;
            let attrs = op.attrs("attrs")?;
            let marks = op.marks("marks")?;
            tr.set_node_markup(pos, node_type.as_ref(), attrs, marks.as_deref())?;
        }
        "setNodeAttribute" => {
            let pos = op.whole("pos")?;
            tr.set_node_attribute(pos, op.string("attr")?, op.get("value").cloned())?;
        }
        "setDocAttribute" => {
            tr.set_doc_attribute(op.string("attr")?, op.get("value").cloned())?;
        }
        "setBlockType" => {
            let (from, to) = op.range(true)?;
            let node_type = op.node_type("type")?;
            let attrs = BlockAttrs::Fixed(op.attrs("attrs")?);
            tr.set_block_type(from, to, &node_type, attrs)?;
        }
        "lift" => {
            let (from, to) = op.range(false)?;
            let depth = op.optional_whole("depth")?;
            let target = op.optional_whole("target")?;
            let range = block_range(tr.doc(), from, to, depth)?;
            let target = match target {
                Some(target) => target,
                None => lift_target(&range)?.ok_or_else(|| {
                    Error::Range(format!("Can't lift the range from {from} to {to}"))
                })?,
            };
            tr.lift(&range, target)?;
        }
        "wrap" => {
            let (from, to) = op.range(false)?;
            let depth = op.optional_whole("depth")?;
            let given = op.given("wrappers").map(|_| op.wrappers("wrappers"));
            match given.transpose()? {
                Some(wrappers) => {
                    let range = block_range(tr.doc(), from, to, depth)?;
                    tr.wrap(&range, &wrappers)?;
                }
                None => {
                    let node_type = op.node_type("nodeType")?;
                    let attrs = op.attrs("attrs")?.cloned();
                    let range = block_range(tr.doc(), from, to, depth)?;
                    let wrappers = find_wrapping(&range, &node_type, attrs, None)?;
                    let wrappers = wrappers.ok_or_else(|| {
                        Error::Range(format!(
                            "Can't wrap the range from {from} to {to} in {}",
                            node_type.name()
                        ))
                    })?;
                    tr.wrap(&range, &wrappers)?;
                }
            }
        }
        "join" => {
            let pos = op.whole("pos")?;
            tr.join(pos, op.whole_or("depth", 1)?)?;
        }
        "split" => {
            let pos = op.whole("pos")?;
            let depth = op.whole_or("depth", 1)?;
            tr.split(pos, depth, &op.types_after("typesAfter")?)?;
        }
        "clearIncompatible" => {
            let pos = op.whole("pos")?;
            tr.clear_incompatible(pos, &op.node_type("parentType")?, None)?;
        }
        "step" => {
            tr.step(Step::from_json(schema, op.field("step"))?)?;
        }
        "maybeStep" => {
            tr.maybe_step(Step::from_json(schema, op.field("step"))?)?;
        }
        name => return Err(Error::Range(format!("Unknown Transform method: {name}"))),
    }
    Ok(())
}

/// `from.blockRange(to)`, or with `depth` the range of the children of the ancestor at that
/// depth, which both positions must be in.
fn block_range<'a>(
    doc: &Node<'a>,
    from: usize,
    to: usize,
    depth: Option<usize>,
) -> Result<NodeRange<'a>> {
    let (start, end) = (doc.resolve(from)?, doc.resolve(to)?);
    match depth {
        None => start
            .block_range(&end, None)?
            .ok_or_else(|| Error::Range(format!("No block range from {from} to {to}"))),
        Some(depth) if depth > start.shared_depth(to) => Err(Error::Range(format!(
            "No block range from {from} to {to} at depth {depth}"
        ))),
        Some(depth) => Ok(NodeRange::new(start, end, depth)),
    }
}

enum MarkOrType<'s> {
    Mark(Mark<'static>),
    Type(MarkType<'s>),
}

impl MarkOrType<'_> {
    fn matching<'a>(&self) -> MarkMatch<'_, 'a> {
        match self {
            MarkOrType::Mark(mark) => MarkMatch::Mark(mark),
            MarkOrType::Type(mark_type) => MarkMatch::Type(*mark_type),
        }
    }
}

/// An op's JSON, read as the arguments of the method it names.
struct Op<'j, 's> {
    json: &'j Value,
    name: Cow<'j, str>,
    schema: &'s Schema,
}

impl<'j, 's> Op<'j, 's> {
    fn get(&self, key: &str) -> Option<&'j Value> {
        self.json.get(key)
    }

    /// The field when it is neither missing nor null.
    fn given(&self, key: &str) -> Option<&'j Value> {
        self.get(key).filter(|value| !value.is_null())
    }

    fn field(&self, key: &str) -> &'j Value {
        self.get(key).unwrap_or(&NULL)
    }

    fn invalid(&self, key: &str) -> Error {
        Error::Range(format!("Invalid {key} for {}", self.name))
    }

    fn whole(&self, key: &str) -> Result<usize> {
        self.get(key)
            .and_then(Value::as_f64)
            .filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n <= MAX_SAFE_INTEGER)
            .map(|n| n as usize)
            .ok_or_else(|| self.invalid(key))
    }

    fn whole_or(&self, key: &str, default: usize) -> Result<usize> {
        match self.given(key) {
            Some(_) => self.whole(key),
            None => Ok(default),
        }
    }

    fn optional_whole(&self, key: &str) -> Result<Option<usize>> {
        self.given(key).map(|_| self.whole(key)).transpose()
    }

    /// `from` and `to`, which must run forwards; `to` is `from` when it's optional and missing.
    fn range(&self, to_is_optional: bool) -> Result<(usize, usize)> {
        let from = self.whole("from")?;
        let to = match to_is_optional {
            true => self.whole_or("to", from)?,
            false => self.whole("to")?,
        };
        if to < from {
            return Err(Error::Range(format!(
                "Invalid range from {from} to {to} for {}",
                self.name
            )));
        }
        Ok((from, to))
    }

    fn string(&self, key: &str) -> Result<&'j str> {
        self.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| self.invalid(key))
    }

    fn node_type(&self, key: &str) -> Result<NodeType<'s>> {
        self.schema.expect_node_type(self.string(key)?)
    }

    fn attrs(&self, key: &str) -> Result<Option<&'j Map>> {
        match self.given(key) {
            None => Ok(None),
            Some(Value::Object(attrs)) => Ok(Some(attrs)),
            Some(_) => Err(self.invalid(key)),
        }
    }

    fn mark(&self, key: &str) -> Result<Mark<'static>> {
        Mark::from_json(self.schema, self.field(key))
    }

    /// A mark type by name, or a mark from its JSON.
    fn mark_or_type(&self, key: &str) -> Result<MarkOrType<'s>> {
        match self.get(key) {
            Some(Value::String(name)) => match self.schema.mark_type(name) {
                Some(mark_type) => Ok(MarkOrType::Type(mark_type)),
                None => Err(Error::Range(format!(
                    "There is no mark type {name} in this schema"
                ))),
            },
            _ => self.mark(key).map(MarkOrType::Mark),
        }
    }

    fn marks(&self, key: &str) -> Result<Option<Vec<Mark<'static>>>> {
        match self.given(key) {
            None => Ok(None),
            Some(Value::Array(marks)) => marks
                .iter()
                .map(|mark| Mark::from_json(self.schema, mark))
                .collect::<Result<_>>()
                .map(Some),
            Some(_) => Err(self.invalid(key)),
        }
    }

    fn node(&self, key: &str) -> Result<Node<'static>> {
        Node::from_json(self.schema, self.field(key))
    }

    /// A fragment from an array of nodes' JSON or from null, or else a node.
    fn content(&self, key: &str) -> Result<Fragment<'static>> {
        match self.given(key) {
            None | Some(Value::Array(_)) => Fragment::from_json(self.schema, self.field(key)),
            Some(node) => Ok(Fragment::from_node(Node::from_json(self.schema, node)?)),
        }
    }

    fn slice(&self, key: &str) -> Result<Slice<'static>> {
        Slice::from_json(self.schema, self.field(key))
    }

    /// `{type, attrs}`, the type by name.
    fn wrapper(&self, json: &Value, key: &str) -> Result<Wrapper<'s>> {
        let attrs = match json.get("attrs") {
            None | Some(Value::Null) => Some(None),
            Some(Value::Object(attrs)) => Some(Some(attrs.clone())),
            Some(_) => None,
        };
        match (json.get("type").and_then(Value::as_str), attrs) {
            (Some(name), Some(attrs)) => Ok(Wrapper {
                node_type: self.schema.expect_node_type(name)?,
                attrs,
            }),
            _ => Err(self.invalid(key)),
        }
    }

    fn wrappers(&self, key: &str) -> Result<Vec<Wrapper<'s>>> {
        match self.get(key) {
            Some(Value::Array(wrappers)) => wrappers
                .iter()
                .map(|wrapper| self.wrapper(wrapper, key))
                .collect(),
            _ => Err(self.invalid(key)),
        }
    }

    fn types_after(&self, key: &str) -> Result<Vec<Option<Wrapper<'s>>>> {
        match self.given(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(types)) => types
                .iter()
                .map(|json| match json {
                    Value::Null => Ok(None),
                    json => self.wrapper(json, key).map(Some),
                })
                .collect(),
            Some(_) => Err(self.invalid(key)),
        }
    }
}
