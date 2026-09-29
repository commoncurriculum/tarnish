//! Steps: the atomic changes a transform is made of.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock, RwLock};

use super::map::{Mappable, StepMap};
use crate::js;
use crate::js::stack;
use crate::json::{Map, NULL, Value};
use crate::model::{Fragment, Mark, Node, REPLACE_ERROR, Schema, Slice};
use crate::{Error, Result};

/// A step type of an application's own, as a subclass of ProseMirror's `Step` is. Register it
/// with [`register_step`] so that [`Step::from_json`] reads it. It outlives any one document,
/// so content it holds is owned, as [`Node::compact`] makes it.
pub trait CustomStep: Any + fmt::Debug + Send + Sync {
    /// The `stepType` it is registered under.
    fn json_id(&self) -> &str;

    fn apply<'a>(&self, doc: Node<'a>) -> Result<StepResult<'a>>;

    fn get_map(&self) -> StepMap {
        StepMap::empty()
    }

    fn invert<'a>(&self, doc: &Node<'a>) -> Result<Step<'a>>;

    fn map<'a>(&self, mapping: &dyn Mappable) -> Option<Step<'a>>;

    fn merge<'a>(&self, _other: &Step<'a>) -> Option<Step<'a>> {
        None
    }

    /// Its JSON, with its `stepType`.
    fn to_json(&self) -> Value;
}

/// A registered step type's `fromJSON`.
pub type StepFromJson = fn(&Schema, &Value) -> Result<Arc<dyn CustomStep>>;

static CUSTOM_STEPS: LazyLock<RwLock<HashMap<String, StepFromJson>>> =
    LazyLock::new(Default::default);

/// `Step.jsonID`: read JSON whose `stepType` is `id` with `from_json`. Each identifier can be
/// registered once, and ProseMirror's own steps hold theirs.
pub fn register_step(id: &str, from_json: StepFromJson) -> Result<()> {
    let mut steps = CUSTOM_STEPS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if STEP_TYPES.iter().any(|(own, ..)| *own == id) || steps.contains_key(id) {
        return Err(Error::Range(format!("Duplicate use of step JSON ID {id}")));
    }
    steps.insert(id.to_owned(), from_json);
    Ok(())
}

fn custom_step(id: &str) -> Option<StepFromJson> {
    let steps = CUSTOM_STEPS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    steps.get(id).copied()
}

/// A step's outcome: the changed document, or why the step can't apply to the document.
#[derive(Clone, Debug)]
pub enum StepResult<'a> {
    Ok(Node<'a>),
    Failed(String),
}

impl<'a> StepResult<'a> {
    /// `doc.replace(from, to, slice)`, a slice that doesn't fit being a failure.
    pub fn from_replace(
        doc: Node<'a>,
        from: usize,
        to: usize,
        slice: &Slice<'a>,
    ) -> Result<StepResult<'a>> {
        match doc.into_replaced(from, to, slice) {
            Ok(doc) => Ok(StepResult::Ok(doc)),
            Err(Error::Of(class, message)) if *class == REPLACE_ERROR => {
                Ok(StepResult::Failed(message))
            }
            Err(error) => Err(error),
        }
    }

    pub fn doc(&self) -> Option<&Node<'a>> {
        match self {
            StepResult::Ok(doc) => Some(doc),
            StepResult::Failed(_) => None,
        }
    }
}

/// Whether a mark step adds its mark or removes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkOp {
    Add,
    Remove,
}

impl MarkOp {
    /// The op that undoes this one.
    pub fn invert(self) -> MarkOp {
        match self {
            MarkOp::Add => MarkOp::Remove,
            MarkOp::Remove => MarkOp::Add,
        }
    }
}

/// A change to a document, which applies to the document it was made for.
#[derive(Clone, Debug)]
pub enum Step<'a> {
    /// Replace `from` to `to` with a slice. A `structure` step only replaces the closing and
    /// opening tokens between them, and fails where there is content.
    Replace {
        from: usize,
        to: usize,
        slice: Slice<'a>,
        structure: bool,
    },
    /// Replace `from` to `to` with a slice, keeping the content from `gap_from` to `gap_to` and
    /// putting it in the slice at `insert`.
    ReplaceAround {
        from: usize,
        to: usize,
        gap_from: usize,
        gap_to: usize,
        slice: Slice<'a>,
        insert: usize,
        structure: bool,
    },
    /// Add a mark to, or remove it from, the inline content from `from` to `to`.
    Mark {
        op: MarkOp,
        from: usize,
        to: usize,
        mark: Mark<'a>,
    },
    /// Add a mark to, or remove it from, the node at `pos`.
    NodeMark {
        op: MarkOp,
        pos: usize,
        mark: Mark<'a>,
    },
    /// Set an attribute of the node at `pos`. A `value` of `None` is `undefined`, which gives
    /// the attribute its default.
    Attr {
        pos: usize,
        attr: String,
        value: Option<Value>,
    },
    /// Set an attribute of the document's top node.
    DocAttr { attr: String, value: Option<Value> },
    /// A step of a type [`register_step`] registered.
    Custom(Arc<dyn CustomStep>),
}

