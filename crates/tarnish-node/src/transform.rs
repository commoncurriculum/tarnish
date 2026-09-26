//! prosemirror-transform: step maps, mappings, steps and transforms.

use std::cell::RefCell;
use std::ptr;
use std::rc::Rc;

use napi::bindgen_prelude::{ClassInstance, FromNapiValue, ToNapiValue};
use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::transform::{
    BlockAttrs, MapResult, Mappable, Mapping, MarkMatch, Step, StepMap, StepResult, Transform,
    Wrapper,
};
use tarnish::{Attrs, Mark, MarkType, NodeRange, Value};

use crate::content::ContentMatchArg;
use crate::fragment::FragmentArg;
use crate::js::{self, Data, Js, OrThrow};
use crate::mark::MarkArg;
use crate::node::{self, NodeArg};
use crate::position::ResolvedPosArg;
use crate::schema::{MarkTypeArg, NodeTypeArg, SchemaHandle};
use crate::slice::{self, SliceArg};

fn number(env: sys::napi_env, value: usize) -> Result<sys::napi_value> {
    js::number(env, value as f64)
}

fn array(env: sys::napi_env, items: Vec<sys::napi_value>) -> Result<sys::napi_value> {
    let mut result = ptr::null_mut();
    js::check(unsafe { sys::napi_create_array_with_length(env, items.len(), &mut result) })?;
    for (index, item) in items.into_iter().enumerate() {
        js::check(unsafe { sys::napi_set_element(env, result, index as u32, item) })?;
    }
    Ok(result)
}

fn usize_of(env: sys::napi_env, value: sys::napi_value) -> Result<usize> {
    Ok(unsafe { f64::from_napi_value(env, value) }? as usize)
}

fn field(env: sys::napi_env, object: sys::napi_value, key: &str) -> Result<usize> {
    usize_of(env, js::get(env, object, key)?)
}

fn wrap_step_map(env: sys::napi_env, map: &StepMap) -> Result<sys::napi_value> {
    let handle = unsafe { StepMapHandle::to_napi_value(env, StepMapHandle { map: map.clone() }) }?;
    js::call_registered(env, "wrapStepMap", &[handle])
}

fn wrap_map_result(env: sys::napi_env, result: MapResult) -> Result<sys::napi_value> {
    let mut flags = Vec::with_capacity(4);
    for flag in [
        result.deleted(),
        result.deleted_before(),
        result.deleted_after(),
        result.deleted_across(),
    ] {
        let mut value = ptr::null_mut();
        js::check(unsafe { sys::napi_get_boolean(env, flag, &mut value) })?;
        flags.push(value);
    }
    let recover = match result.recover {
        Some(recover) => js::number(env, recover.to_number())?,
        None => js::null(env)?,
    };
    let mut args = vec![
        number(env, result.pos)?,
        number(env, result.del_info as usize)?,
        recover,
    ];
    args.extend(flags);
    js::call_registered(env, "wrapMapResult", &args)
}

/// A step as JavaScript's class of it.
pub fn wrap_step(env: sys::napi_env, step: &Step) -> Result<sys::napi_value> {
    let mut args = vec![js::string(env, step.json_id())?];
    let boolean = |value: bool| -> Result<sys::napi_value> {
        let mut result = ptr::null_mut();
        js::check(unsafe { sys::napi_get_boolean(env, value, &mut result) })?;
        Ok(result)
    };
    match step {
        Step::Replace {
            from,
            to,
            slice,
            structure,
        } => args.extend([
            number(env, *from)?,
            number(env, *to)?,
            slice::wrap(env, slice)?,
            boolean(*structure)?,
        ]),
        Step::ReplaceAround {
            from,
            to,
            gap_from,
            gap_to,
            slice,
            insert,
            structure,
        } => args.extend([
            number(env, *from)?,
            number(env, *to)?,
            number(env, *gap_from)?,
            number(env, *gap_to)?,
            slice::wrap(env, slice)?,
            number(env, *insert)?,
            boolean(*structure)?,
        ]),
        Step::AddMark { from, to, mark } | Step::RemoveMark { from, to, mark } => args.extend([
            number(env, *from)?,
            number(env, *to)?,
            crate::mark::wrap(env, mark)?,
        ]),
        Step::AddNodeMark { pos, mark } | Step::RemoveNodeMark { pos, mark } => {
            args.extend([number(env, *pos)?, crate::mark::wrap(env, mark)?])
        }
        Step::Attr { pos, attr, value } => args.extend([
            number(env, *pos)?,
            js::string(env, attr)?,
            js::value_to_js(env, value)?,
        ]),
        Step::DocAttr { attr, value } => {
            args.extend([js::string(env, attr)?, js::value_to_js(env, value)?])
        }
    }
    js::call_registered(env, "makeStep", &args)
}

