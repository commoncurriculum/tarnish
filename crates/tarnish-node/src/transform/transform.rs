//! Transforms.

use std::cell::RefCell;
use std::rc::Rc;

use napi::bindgen_prelude::{Either, Unknown};
use napi::{Env, Result, ValueType};
use napi_derive::napi;
use tarnish::transform::{BlockAttrs, Transform, Wrapper};
use tarnish::{Attrs, Node};

use super::map::{self, MappingSource};
use super::step::{self, StepHandle};
use super::structure::{self, WrapperArg};
use crate::content::ContentMatchHandle;
use crate::fragment::FragmentHandle;
use crate::js::{self, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::node::{self, NodeHandle};
use crate::position::NodeRangeHandle;
use crate::schema::{MarkTypeHandle, NodeTypeHandle};
use crate::slice::SliceHandle;

#[napi(object)]
pub struct ChangedRange {
    pub from: u32,
    pub to: u32,
}

#[napi]
pub struct TransformHandle {
    tr: Rc<RefCell<Transform>>,
}

impl TransformHandle {
    /// Run an operation on the transform; its errors are thrown as ProseMirror throws them.
    fn run(
        &self,
        env: &Env,
        f: impl FnOnce(&mut Transform) -> tarnish::Result<&mut Transform>,
    ) -> Result<()> {
        let result = f(&mut self.tr.borrow_mut()).map(|_| ());
        result.or_throw(env)
    }
}

#[napi]
impl TransformHandle {
    #[napi(constructor)]
    pub fn new(doc: &NodeHandle) -> Self {
        TransformHandle {
            tr: Rc::new(RefCell::new(Transform::new(doc.node.clone()))),
        }
    }

    #[napi]
    pub fn doc<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        let doc = self.tr.borrow().doc().clone();
        node::wrap(env, &doc)
    }

    #[napi]
    pub fn before<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        let before = self.tr.borrow().before().clone();
        node::wrap(env, &before)
    }

    /// The steps from index `start` on.
    #[napi]
    pub fn steps_from<'env>(&self, env: &'env Env, start: u32) -> Result<Vec<Unknown<'env>>> {
        let steps = self.tr.borrow().steps()[start as usize..].to_vec();
        steps.iter().map(|step| step::wrap(env, step)).collect()
    }

    /// The documents before the steps from index `start` on.
    #[napi]
    pub fn docs_from<'env>(&self, env: &'env Env, start: u32) -> Result<Vec<Unknown<'env>>> {
        let docs = self.tr.borrow().docs()[start as usize..].to_vec();
        docs.iter().map(|doc| node::wrap(env, doc)).collect()
    }

    #[napi(getter)]
    pub fn step_count(&self) -> u32 {
        self.tr.borrow().steps().len() as u32
    }

    /// The transform's mapping, which grows as the transform does.
    #[napi]
    pub fn mapping<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        map::wrap_mapping(env, MappingSource::Transform(self.tr.clone()))
    }

    #[napi(getter)]
    pub fn doc_changed(&self) -> bool {
        self.tr.borrow().doc_changed()
    }

    #[napi]
    pub fn step(&self, env: &Env, step: &StepHandle) -> Result<()> {
        self.run(env, |tr| tr.step(step.step.clone()))
    }

    #[napi]
    pub fn maybe_step<'env>(&self, env: &'env Env, step: &StepHandle) -> Result<Unknown<'env>> {
        let result = self
            .tr
            .borrow_mut()
            .maybe_step(step.step.clone())
            .or_throw(env)?;
        step::wrap_result(env, &result)
    }

    #[napi]
    pub fn changed_range(&self) -> Option<ChangedRange> {
        let (from, to) = self.tr.borrow().changed_range()?;
        Some(ChangedRange {
            from: from as u32,
            to: to as u32,
        })
    }

    #[napi]
    pub fn replace(&self, env: &Env, from: f64, to: f64, slice: &SliceHandle) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.replace(from, to, &slice.slice))
    }

    #[napi]
    pub fn replace_with(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        content: &FragmentHandle,
    ) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| {
            tr.replace_with(from, to, content.fragment.clone())
        })
    }

    #[napi]
    pub fn delete(&self, env: &Env, from: f64, to: f64) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.delete(from, to))
    }

    #[napi]
    pub fn insert(&self, env: &Env, pos: f64, content: &FragmentHandle) -> Result<()> {
        let pos = js::pos(env, pos)?;
        self.run(env, |tr| tr.insert(pos, content.fragment.clone()))
    }

    #[napi]
    pub fn replace_range(&self, env: &Env, from: f64, to: f64, slice: &SliceHandle) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.replace_range(from, to, &slice.slice))
    }

    #[napi]
    pub fn replace_range_with(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        node: &NodeHandle,
    ) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.replace_range_with(from, to, node.node.clone()))
    }

    #[napi]
    pub fn delete_range(&self, env: &Env, from: f64, to: f64) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.delete_range(from, to))
    }

    #[napi]
    pub fn lift(&self, env: &Env, range: &NodeRangeHandle, target: u32) -> Result<()> {
        self.run(env, |tr| tr.lift(&range.range, target as usize))
    }

    #[napi]
    pub fn join(&self, env: &Env, pos: f64, depth: u32) -> Result<()> {
        let pos = js::pos(env, pos)?;
        self.run(env, |tr| tr.join(pos, depth as usize))
    }

    #[napi]
    pub fn wrap(
        &self,
        env: &Env,
        range: &NodeRangeHandle,
        wrappers: Vec<Option<WrapperArg>>,
    ) -> Result<()> {
        let wrappers: Vec<Wrapper> = structure::wrappers(wrappers)?
            .into_iter()
            .map(|wrapper| {
                wrapper.ok_or_else(|| napi::Error::from_reason("A wrapper with no type"))
            })
            .collect::<Result<_>>()?;
        self.run(env, |tr| tr.wrap(&range.range, &wrappers))
    }

    /// `attrs` is the attributes, or a function that gives them for each node.
    #[napi]
    pub fn set_block_type(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        node_type: &NodeTypeHandle,
        attrs: Unknown,
    ) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        let node_type = &node_type.node_type;
        if attrs.get_type()? == ValueType::Function {
            let mut attrs_of = |node: &Node| -> tarnish::Result<Option<Attrs>> {
                js::host(|env| js::attrs_from_js(js::call(attrs, node::wrap(env, node)?)?))
            };
            let attrs = BlockAttrs::Hook(&mut attrs_of);
            return self.run(env, |tr| tr.set_block_type(from, to, node_type, attrs));
        }
        let attrs = js::attrs_from_js(attrs)?;
        let attrs = BlockAttrs::Fixed(attrs.as_deref());
        self.run(env, |tr| tr.set_block_type(from, to, node_type, attrs))
    }

    #[napi]
    pub fn set_node_markup(
        &self,
        env: &Env,
        pos: f64,
        node_type: Option<&NodeTypeHandle>,
        attrs: Unknown,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<()> {
        let pos = js::pos(env, pos)?;
        let node_type = node_type.map(|node_type| &node_type.node_type);
        let attrs = js::attrs_from_js(attrs)?;
        let marks = marks.map(mark::list);
        self.run(env, |tr| {
            tr.set_node_markup(pos, node_type, attrs.as_deref(), marks.as_deref())
        })
    }

    #[napi]
    pub fn set_node_attribute(
        &self,
        env: &Env,
        pos: f64,
        attr: String,
        value: Unknown,
    ) -> Result<()> {
        let pos = js::pos(env, pos)?;
        let value = js::value_from_js(value)?;
        self.run(env, |tr| tr.set_node_attribute(pos, &attr, value))
    }

    #[napi]
    pub fn set_doc_attribute(&self, env: &Env, attr: String, value: Unknown) -> Result<()> {
        let value = js::value_from_js(value)?;
        self.run(env, |tr| tr.set_doc_attribute(&attr, value))
    }

    #[napi]
    pub fn add_node_mark(&self, env: &Env, pos: f64, mark: &MarkHandle) -> Result<()> {
        let pos = js::pos(env, pos)?;
        self.run(env, |tr| tr.add_node_mark(pos, mark.mark.clone()))
    }

    #[napi]
    pub fn remove_node_mark(
        &self,
        env: &Env,
        pos: f64,
        mark: Either<&MarkHandle, &MarkTypeHandle>,
    ) -> Result<()> {
        let pos = js::pos(env, pos)?;
        self.run(env, |tr| {
            tr.remove_node_mark(pos, structure::mark_match(&mark))
        })
    }

    #[napi]
    pub fn split(
        &self,
        env: &Env,
        pos: f64,
        depth: u32,
        types_after: Option<Vec<Option<WrapperArg>>>,
    ) -> Result<()> {
        let pos = js::pos(env, pos)?;
        let types = structure::wrappers(types_after.unwrap_or_default())?;
        self.run(env, |tr| tr.split(pos, depth as usize, &types))
    }

    #[napi]
    pub fn add_mark(&self, env: &Env, from: f64, to: f64, mark: &MarkHandle) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        self.run(env, |tr| tr.add_mark(from, to, &mark.mark))
    }

    #[napi]
    pub fn remove_mark(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        mark: Option<Either<&MarkHandle, &MarkTypeHandle>>,
    ) -> Result<()> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        let mark = mark.as_ref().map(structure::mark_match);
        self.run(env, |tr| tr.remove_mark(from, to, mark))
    }

    #[napi]
    pub fn clear_incompatible(
        &self,
        env: &Env,
        pos: f64,
        parent_type: &NodeTypeHandle,
        start: Option<&ContentMatchHandle>,
    ) -> Result<()> {
        let pos = js::pos(env, pos)?;
        let start = start.map(|start| start.content_match.clone());
        self.run(env, |tr| {
            tr.clear_incompatible(pos, &parent_type.node_type, start)
        })
    }
}
