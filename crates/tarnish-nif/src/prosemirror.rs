//! tarnish's ProseMirror functions, which `Tarnish` calls: specs, steps and JSON in and out as
//! terms, schemas as resources, built once, and documents as the binaries of the chunks that hold
//! them, which each call reads in place.
//!
//! Each call is made on the caller's scheduler first, as a light one ([`crate::light`]), and
//! answers `:dirty` when it has more work than that takes on, for `Tarnish` to make it again on a
//! dirty scheduler, through the function of the same name ending in `_dirty`.

use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use tarnish::{Error, Node, Schema, Text, Value, api, stack};

use crate::doc::Doc;
use crate::share;
use crate::term::{Reader, Unread, Writer};
use crate::view;

rustler::atoms! {
    ok,
    error,
    range_error,
    syntax_error,
    replace_error,
    transform_error,
    type_error,
    js_error,
}

pub struct SchemaResource(pub Schema);

#[rustler::resource_impl]
impl Resource for SchemaResource {}

/// How much a call takes on.
struct Limits {
    /// The weight of the terms read, as [`Reader`] weighs them.
    read: usize,
    /// A document's JSON made, in about the bytes of its external format.
    write: usize,
    /// The positions of a document checked, or read for its text.
    check: usize,
    /// Steps applied, or positions ops span, times the top-level nodes of the document, which a
    /// step copies.
    apply: usize,
}

/// A light call's, each about [`LIGHT`]'s time. Reading costs less a unit of weight than
/// converting does, which [`LIGHT`]'s weight is for.
const NORMAL: Limits = Limits {
    read: 100_000,
    write: 150_000,
    check: 250_000,
    apply: 10_000,
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
            Unread::Heavy => Unanswered::Dirty,
            Unread::NotJson => Unanswered::BadArg,
        }
    }
}

type Answer<'a> = Result<Term<'a>, Unanswered>;

/// Makes a call on the caller's scheduler.
fn normal<'a>(env: Env<'a>, call: impl FnOnce(&Limits) -> Answer<'a>) -> NifResult<Term<'a>> {
    match crate::light(env, || call(&NORMAL)) {
        Ok(term) => Ok(term),
        Err(Unanswered::Dirty) => Ok(crate::dirty(env)),
        Err(Unanswered::BadArg) => Err(rustler::Error::BadArg),
    }
}

/// Makes a call on a dirty scheduler, with no limits.
fn dirty<'a>(call: impl FnOnce(&Limits) -> Answer<'a>) -> NifResult<Term<'a>> {
    match call(&DIRTY) {
        Ok(term) => Ok(term),
        Err(Unanswered::Dirty) => unreachable!("a dirty call has no limits"),
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

fn read(limits: &Limits, json: Term) -> Result<Value, Unanswered> {
    Ok(Reader::new(limits.read).read(json)?)
}

/// The steps, when there are few enough for the limits to apply to `doc`.
fn steps(limits: &Limits, doc: &Node, steps: Term) -> Result<Value, Unanswered> {
    let steps = read(limits, steps)?;
    let count = steps.as_array().map_or(0, Vec::len);
    within(count * (doc.child_count() + 1), limits.apply)?;
    Ok(steps)
}

/// The ops, when the positions they span are few enough for the limits to apply to `doc`: an
/// op over a range may make a step for each node in it.
fn ops(limits: &Limits, doc: &Node, ops: Term) -> Result<Value, Unanswered> {
    let ops = read(limits, ops)?;
    let span = |op: &Value| {
        let at = |key: &str| op.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        ((at("to") - at("from")).max(0.0) as usize).saturating_add(1)
    };
    let spans = ops.as_array().into_iter().flatten().map(span);
    let work = spans.fold(0, usize::saturating_add);
    within(work.saturating_mul(doc.child_count() + 1), limits.apply)?;
    Ok(ops)
}

fn read_node<'a>(env: Env<'a>, limits: &Limits, schema: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let resource: ResourceArc<SchemaResource> = schema.decode().map_err(|_| Unanswered::BadArg)?;
    let doc = view::read(json, limits.read, |json| Node::from_json(&resource.0, json))?;
    Ok(respond(env, doc, |doc| crate::doc::read(env, schema, &doc)))
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

/// Steps' term, when they're small enough for the limits: a step undoing a deletion holds the
/// content deleted.
fn write_steps<'a>(env: Env<'a>, limits: &Limits, steps: &Value) -> Answer<'a> {
    let mut left = limits.write;
    within_size(steps, &mut left)?;
    Ok(Writer::new(env).write(steps))
}