fn wrap_step_option(env: sys::napi_env, step: Option<Step>) -> Result<Js> {
    match step {
        Some(step) => wrap_step(env, &step).map(Js),
        None => js::null(env).map(Js),
    }
}

fn wrap_step_result(env: sys::napi_env, result: &StepResult) -> Result<sys::napi_value> {
    let (doc, failed) = match result {
        StepResult::Ok(doc) => (node::wrap(env, doc)?, js::null(env)?),
        StepResult::Failed(message) => (js::null(env)?, js::string(env, message)?),
    };
    js::call_registered(env, "makeStepResult", &[doc, failed])
}

/// One of JavaScript's steps, read by its class's JSON id.
pub struct StepArg(pub Step);

impl FromNapiValue for StepArg {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        let json_id = js::get_string(env, value, "jsonID")?.unwrap_or_default();
        let mark = |key: &str| -> Result<Mark> {
            Ok(unsafe { MarkArg::from_napi_value(env, js::get(env, value, key)?) }?.0)
        };
        let slice = || -> Result<tarnish::Slice> {
            Ok(unsafe { SliceArg::from_napi_value(env, js::get(env, value, "slice")?) }?.0)
        };
        let structure = || js::truthy(env, js::get(env, value, "structure")?);
        let attr = || -> Result<String> {
            unsafe { String::from_napi_value(env, js::get(env, value, "attr")?) }
        };
        let attr_value = || js::value_from_js(env, js::get(env, value, "value")?);
        Ok(StepArg(match json_id.as_str() {
            "replace" => Step::Replace {
                from: field(env, value, "from")?,
                to: field(env, value, "to")?,
                slice: slice()?,
                structure: structure()?,
            },
            "replaceAround" => Step::ReplaceAround {
                from: field(env, value, "from")?,
                to: field(env, value, "to")?,
                gap_from: field(env, value, "gapFrom")?,
                gap_to: field(env, value, "gapTo")?,
                slice: slice()?,
                insert: field(env, value, "insert")?,
                structure: structure()?,
            },
            "addMark" => Step::AddMark {
                from: field(env, value, "from")?,
                to: field(env, value, "to")?,
                mark: mark("mark")?,
            },
            "removeMark" => Step::RemoveMark {
                from: field(env, value, "from")?,
                to: field(env, value, "to")?,
                mark: mark("mark")?,
            },
            "addNodeMark" => Step::AddNodeMark {
                pos: field(env, value, "pos")?,
                mark: mark("mark")?,
            },
            "removeNodeMark" => Step::RemoveNodeMark {
                pos: field(env, value, "pos")?,
                mark: mark("mark")?,
            },
            "attr" => Step::Attr {
                pos: field(env, value, "pos")?,
                attr: attr()?,
                value: attr_value()?,
            },
            "docAttr" => Step::DocAttr {
                attr: attr()?,
                value: attr_value()?,
            },
            other => {
                return Err(napi::Error::from_reason(format!(
                    "tarnish has no step type {other:?}; it applies ProseMirror's own steps"
                )));
            }
        }))
    }
}

impl napi::bindgen_prelude::TypeName for StepArg {
    fn type_name() -> &'static str {
        "Step"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Object
    }
}

impl napi::bindgen_prelude::ValidateNapiValue for StepArg {}

#[napi]
pub struct StepMapHandle {
    map: StepMap,
}

#[napi]
impl StepMapHandle {
    #[napi(constructor)]
    pub fn new(ranges: Vec<f64>, inverted: Option<bool>) -> Self {
        StepMapHandle {
            map: StepMap::new(
                ranges.into_iter().map(|n| n as usize).collect(),
                inverted.unwrap_or(false),
            ),
        }
    }

    #[napi(getter)]
    pub fn ranges(&self) -> Vec<f64> {
        self.map.ranges().iter().map(|&n| n as f64).collect()
    }

    #[napi(getter)]
    pub fn inverted(&self) -> bool {
        self.map.inverted()
    }

