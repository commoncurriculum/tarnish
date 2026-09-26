//! Terms in and out through Erlang's external term format, in which the VM reads out a term, or
//! makes one, in a single call: a term reads as Jason would encode it, and a value writes as
//! Jason would decode the JSON `JSON.stringify` writes for it.

use rustler::{Env, Error, NifResult, Term};
use tarnish::{Value, etf};

/// The value of a term Jason could encode, or `ArgumentError` for one it couldn't.
pub fn read(term: Term) -> NifResult<Value> {
    etf::read(term.to_binary().as_slice()).map_err(|etf::NotJson| Error::BadArg)
}

/// The term the external format holds.
pub fn make<'a>(env: Env<'a>, bytes: &[u8]) -> Term<'a> {
    env.binary_to_term(bytes)
        .expect("the external format of a value")
        .0
}
