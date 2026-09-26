//! Steps: the atomic changes a transform is made of.

use super::map::{Mappable, StepMap};
use crate::error::{Error, Result};
use crate::js;
use crate::json::{Map, NULL, Value};
use crate::model::{Fragment, Mark, Node, Schema, Slice};

/// A step's outcome: the changed document, or why the step can't apply to the document.
#[derive(Clone, Debug)]
pub enum StepResult {
    Ok(Node),
    Failed(String),
}

impl StepResult {
    /// `doc.replace(from, to, slice)`, a slice that doesn't fit being a failure.
    pub fn from_replace(doc: &Node, from: usize, to: usize, slice: &Slice) -> Result<StepResult> {
        match doc.replace(from, to, slice) {
            Ok(doc) => Ok(StepResult::Ok(doc)),
            Err(Error::Replace(message)) => Ok(StepResult::Failed(message)),
            Err(error) => Err(error),
        }
    }

    pub fn doc(&self) -> Option<&Node> {
        match self {
            StepResult::Ok(doc) => Some(doc),
            StepResult::Failed(_) => None,
        }
    }

    pub fn failed(&self) -> Option<&str> {
        match self {
            StepResult::Ok(_) => None,
            StepResult::Failed(message) => Some(message),
        }
    }
}

/// A change to a document, which applies to the document it was made for.
#[derive(Clone, Debug)]
pub enum Step {
    /// Replace `from` to `to` with a slice. A `structure` step only replaces the closing and
    /// opening tokens between them, and fails where there is content.
    Replace {
        from: usize,
        to: usize,
        slice: Slice,
        structure: bool,
    },
    /// Replace `from` to `to` with a slice, keeping the content from `gap_from` to `gap_to` and
    /// putting it in the slice at `insert`.
    ReplaceAround {
        from: usize,
        to: usize,
        gap_from: usize,
        gap_to: usize,
        slice: Slice,
        insert: usize,
        structure: bool,
    },
    /// Add a mark to the inline content from `from` to `to`.
    AddMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
    RemoveMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
    /// Add a mark to the node at `pos`.
    AddNodeMark {
        pos: usize,
        mark: Mark,
    },
    RemoveNodeMark {
        pos: usize,
        mark: Mark,
    },
    /// Set an attribute of the node at `pos`. A `value` of `None` is `undefined`, which gives
    /// the attribute its default.
    Attr {
        pos: usize,
        attr: String,
        value: Option<Value>,
    },
    /// Set an attribute of the document's top node.
    DocAttr {
        attr: String,
        value: Option<Value>,
    },
}

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
            .maybe_child(resolved.index_after(depth))
            .cloned();
        while dist > 0 {
            match next {
                Some(node) if !node.is_leaf() => next = node.first_child().cloned(),
                _ => return Ok(true),
            }
            dist -= 1;
        }
    }
    Ok(false)
}

