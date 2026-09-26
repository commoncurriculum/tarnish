//! The Elixir binding of tarnish: documents, steps and specs in and out as terms, the way Jason
//! reads them from ProseMirror's JSON, and schemas as resources built once.

mod term;

use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use tarnish::{Error, Schema, json};

rustler::atoms! {
    ok,
    error,
    range_error,
    syntax_error,
    replace_error,
    transform_error,
    js_error,
}

pub struct SchemaResource(Schema);

#[rustler::resource_impl]
impl Resource for SchemaResource {}

/// `{:error, {kind, message}}`, the kind naming the class ProseMirror throws.
fn failure<'a>(env: Env<'a>, failed: Error) -> Term<'a> {
    let kind = match &failed {
        Error::Range(_) => range_error(),
        Error::Syntax(_) => syntax_error(),
        Error::Replace(_) => replace_error(),
        Error::Transform(_) => transform_error(),
        Error::Other(_) | Error::Host(_) => js_error(),
    };
    (error(), (kind, failed.message())).encode(env)
}

/// `{:ok, term}`, or the failure.
fn respond<'a, T>(
    env: Env<'a>,
    result: tarnish::Result<T>,
    write: impl FnOnce(T) -> NifResult<Term<'a>>,
) -> NifResult<Term<'a>> {
    match result {
        Ok(value) => Ok((ok(), write(value)?).encode(env)),
        Err(failed) => Ok(failure(env, failed)),
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema<'a>(env: Env<'a>, spec: Term<'a>) -> NifResult<Term<'a>> {
    respond(env, json::schema(&term::read(spec)?), |schema| {
        Ok(ResourceArc::new(SchemaResource(schema)).encode(env))
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
) -> NifResult<Term<'a>> {
    Ok(match json::check(&schema.0, &term::read(doc)?) {
        Ok(()) => ok().encode(env),
        Err(failed) => failure(env, failed),
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    let applied = json::apply_steps(&schema.0, &term::read(doc)?, &term::read(steps)?);
    respond(env, applied, |doc| term::write(env, &doc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    let inverted = json::invert_steps(&schema.0, &term::read(doc)?, &term::read(steps)?);
    respond(env, inverted, |steps| term::write(env, &steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn map_position<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    let mapped = json::map_position(&schema.0, &term::read(steps)?, pos, assoc);
    respond(env, mapped, |pos| Ok(pos.encode(env)))
}

rustler::init!("Elixir.Tarnish.Native");