    #[napi]
    pub fn map(&self, pos: f64, assoc: i32) -> f64 {
        self.map.map(pos as usize, assoc) as f64
    }

    #[napi]
    pub fn map_result(&self, env: Env, pos: f64, assoc: i32) -> Result<Js> {
        wrap_map_result(env.raw(), self.map.map_result(pos as usize, assoc)).map(Js)
    }

    #[napi]
    pub fn recover(&self, value: f64) -> f64 {
        self.map.recover(recover_of(value)) as f64
    }

    #[napi]
    pub fn touches(&self, pos: f64, recover: f64) -> bool {
        self.map.touches(pos as usize, recover_of(recover))
    }

    #[napi]
    pub fn for_each(&self, env: Env, f: Js) -> Result<()> {
        let env = env.raw();
        for (old_start, old_end, new_start, new_end) in self.map.changes() {
            let args = [
                number(env, old_start)?,
                number(env, old_end)?,
                number(env, new_start)?,
                number(env, new_end)?,
            ];
            js::call(env, js::undefined(env)?, f.0, &args)?;
        }
        Ok(())
    }

    #[napi]
    pub fn invert(&self, env: Env) -> Result<Js> {
        wrap_step_map(env.raw(), &self.map.invert()).map(Js)
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.map.to_string()
    }
}

/// The recovery JavaScript encodes as one number, its index in the low 16 bits.
fn recover_of(value: f64) -> tarnish::transform::Recover {
    let value = value as u64;
    tarnish::transform::Recover {
        index: (value & 0xffff) as usize,
        offset: (value >> 16) as usize,
    }
}

#[napi]
pub fn step_map_offset(env: Env, n: f64) -> Result<Js> {
    wrap_step_map(env.raw(), &StepMap::offset(n as isize)).map(Js)
}

/// A mapping of its own, or a transform's, which grows as the transform does.
enum MappingSource {
    Own(RefCell<Mapping>),
    Transform(Rc<RefCell<Transform>>),
}

#[napi]
pub struct MappingHandle {
    source: MappingSource,
}

impl MappingHandle {
    fn with<R>(&self, f: impl FnOnce(&Mapping) -> R) -> R {
        match &self.source {
            MappingSource::Own(mapping) => f(&mapping.borrow()),
            MappingSource::Transform(tr) => f(tr.borrow().mapping()),
        }
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut Mapping) -> R) -> R {
        match &self.source {
            MappingSource::Own(mapping) => f(&mut mapping.borrow_mut()),
            MappingSource::Transform(tr) => f(tr.borrow_mut().mapping_mut()),
        }
    }
}

fn wrap_mapping(env: sys::napi_env, source: MappingSource) -> Result<sys::napi_value> {
    let handle = unsafe { MappingHandle::to_napi_value(env, MappingHandle { source }) }?;
    js::call_registered(env, "wrapMapping", &[handle])
}

fn mapping_arg(env: sys::napi_env, value: sys::napi_value) -> Result<Mapping> {
    let handle = js::get(env, value, "h")?;
    let handle = unsafe { ClassInstance::<MappingHandle>::from_napi_value(env, handle) }?;
    Ok(handle.with(Mapping::clone))
}

/// Call `f` with the Mappable that one of JavaScript's step maps or mappings wraps.
fn with_mappable<R>(
    env: sys::napi_env,
    value: sys::napi_value,
    f: impl FnOnce(&dyn Mappable) -> R,
) -> Result<R> {
    let handle = js::get(env, value, "h")?;
    if let Ok(map) = unsafe { ClassInstance::<StepMapHandle>::from_napi_value(env, handle) } {
        return Ok(f(&map.map));
    }
    let mapping = unsafe { ClassInstance::<MappingHandle>::from_napi_value(env, handle) }?;
    Ok(mapping.with(|mapping| f(mapping)))
}

#[napi]
impl MappingHandle {
    #[napi(constructor)]
    pub fn new(
        env: Env,
        maps: Vec<Js>,
        mirror: Option<Vec<f64>>,
        from: f64,
        to: f64,
    ) -> Result<Self> {
        let raw = env.raw();
        let mut step_maps = Vec::with_capacity(maps.len());
        for map in maps {
            let handle = js::get(raw, map.0, "h")?;
            let handle = unsafe { ClassInstance::<StepMapHandle>::from_napi_value(raw, handle) }?;
            step_maps.push(handle.map.clone());
        }
        let mirror = mirror
            .unwrap_or_default()
            .into_iter()
            .map(|n| n as usize)
            .collect();
        Ok(MappingHandle {
            source: MappingSource::Own(RefCell::new(Mapping::with_maps(
                step_maps,
                mirror,
                from as usize,
                to as usize,
            ))),
        })
    }

