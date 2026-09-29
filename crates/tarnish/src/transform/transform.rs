//! Transforms: a document, and the steps that changed it. The operations that build steps
//! for a transform are in the modules they share code with: `replace`, `structure` and `mark`.

use super::map::{Mappable, Mapping, MappingSlice};
use super::replace::replace_step;
use super::step::{Step, StepResult};
use crate::js::Class;
use crate::json::Value;
use crate::model::{Fragment, Node, Slice};
use crate::{Error, Result};

/// What a step that fails to apply throws.
pub static TRANSFORM_ERROR: Class = Class {
    name: "TransformError",
    message_alone: false,
};

/// A document and the steps that made it, from a starting document.
#[derive(Clone, Debug)]
pub struct Transform<'a> {
    doc: Node<'a>,
    steps: Vec<Step<'a>>,
    docs: Vec<Node<'a>>,
    mapping: Mapping,
}

impl<'a> Transform<'a> {
    pub fn new(doc: Node<'a>) -> Transform<'a> {
        Transform {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            mapping: Mapping::new(),
        }
    }

    /// The current document, with all the steps applied.
    pub fn doc(&self) -> &Node<'a> {
        &self.doc
    }

    pub fn steps(&self) -> &[Step<'a>] {
        &self.steps
    }

    /// The documents before each step.
    pub fn docs(&self) -> &[Node<'a>] {
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
    pub fn before(&self) -> &Node<'a> {
        self.docs.first().unwrap_or(&self.doc)
    }

    pub fn doc_changed(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Apply a step, raising a `TransformError` when it fails.
    pub fn step(&mut self, step: Step<'a>) -> Result<&mut Self> {
        if let StepResult::Failed(message) = self.maybe_step(step)? {
            return Err(Error::Of(&TRANSFORM_ERROR, message));
        }
        Ok(self)
    }

    /// Apply a step if it can apply, and give its result.
    pub fn maybe_step(&mut self, step: Step<'a>) -> Result<StepResult<'a>> {
        let result = step.apply(self.doc.clone())?;
        if let StepResult::Ok(doc) = &result {
            self.add_step(step, doc.clone());
        }
        Ok(result)
    }

    fn add_step(&mut self, step: Step<'a>, doc: Node<'a>) {
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
    pub fn replace(&mut self, from: usize, to: usize, slice: &Slice<'a>) -> Result<&mut Self> {
        if let Some(step) = replace_step(&self.doc, from, to, slice)? {
            self.step(step)?;
        }
        Ok(self)
    }

    pub fn replace_with(
        &mut self,
        from: usize,
        to: usize,
        content: Fragment<'a>,
    ) -> Result<&mut Self> {
        self.replace(from, to, &Slice::new(content, 0, 0))
    }

    pub fn delete(&mut self, from: usize, to: usize) -> Result<&mut Self> {
        self.replace(from, to, &Slice::empty())
    }

    pub fn insert(&mut self, pos: usize, content: Fragment<'a>) -> Result<&mut Self> {
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
}
