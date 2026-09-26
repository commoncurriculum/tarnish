//! Marks.

use std::ptr;

use napi::bindgen_prelude::{FromNapiValue, TypeName, ValidateNapiValue};
use napi::{Env, Result, ValueType, sys};
use napi_derive::napi;
use tarnish::{Mark, Marks};

use crate::js::{self, Data, Js};
use crate::schema;

pub struct MarkArg(pub Mark);

crate::handle_arg!(MarkArg, MarkHandle, |handle| handle.mark.clone());

pub fn list(marks: Option<Vec<MarkArg>>) -> Vec<Mark> {
    marks
        .unwrap_or_default()
        .into_iter()
        .map(|mark| mark.0)
        .collect()
}

/// An array of marks, kept to give back when an operation leaves the set as it is, as
/// ProseMirror gives back the array it was given.
pub struct MarkSetArg {
    js: sys::napi_value,
    marks: Marks,
}

impl FromNapiValue for MarkSetArg {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        let marks = unsafe { Vec::<MarkArg>::from_napi_value(env, value) }?;
        Ok(MarkSetArg {
            js: value,
            marks: list(Some(marks)).into(),
        })
    }
}

impl TypeName for MarkSetArg {
    fn type_name() -> &'static str {
        "Mark[]"
    }

    fn value_type() -> ValueType {
        ValueType::Object
    }
}

impl ValidateNapiValue for MarkSetArg {}

impl MarkSetArg {
    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    /// `result` as an array: the one given when `result` is that set.
    pub fn give_back(&self, env: sys::napi_env, result: &Marks) -> Result<Js> {
        if std::sync::Arc::ptr_eq(result, &self.marks) {
            return Ok(Js(self.js));
        }
        wrap_set(env, result).map(Js)
    }
}

pub fn wrap(env: sys::napi_env, mark: &Mark) -> Result<sys::napi_value> {
    let handle = unsafe {
        napi::bindgen_prelude::ToNapiValue::to_napi_value(env, MarkHandle { mark: mark.clone() })
    }?;
    let id = js::number(env, mark.id() as f64)?;
    js::call_registered(env, "wrapMark", &[handle, id])
}

/// A set of marks as an array of wrappers; the empty set as `Mark.none`.
pub fn wrap_set(env: sys::napi_env, marks: &[Mark]) -> Result<sys::napi_value> {
    if marks.is_empty() {
        return js::call_registered(env, "markNone", &[]);
    }
    let mut array = ptr::null_mut();
    js::check(unsafe { sys::napi_create_array_with_length(env, marks.len(), &mut array) })?;
    for (index, mark) in marks.iter().enumerate() {
        let mark = wrap(env, mark)?;
        js::check(unsafe { sys::napi_set_element(env, array, index as u32, mark) })?;
    }
    Ok(array)
}

#[napi]
pub struct MarkHandle {
    mark: Mark,
}

#[napi]
impl MarkHandle {
    #[napi]
    pub fn mark_type(&self, env: Env) -> Result<Js> {
        schema::wrap_mark_type(env.raw(), self.mark.mark_type()).map(Js)
    }

    #[napi]
    pub fn attrs(&self) -> Data {
        Data(tarnish::Value::Object(self.mark.attrs().clone()))
    }

    #[napi]
    pub fn add_to_set(&self, env: Env, set: MarkSetArg) -> Result<Js> {
        set.give_back(env.raw(), &self.mark.add_to_set(set.marks()))
    }

    #[napi]
    pub fn remove_from_set(&self, env: Env, set: MarkSetArg) -> Result<Js> {
        set.give_back(env.raw(), &self.mark.remove_from_set(set.marks()))
    }

    #[napi]
    pub fn is_in_set(&self, set: Vec<MarkArg>) -> bool {
        self.mark.is_in_set(&list(Some(set)))
    }

    #[napi]
    pub fn eq(&self, other: MarkArg) -> bool {
        self.mark == other.0
    }

    #[napi]
    pub fn to_json(&self) -> Data {
        Data(self.mark.to_json())
    }
}

#[napi]
pub fn marks_same_set(a: Vec<MarkArg>, b: Vec<MarkArg>) -> bool {
    Mark::same_set(&list(Some(a)), &list(Some(b)))
}

#[napi]
pub fn marks_set_from(env: Env, marks: Vec<MarkArg>) -> Result<Js> {
    wrap_set(env.raw(), &Mark::set_from(&list(Some(marks)))).map(Js)
}
