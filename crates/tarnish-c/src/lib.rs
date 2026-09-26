//! tarnish as a C library, whose interface `include/tarnish.h` declares: ProseMirror's JSON in
//! and out, as UTF-8 strings.
//!
//! # Safety
//!
//! Every function takes pointers from C, each of which must be NULL or valid: strings
//! NUL-terminated, a schema one `tarnish_schema_new` made and nothing has freed, `error` and
//! `mapped` writable, and a string passed to `tarnish_free` one the library returned.

use std::ffi::{CStr, CString, c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Arc;

use tarnish::{Error, Object, Result, Schema, Value, json};

pub struct TarnishSchema(Schema);

fn parse(json: *const c_char) -> Result<Value> {
    if json.is_null() {
        return Err(Error::Other("A JSON string was NULL".into()));
    }
    let text = unsafe { CStr::from_ptr(json) }
        .to_str()
        .map_err(|_| Error::Other("A JSON string wasn't UTF-8".into()))?;
    let parsed: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::Other(format!("Invalid JSON: {error}")))?;
    Ok(value(parsed))
}

fn value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(value),
        serde_json::Value::Number(number) => Value::Number(number.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(text) => Value::String(text.into()),
        serde_json::Value::Array(items) => Value::Array(items.into_iter().map(value).collect()),
        serde_json::Value::Object(entries) => Value::Object(Arc::new(
            entries
                .into_iter()
                .map(|(key, item)| (Arc::from(key), value(item)))
                .collect::<Object>(),
        )),
    }
}

fn string(text: String) -> *mut c_char {
    CString::new(text).map_or(ptr::null_mut(), CString::into_raw)
}

/// Run `f`, catching panics, and report an error through `error`.
fn run<T>(error: *mut *mut c_char, failed: T, f: impl FnOnce() -> Result<T>) -> T {
    let outcome = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(Error::Other(
            "tarnish panicked; this is a bug in tarnish".into(),
        ))
    });
    match outcome {
        Ok(result) => result,
        Err(failure) => {
            if !error.is_null() {
                unsafe { *error = string(failure.to_string()) };
            }
            failed
        }
    }
}

fn schema<'a>(schema: *const TarnishSchema) -> Result<&'a Schema> {
    unsafe { schema.as_ref() }
        .map(|schema| &schema.0)
        .ok_or_else(|| Error::Other("The schema was NULL".into()))
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_schema_new(
    spec_json: *const c_char,
    error: *mut *mut c_char,
) -> *mut TarnishSchema {
    run(error, ptr::null_mut(), || {
        let schema = json::schema(&parse(spec_json)?)?;
        Ok(Box::into_raw(Box::new(TarnishSchema(schema))))
    })
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_schema_free(schema: *mut TarnishSchema) {
    if !schema.is_null() {
        drop(unsafe { Box::from_raw(schema) });
    }
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_check(
    schema_ptr: *const TarnishSchema,
    doc_json: *const c_char,
    error: *mut *mut c_char,
) -> bool {
    run(error, false, || {
        json::check(schema(schema_ptr)?, &parse(doc_json)?)?;
        Ok(true)
    })
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_apply_steps(
    schema_ptr: *const TarnishSchema,
    doc_json: *const c_char,
    steps_json: *const c_char,
    error: *mut *mut c_char,
) -> *mut c_char {
    run(error, ptr::null_mut(), || {
        let doc = json::apply_steps(schema(schema_ptr)?, &parse(doc_json)?, &parse(steps_json)?)?;
        Ok(string(doc.to_json_string()))
    })
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_invert_steps(
    schema_ptr: *const TarnishSchema,
    doc_json: *const c_char,
    steps_json: *const c_char,
    error: *mut *mut c_char,
) -> *mut c_char {
    run(error, ptr::null_mut(), || {
        let steps =
            json::invert_steps(schema(schema_ptr)?, &parse(doc_json)?, &parse(steps_json)?)?;
        Ok(string(steps.to_json_string()))
    })
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_map_position(
    schema_ptr: *const TarnishSchema,
    steps_json: *const c_char,
    pos: usize,
    assoc: c_int,
    mapped: *mut usize,
    error: *mut *mut c_char,
) -> bool {
    run(error, false, || {
        if mapped.is_null() {
            return Err(Error::Other("The pointer to map into was NULL".into()));
        }
        let pos = json::map_position(schema(schema_ptr)?, &parse(steps_json)?, pos, assoc)?;
        unsafe { *mapped = pos };
        Ok(true)
    })
}

/// # Safety
///
/// See the [crate's](crate) contract for pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tarnish_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}