/// The kinds of step JSON can hold.
#[derive(Clone, Copy)]
enum Kind {
    Replace,
    ReplaceAround,
    Mark(MarkOp),
    NodeMark(MarkOp),
    Attr,
    DocAttr,
}

/// Each step type's JSON identifier, the class JavaScript names in its errors, and its kind.
const STEP_TYPES: [(&str, &str, Kind); 8] = [
    ("replace", "ReplaceStep", Kind::Replace),
    ("replaceAround", "ReplaceAroundStep", Kind::ReplaceAround),
    ("addMark", "AddMarkStep", Kind::Mark(MarkOp::Add)),
    ("removeMark", "RemoveMarkStep", Kind::Mark(MarkOp::Remove)),
    (
        "addNodeMark",
        "AddNodeMarkStep",
        Kind::NodeMark(MarkOp::Add),
    ),
    (
        "removeNodeMark",
        "RemoveNodeMarkStep",
        Kind::NodeMark(MarkOp::Remove),
    ),
    ("attr", "AttrStep", Kind::Attr),
    ("docAttr", "DocAttrStep", Kind::DocAttr),
];

/// Whether there is content between `from` and `to`, not only closing and opening tokens.
fn content_between(doc: &Node, from: usize, to: usize) -> Result<bool> {
    let resolved = doc.resolve(from)?;
    let mut dist = to as isize - from as isize;
    let mut depth = resolved.depth();
    while dist > 0 && depth > 0 && resolved.index_after(depth) == resolved.node(depth).child_count()
    {
        depth -= 1;
        dist -= 1;
    }
    if dist > 0 {
        let mut next = resolved
            .node(depth)
            .maybe_child(resolved.index_after(depth));
        while dist > 0 {
            match next {
                Some(node) if !node.is_leaf() => next = node.first_child(),
                _ => return Ok(true),
            }
            dist -= 1;
        }
    }
    Ok(false)
}

/// The fragment with `f` applied to its inline nodes, at any depth, each with its parent.
fn map_fragment<'a>(
    fragment: &Fragment<'a>,
    f: &dyn Fn(&Node<'a>, &Node<'a>) -> Node<'a>,
    parent: &Node<'a>,
) -> Fragment<'a> {
    let mapped = fragment
        .children()
        .map(|child| {
            let mut mapped = child.clone();
            if child.content().size() > 0 {
                mapped = mapped.copy(stack::grow(|| map_fragment(child.content(), f, &child)));
            }
            if mapped.is_inline() {
                mapped = f(&mapped, parent);
            }
            mapped
        })
        .collect();
    Fragment::from_array(mapped)
}

/// A slice that puts `node` in place of the node at a position, which keeps its content when
/// it has any.
fn node_slice(node: Node, leaf: bool) -> Slice {
    Slice::new(Fragment::from_node(node), 0, if leaf { 0 } else { 1 })
}

fn attrs_with(attrs: Map, attr: &str, value: &Option<Value>) -> Map {
    let mut copy = attrs;
    match value {
        Some(value) => {
            copy.insert(attr.into(), value.clone());
        }
        None => {
            copy.shift_remove(attr);
        }
    }
    copy
}

impl<'a> Step<'a> {
    /// The step as a `T`, when it is one: JavaScript's `step instanceof T`.
    pub fn custom<T: CustomStep>(&self) -> Option<&T> {
        match self {
            Step::Custom(step) => (step.as_ref() as &dyn Any).downcast_ref(),
            _ => None,
        }
    }