    #[napi]
    pub fn maps(&self, env: Env) -> Result<Js> {
        let raw = env.raw();
        let maps = self.with(|mapping| mapping.maps().to_vec());
        let wrapped = maps
            .iter()
            .map(|map| wrap_step_map(raw, map))
            .collect::<Result<Vec<_>>>()?;
        array(raw, wrapped).map(Js)
    }

    #[napi(getter)]
    pub fn mirror(&self) -> Option<Vec<f64>> {
        let mirror = self.with(|mapping| mapping.mirror().to_vec());
        (!mirror.is_empty()).then(|| mirror.into_iter().map(|n| n as f64).collect())
    }

    #[napi(getter)]
    pub fn from(&self) -> f64 {
        self.with(Mapping::from) as f64
    }

    #[napi(getter)]
    pub fn to(&self) -> f64 {
        self.with(Mapping::to) as f64
    }

    #[napi]
    pub fn slice(&self, env: Env, from: f64, to: f64) -> Result<Js> {
        let sliced = self.with(|mapping| mapping.slice(from as usize, to as usize));
        wrap_mapping(env.raw(), MappingSource::Own(RefCell::new(sliced))).map(Js)
    }

    #[napi]
    pub fn append_map(&self, env: Env, map: Js, mirrors: Option<f64>) -> Result<()> {
        let raw = env.raw();
        let handle = js::get(raw, map.0, "h")?;
        let map = unsafe { ClassInstance::<StepMapHandle>::from_napi_value(raw, handle) }?
            .map
            .clone();
        self.with_mut(|mapping| mapping.append_map(map, mirrors.map(|n| n as usize)));
        Ok(())
    }

    #[napi]
    pub fn append_mapping(&self, env: Env, other: Js) -> Result<()> {
        let other = mapping_arg(env.raw(), other.0)?;
        self.with_mut(|mapping| mapping.append_mapping(&other));
        Ok(())
    }

    #[napi]
    pub fn get_mirror(&self, n: f64) -> Option<f64> {
        self.with(|mapping| mapping.get_mirror(n as usize))
            .map(|n| n as f64)
    }

    #[napi]
    pub fn set_mirror(&self, n: f64, m: f64) {
        self.with_mut(|mapping| mapping.set_mirror(n as usize, m as usize));
    }

    #[napi]
    pub fn append_mapping_inverted(&self, env: Env, other: Js) -> Result<()> {
        let other = mapping_arg(env.raw(), other.0)?;
        self.with_mut(|mapping| mapping.append_mapping_inverted(&other));
        Ok(())
    }

    #[napi]
    pub fn invert(&self, env: Env) -> Result<Js> {
        let inverted = self.with(Mapping::invert);
        wrap_mapping(env.raw(), MappingSource::Own(RefCell::new(inverted))).map(Js)
    }

    #[napi]
    pub fn map(&self, pos: f64, assoc: i32) -> f64 {
        self.with(|mapping| mapping.map(pos as usize, assoc)) as f64
    }

    #[napi]
    pub fn map_result(&self, env: Env, pos: f64, assoc: i32) -> Result<Js> {
        let result = self.with(|mapping| mapping.map_result(pos as usize, assoc));
        wrap_map_result(env.raw(), result).map(Js)
    }
}

#[napi]
pub fn step_apply(env: Env, step: StepArg, doc: NodeArg) -> Result<Js> {
    let result = step.0.apply(&doc.0).or_throw(&env)?;
    wrap_step_result(env.raw(), &result).map(Js)
}

#[napi]
pub fn step_get_map(env: Env, step: StepArg) -> Result<Js> {
    wrap_step_map(env.raw(), &step.0.get_map()).map(Js)
}

#[napi]
pub fn step_invert(env: Env, step: StepArg, doc: NodeArg) -> Result<Js> {
    let inverted = step.0.invert(&doc.0).or_throw(&env)?;
    wrap_step(env.raw(), &inverted).map(Js)
}

