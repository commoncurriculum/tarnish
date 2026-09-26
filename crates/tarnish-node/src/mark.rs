//! Marks.

use std::sync::Arc;

use napi::bindgen_prelude::{FnArgs, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::{Mark, Marks};

use crate::js;
use crate::schema;

pub fn list(marks: Vec<&MarkHandle>) -> Vec<Mark> {
    marks.into_iter().map(|mark| mark.mark.clone()).collect()
}

/// The set an operation made of `given`, or `None` when it left the set as it was, for
/// JavaScript to give back the array it passed, as ProseMirror does.
pub fn changed_set<'env>(
    env: &'env Env,
    given: &Marks,
    result: &Marks,
) -> Result<Option<Unknown<'env>>> {
    if Arc::ptr_eq(given, result) {
        return Ok(None);
    }
    wrap_set(env, result).map(Some)
}

pub fn wrap<'env>(env: &'env Env, mark: &Mark) -> Result<Unknown<'env>> {
    let handle = MarkHandle { mark: mark.clone() };
    js::call_registered(env, "wrapMark", FnArgs::from((handle, mark.id() as f64)))
}

/// A set of marks as an array of wrappers; the empty set as `Mark.none`.
pub fn wrap_set<'env>(env: &'env Env, marks: &[Mark]) -> Result<Unknown<'env>> {
    if marks.is_empty() {
        return js::call_registered(env, "markNone", ());
    }
    let marks = marks
        .iter()
        .map(|mark| wrap(env, mark))
        .collect::<Result<Vec<_>>>()?;
    marks.into_unknown(env)
}

#[napi]
pub struct MarkHandle {
    pub(crate) mark: Mark,
}

#[napi]
impl MarkHandle {
    #[napi]
    pub fn mark_type<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        schema::wrap_mark_type(env, self.mark.mark_type())
    }

    #[napi]
    pub fn attrs<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::attrs_to_js(env, self.mark.attrs())
    }

    #[napi]
    pub fn add_to_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let set: Marks = list(set).into();
        changed_set(env, &set, &self.mark.add_to_set(&set))
    }

    #[napi]
    pub fn remove_from_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let set: Marks = list(set).into();
        changed_set(env, &set, &self.mark.remove_from_set(&set))
    }

    #[napi]
    pub fn is_in_set(&self, set: Vec<&MarkHandle>) -> bool {
        self.mark.is_in_set(&list(set))
    }

    #[napi]
    pub fn eq(&self, other: &MarkHandle) -> bool {
        self.mark == other.mark
    }

    #[napi]
    pub fn to_json<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::value_to_js(env, &self.mark.to_json())
    }
}

#[napi]
pub fn marks_same_set(a: Vec<&MarkHandle>, b: Vec<&MarkHandle>) -> bool {
    Mark::same_set(&list(a), &list(b))
}

#[napi]
pub fn marks_set_from<'env>(env: &'env Env, marks: Vec<&MarkHandle>) -> Result<Unknown<'env>> {
    wrap_set(env, &Mark::set_from(&list(marks)))
}