    /// Apply the step to a document. A caller that keeps the document passes a clone of it; one
    /// that gives it up lets a replace change it in place.
    pub fn apply(&self, doc: Node<'a>) -> Result<StepResult<'a>> {
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                if *structure && content_between(&doc, *from, *to)? {
                    return Ok(StepResult::Failed(
                        "Structure replace would overwrite content".into(),
                    ));
                }
                StepResult::from_replace(doc, *from, *to, slice)
            }
            Step::ReplaceAround {
                from,
                to,
                gap_from,
                gap_to,
                slice,
                insert,
                structure,
            } => {
                if *structure
                    && (content_between(&doc, *from, *gap_from)?
                        || content_between(&doc, *gap_to, *to)?)
                {
                    return Ok(StepResult::Failed(
                        "Structure gap-replace would overwrite content".into(),
                    ));
                }
                let gap = doc.slice(*gap_from, *gap_to, false)?;
                if gap.open_start() > 0 || gap.open_end() > 0 {
                    return Ok(StepResult::Failed("Gap is not a flat range".into()));
                }
                match slice.insert_at(*insert, gap.content())? {
                    Some(inserted) => StepResult::from_replace(doc, *from, *to, &inserted),
                    None => Ok(StepResult::Failed("Content does not fit in gap".into())),
                }
            }
            Step::Mark {
                op: MarkOp::Add,
                from,
                to,
                mark,
            } => {
                let old = doc.slice(*from, *to, false)?;
                let resolved = doc.resolve(*from)?;
                let parent = resolved.node(resolved.shared_depth(*to));
                let mark_type = mark.mark_type();
                let add = |node: &Node<'a>, parent: &Node<'a>| {
                    if !node.is_atom() || !parent.node_type().allows_mark_type(&mark_type) {
                        return node.clone();
                    }
                    node.mark(mark.add_to_set(&node.marks()))
                };
                let content = map_fragment(old.content(), &add, parent);
                let slice = Slice::new(content, old.open_start(), old.open_end());
                StepResult::from_replace(doc, *from, *to, &slice)
            }
            Step::Mark {
                op: MarkOp::Remove,
                from,
                to,
                mark,
            } => {
                let old = doc.slice(*from, *to, false)?;
                let remove =
                    |node: &Node<'a>, _: &Node<'a>| node.mark(mark.remove_from_set(&node.marks()));
                let content = map_fragment(old.content(), &remove, &doc);
                let slice = Slice::new(content, old.open_start(), old.open_end());
                StepResult::from_replace(doc, *from, *to, &slice)
            }
            Step::NodeMark { op, pos, mark } => {
                let Some(node) = doc.node_at(*pos)? else {
                    return Ok(StepResult::Failed("No node at mark step's position".into()));
                };
                let marks = match op {
                    MarkOp::Add => mark.add_to_set(&node.marks()),
                    MarkOp::Remove => mark.remove_from_set(&node.marks()),
                };
                let updated = node.node_type().create(
                    Some(&node.attrs().to_map()),
                    Fragment::empty(),
                    &marks.to_vec(),
                )?;
                let slice = node_slice(updated, node.is_leaf());
                StepResult::from_replace(doc, *pos, *pos + 1, &slice)
            }
            Step::Attr { pos, attr, value } => {
                let Some(node) = doc.node_at(*pos)? else {
                    return Ok(StepResult::Failed(
                        "No node at attribute step's position".into(),
                    ));
                };
                let attrs = attrs_with(node.attrs().to_map(), attr, value);
                let updated = node.node_type().create(
                    Some(&attrs),
                    Fragment::empty(),
                    &node.marks().to_vec(),
                )?;
                let slice = node_slice(updated, node.is_leaf());
                StepResult::from_replace(doc, *pos, *pos + 1, &slice)
            }
            Step::DocAttr { attr, value } => {
                let attrs = attrs_with(doc.attrs().to_map(), attr, value);
                let updated = doc.node_type().create(
                    Some(&attrs),
                    doc.content().clone(),
                    &doc.marks().to_vec(),
                )?;
                Ok(StepResult::Ok(updated))
            }
            Step::Custom(step) => step.apply(doc),
        }
    }

    /// The map of the positions the step changes.
    pub fn get_map(&self) -> StepMap {
        match self {
            Step::Replace {
                from, to, slice, ..
            } => StepMap::new(vec![*from, to - from, slice.size()], false),
            Step::ReplaceAround {
                from,
                to,
                gap_from,
                gap_to,
                slice,
                insert,
                ..
            } => StepMap::new(
                vec![
                    *from,
                    gap_from - from,
                    *insert,
                    *gap_to,
                    to - gap_to,
                    // An `insert` past the slice's end can't apply, the slice having no such
                    // position, but its map can still be taken: JavaScript's holds a negative
                    // size there, which a map here can't.
                    slice.size().saturating_sub(*insert),
                ],
                false,
            ),
            Step::Custom(step) => step.get_map(),
            _ => StepMap::empty(),
        }
    }

    /// The step that undoes this one, given the document before it.
    pub fn invert(&self, doc: &Node<'a>) -> Result<Step<'a>> {
        Ok(match self {
            Step::Replace {
                from, to, slice, ..
            } => Step::Replace {
                from: *from,
                to: from + slice.size(),
                slice: doc.slice(*from, *to, false)?,
                structure: false,
            },
            Step::ReplaceAround {
                from,
                to,
                gap_from,
                gap_to,
                slice,
                insert,
                structure,
            } => {
                let gap = gap_to - gap_from;
                Step::ReplaceAround {
                    from: *from,
                    to: from + slice.size() + gap,
                    gap_from: from + insert,
                    gap_to: from + insert + gap,
                    slice: doc
                        .slice(*from, *to, false)?
                        .remove_between(gap_from - from, gap_to - from)?,
                    insert: gap_from - from,
                    structure: *structure,
                }
            }
            Step::Mark { op, from, to, mark } => Step::Mark {
                op: op.invert(),
                from: *from,
                to: *to,
                mark: mark.clone(),
            },
            Step::NodeMark {
                op: MarkOp::Add,
                pos,
                mark,
            } => {
                if let Some(node) = doc.node_at(*pos)? {
                    let marks = node.marks();
                    let new_set = mark.add_to_set(&marks);
                    if new_set.len() == marks.len() {
                        let replaced = marks.iter().find(|mark| !mark.is_in_set(&new_set));
                        return Ok(Step::NodeMark {
                            op: MarkOp::Add,
                            pos: *pos,
                            mark: replaced.unwrap_or_else(|| mark.clone()),
                        });
                    }
                }
                Step::NodeMark {
                    op: MarkOp::Remove,
                    pos: *pos,
                    mark: mark.clone(),
                }
            }
            Step::NodeMark {
                op: MarkOp::Remove,
                pos,
                mark,
            } => match doc.node_at(*pos)? {
                Some(node) if mark.is_in_set(&node.marks()) => Step::NodeMark {
                    op: MarkOp::Add,
                    pos: *pos,
                    mark: mark.clone(),
                },
                _ => self.clone(),
            },
            Step::Attr { pos, attr, .. } => {
                let node = js::value::non_null(doc.node_at(*pos)?, "attrs")?;
                Step::Attr {
                    pos: *pos,
                    attr: attr.clone(),
                    value: node.attrs_view().get(attr).map(|value| value.to_value()),
                }
            }
            Step::DocAttr { attr, .. } => Step::DocAttr {
                attr: attr.clone(),
                value: doc.attrs_view().get(attr).map(|value| value.to_value()),
            },
            Step::Custom(step) => step.invert(doc)?,
        })
    }

    /// The step with its positions mapped, or `None` when the mapping deleted what it changes.
    pub fn map(&self, mapping: &dyn Mappable) -> Option<Step<'a>> {
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                let to = mapping.map_result(*to, -1);
                // Upstream's `ReplaceStep.MAP_BIAS` is fixed at its default, 1, here: setting it
                // to -1 would map an insertion over one at the same position to before it.
                let from = mapping.map_result(*from, 1);
                if from.deleted_across() && to.deleted_across() {
                    return None;
                }
                Some(Step::Replace {
                    from: from.pos,
                    to: from.pos.max(to.pos),
                    slice: slice.clone(),
                    structure: *structure,
                })
            }
            Step::ReplaceAround {
                from,
                to,
                gap_from,
                gap_to,
                slice,
                insert,
                structure,
            } => {
                let mapped_from = mapping.map_result(*from, 1);
                let mapped_to = mapping.map_result(*to, -1);
                let mapped_gap_from = if from == gap_from {
                    mapped_from.pos
                } else {
                    mapping.map(*gap_from, -1)
                };
                let mapped_gap_to = if to == gap_to {
                    mapped_to.pos
                } else {
                    mapping.map(*gap_to, 1)
                };
                if (mapped_from.deleted_across() && mapped_to.deleted_across())
                    || mapped_gap_from < mapped_from.pos
                    || mapped_gap_to > mapped_to.pos
                {
                    return None;
                }
                Some(Step::ReplaceAround {
                    from: mapped_from.pos,
                    to: mapped_to.pos,
                    gap_from: mapped_gap_from,
                    gap_to: mapped_gap_to,
                    slice: slice.clone(),
                    insert: *insert,
                    structure: *structure,
                })
            }
            Step::Mark { op, from, to, mark } => {
                let from = mapping.map_result(*from, 1);
                let to = mapping.map_result(*to, -1);
                if (from.deleted() && to.deleted()) || from.pos >= to.pos {
                    return None;
                }
                Some(Step::Mark {
                    op: *op,
                    from: from.pos,
                    to: to.pos,
                    mark: mark.clone(),
                })
            }
            Step::NodeMark { op, pos, mark } => {
                let mapped = mapping.map_result(*pos, 1);
                (!mapped.deleted_after()).then(|| Step::NodeMark {
                    op: *op,
                    pos: mapped.pos,
                    mark: mark.clone(),
                })
            }
            Step::Attr { pos, attr, value } => {
                let mapped = mapping.map_result(*pos, 1);
                (!mapped.deleted_after()).then(|| Step::Attr {
                    pos: mapped.pos,
                    attr: attr.clone(),
                    value: value.clone(),
                })
            }
            Step::DocAttr { .. } => Some(self.clone()),
            Step::Custom(step) => step.map(mapping),
        }
    }

    /// The step and `other`, applied after it, as one step, if they can be.
    pub fn merge(&self, other: &Step<'a>) -> Option<Step<'a>> {
        match (self, other) {
            (
                Step::Replace {
                    from,
                    to,
                    slice,
                    structure: false,
                },
                Step::Replace {
                    from: other_from,
                    to: other_to,
                    slice: other_slice,
                    structure: false,
                },
            ) => {
                let joined = |first: &Slice<'a>, second: &Slice<'a>| {
                    if first.size() + second.size() == 0 {
                        Slice::empty()
                    } else {
                        Slice::new(
                            first.content().append(second.content()),
                            first.open_start(),
                            second.open_end(),
                        )
                    }
                };
                if from + slice.size() == *other_from
                    && slice.open_end() == 0
                    && other_slice.open_start() == 0
                {
                    Some(Step::Replace {
                        from: *from,
                        to: to + (other_to - other_from),
                        slice: joined(slice, other_slice),
                        structure: false,
                    })
                } else if other_to == from && slice.open_start() == 0 && other_slice.open_end() == 0
                {
                    Some(Step::Replace {
                        from: *other_from,
                        to: *to,
                        slice: joined(other_slice, slice),
                        structure: false,
                    })
                } else {
                    None
                }
            }
            (
                Step::Mark { op, from, to, mark },
                Step::Mark {
                    op: other_op,
                    from: other_from,
                    to: other_to,
                    mark: other_mark,
                },
            ) if op == other_op && other_mark == mark && from <= other_to && to >= other_from => {
                Some(Step::Mark {
                    op: *op,
                    from: *from.min(other_from),
                    to: *to.max(other_to),
                    mark: mark.clone(),
                })
            }
            (Step::Custom(step), other) => step.merge(other),
            _ => None,
        }
    }

    /// The step's JSON identifier, its `stepType`.
    pub fn json_id(&self) -> &str {
        match self {
            Step::Replace { .. } => "replace",
            Step::ReplaceAround { .. } => "replaceAround",
            Step::Mark {
                op: MarkOp::Add, ..
            } => "addMark",
            Step::Mark {
                op: MarkOp::Remove, ..
            } => "removeMark",
            Step::NodeMark {
                op: MarkOp::Add, ..
            } => "addNodeMark",
            Step::NodeMark {
                op: MarkOp::Remove, ..
            } => "removeNodeMark",
            Step::Attr { .. } => "attr",
            Step::DocAttr { .. } => "docAttr",
            Step::Custom(step) => step.json_id(),
        }
    }

    pub fn to_json(&self) -> Value {
        fn push(json: &mut Map, key: &str, value: impl Into<Value>) {
            json.push(key.into(), value.into());
        }
        fn push_slice(json: &mut Map, slice: &Slice, structure: bool) {
            if slice.size() > 0 {
                push(json, "slice", slice.to_json());
            }
            if structure {
                push(json, "structure", true);
            }
        }
        fn push_attr(json: &mut Map, attr: &str, value: &Option<Value>) {
            push(json, "attr", attr);
            if let Some(value) = value {
                push(json, "value", value.clone());
            }
        }
        let mut json = Map::new();
        push(&mut json, "stepType", self.json_id());
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                push(&mut json, "from", *from);
                push(&mut json, "to", *to);
                push_slice(&mut json, slice, *structure);
            }
            Step::ReplaceAround {
                from,
                to,
                gap_from,
                gap_to,
                slice,
                insert,
                structure,
            } => {
                push(&mut json, "from", *from);
                push(&mut json, "to", *to);
                push(&mut json, "gapFrom", *gap_from);
                push(&mut json, "gapTo", *gap_to);
                push(&mut json, "insert", *insert);
                push_slice(&mut json, slice, *structure);
            }
            Step::Mark { from, to, mark, .. } => {
                push(&mut json, "mark", mark.to_json());
                push(&mut json, "from", *from);
                push(&mut json, "to", *to);
            }
            Step::NodeMark { pos, mark, .. } => {
                push(&mut json, "pos", *pos);
                push(&mut json, "mark", mark.to_json());
            }
            Step::Attr { pos, attr, value } => {
                push(&mut json, "pos", *pos);
                push_attr(&mut json, attr, value);
            }
            Step::DocAttr { attr, value } => push_attr(&mut json, attr, value),
            Step::Custom(step) => return step.to_json(),
        }
        Value::Object(json)
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Step<'static>> {
        if !js::truthy(Some(json)) || !js::truthy(json.get("stepType")) {
            return Err(Error::Range("Invalid input for Step.fromJSON".into()));
        }
        let step_type = js::string(json.get("stepType"))?;
        let Some(&(_, class, kind)) = STEP_TYPES.iter().find(|(id, ..)| *id == step_type) else {
            return match custom_step(&step_type) {
                Some(from_json) => Ok(Step::Custom(from_json(schema, json)?)),
                None => Err(Error::Range(format!("No step type {step_type} defined"))),
            };
        };
        let invalid = || Error::Range(format!("Invalid input for {class}.fromJSON"));
        // JavaScript only checks that a position is a number: one that is negative, fractional
        // or NaN is taken, and so is a range that runs backwards, giving negative sizes. Here
        // each is invalid input, as a position that isn't a number is.
        let position = |key: &str| {
            json.get(key)
                .and_then(Value::as_f64)
                .filter(|n| *n >= 0.0 && n.fract() == 0.0)
                .map(|n| n as usize)
                .ok_or_else(invalid)
        };
        let forwards = |positions: &[usize]| match positions.is_sorted() {
            true => Ok(()),
            false => Err(invalid()),
        };
        let attr = || match json.get("attr") {
            Some(Value::String(attr)) => Ok(attr.clone()),
            _ => Err(invalid()),
        };
        let field = |key: &str| json.get(key).unwrap_or(&NULL);
        let structure = js::truthy(json.get("structure"));
        Ok(match kind {
            Kind::Replace => {
                let (from, to) = (position("from")?, position("to")?);
                forwards(&[from, to])?;
                Step::Replace {
                    from,
                    to,
                    slice: Slice::from_json(schema, field("slice"))?,
                    structure,
                }
            }
            Kind::ReplaceAround => {
                let (from, to) = (position("from")?, position("to")?);
                let (gap_from, gap_to) = (position("gapFrom")?, position("gapTo")?);
                let insert = position("insert")?;
                let slice = Slice::from_json(schema, field("slice"))?;
                forwards(&[from, gap_from, gap_to, to])?;
                forwards(&[insert, slice.size()])?;
                Step::ReplaceAround {
                    from,
                    to,
                    gap_from,
                    gap_to,
                    insert,
                    slice,
                    structure,
                }
            }
            Kind::Mark(op) => {
                let (from, to) = (position("from")?, position("to")?);
                forwards(&[from, to])?;
                Step::Mark {
                    op,
                    from,
                    to,
                    mark: Mark::from_json(schema, field("mark"))?,
                }
            }
            Kind::NodeMark(op) => Step::NodeMark {
                op,
                pos: position("pos")?,
                mark: Mark::from_json(schema, field("mark"))?,
            },
            Kind::Attr => Step::Attr {
                pos: position("pos")?,
                attr: attr()?,
                value: json.get("value").cloned(),
            },
            Kind::DocAttr => Step::DocAttr {
                attr: attr()?,
                value: json.get("value").cloned(),
            },
        })
    }
}
