//! Elixir terms to tarnish's JSON values and back, as Jason encodes and decodes JSON: a term
//! reads as Jason would encode it, and a value writes as Jason would decode the JSON
//! `JSON.stringify` writes for it.

mod read;
mod write;

use rustler::{Error, NifResult, Term};
use tarnish::Value;

pub use write::write;

/// The value of a term Jason could encode, or `ArgumentError` for one it couldn't.
pub fn read(term: Term) -> NifResult<Value> {
    read::Reader::new(term.get_env())
        .to_value(term)
        .map_err(|read::NotJson| Error::BadArg)
}
