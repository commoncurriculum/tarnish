//! Moving values across the boundary: attribute values, strings, positions, errors, and calls
//! back into JavaScript.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use napi::bindgen_prelude::{
    FromNapiValue, Function, FunctionRef, JsObjectValue, JsValuesTupleIntoVec, Null, Object,
    ToNapiValue, Unknown, Utf16String,
};
use napi::{Env, Error, JsString, JsValue, Result, Status, ValueType};
use tarnish::{Attrs, Map, Text, Value};

thread_local! {
    static ENV: Cell<Option<Env>> = const { Cell::new(None) };
    static REGISTRY: RefCell<HashMap<String, Hook>> = RefCell::new(HashMap::new());
}

/// The environment of this thread's JavaScript, for what runs outside a call's own arguments:
/// the hooks tarnish calls, and the references the bridge lets go of.
pub fn env() -> Result<Env> {
    ENV.get()
        .ok_or_else(|| Error::from_reason("The bridge isn't registered"))
}

/// Keep each of the object's functions by name: the JavaScript side's wrapping functions and
/// error classes.
pub fn register(env: Env, functions: Object) -> Result<()> {
    ENV.set(Some(env));
    for name in Object::keys(&functions)? {
        let function = Hook::of(get(&functions, &name)?)?
            .ok_or_else(|| Error::from_reason(format!("{name} isn't a function")))?;
        REGISTRY.with_borrow_mut(|registry| registry.insert(name, function));
    }
    Ok(())
}

fn registered<'env>(env: &'env Env, name: &str) -> Result<Unknown<'env>> {
    REGISTRY.with_borrow(|registry| match registry.get(name) {
        Some(function) => function.value(env),
        None => Err(Error::from_reason(format!("{name} isn't registered"))),
    })
}

pub fn call_registered<'env, Args: JsValuesTupleIntoVec>(
    env: &'env Env,
    name: &str,
    args: Args,
) -> Result<Unknown<'env>> {
    call(registered(env, name)?, args)
}

/// Call `function` with no `this`, leaving an exception it throws pending.
pub fn call<'env, Args: JsValuesTupleIntoVec>(
    function: Unknown<'env>,
    args: Args,
) -> Result<Unknown<'env>> {
    Function::<Args, Unknown<'env>>::from_unknown(function)?.apply((), args)
}

pub fn call_method<'env, Args: JsValuesTupleIntoVec>(
    object: &Object<'env>,
    name: &str,
    args: Args,
) -> Result<Unknown<'env>> {
    let method: Function<Args, Unknown<'env>> = object.get_named_property_unchecked(name)?;
    method.apply(object, args)
}

/// A JavaScript function kept to call later: one the JavaScript side registered, or a hook
/// tarnish holds.
pub struct Hook(FunctionRef<Unknown<'static>, Unknown<'static>>);

impl Hook {
    /// The value, if it is a function.
    pub fn of(value: Unknown) -> Result<Option<Hook>> {
        if value.get_type()? != ValueType::Function {
            return Ok(None);
        }
        Ok(Some(Hook(FunctionRef::from_unknown(value)?)))
    }

    /// The object's function `key`, if it has one, as its method.
    pub fn method(object: &Object, key: &str) -> Result<Option<Hook>> {
        let value = get(object, key)?;
        if value.get_type()? != ValueType::Function {
            return Ok(None);
        }
        let function = Function::<Unknown<'static>, Unknown<'static>>::from_unknown(value)?;
        Ok(Some(Hook(function.bind(object)?.create_ref()?)))
    }

    fn value<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        Ok(self.0.borrow_back(env)?.to_unknown())
    }

    pub fn call<'env, Args: JsValuesTupleIntoVec>(
        &self,
        env: &'env Env,
        args: Args,
    ) -> Result<Unknown<'env>> {
        call(self.value(env)?, args)
    }
}

