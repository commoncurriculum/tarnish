//! The Elixir binding of tarnish: specs, steps and JSON in and out as terms, the way Jason reads
//! them from ProseMirror's JSON, and schemas and documents as resources, built once.
//!
//! A dirty scheduler takes a while to hand a call to, as long as a small document takes to read,
//! and has the call's terms to fetch into another core's cache. So each call runs on a normal
//! scheduler, where it may take up to about a millisecond, and a call with more work than that
//! answers `:dirty`, for the Elixir side to make it again on a dirty scheduler.

#![forbid(unsafe_code)]

mod etf;
mod share;
mod term;

use std::sync::Arc;

use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use share::Source;
use tarnish::{Error, Node, Schema, Value, api};
use term::Unread;

rustler::atoms! {
    ok,
    error,
    dirty,
    range_error,
    syntax_error,
    replace_error,
    transform_error,
    type_error,
    js_error,
}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub struct SchemaResource(Schema);

#[rustler::resource_impl]
impl Resource for SchemaResource {}

/// A document, and the one read from a map that it is, or that steps made it from.
pub struct NodeResource {
    node: Node,
    source: Arc<Source>,
}

#[rustler::resource_impl]
impl Resource for NodeResource {}

/// How much a call takes on, each limit about a millisecond's work on a normal scheduler.
struct Limits {
    /// Maps and lists read, with text counting by its length.
    read: usize,
    /// Bytes of a term written, which the VM then makes the term from.
    write: usize,
    /// The positions of a document checked.
    check: usize,
    /// Steps applied, times the top-level nodes of the document, which a step copies.
    apply: usize,
}

const NORMAL: Limits = Limits {
    read: 3_000,
    write: 300_000,
    check: 500_000,
    apply: 20_000,
};

const DIRTY: Limits = Limits {
    read: usize::MAX,
    write: usize::MAX,
    check: usize::MAX,
    apply: usize::MAX,
};

fn within(work: usize, limit: usize) -> Result<(), Unanswered> {
    match work > limit {
        true => Err(Unanswered::Dirty),
        false => Ok(()),
    }
}

/// Why a call has no answer of its own.
enum Unanswered {
    /// It has too much work for a normal scheduler.
    Dirty,
    /// A term Jason couldn't encode, which raises `ArgumentError`.
    BadArg,
}

impl From<Unread> for Unanswered {
    fn from(unread: Unread) -> Unanswered {
        match unread {
            Unread::TooBig => Unanswered::Dirty,
            Unread::NotJson => Unanswered::BadArg,
        }
    }
}

type Answer<'a> = Result<Term<'a>, Unanswered>;

fn answer<'a>(env: Env<'a>, answer: Answer<'a>) -> NifResult<Term<'a>> {
    match answer {
        Ok(term) => Ok(term),
        Err(Unanswered::Dirty) => Ok(dirty().encode(env)),
        Err(Unanswered::BadArg) => Err(rustler::Error::BadArg),
    }
}

/// `{:error, {kind, message}}`, the kind naming the class ProseMirror throws.
fn failure(env: Env, failed: Error) -> Term {
    let kind = match &failed {
        Error::Range(_) => range_error(),
        Error::Syntax(_) => syntax_error(),
        Error::Replace(_) => replace_error(),
        Error::Transform(_) => transform_error(),
        Error::Type(_) => type_error(),
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

fn document(env: Env, node: Node, source: Arc<Source>) -> Term {
    ResourceArc::new(NodeResource { node, source }).encode(env)
}

/// The steps, when there are few enough for the limits to apply to `doc`.
fn steps(limits: &Limits, doc: &Node, steps: Term) -> Result<Value, Unanswered> {
    let steps = term::read(steps, limits.read)?;
    let count = steps.as_array().map_or(0, Vec::len);
    within(count * (doc.child_count() + 1), limits.apply)?;
    Ok(steps)
}

/// The term of the bytes [`etf`] wrote, when they're few enough.
fn make<'a>(env: Env<'a>, bytes: Option<Vec<u8>>) -> Answer<'a> {
    let bytes = bytes.ok_or(Unanswered::Dirty)?;
    Ok(term::make(env, &bytes))
}

