//! Moving values across the boundary: attribute values, strings, errors, and the JavaScript
//! wrapper objects the bridge hands out in place of its handles.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr;
use std::sync::Arc;

use napi::bindgen_prelude::{FromNapiValue, ToNapiValue, TypeName, ValidateNapiValue};
use napi::{Env, Error, Result, Status, ValueType, sys};
use tarnish::{Object, Text, Value};

/// A JavaScript value, as napi hands it over and takes it back.
#[derive(Clone, Copy)]
pub struct Js(pub sys::napi_value);

impl ToNapiValue for Js {
    unsafe fn to_napi_value(_env: sys::napi_env, value: Self) -> Result<sys::napi_value> {
        Ok(value.0)
    }
}

impl FromNapiValue for Js {
    unsafe fn from_napi_value(_env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        Ok(Js(value))
    }
}

impl TypeName for Js {
    fn type_name() -> &'static str {
        "unknown"
    }

    fn value_type() -> ValueType {
        ValueType::Unknown
    }
}

impl ValidateNapiValue for Js {}

pub fn check(status: sys::napi_status) -> Result<()> {
    if status == sys::Status::napi_ok {
        return Ok(());
    }
    if status == sys::Status::napi_pending_exception {
        return Err(pending());
    }
    Err(Error::new(Status::from(status), "napi call failed"))
}

/// The error to return when a JavaScript exception is already pending, to let it through.
pub fn pending() -> Error {
    Error::new(Status::PendingException, String::new())
}

pub fn undefined(env: sys::napi_env) -> Result<sys::napi_value> {
    let mut value = ptr::null_mut();
    check(unsafe { sys::napi_get_undefined(env, &mut value) })?;
    Ok(value)
}

pub fn null(env: sys::napi_env) -> Result<sys::napi_value> {
    let mut value = ptr::null_mut();
    check(unsafe { sys::napi_get_null(env, &mut value) })?;
    Ok(value)
}

pub fn type_of(env: sys::napi_env, value: sys::napi_value) -> Result<sys::napi_valuetype> {
    let mut kind = 0;
    check(unsafe { sys::napi_typeof(env, value, &mut kind) })?;
    Ok(kind)
}

pub fn get(env: sys::napi_env, object: sys::napi_value, key: &str) -> Result<sys::napi_value> {
    let key = string(env, key)?;
    let mut value = ptr::null_mut();
    check(unsafe { sys::napi_get_property(env, object, key, &mut value) })?;
    Ok(value)
}

pub fn has_own(env: sys::napi_env, object: sys::napi_value, key: &str) -> Result<bool> {
    let key = string(env, key)?;
    let mut has = false;
    check(unsafe { sys::napi_has_own_property(env, object, key, &mut has) })?;
    Ok(has)
}

pub fn truthy(env: sys::napi_env, value: sys::napi_value) -> Result<bool> {
    let mut coerced = ptr::null_mut();
    check(unsafe { sys::napi_coerce_to_bool(env, value, &mut coerced) })?;
    let mut result = false;
    check(unsafe { sys::napi_get_value_bool(env, coerced, &mut result) })?;
    Ok(result)
}

/// `String(value)`.
pub fn coerce_to_string(env: sys::napi_env, value: sys::napi_value) -> Result<sys::napi_value> {
    let mut coerced = ptr::null_mut();
    check(unsafe { sys::napi_coerce_to_string(env, value, &mut coerced) })?;
    Ok(coerced)
}

pub fn is_false(env: sys::napi_env, value: sys::napi_value) -> Result<bool> {
    if type_of(env, value)? != sys::ValueType::napi_boolean {
        return Ok(false);
    }
    let mut result = false;
    check(unsafe { sys::napi_get_value_bool(env, value, &mut result) })?;
    Ok(!result)
}

/// A property that is a string, if it is one.
pub fn get_string(
    env: sys::napi_env,
    object: sys::napi_value,
    key: &str,
) -> Result<Option<String>> {
    let value = get(env, object, key)?;
    if type_of(env, value)? != sys::ValueType::napi_string {
        return Ok(None);
    }
    Ok(Some(unsafe { String::from_napi_value(env, value) }?))
}

pub fn string(env: sys::napi_env, text: &str) -> Result<sys::napi_value> {
    let mut value = ptr::null_mut();
    check(unsafe {
        sys::napi_create_string_utf8(env, text.as_ptr().cast(), text.len() as isize, &mut value)
    })?;
    Ok(value)
}