/// Reach into JavaScript for tarnish, from a hook or the host's DOM. A failure is the host's
/// error, with what JavaScript threw left pending for the bridge's entry point to let through.
pub fn host<T>(f: impl FnOnce(&Env) -> Result<T>) -> tarnish::Result<T> {
    let env = env().map_err(|error| tarnish::Error::Other(error.reason))?;
    f(&env).map_err(|error| {
        if error.status != Status::PendingException {
            // Throwing fails when an exception is already pending, and that one goes on instead.
            let _ = env.throw(error);
        }
        tarnish::Error::Host
    })
}

/// The error to return when a JavaScript exception is pending, to let it through.
fn pending() -> Error {
    Error::new(Status::PendingException, String::new())
}

/// Throw `error` as the class ProseMirror throws it as, and return the error that lets it
/// through napi.
pub fn throw(env: &Env, error: tarnish::Error) -> Error {
    if let tarnish::Error::Host = error {
        return pending();
    }
    let thrown = registered(env, error.class())
        .and_then(|class| {
            Function::<&str, Unknown>::from_unknown(class)?.new_instance(error.message())
        })
        .and_then(|instance| env.throw(instance));
    match thrown {
        Ok(()) => pending(),
        Err(error) => error,
    }
}

pub trait OrThrow<T> {
    fn or_throw(self, env: &Env) -> Result<T>;
}

impl<T> OrThrow<T> for tarnish::Result<T> {
    fn or_throw(self, env: &Env) -> Result<T> {
        self.map_err(|error| throw(env, error))
    }
}

/// A document position from JavaScript. One that is negative or not whole can't be in a
/// document, and throws the RangeError a position out of range does.
pub fn pos(env: &Env, pos: f64) -> Result<usize> {
    if pos >= 0.0 && pos.fract() == 0.0 {
        return Ok(pos as usize);
    }
    let pos = tarnish::js::number_to_string(pos);
    Err(throw(
        env,
        tarnish::Error::Range(format!("Position {pos} out of range")),
    ))
}

pub fn get<'env>(object: &Object<'env>, key: &str) -> Result<Unknown<'env>> {
    object.get_named_property_unchecked(key)
}

pub fn is_nullish(value: &Unknown) -> Result<bool> {
    Ok(matches!(
        value.get_type()?,
        ValueType::Undefined | ValueType::Null
    ))
}

pub fn is_false(value: &Unknown) -> Result<bool> {
    Ok(value.get_type()? == ValueType::Boolean && !bool::from_unknown(*value)?)
}

/// A property that is a string, if it is one.
pub fn get_string(object: &Object, key: &str) -> Result<Option<String>> {
    let value = get(object, key)?;
    match value.get_type()? {
        ValueType::String => String::from_unknown(value).map(Some),
        _ => Ok(None),
    }
}

/// `String(value)`.
pub fn coerce_to_string(value: &Unknown) -> Result<String> {
    String::from_unknown(value.coerce_to_string()?.to_unknown())
}

pub fn text_to_js<'env>(env: &'env Env, text: &Text) -> Result<JsString<'env>> {
    match text.as_str() {
        Some(text) => env.create_string(text),
        None => env.create_string_utf16(text.units()),
    }
}

/// A string as JavaScript holds it, with every UTF-16 unit.
pub fn text_from_js(value: Unknown) -> Result<Text> {
    Ok(Text::from_units(&Utf16String::from_unknown(value)?))
}