fn read_node<'a>(env: Env<'a>, limits: &Limits, schema: &Schema, json: Term<'a>) -> Answer<'a> {
    let (doc, irregular) =
        term::read_json(json, limits.read, |json| Node::from_json(schema, json))?;
    Ok(respond(env, doc, |doc| {
        let source = Source {
            doc: doc.clone(),
            irregular,
        };
        document(env, doc, Arc::new(source))
    }))
}

fn write_node<'a>(env: Env<'a>, limits: &Limits, doc: &NodeResource, json: Term<'a>) -> Answer<'a> {
    share::json(env, &doc.node, &doc.source, json, limits.write)
        .map_err(|share::TooBig| Unanswered::Dirty)
}

fn check_node<'a>(env: Env<'a>, limits: &Limits, doc: &Node) -> Answer<'a> {
    within(doc.node_size(), limits.check)?;
    Ok(match doc.check() {
        Ok(()) => ok().encode(env),
        Err(failed) => failure(env, failed),
    })
}

fn apply<'a>(env: Env<'a>, limits: &Limits, doc: &NodeResource, json: Term<'a>) -> Answer<'a> {
    let applied = api::apply_steps(&doc.node, &steps(limits, &doc.node, json)?);
    Ok(respond(env, applied, |node| {
        document(env, node, doc.source.clone())
    }))
}

fn invert<'a>(env: Env<'a>, limits: &Limits, doc: &Node, json: Term<'a>) -> Answer<'a> {
    match api::invert_steps(doc, &steps(limits, doc, json)?) {
        Ok(steps) => {
            let steps = make(env, etf::write(&steps, limits.write))?;
            Ok((ok(), steps).encode(env))
        }
        Err(failed) => Ok(failure(env, failed)),
    }
}

fn map<'a>(
    env: Env<'a>,
    limits: &Limits,
    schema: &Schema,
    json: Term<'a>,
    pos: usize,
    assoc: i32,
) -> Answer<'a> {
    let steps = term::read(json, limits.read)?;
    let mapped = api::map_position(schema, &steps, pos, assoc);
    Ok(respond(env, mapped, |pos| pos.encode(env)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema<'a>(env: Env<'a>, spec: Term<'a>) -> NifResult<Term<'a>> {
    let spec = term::read(spec, usize::MAX).map_err(|_| rustler::Error::BadArg)?;
    Ok(respond(env, api::schema(&spec), |schema| {
        ResourceArc::new(SchemaResource(schema)).encode(env)
    }))
}

#[rustler::nif]
fn node_from_json<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    json: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, read_node(env, &NORMAL, &schema.0, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn node_from_json_dirty<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    json: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, read_node(env, &DIRTY, &schema.0, json))
}

#[rustler::nif]
fn to_json<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    json: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, write_node(env, &NORMAL, &doc, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn to_json_dirty<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    json: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, write_node(env, &DIRTY, &doc, json))
}

#[rustler::nif]
fn check(env: Env, doc: ResourceArc<NodeResource>) -> NifResult<Term> {
    answer(env, check_node(env, &NORMAL, &doc.node))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check_dirty(env: Env, doc: ResourceArc<NodeResource>) -> NifResult<Term> {
    answer(env, check_node(env, &DIRTY, &doc.node))
}

#[rustler::nif]
fn apply_steps<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, apply(env, &NORMAL, &doc, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps_dirty<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, apply(env, &DIRTY, &doc, steps))
}

#[rustler::nif]
fn invert_steps<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, invert(env, &NORMAL, &doc.node, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps_dirty<'a>(
    env: Env<'a>,
    doc: ResourceArc<NodeResource>,
    steps: Term<'a>,
) -> NifResult<Term<'a>> {
    answer(env, invert(env, &DIRTY, &doc.node, steps))
}

#[rustler::nif]
fn map_position<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    answer(env, map(env, &NORMAL, &schema.0, steps, pos, assoc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn map_position_dirty<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    answer(env, map(env, &DIRTY, &schema.0, steps, pos, assoc))
}

rustler::init!("Elixir.Tarnish.Native");