#[napi]
pub fn step_map(env: Env, step: StepArg, mapping: Js) -> Result<Js> {
    let mapped = with_mappable(env.raw(), mapping.0, |mapping| step.0.map(mapping))?;
    wrap_step_option(env.raw(), mapped)
}

#[napi]
pub fn step_merge(env: Env, step: StepArg, other: Js) -> Result<Js> {
    // Only ProseMirror's own steps merge; any other step merges with nothing.
    let Ok(other) = (unsafe { StepArg::from_napi_value(env.raw(), other.0) }) else {
        return js::null(env.raw()).map(Js);
    };
    wrap_step_option(env.raw(), step.0.merge(&other.0))
}

#[napi]
pub fn step_to_json(step: StepArg) -> Data {
    Data(step.0.to_json())
}

#[napi]
pub fn step_from_json(env: Env, schema: &SchemaHandle, json: Data) -> Result<Js> {
    let step = Step::from_json(&schema.schema, &json.0).or_throw(&env)?;
    wrap_step(env.raw(), &step).map(Js)
}

#[napi]
pub fn step_result_from_replace(
    env: Env,
    doc: NodeArg,
    from: f64,
    to: f64,
    slice: SliceArg,
) -> Result<Js> {
    let result =
        StepResult::from_replace(&doc.0, from as usize, to as usize, &slice.0).or_throw(&env)?;
    wrap_step_result(env.raw(), &result).map(Js)
}

/// A `NodeRange` of JavaScript's, of its two positions and depth.
fn node_range(env: sys::napi_env, value: sys::napi_value) -> Result<NodeRange> {
    let from = unsafe { ResolvedPosArg::from_napi_value(env, js::get(env, value, "$from")?) }?;
    let to = unsafe { ResolvedPosArg::from_napi_value(env, js::get(env, value, "$to")?) }?;
    Ok(NodeRange::new(from.0, to.0, field(env, value, "depth")?))
}

fn attrs_of(env: sys::napi_env, value: sys::napi_value) -> Result<Option<Attrs>> {
    let value = js::value_from_js(env, value)?;
    Ok(value
        .as_attrs()
        .map(|attrs| std::sync::Arc::new(attrs.clone())))
}

/// A `{type, attrs}` wrapper of JavaScript's, `None` for a falsy entry.
fn wrapper(env: sys::napi_env, value: sys::napi_value) -> Result<Option<Wrapper>> {
    if !js::truthy(env, value)? {
        return Ok(None);
    }
    let node_type = unsafe { NodeTypeArg::from_napi_value(env, js::get(env, value, "type")?) }?.0;
    Ok(Some(Wrapper {
        node_type,
        attrs: attrs_of(env, js::get(env, value, "attrs")?)?,
    }))
}

fn wrappers(env: sys::napi_env, value: sys::napi_value) -> Result<Vec<Option<Wrapper>>> {
    unsafe { Vec::<Js>::from_napi_value(env, value) }?
        .into_iter()
        .map(|item| wrapper(env, item.0))
        .collect()
}

/// A mark, or a mark type, of JavaScript's; `None` for a falsy value.
fn mark_match(env: sys::napi_env, value: sys::napi_value) -> Result<Option<MarkOrType>> {
    if !js::truthy(env, value)? {
        return Ok(None);
    }
    if let Ok(mark) = unsafe { MarkArg::from_napi_value(env, value) } {
        return Ok(Some(MarkOrType::Mark(mark.0)));
    }
    let mark_type = unsafe { MarkTypeArg::from_napi_value(env, value) }?;
    Ok(Some(MarkOrType::Type(mark_type.0)))
}

enum MarkOrType {
    Mark(Mark),
    Type(MarkType),
}

impl MarkOrType {
    fn as_match(&self) -> MarkMatch<'_> {
        match self {
            MarkOrType::Mark(mark) => MarkMatch::Mark(mark),
            MarkOrType::Type(mark_type) => MarkMatch::Type(mark_type),
        }
    }
}

#[napi]
pub struct TransformHandle {
    tr: Rc<RefCell<Transform>>,
}

impl TransformHandle {
    /// Run an operation on the transform; its errors are thrown as ProseMirror throws them.
    fn run(&self, env: &Env, f: impl FnOnce(&mut Transform) -> tarnish::Result<()>) -> Result<()> {
        let result = f(&mut self.tr.borrow_mut());
        result.or_throw(env)
    }
}

