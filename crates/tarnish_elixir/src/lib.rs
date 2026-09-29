//! The Elixir binding of tarnish: specs, steps and JSON in and out as terms, the way Jason reads
//! them from ProseMirror's JSON, schemas as resources, built once, and documents as the binaries
//! of the chunks that hold them, which each call reads in place.
//!
//! A dirty scheduler takes a while to hand a call to, as long as a small document takes to read,
//! and has the call's terms to fetch into another core's cache. So each call runs on a normal
//! scheduler, where it may take up to about a millisecond, and a call with more work than that
//! answers `:dirty`, for the Elixir side to make it again on a dirty scheduler.
//!
//! Without the `standalone` feature, the functions are linked into another crate's NIF, and
//! `rustler::init!` there registers them in the module it names.

#![forbid(unsafe_code)]

mod doc;
mod etf;
mod share;
mod term;

use doc::Doc;
use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use tarnish::{Error, Node, Schema, Text, Value, api};
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

#[cfg(feature = "standalone")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub struct SchemaResource(Schema);

#[rustler::resource_impl]
impl Resource for SchemaResource {}

/// How much a call takes on, each limit about a millisecond's work on a normal scheduler.
struct Limits {
    /// Maps and lists read, with text counting by its length.
    read: usize,
    /// Bytes of a term written, which the VM then makes the term from.
    write: usize,
    /// The positions of a document checked, or read for its text.
    check: usize,
    /// Steps applied, or positions ops span, times the top-level nodes of the document, which a
    /// step copies.
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
    /// A term Jason couldn't encode, or a document ref that isn't one, which raises
    /// `ArgumentError`.
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

fn load(doc: Term) -> Result<Doc, Unanswered> {
    Doc::load(doc).ok_or(Unanswered::BadArg)
}

/// The steps, when there are few enough for the limits to apply to `doc`.
fn steps(limits: &Limits, doc: &Node, steps: Term) -> Result<Value, Unanswered> {
    let steps = term::read(steps, limits.read)?;
    let count = steps.as_array().map_or(0, Vec::len);
    within(count * (doc.child_count() + 1), limits.apply)?;
    Ok(steps)
}

/// The ops, when the positions they span are few enough for the limits to apply to `doc`: an
/// op over a range may make a step for each node in it.
fn ops(limits: &Limits, doc: &Node, ops: Term) -> Result<Value, Unanswered> {
    let ops = term::read(ops, limits.read)?;
    let span = |op: &Value| {
        let at = |key: &str| op.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        ((at("to") - at("from")).max(0.0) as usize).saturating_add(1)
    };
    let spans = ops.as_array().into_iter().flatten().map(span);
    let work = spans.fold(0, usize::saturating_add);
    within(work.saturating_mul(doc.child_count() + 1), limits.apply)?;
    Ok(ops)
}

/// The term of the bytes [`etf`] wrote, when they're few enough.
fn make<'a>(env: Env<'a>, bytes: Option<Vec<u8>>) -> Answer<'a> {
    let bytes = bytes.ok_or(Unanswered::Dirty)?;
    Ok(term::make(env, &bytes))
}

fn read_node<'a>(env: Env<'a>, limits: &Limits, schema: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let resource: ResourceArc<SchemaResource> = schema.decode().map_err(|_| Unanswered::BadArg)?;
    let doc = term::read_json(json, limits.read, |json| Node::from_json(&resource.0, json))?;
    Ok(respond(env, doc, |doc| doc::read(env, schema, &doc)))
}

fn write_node<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?;
    share::json(env, doc.view(), doc.read(), json, limits.write)
        .map_err(|share::TooBig| Unanswered::Dirty)
}

fn check_node<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?.root();
    within(doc.node_size(), limits.check)?;
    Ok(match doc.check() {
        Ok(()) => ok().encode(env),
        Err(failed) => failure(env, failed),
    })
}

