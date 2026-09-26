//! The Elixir binding of tarnish: specs, steps and JSON in and out as terms, the way Jason reads
//! them from ProseMirror's JSON, and schemas and documents as resources, built once.

#![forbid(unsafe_code)]

mod term;

use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use tarnish::{Error, Node, Schema, api, etf};

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
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub struct SchemaResource(Schema);

#[rustler::resource_impl]
impl Resource for SchemaResource {}

pub struct NodeResource(Node);

#[rustler::resource_impl]
impl Resource for NodeResource {}

/// `{:error, {kind, message}}`, the kind naming the class ProseMirror throws.
fn failure(env: Env, failed: Error) -> Term {
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

fn node(env: Env, node: Node) -> Term {
    ResourceArc::new(NodeResource(node)).encode(env)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema<'a>(env: Env<'a>, spec: Term<'a>) -> NifResult<Term<'a>> {
    let schema = api::schema(&term::read(spec)?);
    Ok(respond(env, schema, |schema| {
        ResourceArc::new(SchemaResource(schema)).encode(env)
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn node_from_json<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    json: Term<'a>,
) -> NifResult<Term<'a>> {
    let bytes = json.to_binary();
    let term =
        etf::Document::new(bytes.as_slice()).map_err(|etf::NotJson| rustler::Error::BadArg)?;
    let doc = Node::from_json(&schema.0, term.root());
    Ok(respond(env, doc, |doc| node(env, doc)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn to_json(env: Env, doc: ResourceArc<NodeResource>) -> Term {
    term::make(env, &etf::write_node(&doc.0))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check(env: Env, doc: ResourceArc<NodeResource>) -> Term {
    match doc.0.check() {
        Ok(()) => ok().encode(env),
        Err(failed) => failure(env, failed),
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    let applied = api::apply_steps(&doc.0, &term::read(steps)?);
    Ok(respond(env, applied, |doc| node(env, doc)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    let inverted = api::invert_steps(&doc.0, &term::read(steps)?);
    Ok(respond(env, inverted, |steps| {
        term::make(env, &etf::write(&steps))
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn map_position<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    let mapped = api::map_position(&schema.0, &term::read(steps)?, pos, assoc);
    Ok(respond(env, mapped, |pos| pos.encode(env)))
}

rustler::init!("Elixir.Tarnish.Native");