#[napi]
impl TransformHandle {
    #[napi(constructor)]
    pub fn new(doc: NodeArg) -> Self {
        TransformHandle {
            tr: Rc::new(RefCell::new(Transform::new(doc.0))),
        }
    }

    #[napi]
    pub fn doc(&self, env: Env) -> Result<Js> {
        let doc = self.tr.borrow().doc().clone();
        node::wrap(env.raw(), &doc).map(Js)
    }

    #[napi]
    pub fn before(&self, env: Env) -> Result<Js> {
        let before = self.tr.borrow().before().clone();
        node::wrap(env.raw(), &before).map(Js)
    }

    /// The steps from index `start` on.
    #[napi]
    pub fn steps_from(&self, env: Env, start: u32) -> Result<Js> {
        let steps: Vec<Step> = self.tr.borrow().steps()[start as usize..].to_vec();
        let wrapped = steps
            .iter()
            .map(|step| wrap_step(env.raw(), step))
            .collect::<Result<Vec<_>>>()?;
        array(env.raw(), wrapped).map(Js)
    }

    /// The documents before the steps from index `start` on.
    #[napi]
    pub fn docs_from(&self, env: Env, start: u32) -> Result<Js> {
        let docs: Vec<tarnish::Node> = self.tr.borrow().docs()[start as usize..].to_vec();
        let wrapped = docs
            .iter()
            .map(|doc| node::wrap(env.raw(), doc))
            .collect::<Result<Vec<_>>>()?;
        array(env.raw(), wrapped).map(Js)
    }

    #[napi(getter)]
    pub fn step_count(&self) -> u32 {
        self.tr.borrow().steps().len() as u32
    }

    /// The transform's mapping, which grows as the transform does.
    #[napi]
    pub fn mapping(&self, env: Env) -> Result<Js> {
        wrap_mapping(env.raw(), MappingSource::Transform(self.tr.clone())).map(Js)
    }

    #[napi(getter)]
    pub fn doc_changed(&self) -> bool {
        self.tr.borrow().doc_changed()
    }

    #[napi]
    pub fn step(&self, env: Env, step: StepArg) -> Result<()> {
        self.run(&env, |tr| tr.step(step.0).map(|_| ()))
    }

    #[napi]
    pub fn maybe_step(&self, env: Env, step: StepArg) -> Result<Js> {
        let result = self.tr.borrow_mut().maybe_step(step.0).or_throw(&env)?;
        wrap_step_result(env.raw(), &result).map(Js)
    }

    #[napi]
    pub fn changed_range(&self) -> Option<ChangedRange> {
        self.tr
            .borrow()
            .changed_range()
            .map(|(from, to)| ChangedRange {
                from: from as f64,
                to: to as f64,
            })
    }

