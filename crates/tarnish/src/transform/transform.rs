//! Transforms: a document, and the steps that changed it.

use super::map::{Mappable, Mapping, MappingSlice};
use super::step::{Step, StepResult};
use super::structure::{self, Wrapper};
use super::{mark, replace};
use crate::error::{Error, Result};
use crate::json::{Map, Value};
use crate::model::{
    Attrs, ContentMatch, Fragment, Mark, MarkType, Node, NodeRange, NodeType, Slice,
};

/// A mark, or all marks of a type.
#[derive(Clone, Copy)]
pub enum MarkMatch<'a> {
    Mark(&'a Mark),
    Type(&'a MarkType),
}

/// The attributes `set_block_type` gives each textblock: the same for all, or from a function
/// of the old block.
pub enum BlockAttrs<'a> {
    Fixed(Option<&'a Map>),
    Hook(&'a mut dyn FnMut(&Node) -> Result<Option<Attrs>>),
}

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
        let result = step.apply(&self.doc)?;
        if let StepResult::Ok(doc) = &result {
            self.add_step(step, doc.clone());
        }
        Ok(result)
    }

    pub(crate) fn add_step(&mut self, step: Step, doc: Node) {
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
        if let Some(step) = replace::replace_step(&self.doc, from, to, slice)? {
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

    /// Replace a range with a slice, taking `from`, `to` and the slice's open start as hints
    /// rather than fixed points, as for a paste.
    pub fn replace_range(&mut self, from: usize, to: usize, slice: &Slice) -> Result<&mut Self> {
        replace::replace_range(self, from, to, slice)?;
        Ok(self)
    }

    /// Replace a range with a node, moving the range out of a parent where the node doesn't fit.
    pub fn replace_range_with(&mut self, from: usize, to: usize, node: Node) -> Result<&mut Self> {
        replace::replace_range_with(self, from, to, node)?;
        Ok(self)
    }

    /// Delete a range, growing it over whole parents until the deletion is valid.
    pub fn delete_range(&mut self, from: usize, to: usize) -> Result<&mut Self> {
        replace::delete_range(self, from, to)?;
        Ok(self)
    }

    /// Lift the range's content out of its parent to `target` depth.
    pub fn lift(&mut self, range: &NodeRange, target: usize) -> Result<&mut Self> {
        structure::lift(self, range, target)?;
        Ok(self)
    }

    /// Join the blocks around `pos`, and their last and first descendants down `depth` levels.
    pub fn join(&mut self, pos: usize, depth: usize) -> Result<&mut Self> {
        structure::join(self, pos, depth)?;
        Ok(self)
    }

    /// Wrap the range in these nodes, outermost first.
    pub fn wrap(&mut self, range: &NodeRange, wrappers: &[Wrapper]) -> Result<&mut Self> {
        structure::wrap(self, range, wrappers)?;
        Ok(self)
    }

    /// Give the textblocks between `from` and `to` this type, and these attributes.
    pub fn set_block_type(
        &mut self,
        from: usize,
        to: usize,
        node_type: &NodeType,
        attrs: BlockAttrs,
    ) -> Result<&mut Self> {
        structure::set_block_type(self, from, to, node_type, attrs)?;
        Ok(self)
    }

    /// Change the type, attributes or marks of the node at `pos`, keeping its type when none
    /// is given and its marks when none are.
    pub fn set_node_markup(
        &mut self,
        pos: usize,
        node_type: Option<&NodeType>,
        attrs: Option<&Map>,
        marks: Option<&[Mark]>,
    ) -> Result<&mut Self> {
        structure::set_node_markup(self, pos, node_type, attrs, marks)?;
        Ok(self)
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
        self.step(Step::AddNodeMark { pos, mark })
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
                    self.step(Step::RemoveNodeMark {
                        pos,
                        mark: mark.clone(),
                    })?;
                }
            }
            MarkMatch::Type(mark_type) => {
                let mut set = node.marks().clone();
                let mut steps = Vec::new();
                while let Some(found) = mark_type.is_in_set(&set).cloned() {
                    set = found.remove_from_set(&set);
                    steps.push(Step::RemoveNodeMark { pos, mark: found });
                }
                for step in steps.into_iter().rev() {
                    self.step(step)?;
                }
            }
        }
        Ok(self)
    }

    /// Split the node at `pos`, and `depth - 1` of its ancestors. Each part split off gets the
    /// matching type of `types_after`, outermost first, when it has one, or its original's.
    pub fn split(
        &mut self,
        pos: usize,
        depth: usize,
        types_after: Option<&[Option<Wrapper>]>,
    ) -> Result<&mut Self> {
        structure::split(self, pos, depth, types_after)?;
        Ok(self)
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: &Mark) -> Result<&mut Self> {
        mark::add_mark(self, from, to, mark)?;
        Ok(self)
    }

    /// Remove the mark, the marks of the type, or with `None` all marks, from the inline
    /// content between `from` and `to`.
    pub fn remove_mark(
        &mut self,
        from: usize,
        to: usize,
        mark: Option<MarkMatch>,
    ) -> Result<&mut Self> {
        mark::remove_mark(self, from, to, mark)?;
        Ok(self)
    }

    /// Remove the content and marks of the node at `pos` that a node of `parent_type` wouldn't
    /// allow, matching from `start` when given.
    pub fn clear_incompatible(
        &mut self,
        pos: usize,
        parent_type: &NodeType,
        start: Option<ContentMatch>,
    ) -> Result<&mut Self> {
        mark::clear_incompatible(self, pos, parent_type, start, true)?;
        Ok(self)
    }

    /// The mapping of the steps from `from` on.
    pub(crate) fn mapping_from(&self, from: usize) -> MappingSlice<'_> {
        self.mapping.slice(from, self.mapping.maps().len())
    }
}