pub fn number(env: sys::napi_env, number: f64) -> Result<sys::napi_value> {
    let mut value = ptr::null_mut();
    check(unsafe { sys::napi_create_double(env, number, &mut value) })?;
    Ok(value)
}

pub fn text_to_js(env: sys::napi_env, text: &Text) -> Result<sys::napi_value> {
    let units = text.units();
    let mut value = ptr::null_mut();
    check(unsafe {
        sys::napi_create_string_utf16(env, units.as_ptr(), units.len() as isize, &mut value)
    })?;
    Ok(value)
}

pub fn text_from_js(env: sys::napi_env, value: sys::napi_value) -> Result<Text> {
    let mut length = 0;
    check(unsafe {
        sys::napi_get_value_string_utf16(env, value, ptr::null_mut(), 0, &mut length)
    })?;
    let mut units = vec![0u16; length + 1];
    check(unsafe {
        sys::napi_get_value_string_utf16(env, value, units.as_mut_ptr(), units.len(), &mut length)
    })?;
    units.truncate(length);
    Ok(Text::from(units))
}

/// A string as JavaScript holds it, with every UTF-16 unit.
pub struct JsText(pub Text);

impl FromNapiValue for JsText {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        Ok(JsText(text_from_js(env, value)?))
    }
}

impl ToNapiValue for JsText {
    unsafe fn to_napi_value(env: sys::napi_env, value: Self) -> Result<sys::napi_value> {
        text_to_js(env, &value.0)
    }
}

impl TypeName for JsText {
    fn type_name() -> &'static str {
        "string"
    }

    fn value_type() -> ValueType {
        ValueType::String
    }
}

impl ValidateNapiValue for JsText {}

/// An attribute value, or a document's JSON.
pub struct Data(pub Value);

impl FromNapiValue for Data {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        Ok(Data(value_from_js(env, value)?))
    }
}

impl ToNapiValue for Data {
    unsafe fn to_napi_value(env: sys::napi_env, value: Self) -> Result<sys::napi_value> {
        value_to_js(env, &value.0)
    }
}

impl TypeName for Data {
    fn type_name() -> &'static str {
        "unknown"
    }

    fn value_type() -> ValueType {
        ValueType::Unknown
    }
}

impl ValidateNapiValue for Data {}

pub fn value_from_js(env: sys::napi_env, value: sys::napi_value) -> Result<Value> {
    Ok(match type_of(env, value)? {
        sys::ValueType::napi_undefined => Value::Undefined,
        sys::ValueType::napi_null => Value::Null,
        sys::ValueType::napi_boolean => Value::Bool(unsafe { bool::from_napi_value(env, value) }?),
        sys::ValueType::napi_number => Value::Number(unsafe { f64::from_napi_value(env, value) }?),
        sys::ValueType::napi_string => {
            Value::String(unsafe { String::from_napi_value(env, value) }?.into())
        }
        sys::ValueType::napi_object => {
            let mut is_array = false;
            check(unsafe { sys::napi_is_array(env, value, &mut is_array) })?;
            if is_array {
                let mut length = 0;
                check(unsafe { sys::napi_get_array_length(env, value, &mut length) })?;
                let mut items = Vec::with_capacity(length as usize);
                for index in 0..length {
                    let mut item = ptr::null_mut();
                    check(unsafe { sys::napi_get_element(env, value, index, &mut item) })?;
                    items.push(value_from_js(env, item)?);
                }
                Value::Array(items.into())
            } else {
                let mut object = Object::new();
                for key in property_names(env, value)? {
                    let item = get(env, value, &key)?;
                    object.insert(key, value_from_js(env, item)?);
                }
                Value::Object(Arc::new(object))
            }
        }
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                "tarnish holds attribute values that JSON can, and undefined",
            ));
        }
    })
}

/// The names of an object's enumerable string properties, as `for...in` visits them.
pub fn property_names(env: sys::napi_env, object: sys::napi_value) -> Result<Vec<String>> {
    let mut names = ptr::null_mut();
    check(unsafe { sys::napi_get_property_names(env, object, &mut names) })?;
    let mut length = 0;
    check(unsafe { sys::napi_get_array_length(env, names, &mut length) })?;
    let mut result = Vec::with_capacity(length as usize);
    for index in 0..length {
        let mut name = ptr::null_mut();
        check(unsafe { sys::napi_get_element(env, names, index, &mut name) })?;
        result.push(unsafe { String::from_napi_value(env, name) }?);
    }
    Ok(result)
}