    #[napi]
    pub fn replace(&self, env: Env, from: f64, to: f64, slice: SliceArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.replace(from as usize, to as usize, &slice.0).map(|_| ())
        })
    }

    #[napi]
    pub fn replace_with(&self, env: Env, from: f64, to: f64, content: FragmentArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.replace_with(from as usize, to as usize, content.0)
                .map(|_| ())
        })
    }

    #[napi]
    pub fn delete(&self, env: Env, from: f64, to: f64) -> Result<()> {
        self.run(&env, |tr| tr.delete(from as usize, to as usize).map(|_| ()))
    }

    #[napi]
    pub fn insert(&self, env: Env, pos: f64, content: FragmentArg) -> Result<()> {
        self.run(&env, |tr| tr.insert(pos as usize, content.0).map(|_| ()))
    }

    #[napi]
    pub fn replace_range(&self, env: Env, from: f64, to: f64, slice: SliceArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.replace_range(from as usize, to as usize, &slice.0)
                .map(|_| ())
        })
    }

    #[napi]
    pub fn replace_range_with(&self, env: Env, from: f64, to: f64, node: NodeArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.replace_range_with(from as usize, to as usize, node.0)
                .map(|_| ())
        })
    }

    #[napi]
    pub fn delete_range(&self, env: Env, from: f64, to: f64) -> Result<()> {
        self.run(&env, |tr| {
            tr.delete_range(from as usize, to as usize).map(|_| ())
        })
    }

    #[napi]
    pub fn lift(&self, env: Env, range: Js, target: f64) -> Result<()> {
        let range = node_range(env.raw(), range.0)?;
        self.run(&env, |tr| tr.lift(&range, target as usize).map(|_| ()))
    }

    #[napi]
    pub fn join(&self, env: Env, pos: f64, depth: f64) -> Result<()> {
        self.run(&env, |tr| tr.join(pos as usize, depth as usize).map(|_| ()))
    }

    #[napi]
    pub fn wrap(&self, env: Env, range: Js, wrapper_list: Js) -> Result<()> {
        let range = node_range(env.raw(), range.0)?;
        let list = wrappers(env.raw(), wrapper_list.0)?;
        let list: Vec<Wrapper> = list
            .into_iter()
            .map(|wrapper| {
                wrapper.ok_or_else(|| napi::Error::from_reason("A wrapper with no type"))
            })
            .collect::<Result<_>>()?;
        self.run(&env, |tr| tr.wrap(&range, &list).map(|_| ()))
    }

    #[napi]
    pub fn set_block_type(
        &self,
        env: Env,
        from: f64,
        to: f64,
        node_type: NodeTypeArg,
        attrs: Js,
    ) -> Result<()> {
        let raw = env.raw();
        if js::type_of(raw, attrs.0)? == sys::ValueType::napi_function {
            let hook = js::Hook::function(raw, attrs.0)?;
            let mut attrs_of_node = |node: &tarnish::Node| -> tarnish::Result<Option<Attrs>> {
                let result = hook.call(|env| Ok(vec![node::wrap(env, node)?]))?;
                hook.read(|env| attrs_of(env, result))
            };
            return self.run(&env, |tr| {
                tr.set_block_type(
                    from as usize,
                    to as usize,
                    &node_type.0,
                    BlockAttrs::Hook(&mut attrs_of_node),
                )
                .map(|_| ())
            });
        }
        let attrs = attrs_of(raw, attrs.0)?;
        self.run(&env, |tr| {
            let fixed = BlockAttrs::Fixed(attrs.as_deref());
            tr.set_block_type(from as usize, to as usize, &node_type.0, fixed)
                .map(|_| ())
        })
    }

    #[napi]
    pub fn set_node_markup(
        &self,
        env: Env,
        pos: f64,
        node_type: Option<NodeTypeArg>,
        attrs: Js,
        marks: Option<Vec<MarkArg>>,
    ) -> Result<()> {
        let attrs = attrs_of(env.raw(), attrs.0)?;
        let marks = marks.map(|marks| crate::mark::list(Some(marks)));
        self.run(&env, |tr| {
            tr.set_node_markup(
                pos as usize,
                node_type.as_ref().map(|node_type| &node_type.0),
                attrs.as_deref(),
                marks.as_deref(),
            )
            .map(|_| ())
        })
    }

    #[napi]
    pub fn set_node_attribute(&self, env: Env, pos: f64, attr: String, value: Data) -> Result<()> {
        self.run(&env, |tr| {
            tr.set_node_attribute(pos as usize, &attr, value.0)
                .map(|_| ())
        })
    }

    #[napi]
    pub fn set_doc_attribute(&self, env: Env, attr: String, value: Data) -> Result<()> {
        self.run(&env, |tr| tr.set_doc_attribute(&attr, value.0).map(|_| ()))
    }

    #[napi]
    pub fn add_node_mark(&self, env: Env, pos: f64, mark: MarkArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.add_node_mark(pos as usize, mark.0).map(|_| ())
        })
    }

    #[napi]
    pub fn remove_node_mark(&self, env: Env, pos: f64, mark: Js) -> Result<()> {
        let mark = mark_match(env.raw(), mark.0)?.ok_or_else(|| {
            napi::Error::from_reason("removeNodeMark needs a mark or a mark type")
        })?;
        self.run(&env, |tr| {
            tr.remove_node_mark(pos as usize, mark.as_match())
                .map(|_| ())
        })
    }

    #[napi]
    pub fn split(&self, env: Env, pos: f64, depth: f64, types_after: Option<Js>) -> Result<()> {
        let types = types_after
            .map(|types| wrappers(env.raw(), types.0))
            .transpose()?;
        self.run(&env, |tr| {
            tr.split(pos as usize, depth as usize, types.as_deref())
                .map(|_| ())
        })
    }

    #[napi]
    pub fn add_mark(&self, env: Env, from: f64, to: f64, mark: MarkArg) -> Result<()> {
        self.run(&env, |tr| {
            tr.add_mark(from as usize, to as usize, &mark.0).map(|_| ())
        })
    }

    #[napi]
    pub fn remove_mark(&self, env: Env, from: f64, to: f64, mark: Js) -> Result<()> {
        let mark = mark_match(env.raw(), mark.0)?;
        self.run(&env, |tr| {
            tr.remove_mark(
                from as usize,
                to as usize,
                mark.as_ref().map(MarkOrType::as_match),
            )
            .map(|_| ())
        })
    }

    #[napi]
    pub fn clear_incompatible(
        &self,
        env: Env,
        pos: f64,
        parent_type: NodeTypeArg,
        start: Option<ContentMatchArg>,
    ) -> Result<()> {
        self.run(&env, |tr| {
            tr.clear_incompatible(pos as usize, &parent_type.0, start.map(|start| start.0))
                .map(|_| ())
        })
    }
}

