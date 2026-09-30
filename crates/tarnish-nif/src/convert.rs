//! `Tarnish.Bridge`'s conversions in the NIF: `convert/1` and `convert_light/1`, which answer the
//! requests a Node worker answers, with the application's [`Conversions`], which its NIF's `load`
//! hands [`serve`].
//!
//! A request is `{operation, input}` or `{operation, input, options}`, read as Jason would encode
//! it. Its answer is `{:ok, result}` or `{:error, message}`, the result the term Jason would decode
//! from the JSON the worker writes for it. The worker checks a request as its zod schema does, and
//! so do these, giving the same messages.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use rustler::types::atom;
use rustler::types::tuple::get_tuple;
use rustler::{Encoder, Env, Term, TermType};
use tarnish::js::{self, deadline};
use tarnish::{Error, Node, Schema, Value};

use crate::etf::{self, NotJson};
use crate::term::{Reader, Unread, Writer};
use crate::{Budget, LIGHT, UNLIMITED, view};

mod atoms {
    rustler::atoms! {
        not_json,
    }
}

/// An application's conversions of its documents.
pub trait Conversions: Sync {
    /// The schema of the documents the conversions read and make.
    fn schema(&self) -> &Schema;

    fn parse_markdown(&self, markdown: &str) -> Result<Value, Error>;

    fn serialize_markdown(
        &self,
        document: &Value,
        options: &MarkdownOptions,
    ) -> Result<String, Error>;

    fn parse_html(&self, html: &str) -> Result<Node<'static>, Error>;

    fn serialize_html(&self, document: &Node<'static>) -> Result<String, Error>;
}

/// The options of `parseMarkdown` and `serializeMarkdown`.
pub struct MarkdownOptions {
    pub include_attributes: bool,
}

static CONVERSIONS: OnceLock<&'static dyn Conversions> = OnceLock::new();

/// Makes `conversions` the ones `convert/1` and `convert_light/1` answer with, for the NIF's
/// `load` to call before [`load`](crate::load).
pub fn serve(conversions: &'static dyn Conversions) {
    CONVERSIONS.get_or_init(|| conversions);
}

fn conversions() -> &'static dyn Conversions {
    *CONVERSIONS
        .get()
        .expect("the NIF's load serves its conversions")
}

const MAX_MARKDOWN_LINE_LENGTH: usize = 100_000;
const INVALID_REQUEST: &str = "Unknown operation or invalid input";

/// A `Tarnish.Bridge.request()`, each part read from a `T`: its operation, its input, and its
/// options when they aren't empty. The input may be read otherwise, as an [`Input`].
pub struct Request<T = Value, I = T> {
    pub operation: T,
    pub input: I,
    pub options: Option<T>,
}

impl<T> Request<T> {
    pub fn map<U>(self, mut part: impl FnMut(T) -> U) -> Request<U> {
        Request {
            operation: part(self.operation),
            input: part(self.input),
            options: self.options.map(part),
        }
    }

    /// The request with each part read with `read`.
    pub fn read<E>(self, mut read: impl FnMut(T) -> Result<Value, E>) -> Result<Request, E> {
        Ok(Request {
            operation: read(self.operation)?,
            input: read(self.input)?,
            options: self.options.map(read).transpose()?,
        })
    }
}

/// A request's input: its JSON, or the document `serializeHTML` serializes, read from its JSON
/// as `Node.fromJSON` reads it, which gives its error only once the request is checked.
pub enum Input {
    Json(Value),
    Document(Result<Node<'static>, Error>),
}

impl Input {
    fn as_str(&self) -> Option<&str> {
        match self {
            Input::Json(json) => json.as_str(),
            Input::Document(_) => None,
        }
    }