/// Keep the tarnish array a JavaScript array was made from on it, so that a DOM spec that is
/// an attribute's array can be told from one that equals it, as ProseMirror tells them apart.
fn mark_origin(env: sys::napi_env, array: sys::napi_value, items: &Arc<[Value]>) -> Result<()> {
    unsafe extern "C" fn release(
        _env: sys::napi_env,
        data: *mut std::ffi::c_void,
        _hint: *mut std::ffi::c_void,
    ) {
        drop(unsafe { Box::from_raw(data as *mut Arc<[Value]>) });
    }
    let data = Box::into_raw(Box::new(items.clone()));
    let status = unsafe {
        sys::napi_wrap(
            env,
            array,
            data.cast(),
            Some(release),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status != sys::Status::napi_ok {
        drop(unsafe { Box::from_raw(data) });
    }
    check(status)
}

/// The identity of the tarnish array a JavaScript array was made from, if it was.
pub fn array_origin(env: sys::napi_env, array: sys::napi_value) -> Option<usize> {
    let mut data = ptr::null_mut();
    let status = unsafe { sys::napi_unwrap(env, array, &mut data) };
    (status == sys::Status::napi_ok && !data.is_null())
        .then(|| unsafe { (*(data as *const Arc<[Value]>)).as_ptr() } as usize)
}

pub fn value_to_js(env: sys::napi_env, value: &Value) -> Result<sys::napi_value> {
    let mut result = ptr::null_mut();
    match value {
        Value::Undefined => return undefined(env),
        Value::Null => return null(env),
        Value::Bool(value) => check(unsafe { sys::napi_get_boolean(env, *value, &mut result) })?,
        Value::Number(value) => return number(env, *value),
        Value::String(value) => return string(env, value),
        Value::Array(items) => {
            check(unsafe { sys::napi_create_array_with_length(env, items.len(), &mut result) })?;
            for (index, item) in items.iter().enumerate() {
                let item = value_to_js(env, item)?;
                check(unsafe { sys::napi_set_element(env, result, index as u32, item) })?;
            }
            mark_origin(env, result, items)?;
        }
        Value::Object(object) => {
            check(unsafe { sys::napi_create_object(env, &mut result) })?;
            for (key, item) in object.iter() {
                let (key, item) = (string(env, key)?, value_to_js(env, item)?);
                check(unsafe { sys::napi_set_property(env, result, key, item) })?;
            }
        }
    }
    Ok(result)
}

/// A reference that keeps a JavaScript value alive while Rust holds it.
pub struct JsRef {
    env: sys::napi_env,
    raw: sys::napi_ref,
}

// Refs are only made, used and dropped on the JavaScript thread: the bridge never hands tarnish's
// values to another thread.
unsafe impl Send for JsRef {}
unsafe impl Sync for JsRef {}

impl JsRef {
    pub fn new(env: sys::napi_env, value: sys::napi_value) -> Result<JsRef> {
        let mut raw = ptr::null_mut();
        check(unsafe { sys::napi_create_reference(env, value, 1, &mut raw) })?;
        Ok(JsRef { env, raw })
    }

    pub fn env(&self) -> sys::napi_env {
        self.env
    }

    pub fn value(&self) -> Result<sys::napi_value> {
        let mut value = ptr::null_mut();
        check(unsafe { sys::napi_get_reference_value(self.env, self.raw, &mut value) })?;
        Ok(value)
    }
}

impl Drop for JsRef {
    fn drop(&mut self) {
        unsafe { sys::napi_delete_reference(self.env, self.raw) };
    }
}

/// A JavaScript function kept to be called later, as a method of `this` when there is one.
pub struct Hook {
    this: Option<JsRef>,
    function: JsRef,
}

impl Hook {
    /// The object's function `key`, if it has one, as its method.
    pub fn method(
        env: sys::napi_env,
        object: sys::napi_value,
        key: &str,
    ) -> Result<Option<Arc<Hook>>> {
        let function = get(env, object, key)?;
        if type_of(env, function)? != sys::ValueType::napi_function {
            return Ok(None);
        }
        Ok(Some(Arc::new(Hook {
            this: Some(JsRef::new(env, object)?),
            function: JsRef::new(env, function)?,
        })))
    }

    /// A function to call with no `this`.
    pub fn function(env: sys::napi_env, function: sys::napi_value) -> Result<Arc<Hook>> {
        Ok(Arc::new(Hook {
            this: None,
            function: JsRef::new(env, function)?,
        }))
    }

    /// Call the function with the arguments `args` makes, a failure being the host's error.
    pub fn call(
        &self,
        args: impl FnOnce(sys::napi_env) -> Result<Vec<sys::napi_value>>,
    ) -> tarnish::Result<sys::napi_value> {
        self.read(|env| {
            let args = args(env)?;
            let this = match &self.this {
                Some(this) => this.value()?,
                None => undefined(env)?,
            };
            call(env, this, self.function.value()?, &args)
        })
    }

    /// Read a value in the function's environment, a failure being the host's error.
    pub fn read<T>(&self, read: impl FnOnce(sys::napi_env) -> Result<T>) -> tarnish::Result<T> {
        let env = self.function.env();
        read(env).map_err(|error| host_error(env, error))
    }
}

/// Call `function` with `this` and `args`, leaving an exception it throws pending.
pub fn call(
    env: sys::napi_env,
    this: sys::napi_value,
    function: sys::napi_value,
    args: &[sys::napi_value],
) -> Result<sys::napi_value> {
    let mut result = ptr::null_mut();
    check(unsafe {
        sys::napi_call_function(env, this, function, args.len(), args.as_ptr(), &mut result)
    })?;
    Ok(result)
}

/// The JavaScript side's functions the bridge calls: those that turn handles into the wrapper
/// objects JavaScript code sees, and the error classes.
pub struct Registry {
    functions: HashMap<String, JsRef>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry { functions: HashMap::new() });
}

/// Keep each of the object's functions, by name, for [`call_registered`].
pub fn register(env: sys::napi_env, functions: sys::napi_value) -> Result<()> {
    for name in property_names(env, functions)? {
        let function = JsRef::new(env, get(env, functions, &name)?)?;
        REGISTRY.with(|registry| registry.borrow_mut().functions.insert(name, function));
    }
    Ok(())
}

fn registered(env: sys::napi_env, name: &str) -> Result<sys::napi_value> {
    REGISTRY.with(|registry| match registry.borrow().functions.get(name) {
        Some(function) => function.value(),
        None => {
            let _ = env;
            Err(Error::new(
                Status::GenericFailure,
                format!("{name} isn't registered"),
            ))
        }
    })
}

pub fn call_registered(
    env: sys::napi_env,
    name: &str,
    args: &[sys::napi_value],
) -> Result<sys::napi_value> {
    let function = registered(env, name)?;
    call(env, undefined(env)?, function, args)
}

/// Throw `error` as the class ProseMirror throws it as, and return the error that lets it
/// through napi.
pub fn throw(env: sys::napi_env, error: tarnish::Error) -> Error {
    let (class, message) = match &error {
        tarnish::Error::Host(_) => return pending(),
        tarnish::Error::Range(message) => ("RangeError", message),
        tarnish::Error::Syntax(message) => ("SyntaxError", message),
        tarnish::Error::Replace(message) => ("ReplaceError", message),
        tarnish::Error::Transform(message) => ("TransformError", message),
        tarnish::Error::Other(message) => ("Error", message),
    };
    let thrown = (|| {
        let class = registered(env, class)?;
        let message = string(env, message)?;
        let mut instance = ptr::null_mut();
        check(unsafe { sys::napi_new_instance(env, class, 1, [message].as_ptr(), &mut instance) })?;
        check(unsafe { sys::napi_throw(env, instance) })
    })();
    match thrown {
        Ok(()) => pending(),
        Err(error) => error,
    }
}

/// A hook's failure, as tarnish carries it: a JavaScript exception, left pending for the
/// bridge's entry point to let through.
pub fn host_error(env: sys::napi_env, error: Error) -> tarnish::Error {
    if error.status != Status::PendingException {
        unsafe { napi::JsError::from(error).throw_into(env) };
    }
    tarnish::Error::Host(Arc::new(()))
}

pub trait OrThrow<T> {
    fn or_throw(self, env: &Env) -> Result<T>;
}

impl<T> OrThrow<T> for tarnish::Result<T> {
    fn or_throw(self, env: &Env) -> Result<T> {
        self.map_err(|error| throw(env.raw(), error))
    }
}