/// The fragment with `f` applied to its inline nodes, at any depth, each with its parent.
fn map_fragment(fragment: &Fragment, f: &dyn Fn(&Node, &Node) -> Node, parent: &Node) -> Fragment {
    let mapped = fragment
        .children()
        .iter()
        .map(|child| {
            let mut mapped = child.clone();
            if child.content().size() > 0 {
                mapped = mapped.copy(map_fragment(child.content(), f, child));
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

fn attrs_with(attrs: &Map, attr: &str, value: &Option<Value>) -> Map {
    let mut copy = attrs.clone();
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

impl Step {
    /// Apply the step to a document.
    pub fn apply(&self, doc: &Node) -> Result<StepResult> {
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                if *structure && content_between(doc, *from, *to)? {
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
                    && (content_between(doc, *from, *gap_from)?
                        || content_between(doc, *gap_to, *to)?)
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
            Step::AddMark { from, to, mark } => {
                let old = doc.slice(*from, *to, false)?;
                let resolved = doc.resolve(*from)?;
                let parent = resolved.node(resolved.shared_depth(*to));
                let add = |node: &Node, parent: &Node| {
                    if !node.is_atom() || !parent.node_type().allows_mark_type(mark.mark_type()) {
                        return node.clone();
                    }
                    node.mark(mark.add_to_set(node.marks()))
                };
                let content = map_fragment(old.content(), &add, parent);
                let slice = Slice::new(content, old.open_start(), old.open_end());
                StepResult::from_replace(doc, *from, *to, &slice)
            }
            Step::RemoveMark { from, to, mark } => {
                let old = doc.slice(*from, *to, false)?;
                let remove = |node: &Node, _: &Node| node.mark(mark.remove_from_set(node.marks()));
                let content = map_fragment(old.content(), &remove, doc);
                let slice = Slice::new(content, old.open_start(), old.open_end());
                StepResult::from_replace(doc, *from, *to, &slice)
            }
            Step::AddNodeMark { pos, mark } | Step::RemoveNodeMark { pos, mark } => {
                let Some(node) = doc.node_at(*pos)? else {
                    return Ok(StepResult::Failed("No node at mark step's position".into()));
                };
                let marks = match self {
                    Step::AddNodeMark { .. } => mark.add_to_set(node.marks()),
                    _ => mark.remove_from_set(node.marks()),
                };
                let updated =
                    node.node_type()
                        .create(Some(node.attrs()), Fragment::empty(), &marks)?;
                StepResult::from_replace(doc, *pos, *pos + 1, &node_slice(updated, node.is_leaf()))
            }
            Step::Attr { pos, attr, value } => {
                let Some(node) = doc.node_at(*pos)? else {
                    return Ok(StepResult::Failed(
                        "No node at attribute step's position".into(),
                    ));
                };
                let attrs = attrs_with(node.attrs(), attr, value);
                let updated =
                    node.node_type()
                        .create(Some(&attrs), Fragment::empty(), node.marks())?;
                StepResult::from_replace(doc, *pos, *pos + 1, &node_slice(updated, node.is_leaf()))
            }
            Step::DocAttr { attr, value } => {
                let attrs = attrs_with(doc.attrs(), attr, value);
                let updated =
                    doc.node_type()
                        .create(Some(&attrs), doc.content().clone(), doc.marks())?;
                Ok(StepResult::Ok(updated))
            }
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
                    slice.size().saturating_sub(*insert),
                ],
                false,
            ),
            _ => StepMap::empty(),
        }
    }

    /// The step that undoes this one, given the document before it.
    pub fn invert(&self, doc: &Node) -> Result<Step> {
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
            Step::AddMark { from, to, mark } => Step::RemoveMark {
                from: *from,
                to: *to,
                mark: mark.clone(),
            },
            Step::RemoveMark { from, to, mark } => Step::AddMark {
                from: *from,
                to: *to,
                mark: mark.clone(),
            },
            Step::AddNodeMark { pos, mark } => {
                if let Some(node) = doc.node_at(*pos)? {
                    let new_set = mark.add_to_set(node.marks());
                    if new_set.len() == node.marks().len() {
                        let replaced = node.marks().iter().find(|mark| !mark.is_in_set(&new_set));
                        return Ok(Step::AddNodeMark {
                            pos: *pos,
                            mark: replaced.unwrap_or(mark).clone(),
                        });
                    }
                }
                Step::RemoveNodeMark {
                    pos: *pos,
                    mark: mark.clone(),
                }
            }
            Step::RemoveNodeMark { pos, mark } => match doc.node_at(*pos)? {
                Some(node) if mark.is_in_set(node.marks()) => Step::AddNodeMark {
                    pos: *pos,
                    mark: mark.clone(),
                },
                _ => self.clone(),
            },
            Step::Attr { pos, attr, .. } => {
                let node = doc.node_at(*pos)?.ok_or_else(|| {
                    Error::Other(format!("No node at {pos} to invert an attribute step on"))
                })?;
                Step::Attr {
                    pos: *pos,
                    attr: attr.clone(),
                    value: node.attrs().get(attr).cloned(),
                }
            }
            Step::DocAttr { attr, .. } => Step::DocAttr {
                attr: attr.clone(),
                value: doc.attrs().get(attr).cloned(),
            },
        })
    }

    /// The step with its positions mapped, or `None` when the mapping deleted what it changes.
    pub fn map(&self, mapping: &dyn Mappable) -> Option<Step> {
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                let to = mapping.map_result(*to, -1);
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
            Step::AddMark { from, to, mark } | Step::RemoveMark { from, to, mark } => {
                let from = mapping.map_result(*from, 1);
                let to = mapping.map_result(*to, -1);
                if (from.deleted() && to.deleted()) || from.pos >= to.pos {
                    return None;
                }
                let (from, to, mark) = (from.pos, to.pos, mark.clone());
                Some(match self {
                    Step::AddMark { .. } => Step::AddMark { from, to, mark },
                    _ => Step::RemoveMark { from, to, mark },
                })
            }
            Step::AddNodeMark { pos, mark } | Step::RemoveNodeMark { pos, mark } => {
                let mapped = mapping.map_result(*pos, 1);
                if mapped.deleted_after() {
                    return None;
                }
                let (pos, mark) = (mapped.pos, mark.clone());
                Some(match self {
                    Step::AddNodeMark { .. } => Step::AddNodeMark { pos, mark },
                    _ => Step::RemoveNodeMark { pos, mark },
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
        }
    }

    /// The step and `other`, applied after it, as one step, if they can be.
    pub fn merge(&self, other: &Step) -> Option<Step> {
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
                let joined = |first: &Slice, second: &Slice| {
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
                Step::AddMark { from, to, mark },
                Step::AddMark {
                    from: other_from,
                    to: other_to,
                    mark: other_mark,
                },
            ) if other_mark == mark && from <= other_to && to >= other_from => {
                Some(Step::AddMark {
                    from: *from.min(other_from),
                    to: *to.max(other_to),
                    mark: mark.clone(),
                })
            }
            (
                Step::RemoveMark { from, to, mark },
                Step::RemoveMark {
                    from: other_from,
                    to: other_to,
                    mark: other_mark,
                },
            ) if other_mark == mark && from <= other_to && to >= other_from => {
                Some(Step::RemoveMark {
                    from: *from.min(other_from),
                    to: *to.max(other_to),
                    mark: mark.clone(),
                })
            }
            _ => None,
        }
    }

    /// The step's JSON identifier, its `stepType`.
    pub fn json_id(&self) -> &'static str {
        match self {
            Step::Replace { .. } => "replace",
            Step::ReplaceAround { .. } => "replaceAround",
            Step::AddMark { .. } => "addMark",
            Step::RemoveMark { .. } => "removeMark",
            Step::AddNodeMark { .. } => "addNodeMark",
            Step::RemoveNodeMark { .. } => "removeNodeMark",
            Step::Attr { .. } => "attr",
            Step::DocAttr { .. } => "docAttr",
        }
    }

    pub fn to_json(&self) -> Value {
        let mut json = Map::new();
        json.push("stepType".into(), Value::String(self.json_id().into()));
        let number = Value::from;
        match self {
            Step::Replace {
                from,
                to,
                slice,
                structure,
            } => {
                json.push("from".into(), number(*from));
                json.push("to".into(), number(*to));
                if slice.size() > 0 {
                    json.push("slice".into(), slice.to_json());
                }
                if *structure {
                    json.push("structure".into(), Value::Bool(true));
                }
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
                json.push("from".into(), number(*from));
                json.push("to".into(), number(*to));
                json.push("gapFrom".into(), number(*gap_from));
                json.push("gapTo".into(), number(*gap_to));
                json.push("insert".into(), number(*insert));
                if slice.size() > 0 {
                    json.push("slice".into(), slice.to_json());
                }
                if *structure {
                    json.push("structure".into(), Value::Bool(true));
                }
            }
            Step::AddMark { from, to, mark } | Step::RemoveMark { from, to, mark } => {
                json.push("mark".into(), mark.to_json());
                json.push("from".into(), number(*from));
                json.push("to".into(), number(*to));
            }
            Step::AddNodeMark { pos, mark } | Step::RemoveNodeMark { pos, mark } => {
                json.push("pos".into(), number(*pos));
                json.push("mark".into(), mark.to_json());
            }
            Step::Attr { pos, attr, value } => {
                json.push("pos".into(), number(*pos));
                json.push("attr".into(), Value::String(attr.clone()));
                if let Some(value) = value {
                    json.push("value".into(), value.clone());
                }
            }
            Step::DocAttr { attr, value } => {
                json.push("attr".into(), Value::String(attr.clone()));
                if let Some(value) = value {
                    json.push("value".into(), value.clone());
                }
            }
        }
        Value::Object(json)
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Step> {
        if !js::truthy(Some(json)) || !js::truthy(json.get("stepType")) {
            return Err(Error::Range("Invalid input for Step.fromJSON".into()));
        }
        let step_type = js::string(json.get("stepType"));
        let invalid = |class: &str| Error::Range(format!("Invalid input for {class}.fromJSON"));
        let position = |key: &str, class: &str| {
            json.get(key)
                .and_then(Value::as_f64)
                .filter(|n| *n >= 0.0 && n.fract() == 0.0)
                .map(|n| n as usize)
                .ok_or_else(|| invalid(class))
        };
        let attr = |class: &str| match json.get("attr") {
            Some(Value::String(attr)) => Ok(attr.clone()),
            _ => Err(invalid(class)),
        };
        let field = |key: &str| json.get(key).unwrap_or(&NULL);
        let structure = js::truthy(json.get("structure"));
        Ok(match &*step_type {
            "replace" => {
                let (from, to) = (
                    position("from", "ReplaceStep")?,
                    position("to", "ReplaceStep")?,
                );
                Step::Replace {
                    from,
                    to,
                    slice: Slice::from_json(schema, field("slice"))?,
                    structure,
                }
            }
            "replaceAround" => {
                let class = "ReplaceAroundStep";
                let (from, to) = (position("from", class)?, position("to", class)?);
                let (gap_from, gap_to) = (position("gapFrom", class)?, position("gapTo", class)?);
                let insert = position("insert", class)?;
                Step::ReplaceAround {
                    from,
                    to,
                    gap_from,
                    gap_to,
                    slice: Slice::from_json(schema, field("slice"))?,
                    insert,
                    structure,
                }
            }
            "addMark" | "removeMark" => {
                let class = if step_type == "addMark" {
                    "AddMarkStep"
                } else {
                    "RemoveMarkStep"
                };
                let (from, to) = (position("from", class)?, position("to", class)?);
                let mark = Mark::from_json(schema, field("mark"))?;
                if step_type == "addMark" {
                    Step::AddMark { from, to, mark }
                } else {
                    Step::RemoveMark { from, to, mark }
                }
            }
            "addNodeMark" | "removeNodeMark" => {
                let class = if step_type == "addNodeMark" {
                    "AddNodeMarkStep"
                } else {
                    "RemoveNodeMarkStep"
                };
                let pos = position("pos", class)?;
                let mark = Mark::from_json(schema, field("mark"))?;
                if step_type == "addNodeMark" {
                    Step::AddNodeMark { pos, mark }
                } else {
                    Step::RemoveNodeMark { pos, mark }
                }
            }
            "attr" => {
                let pos = position("pos", "AttrStep")?;
                Step::Attr {
                    pos,
                    attr: attr("AttrStep")?,
                    value: json.get("value").cloned(),
                }
            }
            "docAttr" => Step::DocAttr {
                attr: attr("DocAttrStep")?,
                value: json.get("value").cloned(),
            },
            _ => return Err(Error::Range(format!("No step type {step_type} defined"))),
        })
    }
}