#[napi(object)]
pub struct ChangedRange {
    pub from: f64,
    pub to: f64,
}

#[napi]
pub fn replace_step(env: Env, doc: NodeArg, from: f64, to: f64, slice: SliceArg) -> Result<Js> {
    let step = tarnish::transform::replace_step(&doc.0, from as usize, to as usize, &slice.0)
        .or_throw(&env)?;
    wrap_step_option(env.raw(), step)
}

#[napi]
pub fn can_join(env: Env, doc: NodeArg, pos: f64) -> Result<bool> {
    tarnish::transform::can_join(&doc.0, pos as usize).or_throw(&env)
}

#[napi]
pub fn can_split(
    env: Env,
    doc: NodeArg,
    pos: f64,
    depth: f64,
    types_after: Option<Js>,
) -> Result<bool> {
    let types = types_after
        .map(|types| wrappers(env.raw(), types.0))
        .transpose()?;
    tarnish::transform::can_split(&doc.0, pos as usize, depth as usize, types.as_deref())
        .or_throw(&env)
}

#[napi]
pub fn join_point(env: Env, doc: NodeArg, pos: f64, dir: i32) -> Result<Option<f64>> {
    let point = tarnish::transform::join_point(&doc.0, pos as usize, dir).or_throw(&env)?;
    Ok(point.map(|pos| pos as f64))
}

#[napi]
pub fn insert_point(
    env: Env,
    doc: NodeArg,
    pos: f64,
    node_type: NodeTypeArg,
) -> Result<Option<f64>> {
    let point =
        tarnish::transform::insert_point(&doc.0, pos as usize, &node_type.0).or_throw(&env)?;
    Ok(point.map(|pos| pos as f64))
}

#[napi]
pub fn drop_point(env: Env, doc: NodeArg, pos: f64, slice: SliceArg) -> Result<Option<f64>> {
    let point = tarnish::transform::drop_point(&doc.0, pos as usize, &slice.0).or_throw(&env)?;
    Ok(point.map(|pos| pos as f64))
}

#[napi]
pub fn lift_target(env: Env, range: Js) -> Result<Option<f64>> {
    let range = node_range(env.raw(), range.0)?;
    let target = tarnish::transform::lift_target(&range).or_throw(&env)?;
    Ok(target.map(|depth| depth as f64))
}

#[napi]
pub fn find_wrapping(
    env: Env,
    range: Js,
    node_type: NodeTypeArg,
    attrs: Js,
    inner_range: Js,
) -> Result<Js> {
    let raw = env.raw();
    let range = node_range(raw, range.0)?;
    let inner = node_range(raw, inner_range.0)?;
    let attrs = attrs_of(raw, attrs.0)?;
    let found = tarnish::transform::find_wrapping(&range, &node_type.0, attrs, Some(&inner))
        .or_throw(&env)?;
    let Some(found) = found else {
        return js::null(raw).map(Js);
    };
    let mut items = Vec::with_capacity(found.len());
    for wrapper in found {
        let mut object = ptr::null_mut();
        js::check(unsafe { sys::napi_create_object(raw, &mut object) })?;
        let node_type = crate::schema::wrap_node_type(raw, &wrapper.node_type)?;
        let attrs = match &wrapper.attrs {
            Some(attrs) => js::value_to_js(raw, &Value::Object(attrs.clone()))?,
            None => js::null(raw)?,
        };
        for (key, value) in [("type", node_type), ("attrs", attrs)] {
            let key = js::string(raw, key)?;
            js::check(unsafe { sys::napi_set_property(raw, object, key, value) })?;
        }
        items.push(object);
    }
    array(raw, items).map(Js)
}
