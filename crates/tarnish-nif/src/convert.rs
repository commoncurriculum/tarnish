//! `Tarnish.Bridge`'s conversions in the NIF: `convert/1` and `convert_light/1`, which answer the
//! requests a Node worker answers, with the conversions the NIF's `load` hands
//! [`load`](crate::load).
//!
//! A request is `{operation, input}` or `{operation, input, options}`, read as Jason would encode
//! it. Its answer is `{:ok, result}` or `{:error, message}`, the result the term Jason would decode
//! from the JSON the worker writes for it. A request whose operation isn't one of the four, or
//! whose input isn't text for a parse and an object for a serialization, is refused with the
//! worker's message; any other is the application's [`Conversions`]' to convert or refuse.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;
use std::sync::mpsc;

use rustler::types::tuple::get_tuple;
use rustler::{Encoder, Env, Term, TermType};
use tarnish::js::deadline;
use tarnish::{Error, Node, Schema, Value};

use crate::etf::{self, NotJson};
use crate::term::{Reader, Unread, Writer};
use crate::{Budget, LIGHT, UNLIMITED, view};

mod atoms {
    rustler::atoms! {
        not_json,
    }
}

/// `elixir/priv/worker.mjs`'s refusal of a request it has no conversion for.
const INVALID: &str = "Unknown operation or invalid input";

/// An application's four conversions of its documents. Each is given the request's options as
/// they were sent, and refuses a request as the application's worker conversion does, with its
/// message.
pub trait Conversions: Sync {
    /// The schema of the documents the conversions read and make, which a document sent to
    /// `serializeHTML` as terms is read against as its terms are read.
    fn schema(&self) -> &Schema;

    fn parse_markdown(&self, markdown: &str, options: Option<&Value>) -> Result<Value, Error>;

    fn serialize_markdown(
        &self,
        document: &Value,
        options: Option<&Value>,
    ) -> Result<String, Error>;

    fn parse_html(&self, html: &str, options: Option<&Value>) -> Result<Node<'static>, Error>;

    /// The document isn't read yet, so that the conversion checks its options before a document
    /// that doesn't read fails it.
    fn serialize_html(
        &self,
        document: Document<'_>,
        options: Option<&Value>,
    ) -> Result<String, Error>;
}

/// What a NIF serving conversions converts with.
struct Served {
    conversions: &'static dyn Conversions,
    /// The threads a batch's conversions run on.
    pool: rayon::ThreadPool,
}

static SERVED: OnceLock<Served> = OnceLock::new();

pub(crate) fn serve(conversions: &'static dyn Conversions, threads: usize) {
    SERVED.get_or_init(|| Served {
        conversions,
        pool: rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("tarnish-{index}"))
            .build()
            .expect("the pool's threads start"),
    });
}

fn served() -> &'static Served {
    SERVED.get().expect("the NIF's load serves no conversions")
}

/// A `Tarnish.Bridge.request()`, each part read from a `T`: its operation, its input, and its
/// options when they aren't empty.
#[derive(Clone)]
pub struct Request<T = Value, I = T> {
    pub operation: T,
    pub input: I,
    pub options: Option<T>,
}

impl<T> Request<T> {
    fn map<U>(self, mut part: impl FnMut(T) -> U) -> Request<U> {
        Request {
            operation: part(self.operation),
            input: part(self.input),
            options: self.options.map(part),
        }
    }

    fn read<E>(self, mut read: impl FnMut(T) -> Result<Value, E>) -> Result<Request, E> {
        Ok(Request {
            operation: read(self.operation)?,
            input: read(self.input)?,
            options: self.options.map(read).transpose()?,
        })
    }
}

/// `serializeHTML`'s document as the request sent it, for [`Document::read`] to read against
/// [`Conversions::schema`].
pub struct Document<'a> {
    source: Source<'a>,
    schema: &'a Schema,
}

enum Source<'a> {
    Json(&'a Value),
    /// Read straight from its terms, as the request was read.
    Terms(Result<Node<'static>, Error>),
}

impl<'a> Document<'a> {
    /// The document whose JSON is `json`, to be read against `schema`.
    pub fn json(json: &'a Value, schema: &'a Schema) -> Self {
        Document {
            source: Source::Json(json),
            schema,
        }
    }

    /// `Node.fromJSON(schema, json)`.
    pub fn read(self) -> Result<Node<'static>, Error> {
        match self.source {
            Source::Json(json) => Node::from_json(self.schema, json),
            Source::Terms(read) => read,
        }
    }
}

