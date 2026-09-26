//! The Elixir binding of tarnish: documents, steps and specs in and out as terms, the way Jason
//! reads them from ProseMirror's JSON, and schemas as resources built once. Each call runs on a
//! stack of its own, since a dirty scheduler's is too small for a deeply nested document.

mod term;

use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use tarnish::{Error, Schema, api, stack};

rustler::atoms! {
    ok,
    error,
    range_error,
    syntax_error,
    replace_error,
    transform_error,
    js_error,
}

#[global_allocator]
static GLOBAL: tarnish::allocator::MiMalloc = tarnish::allocator::MiMalloc;

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
        Error::Other(_) | Error::Host => js_error(),
    };
    (error(), (kind, failed.message())).encode(env)
}

/// `{:ok, term}`, or the failure.
fn respond<'a, T>(
    env: Env<'a>,
    result: tarnish::Result<T>,
    write: impl FnOnce(T) -> Term<'a>,
) -> Term<'a> {
    match result {
        Ok(value) => (ok(), write(value)).encode(env),
        Err(failed) => failure(env, failed),
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema<'a>(env: Env<'a>, spec: Term<'a>) -> NifResult<Term<'a>> {
    stack::run(|| {
        let schema = api::schema(&term::read(spec)?);
        Ok(respond(env, schema, |schema| {
            ResourceArc::new(SchemaResource(schema)).encode(env)
        }))
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
) -> NifResult<Term<'a>> {
    stack::run(|| {
        Ok(match api::check(&schema.0, &term::read(doc)?) {
            Ok(()) => ok().encode(env),
            Err(failed) => failure(env, failed),
        })
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    stack::run(|| {
        let applied = api::apply_steps(&schema.0, &term::read(doc)?, &term::read(steps)?);
        Ok(respond(env, applied, |doc| term::write(env, &doc)))
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    doc: Term<'a>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    stack::run(|| {
        let inverted = api::invert_steps(&schema.0, &term::read(doc)?, &term::read(steps)?);
        Ok(respond(env, inverted, |steps| term::write(env, &steps)))
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn map_position<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    stack::run(|| {
        let mapped = api::map_position(&schema.0, &term::read(steps)?, pos, assoc);
        Ok(respond(env, mapped, |pos| pos.encode(env)))
    })
}

rustler::init!("Elixir.Tarnish.Native");
