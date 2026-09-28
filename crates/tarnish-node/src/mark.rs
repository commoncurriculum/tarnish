//! Marks.

use napi::bindgen_prelude::{FnArgs, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::{Mark, Marks};

use crate::js;
use crate::schema;

pub fn list(marks: Vec<&MarkHandle>) -> Vec<Mark<'static>> {
    marks.into_iter().map(|mark| mark.mark.clone()).collect()
}

/// The list an operation made, or `None` when it left the list as it was, for JavaScript to
/// give back the array it passed, as ProseMirror does.
pub fn changed_list<'env>(
    env: &'env Env,
    result: Option<Vec<Mark<'static>>>,
) -> Result<Option<Unknown<'env>>> {
    result.map(|marks| wrap_list(env, &marks)).transpose()
}

pub fn wrap<'env>(env: &'env Env, mark: &Mark<'static>) -> Result<Unknown<'env>> {
    let handle = MarkHandle { mark: mark.clone() };
    let (chunk, index) = mark.id();
    let key = format!("{chunk}:{index}");
    js::call_registered(env, "wrapMark", FnArgs::from((handle, key)))
}

/// A list of marks as an array of wrappers; the empty list as `Mark.none`.
pub fn wrap_list<'env>(env: &'env Env, marks: &[Mark<'static>]) -> Result<Unknown<'env>> {
    if marks.is_empty() {
        return js::call_registered(env, "markNone", ());
    }
    let marks = marks
        .iter()
        .map(|mark| wrap(env, mark))
        .collect::<Result<Vec<_>>>()?;
    marks.into_unknown(env)
}

pub fn wrap_set<'env>(env: &'env Env, marks: &Marks<'static>) -> Result<Unknown<'env>> {
    wrap_list(env, &marks.to_vec())
}

#[napi]
pub struct MarkHandle {
    pub(crate) mark: Mark<'static>,
}

#[napi]
impl MarkHandle {
    #[napi]
    pub fn mark_type<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        schema::wrap_mark_type(env, &self.mark.mark_type())
    }

    #[napi]
    pub fn attrs<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::attrs_to_js(env, &self.mark.attrs())
    }

    #[napi]
    pub fn add_to_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        changed_list(env, self.mark.added_to(&list(set)))
    }

    #[napi]
    pub fn remove_from_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let mut set = list(set);
        let removed = set.iter().position(|other| self.mark == *other).map(|index| {
            set.remove(index);
            set
        });
        changed_list(env, removed)
    }

    #[napi]
    pub fn is_in_set(&self, set: Vec<&MarkHandle>) -> bool {
        self.mark.is_in_list(&list(set))
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
    Mark::same_list(&list(a), &list(b))
}

#[napi]
pub fn marks_set_from<'env>(env: &'env Env, marks: Vec<&MarkHandle>) -> Result<Unknown<'env>> {
    let mut marks = list(marks);
    marks.sort_by_key(|mark| mark.mark_type().rank());
    wrap_list(env, &marks)
}
