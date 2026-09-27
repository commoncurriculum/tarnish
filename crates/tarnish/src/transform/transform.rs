//! Transforms: a document, and the steps that changed it. The operations that build steps
//! for a transform are in the modules they share code with: `replace`, `structure` and `mark`.

use super::map::{Mappable, Mapping, MappingSlice};
use super::mark::MarkMatch;
use super::replace::replace_step;
use super::step::{MarkOp, Step, StepResult};
use crate::error::{Error, Result};
use crate::json::Value;
use crate::model::{Fragment, Mark, Node, Slice};

/// A document and the steps that made it, from a starting document.
#[derive(Clone, Debug)]
pub struct Transform {
    doc: Node,
    steps: Vec<Step>,
    docs: Vec<Node>,
    mapping: Mapping,
}

impl Transform {
    pub fn new(doc: Node) -> Transform {
        Transform {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            mapping: Mapping::new(),
        }
    }

    /// The current document, with all the steps applied.
    pub fn doc(&self) -> &Node {
        &self.doc
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The documents before each step.
    pub fn docs(&self) -> &[Node] {
        &self.docs
    }

    /// The maps of the steps.
    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    pub fn mapping_mut(&mut self) -> &mut Mapping {
        &mut self.mapping
    }

    /// The mapping of the steps from `from` on.
    pub(crate) fn mapping_from(&self, from: usize) -> MappingSlice<'_> {
        self.mapping.slice(from, self.mapping.maps().len())
    }

    /// The starting document.
    pub fn before(&self) -> &Node {
        self.docs.first().unwrap_or(&self.doc)
    }

    pub fn doc_changed(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Apply a step, raising a `TransformError` when it fails.
    pub fn step(&mut self, step: Step) -> Result<&mut Self> {
        if let StepResult::Failed(message) = self.maybe_step(step)? {
            return Err(Error::Transform(message));
        }
        Ok(self)
    }

    /// Apply a step if it can apply, and give its result.
    pub fn maybe_step(&mut self, step: Step) -> Result<StepResult> {
        let result = step.apply(self.doc.clone())?;
        if let StepResult::Ok(doc) = &result {
            self.add_step(step, doc.clone());
        }
        Ok(result)
    }

    fn add_step(&mut self, step: Step, doc: Node) {
        self.docs.push(std::mem::replace(&mut self.doc, doc));
        self.mapping.append_map(step.get_map(), None);
        self.steps.push(step);
    }

    /// The range, in the current document, that covers everything the steps replaced. `None`
    /// when they replaced nothing: marks added or removed don't count.
    pub fn changed_range(&self) -> Option<(usize, usize)> {
        let mut range: Option<(usize, usize)> = None;
        for map in self.mapping.maps() {
            if let Some((from, to)) = &mut range {
                *from = map.map(*from, 1);
                *to = map.map(*to, -1);
            }
            for (_, _, new_from, new_to) in map.changes() {
                let (from, to) = range.get_or_insert((new_from, new_to));
                *from = (*from).min(new_from);
                *to = (*to).max(new_to);
            }
        }
        range
    }

    /// Replace `from` to `to` with a slice, fitting it in as it can.
    pub fn replace(&mut self, from: usize, to: usize, slice: &Slice) -> Result<&mut Self> {
        if let Some(step) = replace_step(&self.doc, from, to, slice)? {
            self.step(step)?;
        }
        Ok(self)
    }

    pub fn replace_with(&mut self, from: usize, to: usize, content: Fragment) -> Result<&mut Self> {
        self.replace(from, to, &Slice::new(content, 0, 0))
    }

    pub fn delete(&mut self, from: usize, to: usize) -> Result<&mut Self> {
        self.replace(from, to, &Slice::empty())
    }

    pub fn insert(&mut self, pos: usize, content: Fragment) -> Result<&mut Self> {
        self.replace_with(pos, pos, content)
    }

    /// Sets an attribute of the node at `pos`, `None` being `undefined`.
    pub fn set_node_attribute(
        &mut self,
        pos: usize,
        attr: &str,
        value: Option<Value>,
    ) -> Result<&mut Self> {
        self.step(Step::Attr {
            pos,
            attr: attr.to_owned(),
            value,
        })
    }

    pub fn set_doc_attribute(&mut self, attr: &str, value: Option<Value>) -> Result<&mut Self> {
        self.step(Step::DocAttr {
            attr: attr.to_owned(),
            value,
        })
    }

    pub fn add_node_mark(&mut self, pos: usize, mark: Mark) -> Result<&mut Self> {
        self.step(Step::NodeMark {
            op: MarkOp::Add,
            pos,
            mark,
        })
    }

    /// Remove the mark, or all marks of the type, from the node at `pos`.
    pub fn remove_node_mark(&mut self, pos: usize, mark: MarkMatch) -> Result<&mut Self> {
        let node = self
            .doc
            .node_at(pos)?
            .cloned()
            .ok_or_else(|| Error::Range(format!("No node at position {pos}")))?;
        match mark {
            MarkMatch::Mark(mark) => {
                if mark.is_in_set(node.marks()) {
                    self.step(Step::NodeMark {
                        op: MarkOp::Remove,
                        pos,
                        mark: mark.clone(),
                    })?;
                }
            }
            MarkMatch::Type(mark_type) => {
                let found = node
                    .marks()
                    .iter()
                    .filter(|mark| mark.mark_type() == mark_type);
                for mark in found.rev() {
                    self.step(Step::NodeMark {
                        op: MarkOp::Remove,
                        pos,
                        mark: mark.clone(),
                    })?;
                }
            }
        }
        Ok(self)
    }
}