    fn is_object(&self) -> bool {
        match self {
            Input::Json(json) => json.is_object(),
            Input::Document(_) => true,
        }
    }
}

/// What a checked request asks for.
enum Operation {
    ParseMarkdown,
    SerializeMarkdown(MarkdownOptions),
    ParseHtml,
    SerializeHtml,
}

/// A request's result: its JSON, or the document `parseHTML` made, whose JSON can be written
/// straight from its nodes.
pub enum Answer {
    Json(Value),
    Document(Node<'static>),
}

impl Answer {
    pub fn into_json(self) -> Value {
        match self {
            Answer::Json(json) => json,
            Answer::Document(document) => document.to_json(),
        }
    }
}

/// A request answered with its result, or its error.
pub fn answer(
    conversions: &dyn Conversions,
    request: Request<Value, Input>,
) -> Result<Answer, String> {
    let operation = check(&request)?;
    let input = request.input;
    let text = || input.as_str().expect("a checked string input");
    let message = |error: Error| error.message().to_string();
    let json = match (operation, &input) {
        (Operation::ParseMarkdown, _) => conversions.parse_markdown(text()).map_err(message),
        (Operation::SerializeMarkdown(options), Input::Json(document)) => conversions
            .serialize_markdown(document, &options)
            .map(Value::String)
            .map_err(message),
        (Operation::ParseHtml, _) => {
            return conversions
                .parse_html(text())
                .map(Answer::Document)
                .map_err(message);
        }
        (Operation::SerializeHtml, Input::Json(document)) => {
            Node::from_json(conversions.schema(), document)
                .and_then(|document| conversions.serialize_html(&document))
                .map(Value::String)
                .map_err(message)
        }
        (Operation::SerializeHtml, Input::Document(document)) => match document {
            Ok(document) => conversions
                .serialize_html(document)
                .map(Value::String)
                .map_err(message),
            Err(error) => Err(error.message().to_string()),
        },
        (Operation::SerializeMarkdown(_), Input::Document(_)) => {
            unreachable!("only serializeHTML's document is read as one")
        }
    };
    json.map(Answer::Json)
}

/// [`answer`] of a request read as JSON, its result as JSON.
pub fn handle(conversions: &dyn Conversions, request: Request) -> Result<Value, String> {
    answer_json(conversions, request).map(Answer::into_json)
}

/// [`answer`] of a request read as JSON.
fn answer_json(conversions: &dyn Conversions, request: Request) -> Result<Answer, String> {
    let request = Request {
        operation: request.operation,
        input: Input::Json(request.input),
        options: request.options,
    };
    answer(conversions, request)
}

// zod checks the fields in order and reports the first failure.
fn check(request: &Request<Value, Input>) -> Result<Operation, String> {
    let Value::String(operation) = &request.operation else {
        return Err(INVALID_REQUEST.into());
    };
    let text = || request.input.as_str().ok_or(INVALID_REQUEST);
    let document = || {
        request
            .input
            .is_object()
            .then_some(())
            .ok_or(INVALID_REQUEST)
    };
    let options = request.options.as_ref();
    Ok(match operation.as_str() {
        "parseMarkdown" => {
            if text()?.split('\n').any(|line| {
                js::utf16_len(line.strip_suffix('\r').unwrap_or(line)) > MAX_MARKDOWN_LINE_LENGTH
            }) {
                return Err(format!(
                    "Markdown lines cannot exceed {MAX_MARKDOWN_LINE_LENGTH} characters"
                ));
            }
            parse_markdown_options(options)?;
            Operation::ParseMarkdown
        }
        "serializeMarkdown" => {
            document()?;
            Operation::SerializeMarkdown(parse_markdown_options(options)?)
        }
        "parseHTML" => {
            text()?;
            refuse_options(options)?;
            Operation::ParseHtml
        }
        "serializeHTML" => {
            document()?;
            refuse_options(options)?;
            Operation::SerializeHtml
        }
        _ => return Err(INVALID_REQUEST.into()),
    })
}

fn parse_markdown_options(options: Option<&Value>) -> Result<MarkdownOptions, String> {
    let Some(options) = options else {
        return Ok(MarkdownOptions {
            include_attributes: false,
        });
    };
    let Value::Object(options) = options else {
        return Err(format!(
            "Invalid input: expected object, received {}",
            js_type_name(options)
        ));
    };
    let unrecognized: Vec<String> = options
        .keys()
        .filter(|key| *key != "includeAttributes")
        .map(|key| format!("\"{key}\""))
        .collect();
    let include_attributes = match options.get("includeAttributes") {
        None => false,
        Some(Value::Bool(include)) => *include,
        Some(other) => {
            return Err(format!(
                "Invalid input: expected boolean, received {}",
                js_type_name(other)
            ));
        }
    };
    match unrecognized.len() {
        0 => Ok(MarkdownOptions { include_attributes }),
        1 => Err(format!("Unrecognized key: {}", unrecognized[0])),
        _ => Err(format!("Unrecognized keys: {}", unrecognized.join(", "))),
    }
}

fn refuse_options(options: Option<&Value>) -> Result<(), String> {
    match options {
        None => Ok(()),
        Some(_) => Err("Options are only valid for Markdown operations".into()),
    }
}

fn js_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
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
    let conversions = conversions();
    let request = match read(conversions, parts, budget.weight) {
        Ok(request) => request,
        Err(Unread::NotJson) => return Some(atoms::not_json().encode(env)),
        Err(Unread::Heavy) => return None,
    };
    let answered = match budget.time {
        Some(limit) => deadline::within(limit, || answer(conversions, request))?,
        None => answer(conversions, request),
    };
    Some(Reply(answered).encode(env))
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
/// A conversion that panics fails the batch, and the requests after it go unconverted. The
/// first panic in order is the one raised, as it would be converting the requests one at a time.
fn convert_on_pool<'a>(env: Env<'a>, requests: &[Term<'a>]) -> Vec<Term<'a>> {
    let conversions = conversions();
    let first_panic = AtomicUsize::new(usize::MAX);
    let (sender, receiver) = mpsc::channel();
    let mut answers = vec![None; requests.len()];
    let mut panic: Option<(usize, Box<dyn Any + Send>)> = None;
    crate::pool().in_place_scope(|scope| {
        let mut take = |(index, outcome)| match outcome {
            Outcome::Converted(handled) => answers[index] = Some(handled.encode(env)),
            Outcome::NotJson => answers[index] = Some(atoms::not_json().encode(env)),
            Outcome::Skipped => {}
            Outcome::Panicked(payload) => {
                if panic.as_ref().is_none_or(|(first, _)| index < *first) {
                    panic = Some((index, payload));
                }
            }
        };
        for (index, &request) in requests.iter().enumerate() {
            if index > first_panic.load(Ordering::Relaxed) {
                break;
            }
            let Ok(parts) = parts(request) else {
                take((index, Outcome::NotJson));
                continue;
            };
            let parts = parts.map(Term::to_binary);
            let (sender, first_panic) = (sender.clone(), &first_panic);
            scope.spawn(move |_| {
                let outcome = if index > first_panic.load(Ordering::Relaxed) {
                    Outcome::Skipped
                } else {
                    let converted = std::panic::catch_unwind(AssertUnwindSafe(move || {
                        let request = parts.read(|part| etf::read(&part))?;
                        Ok(answer_json(conversions, request).map(Sent::of))
                    }));
                    match converted {
                        Ok(Ok(handled)) => Outcome::Converted(handled),
                        Ok(Err(NotJson)) => Outcome::NotJson,
                        Err(panic) => {
                            first_panic.fetch_min(index, Ordering::Relaxed);
                            Outcome::Panicked(panic)
                        }
                    }
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
    /// Left unconverted, since a request before it panicked.
    Skipped,
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

/// A conversion's result as `Tarnish.Bridge` gets it: `{:ok, result}` or `{:error, message}`.
struct Reply(Result<Answer, String>);

impl Encoder for Reply {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let result = match &self.0 {
            Ok(Answer::Json(json)) => crate::term::write(env, json),
            Ok(Answer::Document(document)) => Writer::new(env).fields(document.view()),
            Err(message) => return (atom::error(), message).encode(env),
        };
        (atom::ok(), result).encode(env)
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
            Answer::Json(Value::String(_)) => {
                Sent::Text(answer.into_json().into_string().expect("a string"))
            }
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