/// Spends a byte of `left` for each byte of the value's text, and a map's or list's item's
/// worth for each value.
fn within_size(value: &Value, left: &mut usize) -> Result<(), Unanswered> {
    *left = left.checked_sub(8).ok_or(Unanswered::Dirty)?;
    match value {
        Value::String(text) => *left = left.checked_sub(text.len()).ok_or(Unanswered::Dirty)?,
        Value::Array(items) => {
            for item in items {
                stack::grow(|| within_size(item, left))?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                *left = left.checked_sub(key.len()).ok_or(Unanswered::Dirty)?;
                stack::grow(|| within_size(item, left))?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn invert<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?.root();
    match api::invert_steps(&doc, &steps(limits, &doc, json)?) {
        Ok(steps) => Ok((ok(), write_steps(env, limits, &steps)?).encode(env)),
        Err(failed) => Ok(failure(env, failed)),
    }
}

fn transformed<'a>(env: Env<'a>, limits: &Limits, doc: Term<'a>, json: Term<'a>) -> Answer<'a> {
    let doc = load(doc)?;
    let root = doc.root();
    match api::transform(&root, &ops(limits, &root, json)?) {
        Ok((node, steps)) => {
            let steps = write_steps(env, limits, &steps)?;
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
    let steps = read(limits, json)?;
    let mapped = api::map_position(schema, &steps, pos, assoc);
    Ok(respond(env, mapped, |pos| pos.encode(env)))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema<'a>(env: Env<'a>, spec: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| {
        let spec = read(limits, spec)?;
        Ok(respond(env, api::schema(&spec), |schema| {
            ResourceArc::new(SchemaResource(schema)).encode(env)
        }))
    })
}

#[rustler::nif]
fn node_from_json<'a>(env: Env<'a>, schema: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| read_node(env, limits, schema, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn node_from_json_dirty<'a>(env: Env<'a>, schema: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| read_node(env, limits, schema, json))
}

#[rustler::nif]
fn to_json<'a>(env: Env<'a>, doc: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| write_node(env, limits, doc, json))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn to_json_dirty<'a>(env: Env<'a>, doc: Term<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| write_node(env, limits, doc, json))
}

#[rustler::nif]
fn check<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| check_node(env, limits, doc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn check_dirty<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| check_node(env, limits, doc))
}

#[rustler::nif]
fn apply_steps<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| apply(env, limits, doc, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn apply_steps_dirty<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| apply(env, limits, doc, steps))
}

#[rustler::nif]
fn invert_steps<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| invert(env, limits, doc, steps))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn invert_steps_dirty<'a>(env: Env<'a>, doc: Term<'a>, steps: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| invert(env, limits, doc, steps))
}

#[rustler::nif]
fn transform<'a>(env: Env<'a>, doc: Term<'a>, ops: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| transformed(env, limits, doc, ops))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn transform_dirty<'a>(env: Env<'a>, doc: Term<'a>, ops: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| transformed(env, limits, doc, ops))
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
    normal(env, |limits| {
        between(env, limits, doc, from, to, block_separator, leaf_text)
    })
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
    dirty(|limits| between(env, limits, doc, from, to, block_separator, leaf_text))
}

#[rustler::nif]
fn text_content<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    normal(env, |limits| content(env, limits, doc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn text_content_dirty<'a>(env: Env<'a>, doc: Term<'a>) -> NifResult<Term<'a>> {
    dirty(|limits| content(env, limits, doc))
}

#[rustler::nif]
fn map_position<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    normal(env, |limits| map(env, limits, &schema.0, steps, pos, assoc))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn map_position_dirty<'a>(
    env: Env<'a>,
    schema: ResourceArc<SchemaResource>,
    steps: Term<'a>,
    pos: usize,
    assoc: i32,
) -> NifResult<Term<'a>> {
    dirty(|limits| map(env, limits, &schema.0, steps, pos, assoc))
}
