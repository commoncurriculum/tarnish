//! Terms to tarnish's values and back, as Jason reads and writes JSON: maps with string or atom
//! keys, lists, strings, numbers, `true`, `false` and `nil`. A list of `{key, value}` pairs
//! reads as an object that keeps its keys' order, which a schema's types need.

use std::sync::Arc;

use rustler::types::tuple::get_tuple;
use rustler::{Binary, Encoder, Env, Error, MapIterator, NifResult, Term, TermType};
use tarnish::{Object, Value};

fn key(term: Term) -> NifResult<Arc<str>> {
    match term.get_type() {
        TermType::Binary => Ok(text(term)?.into()),
        TermType::Atom => Ok(term.atom_to_string()?.into()),
        _ => Err(Error::BadArg),
    }
}

fn text(term: Term<'_>) -> NifResult<&str> {
    let binary: Binary = term.decode()?;
    std::str::from_utf8(binary.as_slice()).map_err(|_| Error::BadArg)
}

/// The `{key, value}` pair an item of an ordered object is.
fn pair(term: Term) -> Option<(Term, Term)> {
    if term.get_type() != TermType::Tuple {
        return None;
    }
    match get_tuple(term).ok()?.as_slice() {
        [key, value] if matches!(key.get_type(), TermType::Binary | TermType::Atom) => {
            Some((*key, *value))
        }
        _ => None,
    }
}

pub fn read(term: Term) -> NifResult<Value> {
    Ok(match term.get_type() {
        TermType::Map => {
            let mut object = Object::with_capacity(term.map_size()?);
            for (name, item) in MapIterator::new(term).ok_or(Error::BadArg)? {
                object.insert(key(name)?, read(item)?);
            }
            Value::Object(Arc::new(object))
        }
        TermType::List => {
            let items: Vec<Term> = term.decode()?;
            let pairs: Option<Vec<(Term, Term)>> = items.iter().map(|item| pair(*item)).collect();
            match pairs {
                Some(pairs) if !pairs.is_empty() => {
                    let mut object = Object::with_capacity(pairs.len());
                    for (name, item) in pairs {
                        object.insert(key(name)?, read(item)?);
                    }
                    Value::Object(Arc::new(object))
                }
                _ => Value::Array(
                    items
                        .into_iter()
                        .map(read)
                        .collect::<NifResult<Vec<_>>>()?
                        .into(),
                ),
            }
        }
        TermType::Binary => Value::String(text(term)?.into()),
        TermType::Integer => Value::Number(term.decode::<i64>()? as f64),
        TermType::Float => Value::Number(term.decode::<f64>()?),
        TermType::Atom => match term.atom_to_string()?.as_str() {
            "nil" => Value::Null,
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            other => Value::String(other.into()),
        },
        _ => return Err(Error::BadArg),
    })
}

/// The largest integer a double holds exactly, below which a whole number writes as an
/// integer, as Jason reads one from JSON.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub fn write<'a>(env: Env<'a>, value: &Value) -> NifResult<Term<'a>> {
    Ok(match value {
        Value::Undefined | Value::Null => rustler::types::atom::nil().encode(env),
        Value::Bool(value) => value.encode(env),
        Value::Number(number) if number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER => {
            (*number as i64).encode(env)
        }
        Value::Number(number) => number.encode(env),
        Value::String(text) => (**text).encode(env),
        Value::Array(items) => items
            .iter()
            .map(|item| write(env, item))
            .collect::<NifResult<Vec<Term>>>()?
            .encode(env),
        Value::Object(object) => {
            let mut keys = Vec::with_capacity(object.len());
            let mut values = Vec::with_capacity(object.len());
            for (name, item) in object.iter() {
                // JSON leaves out properties that are undefined.
                if matches!(item, Value::Undefined) {
                    continue;
                }
                keys.push((**name).encode(env));
                values.push(write(env, item)?);
            }
            Term::map_from_term_arrays(env, &keys, &values)?
        }
    })
}