/// A JavaScript value as JSON holds it, `None` for `undefined`: inside it, an object's property
/// that is `undefined` is left out, and an array's item is `null`, as `JSON.stringify` has them.
pub fn value_from_js(value: Unknown) -> Result<Option<Value>> {
    Ok(Some(match value.get_type()? {
        ValueType::Undefined => return Ok(None),
        ValueType::Null => Value::Null,
        ValueType::Boolean => Value::Bool(bool::from_unknown(value)?),
        ValueType::Number => tarnish::js::number(f64::from_unknown(value)?),
        ValueType::String => Value::String(String::from_unknown(value)?),
        ValueType::Object if value.is_array()? => Value::Array(
            Vec::<Unknown>::from_unknown(value)?
                .into_iter()
                .map(|item| Ok(value_from_js(item)?.unwrap_or(Value::Null)))
                .collect::<Result<_>>()?,
        ),
        ValueType::Object => {
            let object = Object::from_unknown(value)?;
            let mut map = Map::new();
            for key in Object::keys(&object)? {
                if let Some(item) = value_from_js(get(&object, &key)?)? {
                    map.insert(key.into(), item);
                }
            }
            Value::Object(map)
        }
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                "tarnish holds attribute values that JSON can, and undefined",
            ));
        }
    }))
}

/// A document's JSON, or another value that functions read with `undefined` as `null`.
pub fn json_from_js(value: Unknown) -> Result<Value> {
    Ok(value_from_js(value)?.unwrap_or(Value::Null))
}

/// Attributes as a type's `create` reads them from a value: `null` and `undefined` are none,
/// and a value that isn't an object has no properties.
pub fn attrs_from_js(value: Unknown) -> Result<Option<Attrs>> {
    let value = value_from_js(value)?;
    Ok(tarnish::js::attrs(value.as_ref()).map(|attrs| Arc::new(attrs.clone())))
}

pub fn value_to_js<'env>(env: &'env Env, value: &Value) -> Result<Unknown<'env>> {
    to_js(env, value, None)
}

/// [`value_to_js`], `None` being `undefined`.
pub fn optional_to_js<'env>(env: &'env Env, value: Option<&Value>) -> Result<Unknown<'env>> {
    match value {
        Some(value) => value_to_js(env, value),
        None => ().into_unknown(env),
    }
}

/// Where a JavaScript array made from an attribute's array came from: the attributes, kept
/// alive so that the array's address stays theirs, and that address.
struct Origin {
    _attrs: Attrs,
    address: usize,
}

/// The address of the attribute array a JavaScript array was made from, if it was, so that a
/// DOM spec that is an attribute's array can be told from one that equals it, as ProseMirror
/// tells them apart.
pub fn array_origin(array: Unknown) -> Option<usize> {
    let array = Object::from_unknown(array).ok()?;
    array.unwrap::<Origin>().ok().map(|origin| origin.address)
}

/// A node's or mark's attributes, each array in them tagged with where it came from.
pub fn attrs_to_js<'env>(env: &'env Env, attrs: &Attrs) -> Result<Unknown<'env>> {
    let mut object = Object::new(env)?;
    for (key, item) in attrs.iter() {
        object.set(key, to_js(env, item, Some(attrs))?)?;
    }
    Ok(object.to_unknown())
}

fn to_js<'env>(env: &'env Env, value: &Value, attrs: Option<&Attrs>) -> Result<Unknown<'env>> {
    match value {
        Value::Null => Null.into_unknown(env),
        Value::Bool(value) => value.into_unknown(env),
        Value::Number(value) => value.as_f64().unwrap_or(f64::NAN).into_unknown(env),
        Value::String(value) => value.as_str().into_unknown(env),
        Value::Array(items) => {
            let mut array = env.create_array(items.len() as u32)?;
            for (index, item) in items.iter().enumerate() {
                array.set(index as u32, to_js(env, item, attrs)?)?;
            }
            if let Some(attrs) = attrs {
                let origin = Origin {
                    _attrs: attrs.clone(),
                    address: items.as_ptr() as usize,
                };
                array.wrap(origin, None)?;
            }
            Ok(array.to_unknown())
        }
        Value::Object(map) => {
            let mut object = Object::new(env)?;
            for (key, item) in map.iter() {
                object.set(key, to_js(env, item, attrs)?)?;
            }
            Ok(object.to_unknown())
        }
    }
}