/// A request's input: its JSON, or `serializeHTML`'s document read from its terms.
enum Input {
    Json(Value),
    Document(Result<Node<'static>, Error>),
}

/// A request's result: a serialization's text, `parseMarkdown`'s JSON, or the document
/// `parseHTML` made, whose JSON can be written straight from its nodes.
pub(crate) enum Answer {
    Text(String),
    Json(Value),
    Document(Node<'static>),
}

impl Answer {
    pub(crate) fn into_json(self) -> Value {
        match self {
            Answer::Text(text) => Value::String(text),
            Answer::Json(json) => json,
            Answer::Document(document) => document.to_json(),
        }
    }
}

/// The term Jason decodes from the result's JSON.
impl Encoder for Answer {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let mut writer = Writer::new(env);
        match self {
            Answer::Text(text) => writer.text(text),
            Answer::Json(json) => writer.write(json),
            Answer::Document(document) => writer.fields(document.view()),
        }
    }
}

/// A request answered with its result, or its error.
fn answer(conversions: &dyn Conversions, request: Request<Value, Input>) -> Result<Answer, String> {
    let options = request.options.as_ref();
    let schema = conversions.schema();
    let answered = match (request.operation.as_str(), request.input) {
        (Some("parseMarkdown"), Input::Json(Value::String(ref markdown))) => conversions
            .parse_markdown(markdown, options)
            .map(Answer::Json),
        (Some("serializeMarkdown"), Input::Json(ref document @ Value::Object(_))) => conversions
            .serialize_markdown(document, options)
            .map(Answer::Text),
        (Some("parseHTML"), Input::Json(Value::String(ref html))) => {
            conversions.parse_html(html, options).map(Answer::Document)
        }
        (Some("serializeHTML"), Input::Json(ref document @ Value::Object(_))) => conversions
            .serialize_html(Document::json(document, schema), options)
            .map(Answer::Text),
        (Some("serializeHTML"), Input::Document(read)) => {
            let document = Document {
                source: Source::Terms(read),
                schema,
            };
            conversions
                .serialize_html(document, options)
                .map(Answer::Text)
        }
        _ => return Err(INVALID.into()),
    };
    answered.map_err(|error| error.message().into())
}

/// [`answer`] of a request read as JSON.
pub(crate) fn answer_json(
    conversions: &dyn Conversions,
    request: Request,
) -> Result<Answer, String> {
    let request = Request {
        operation: request.operation,
        input: Input::Json(request.input),
        options: request.options,
    };
    answer(conversions, request)
}

/// `{:ok, result}` or `{:error, message}` for each `Tarnish.Bridge.request()` of a list, in
/// order, or `:not_json` for one holding terms Jason encodes only through their `Jason.Encoder`,
/// such as structs and tuples. It runs on a dirty scheduler, since a conversion of a large
/// document takes far longer than a millisecond.
#[rustler::nif(schedule = "DirtyCpu")]
fn convert<'a>(env: Env<'a>, requests: Vec<Term<'a>>) -> Term<'a> {
    let answers = match requests[..] {
        [request] => vec![match parts(request) {
            Ok(parts) => {
                answer_term(env, parts, UNLIMITED).expect("an unlimited conversion answers")
            }
            Err(NotJson) => atoms::not_json().encode(env),
        }],
        _ => convert_on_pool(env, &requests),
    };
    answers.encode(env)
}

/// `convert` for one request as a light call, or `:dirty` for one that takes more than `LIGHT`:
/// marked and the HTML tree builder take time quadratic in the length of some texts, however
/// short, so a conversion is cut off at `LIGHT`'s time too.
#[rustler::nif]
fn convert_light<'a>(env: Env<'a>, request: Term<'a>) -> Term<'a> {
    let answer = crate::light(env, || match parts(request) {
        Ok(parts) => answer_term(env, parts, LIGHT),
        Err(NotJson) => Some(atoms::not_json().encode(env)),
    });
    answer.unwrap_or_else(|| crate::dirty(env))
}

/// The answer to a request, or `None` if it takes more than `budget`.
fn answer_term<'a>(env: Env<'a>, parts: Request<Term<'a>>, budget: Budget) -> Option<Term<'a>> {
    let conversions = served().conversions;
    let request = match read(conversions, parts, budget.weight) {
        Ok(request) => request,
        Err(Unread::NotJson) => return Some(atoms::not_json().encode(env)),
        Err(Unread::Heavy) => return None,
    };
    let answered = match budget.time {
        Some(limit) => deadline::within(limit, || answer(conversions, request))?,
        None => answer(conversions, request),
    };
    Some(answered.encode(env))
}

/// A request's parts read as the JSON Jason encodes them, each weighing no more than `weight`,
/// but for `serializeHTML`'s document, which is read from its terms straight into the document it
/// serializes.
fn read(
    conversions: &dyn Conversions,
    parts: Request<Term>,
    weight: usize,
) -> Result<Request<Value, Input>, Unread> {
    let mut reader = Reader::new(weight);
    let operation = reader.read(parts.operation)?;
    let input = match operation.as_str() == Some("serializeHTML")
        && parts.input.get_type() == TermType::Map
    {
        true => Input::Document(view::read(parts.input, weight, |json| {
            Node::from_json(conversions.schema(), json)
        })?),
        false => Input::Json(reader.read(parts.input)?),
    };
    let options = parts
        .options
        .map(|options| reader.read(options))
        .transpose()?;
    Ok(Request {
        operation,
        input,
        options,
    })
}

/// `convert` for a batch, whose requests the pool converts while this thread makes the answers'
/// terms. A term is only its environment's to read, so this thread has the VM write each
/// request's parts out in the external format for a pool thread to read, and makes each
/// answer's term from what the pool thread sends back. It makes the answers that have come in
/// after each request it sends, so that making terms overlaps the conversions still running.
///
/// A conversion that panics fails the batch, with the first panic in order, as converting the
/// requests one at a time would.
fn convert_on_pool<'a>(env: Env<'a>, requests: &[Term<'a>]) -> Vec<Term<'a>> {
    let Served { conversions, pool } = served();
    let (sender, receiver) = mpsc::channel();
    let mut answers = vec![None; requests.len()];
    let mut panic: Option<(usize, Box<dyn Any + Send>)> = None;
    pool.in_place_scope(|scope| {
        let mut take = |(index, outcome)| match outcome {
            Outcome::Converted(handled) => answers[index] = Some(handled.encode(env)),
            Outcome::NotJson => answers[index] = Some(atoms::not_json().encode(env)),
            Outcome::Panicked(payload) => {
                if panic.as_ref().is_none_or(|(first, _)| index < *first) {
                    panic = Some((index, payload));
                }
            }
        };
        for (index, &request) in requests.iter().enumerate() {
            let Ok(parts) = parts(request) else {
                take((index, Outcome::NotJson));
                continue;
            };
            let parts = parts.map(Term::to_binary);
            let sender = sender.clone();
            scope.spawn(move |_| {
                let converted = std::panic::catch_unwind(AssertUnwindSafe(move || {
                    let request = parts.read(|part| etf::read(&part))?;
                    Ok(answer_json(*conversions, request).map(Sent::of))
                }));
                let outcome = match converted {
                    Ok(Ok(handled)) => Outcome::Converted(handled),
                    Ok(Err(NotJson)) => Outcome::NotJson,
                    Err(panic) => Outcome::Panicked(panic),
                };
                sender
                    .send((index, outcome))
                    .expect("the batch waits for every answer");
            });
            while let Ok(answer) = receiver.try_recv() {
                take(answer);
            }
        }
        // Every job holds a sender, so the answers end when the last job does.
        drop(sender);
        for answer in &receiver {
            take(answer);
        }
    });
    if let Some((_, payload)) = panic {
        std::panic::resume_unwind(payload);
    }
    answers
        .into_iter()
        .map(|answer| answer.expect("every request answered"))
        .collect()
}

/// What a pool thread sends back for a request.
enum Outcome {
    Converted(Result<Sent, String>),
    NotJson,
    Panicked(Box<dyn Any + Send>),
}

/// A `Tarnish.Bridge.request()`'s parts: `{operation, input}`, or `{operation, input, options}`.
fn parts(request: Term) -> Result<Request<Term>, NotJson> {
    match *get_tuple(request).map_err(|_| NotJson)? {
        [operation, input] => Ok(Request {
            operation,
            input,
            options: None,
        }),
        [operation, input, options] => Ok(Request {
            operation,
            input,
            options: Some(options),
        }),
        _ => Err(NotJson),
    }
}

/// A result a pool thread sends back, for this thread to make its term from.
enum Sent {
    /// A serialization, which goes into a binary as it is: the external format's binaries hold
    /// less than 4 GiB.
    Text(String),
    /// Any other result, in the external format.
    Term(Vec<u8>),
}

impl Sent {
    fn of(answer: Answer) -> Sent {
        match answer {
            Answer::Text(text) => Sent::Text(text),
            Answer::Json(json) => Sent::Term(etf::write(&json)),
            Answer::Document(document) => Sent::Term(etf::write_fields(document.view())),
        }
    }
}

impl Encoder for Sent {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        match self {
            Sent::Text(text) => text.encode(env),
            Sent::Term(bytes) => {
                env.binary_to_term(bytes)
                    .expect("the external format of a value")
                    .0
            }
        }
    }
}