fn apply<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?;
    let root = doc.root();
    let applied = api::apply_steps(&root, &steps(limits, &root, json)?);
    Ok(respond(env, applied, |node| doc.changed(env, &node)))
}

fn invert<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?.root();
    match api::invert_steps(&doc, &steps(limits, &doc, json)?) {
        Ok(steps) => {
            let steps = make(env, etf::write(&steps, limits.write))?;
            Ok((ok(), steps).encode(env))
        }
        Err(failed) => Ok(failure(env, failed)),
    }
}

fn transformed<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?;
    let root = doc.root();
    match api::transform(&root, &ops(limits, &root, json)?) {
        Ok((node, steps)) => {
            let steps = make(env, etf::write(&steps, limits.write))?;
            Ok((ok(), doc.changed(env, &node), steps).encode(env))
        }
        Err(failed) => Ok(failure(env, failed)),
    }
}

/// `{:ok, text}`, a lone surrogate in it being U+FFFD, or the failure.
fn text_term<'a>(env: Env<'a>, text: tarnish::Result<Text>) -> Term<'a> {
    respond(env, text, |text| {
        text.to_string_lossy().as_ref().encode(env)
    })
}

fn between<'a>(
    env: Env<'a>,
    limits: &Limits,
    doc: Term<'a>,
    from: usize,
    to: usize,
    block_separator: Option<&str>,
    leaf_text: Option<&str>,
) -> Answer<'a> {
    let doc = load(doc)?.root();
    within(to.min(doc.node_size()), limits.check)?;
    let text = api::text_between(&doc, from, to, block_separator, leaf_text);
    Ok(text_term(env, text))
}

fn content<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?.root();
    within(doc.node_size(), limits.check)?;
    Ok(text_term(env, api::text_content(&doc)))
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
fn node_from_json<'a>(env: Env<'a>, schema: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, read_node(env, &NORMAL, schema, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn node_from_json_dirty<'a>(env: Env<'a>, schema: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, read_node(env, &DIRTY, schema, json))
}

#[rustler::nif]
fn to_json<'a>(env: Env<'a>, doc: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, write_node(env, &NORMAL, doc, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn to_json_dirty<'a>(env: Env<'a>, doc: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, write_node(env, &DIRTY, doc, json))
}

#[rustler::nif]
fn check<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, check_node(env, &NORMAL, doc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check_dirty<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, check_node(env, &DIRTY, doc))
}

#[rustler::nif]
fn apply_steps<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, apply(env, &NORMAL, doc, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps_dirty<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, apply(env, &DIRTY, doc, steps))
}

#[rustler::nif]
fn invert_steps<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, invert(env, &NORMAL, doc, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps_dirty<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, invert(env, &DIRTY, doc, steps))
}

#[rustler::nif]
fn transform<'a>(env: Env<'a>, doc: Term<'a>, ops: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, transformed(env, &NORMAL, doc, ops))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn transform_dirty<'a>(env: Env<'a>, doc: Term<'a>, ops: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, transformed(env, &DIRTY, doc, ops))
}

#[rustler::nif]
fn text_between<'a>(
    env: Env<'a>,
    doc: Term<'a>,
    from: usize,
    to: usize,
    block_separator: Option<&'a str>,
    leaf_text: Option<&'a str>,
) -> NifResult<Term<'a>> {
    let text = between(env, &NORMAL, doc, from, to, block_separator, leaf_text);
    answer(env, text)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn text_between_dirty<'a>(
    env: Env<'a>,
    doc: Term<'a>,
    from: usize,
    to: usize,
    block_separator: Option<&'a str>,
    leaf_text: Option<&'a str>,
) -> NifResult<Term<'a>> {
    let text = between(env, &DIRTY, doc, from, to, block_separator, leaf_text);
    answer(env, text)
}

#[rustler::nif]
fn text_content<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, content(env, &NORMAL, doc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn text_content_dirty<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    answer(env, content(env, &DIRTY, doc))
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

#[cfg(feature = "standalone")]
rustler::init!("Elixir.Tarnish.Native");
