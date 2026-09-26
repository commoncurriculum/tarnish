//! Step maps and mappings.

use std::cell::RefCell;
use std::rc::Rc;

use napi::bindgen_prelude::{Either, FnArgs, Function, Unknown};
use napi::{Env, JsValue, Result};
use napi_derive::napi;
use tarnish::transform::{MapResult, Mappable, Mapping, Recover, StepMap, Transform};

use crate::js;

pub fn wrap_step_map<'env>(env: &'env Env, map: &StepMap) -> Result<Unknown<'env>> {
    js::call_registered(env, "wrapStepMap", StepMapHandle { map: map.clone() })
}

fn wrap_map_result<'env>(env: &'env Env, result: MapResult) -> Result<Unknown<'env>> {
    let args = (
        result.pos as f64,
        result.del_info as f64,
        result.recover.map(Recover::to_number),
        result.deleted(),
        result.deleted_before(),
        result.deleted_after(),
        result.deleted_across(),
    );
    js::call_registered(env, "wrapMapResult", FnArgs::from(args))
}

/// Call `f` with what one of JavaScript's step maps or mappings maps by.
pub fn with_mappable<R>(
    mappable: Either<&StepMapHandle, &MappingHandle>,
    f: impl FnOnce(&dyn Mappable) -> R,
) -> R {
    match mappable {
        Either::A(map) => f(&map.map),
        Either::B(mapping) => mapping.with(|mapping| f(mapping)),
    }
}

#[napi]
pub struct StepMapHandle {
    map: StepMap,
}

#[napi]
impl StepMapHandle {
    #[napi(constructor)]
    pub fn new(env: &Env, ranges: Vec<f64>, inverted: Option<bool>) -> Result<Self> {
        let ranges = ranges
            .into_iter()
            .map(|n| js::pos(env, n))
            .collect::<Result<_>>()?;
        Ok(StepMapHandle {
            map: StepMap::new(ranges, inverted.unwrap_or(false)),
        })
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
    pub fn map(&self, env: &Env, pos: f64, assoc: i32) -> Result<f64> {
        Ok(self.map.map(js::pos(env, pos)?, assoc) as f64)
    }

    #[napi]
    pub fn map_result<'env>(&self, env: &'env Env, pos: f64, assoc: i32) -> Result<Unknown<'env>> {
        wrap_map_result(env, self.map.map_result(js::pos(env, pos)?, assoc))
    }

    #[napi]
    pub fn recover(&self, value: f64) -> f64 {
        self.map.recover(Recover::from_number(value)) as f64
    }

    #[napi]
    pub fn touches(&self, env: &Env, pos: f64, recover: f64) -> Result<bool> {
        let recover = Recover::from_number(recover);
        Ok(self.map.touches(js::pos(env, pos)?, recover))
    }

    #[napi]
    pub fn for_each(&self, f: Function) -> Result<()> {
        for (old_start, old_end, new_start, new_end) in self.map.changes() {
            let args = (
                old_start as f64,
                old_end as f64,
                new_start as f64,
                new_end as f64,
            );
            js::call(f.to_unknown(), FnArgs::from(args))?;
        }
        Ok(())
    }

    #[napi]
    pub fn invert<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        wrap_step_map(env, &self.map.invert())
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.map.to_string()
    }
}

#[napi]
pub fn step_map_offset<'env>(env: &'env Env, n: f64) -> Result<Unknown<'env>> {
    let size = js::pos(env, n.abs())? as isize;
    let offset = if n < 0.0 { -size } else { size };
    wrap_step_map(env, &StepMap::offset(offset))
}

/// A mapping of its own, or a transform's, which grows as the transform does.
pub enum MappingSource {
    Own(RefCell<Mapping>),
    Transform(Rc<RefCell<Transform>>),
}

pub fn wrap_mapping<'env>(env: &'env Env, source: MappingSource) -> Result<Unknown<'env>> {
    js::call_registered(env, "wrapMapping", MappingHandle { source })
}

fn wrap_own_mapping<'env>(env: &'env Env, mapping: Mapping) -> Result<Unknown<'env>> {
    wrap_mapping(env, MappingSource::Own(RefCell::new(mapping)))
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

#[napi]
impl MappingHandle {
    #[napi(constructor)]
    pub fn new(maps: Vec<&StepMapHandle>, mirror: Option<Vec<u32>>, from: u32, to: u32) -> Self {
        let maps = maps.into_iter().map(|map| map.map.clone()).collect();
        let mirror = mirror
            .unwrap_or_default()
            .into_iter()
            .map(|n| n as usize)
            .collect();
        let mapping = Mapping::with_maps(maps, mirror, from as usize, to as usize);
        MappingHandle {
            source: MappingSource::Own(RefCell::new(mapping)),
        }
    }

    #[napi]
    pub fn maps<'env>(&self, env: &'env Env) -> Result<Vec<Unknown<'env>>> {
        let maps = self.with(|mapping| mapping.maps().to_vec());
        maps.iter().map(|map| wrap_step_map(env, map)).collect()
    }

    #[napi(getter)]
    pub fn mirror(&self) -> Option<Vec<u32>> {
        let mirror = self.with(|mapping| mapping.mirror().to_vec());
        (!mirror.is_empty()).then(|| mirror.into_iter().map(|n| n as u32).collect())
    }

    #[napi(getter)]
    pub fn from(&self) -> u32 {
        self.with(Mapping::from) as u32
    }

    #[napi(getter)]
    pub fn to(&self) -> u32 {
        self.with(Mapping::to) as u32
    }

    #[napi]
    pub fn slice<'env>(&self, env: &'env Env, from: u32, to: u32) -> Result<Unknown<'env>> {
        let sliced = self.with(|mapping| mapping.slice(from as usize, to as usize));
        wrap_own_mapping(env, sliced)
    }

    #[napi]
    pub fn append_map(&self, map: &StepMapHandle, mirrors: Option<u32>) {
        let mirrors = mirrors.map(|n| n as usize);
        self.with_mut(|mapping| mapping.append_map(map.map.clone(), mirrors));
    }

    #[napi]
    pub fn append_mapping(&self, other: &MappingHandle) {
        let other = other.with(Mapping::clone);
        self.with_mut(|mapping| mapping.append_mapping(&other));
    }

    #[napi]
    pub fn get_mirror(&self, n: u32) -> Option<u32> {
        let mirror = self.with(|mapping| mapping.get_mirror(n as usize));
        mirror.map(|n| n as u32)
    }

    #[napi]
    pub fn set_mirror(&self, n: u32, m: u32) {
        self.with_mut(|mapping| mapping.set_mirror(n as usize, m as usize));
    }

    #[napi]
    pub fn append_mapping_inverted(&self, other: &MappingHandle) {
        let other = other.with(Mapping::clone);
        self.with_mut(|mapping| mapping.append_mapping_inverted(&other));
    }

    #[napi]
    pub fn invert<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        wrap_own_mapping(env, self.with(Mapping::invert))
    }

    #[napi]
    pub fn map(&self, env: &Env, pos: f64, assoc: i32) -> Result<f64> {
        let pos = js::pos(env, pos)?;
        Ok(self.with(|mapping| mapping.map(pos, assoc)) as f64)
    }

    #[napi]
    pub fn map_result<'env>(&self, env: &'env Env, pos: f64, assoc: i32) -> Result<Unknown<'env>> {
        let pos = js::pos(env, pos)?;
        wrap_map_result(env, self.with(|mapping| mapping.map_result(pos, assoc)))
    }
}
