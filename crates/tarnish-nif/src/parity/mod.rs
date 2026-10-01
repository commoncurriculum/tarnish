//! The tools that hold an application's NIF to its Node worker, as tarnish's README describes:
//! `record`, `replay`, `reduce` and `speed`. An application runs them from an example of its own,
//! whose `main` is [`main`] of its [`Parity`].
//!
//! They run the NIF's conversions as [`answer`] does, on a request read from JSON and with the
//! result made into JSON, so the terms the NIF reads and writes in the VM are not on their path:
//! tarnish's Elixir tests hold those, and an application's own tests of `Tarnish.Bridge`.

mod difference;
mod smaller;
mod worker;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::hint::black_box;
use std::ops::RangeInclusive;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tarnish::js::json::{from_str, json, stringify};
use tarnish::{Map, Value};

use crate::convert::{Answer, Conversions, Document, Request, answer_json};
use difference::{Difference, Group, describe, difference, truncated};
pub use difference::{Segment, at};
use worker::Worker;

/// An application's two makings of its conversions, and what it has decided about where their
/// answers differ.
pub struct Parity<'a> {
    /// The NIF's.
    pub conversions: &'a dyn Conversions,
    /// The worker's: the module `config :tarnish, Tarnish.Bridge, conversions:` names.
    pub module: &'a Path,
    pub made_up: MadeUp,
    pub expected: Expected,
}

/// Whether two values where the worker's answer to `request` and the NIF's differ, at `path` in
/// each, are values each answer makes up anew, as an id is. Replay takes them as the same only
/// while the pairs it takes so in one answer pair the worker's values with the NIF's one to one:
/// a value made up once on one side is made up once on the other.
pub type MadeUp = fn(request: &Request, path: &[Segment], theirs: Made<'_>, ours: Made<'_>) -> bool;

/// Why the NIF's answer to `request` differs from the worker's, where the application has decided
/// that it should.
pub type Expected = fn(
    request: &Request,
    theirs: &Result<Value, String>,
    ours: &Result<Value, String>,
) -> Option<&'static str>;

/// A value one side's answer holds where the other side's differs.
#[derive(Clone, Copy)]
pub struct Made<'a> {
    pub value: &'a Value,
    /// The whole answer that holds it.
    pub answer: &'a Value,
    /// When the answer was made, where that's known.
    pub when: Option<&'a Span>,
}

/// From when to when an answer was made, in milliseconds since the Unix epoch, rounded down.
pub type Span = RangeInclusive<u64>;

const USAGE: &str = "record <requests.json> | replay <records.json> | reduce <records.json> | speed <requests.json> [operation]";

/// Runs the tool the arguments name.
pub fn main(parity: &Parity) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["record", path] => record(parity, path),
        ["replay", path] => replay(parity, path),
        ["reduce", path] => reduce(parity, path),
        ["speed", path] => speed(parity.conversions, path, None),
        ["speed", path, operation] => speed(parity.conversions, path, Some(operation)),
        _ => {
            eprintln!("usage: {USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// The NIF's answer to a request read as JSON, its result as JSON.
pub fn answer(conversions: &dyn Conversions, request: Request) -> Result<Value, String> {
    answer_json(conversions, request).map(|answer| match answer {
        Answer::Text(text) => Value::String(text),
        Answer::Json(json) => json,
        Answer::Document(document) => document.to_json(),
    })
}

/// One side's answer to a request, and when it was made, where that's known.
struct Answered {
    result: Result<Value, String>,
    when: Option<Span>,
}

impl Answered {
    /// The answer `answer` makes, timed.
    fn timed(answer: impl FnOnce() -> Result<Value, String>) -> Answered {
        let from = now();
        let result = answer();
        Answered {
            result,
            when: Some(from..=now()),
        }
    }

    fn error(&self) -> Option<&String> {
        self.result.as_ref().err()
    }
}

fn now() -> u64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a time after 1970");
    since_epoch.as_millis() as u64
}

impl Parity<'_> {
    fn ours(&self, request: &Request) -> Answered {
        Answered::timed(|| answer(self.conversions, request.clone()))
    }

    /// Whether the NIF answers `candidate` wrong the way it answered the request it's shrunk
    /// from. The NIF answers first: an error other than the one it gave there spares asking the
    /// worker, which costs a round trip.
    fn wrong_the_same_way(
        &self,
        worker: &mut Worker,
        candidate: &Request,
        original: &Wrong,
    ) -> bool {
        let ours = self.ours(candidate);
        if ours.error() != original.errors[1].as_ref() {
            return false;
        }
        let theirs = worker.answer(candidate);
        wrong(self.made_up, self.expected, candidate, &theirs, &ours).as_ref() == Some(original)
    }
}

/// A request and the worker's answer to it. A record file is a JSON array of them, each
/// `{"operation", "input", "options"}`, the options left out when there are none, with the
/// worker's `"output"` or its `"error"`, and `"answered": [from, to]`, the [`Span`] the worker
/// answered in, which a record written before it was added lacks.
struct Record {
    request: Request,
    theirs: Answered,
}

impl Record {
    fn read(mut record: Value) -> Record {
        let result = match record.get_mut("error") {
            Some(error) => Err(error.take().into_string().expect("an error's message")),
            None => Ok(record["output"].take()),
        };
        let when = record.get("answered").map(|answered| {
            let time = |index: usize| answered[index].as_u64().expect("a time in milliseconds");
            time(0)..=time(1)
        });
        Record {
            request: read_request(record),
            theirs: Answered { result, when },
        }
    }

    fn into_json(self) -> Value {
        let Request {
            operation,
            input,
            options,
        } = self.request;
        let mut record = Map::new();
        record.push("operation".into(), operation);
        record.push("input".into(), input);
        if let Some(options) = options {
            record.push("options".into(), options);
        }
        match self.theirs.result {
            Ok(output) => record.push("output".into(), output),
            Err(error) => record.push("error".into(), error.into()),
        }
        if let Some(when) = self.theirs.when {
            record.push("answered".into(), json!([*when.start(), *when.end()]));
        }
        Value::Object(record)
    }
}

/// A request of a request file, a JSON array of requests, or of a record file.
fn read_request(mut request: Value) -> Request {
    Request {
        operation: request["operation"].take(),
        input: request["input"].take(),
        options: request.get_mut("options").map(Value::take),
    }
}

/// The JSON array in the file at `path`.
fn array(path: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    from_str(&text)
        .ok()
        .and_then(Value::into_array)
        .unwrap_or_else(|| panic!("{path} holds a JSON array"))
}

/// Prints a record file of the worker's answers to the requests in the file at `path`.
fn record(parity: &Parity, path: &str) -> ExitCode {
    let mut worker = Worker::start(parity.module);
    let records = array(path)
        .into_iter()
        .map(|request| {
            let request = read_request(request);
            let theirs = worker.answer(&request);
            Record { request, theirs }.into_json()
        })
        .collect();
    println!("{}", stringify(&Value::Array(records)));
    ExitCode::SUCCESS
}

/// How the NIF's answer to a request compares with the worker's.
#[derive(Debug, PartialEq)]
enum Verdict {
    Same,
    /// Different, as the application decided it should be, for a reason it gives.
    Expected(&'static str, Difference),
    Differs(Difference),
}

fn judge(
    made_up: MadeUp,
    expected: Expected,
    request: &Request,
    theirs: &Answered,
    ours: &Answered,
) -> Verdict {
    let Some(difference) = difference(made_up, request, theirs, ours) else {
        return Verdict::Same;
    };
    match expected(request, &theirs.result, &ours.result) {
        Some(reason) => Verdict::Expected(reason, difference),
        None => Verdict::Differs(difference),
    }
}

/// Replays the records in the file at `path` through the NIF, and prints how many answers differ
/// in each way, with the first few of each, then how many differ as the application expects. It
/// fails if any differ otherwise.
fn replay(parity: &Parity, path: &str) -> ExitCode {
    let records = array(path);
    let count = records.len();
    let mut differ = Tally::default();
    let mut expected = Tally::default();
    for Record { request, theirs } in records.into_iter().map(Record::read) {
        let ours = parity.ours(&request);
        let shown = |difference: &Difference| {
            format!(
                "{}\n        in: {}",
                describe(difference, &theirs.result, &ours.result),
                truncated(&stringify(&request.input), 400)
            )
        };
        match judge(parity.made_up, parity.expected, &request, &theirs, &ours) {
            Verdict::Same => {}
            Verdict::Expected(reason, difference) => expected.add(reason, || shown(&difference)),
            Verdict::Differs(difference) => {
                differ.add(difference.group(&request), || shown(&difference))
            }
        }
    }
    differ.print(3);
    if expected.total() > 0 {
        println!("as expected:");
        expected.print(1);
    }
    println!(
        "{} same, {} differ, {} differ as expected",
        count - differ.total() - expected.total(),
        differ.total(),
        expected.total()
    );
    match differ.total() {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Answers counted by what they're grouped by, with the first few of each shown.
struct Tally<K> {
    counts: HashMap<K, (usize, Vec<String>)>,
}

impl<K> Default for Tally<K> {
    fn default() -> Self {
        Tally {
            counts: HashMap::new(),
        }
    }
}

/// The most answers of a group shown.
const SHOWN: usize = 3;

impl<K: Display + Eq + Hash> Tally<K> {
    fn add(&mut self, key: K, shown: impl FnOnce() -> String) {
        let (count, shown_so_far) = self.counts.entry(key).or_default();
        *count += 1;
        if shown_so_far.len() < SHOWN {
            shown_so_far.push(shown());
        }
    }

    fn total(&self) -> usize {
        self.counts.values().map(|(count, _)| count).sum()
    }

    /// Prints each group, most answers first, with the first `shown` of its answers.
    fn print(&self, shown: usize) {
        let mut groups: Vec<(String, &(usize, Vec<String>))> = self
            .counts
            .iter()
            .map(|(key, counted)| (key.to_string(), counted))
            .collect();
        groups.sort_by(|(a, (a_count, _)), (b, (b_count, _))| {
            (Reverse(a_count), a).cmp(&(Reverse(b_count), b))
        });
        for (key, (count, answers)) in groups {
            println!("{count:6}  {key}");
            for answer in answers.iter().take(shown) {
                println!("        {answer}");
            }
        }
    }
}

/// How the NIF answers a request wrong: what `reduce` keeps while it shrinks the request.
#[derive(Debug, PartialEq)]
struct Wrong {
    group: Group,
    /// The worker's error and the NIF's, where each erred.
    errors: [Option<String>; 2],
}

/// How the NIF answers `request` wrong, if it answers otherwise than the worker does where the
/// application doesn't expect it to.
fn wrong(
    made_up: MadeUp,
    expected: Expected,
    request: &Request,
    theirs: &Answered,
    ours: &Answered,
) -> Option<Wrong> {
    let Verdict::Differs(difference) = judge(made_up, expected, request, theirs, ours) else {
        return None;
    };
    Some(Wrong {
        group: difference.group(request),
        errors: [theirs.error().cloned(), ours.error().cloned()],
    })
}

/// Shrinks the input of each record whose answer the NIF gets wrong to a smallest input it still
/// gets wrong the same way, asking the worker for its answer to each smaller one, and prints the
/// distinct inputs it ends at, with how many records shrank to each. A smaller input is wrong the
/// same way where the answers differ in the same group, and each side that erred errs with the
/// same message.
fn reduce(parity: &Parity, path: &str) -> ExitCode {
    let mut worker = Worker::start(parity.module);
    let mut reduced = Tally::default();
    for Record { request, theirs } in array(path).into_iter().map(Record::read) {
        let ours = parity.ours(&request);
        let Some(wrong) = wrong(parity.made_up, parity.expected, &request, &theirs, &ours) else {
            continue;
        };
        let input = shrink(request.input.clone(), |input| {
            let candidate = Request {
                operation: request.operation.clone(),
                input: input.clone(),
                options: request.options.clone(),
            };
            parity.wrong_the_same_way(&mut worker, &candidate, &wrong)
        });
        let request = Request { input, ..request };
        let (theirs, ours) = (worker.answer(&request), parity.ours(&request));
        let key = format!("{}  {}", wrong.group, stringify(&request.input));
        reduced.add(key, || {
            match difference(parity.made_up, &request, &theirs, &ours) {
                Some(difference) => describe(&difference, &theirs.result, &ours.result),
                None => "the answers agree when asked again".into(),
            }
        });
    }
    reduced.print(1);
    ExitCode::SUCCESS
}

/// `value` cut as small as [`smaller::first`] cuts it while `keeps` holds of what's left.
fn shrink(mut value: Value, mut keeps: impl FnMut(&Value) -> bool) -> Value {
    while let Some(smaller) = smaller::first(&value, |smaller| {
        let smaller = smaller.value();
        keeps(&smaller).then_some(smaller)
    }) {
        value = smaller;
    }
    value
}

/// Times the NIF's conversions of the requests in the file at `path`, of each operation or only
/// `only`: the median of 5 rounds after a warm-up, per request. A request the NIF refuses isn't
/// timed, and how many were is printed.
fn speed(conversions: &dyn Conversions, path: &str, only: Option<&str>) -> ExitCode {
    let mut operations: Vec<(String, Vec<Request>, usize)> = Vec::new();
    for request in array(path).into_iter().map(read_request) {
        let operation = request.operation.as_str().expect("an operation");
        if only.is_some_and(|only| only != operation) {
            continue;
        }
        let index = match operations.iter().position(|(named, ..)| named == operation) {
            Some(index) => index,
            None => {
                operations.push((operation.to_string(), Vec::new(), 0));
                operations.len() - 1
            }
        };
        let (_, answered, refused) = &mut operations[index];
        match answer(conversions, request.clone()) {
            Ok(_) => answered.push(request),
            Err(_) => *refused += 1,
        }
    }
    for (operation, answered, refused) in &operations {
        let timed = match answered.len() {
            0 => "none answered".to_string(),
            _ => format!(
                "{:>8.1} µs per request",
                median_micros_per_request(conversions, answered)
            ),
        };
        let refused = match refused {
            0 => String::new(),
            refused => format!(", {refused} refused and not timed"),
        };
        println!("{operation:<18} {timed}{refused}");
    }
    ExitCode::SUCCESS
}

fn median_micros_per_request(conversions: &dyn Conversions, requests: &[Request]) -> f64 {
    let run = || {
        for request in requests {
            convert(conversions, request).expect("a request the NIF answers");
        }
    };
    run();
    let mut samples: Vec<f64> = (0..5)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1e6 / requests.len() as f64
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[2]
}

/// The conversion a request asks for, as the NIF makes it once it has checked the request.
fn convert(conversions: &dyn Conversions, request: &Request) -> Result<(), tarnish::Error> {
    let (input, options) = (&request.input, request.options.as_ref());
    let text = || input.as_str().expect("a parse's input is text");
    match request.operation.as_str() {
        Some("serializeHTML") => conversions
            .serialize_html(Document::json(input, conversions.schema()), options)
            .map(used),
        Some("parseHTML") => conversions.parse_html(text(), options).map(used),
        Some("serializeMarkdown") => conversions.serialize_markdown(input, options).map(used),
        Some("parseMarkdown") => conversions.parse_markdown(text(), options).map(used),
        operation => panic!("an operation, not {operation:?}"),
    }
}

fn used<T>(output: T) {
    black_box(output);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(operation: &str) -> Request {
        Request {
            operation: operation.into(),
            input: Value::Null,
            options: None,
        }
    }

    fn answered(result: Result<Value, String>) -> Answered {
        Answered { result, when: None }
    }

    fn none(_: &Request, _: &[Segment], _: Made, _: Made) -> bool {
        false
    }

    /// The worker refuses what the NIF converts, for a parse, as an application may decide.
    fn refusals(
        request: &Request,
        theirs: &Result<Value, String>,
        ours: &Result<Value, String>,
    ) -> Option<&'static str> {
        (request.operation == "parse" && *theirs == Err("too long".into()) && ours.is_ok())
            .then_some("the worker refuses what's too long")
    }

    fn judged(
        operation: &str,
        theirs: Result<Value, String>,
        ours: Result<Value, String>,
    ) -> Verdict {
        judge(
            none,
            refusals,
            &request(operation),
            &answered(theirs),
            &answered(ours),
        )
    }

    #[test]
    fn sets_apart_the_differences_the_application_expects() {
        assert_eq!(judged("parse", Ok(json!(1)), Ok(json!(1))), Verdict::Same);
        assert!(matches!(
            judged("parse", Err("too long".into()), Ok(json!(1))),
            Verdict::Expected("the worker refuses what's too long", _)
        ));
        let differs = |verdict| matches!(verdict, Verdict::Differs(_));
        assert!(differs(judged(
            "parse",
            Err("too long!".into()),
            Ok(json!(1))
        )));
        assert!(differs(judged(
            "parse",
            Err("too long".into()),
            Err("x".into())
        )));
        assert!(differs(judged(
            "serialize",
            Err("too long".into()),
            Ok(json!(1))
        )));
    }

    fn wrong_way(theirs: Result<Value, String>, ours: Result<Value, String>) -> Option<Wrong> {
        wrong(
            none,
            refusals,
            &request("parse"),
            &answered(theirs),
            &answered(ours),
        )
    }

    #[test]
    fn keeps_a_smaller_request_wrong_only_the_same_way() {
        let original = wrong_way(Ok(json!({"a": [{"b": 1}]})), Ok(json!({"a": [{"b": 2}]})));
        assert!(original.is_some());
        let same_group = wrong_way(
            Ok(json!({"a": [{}, {"b": 1}]})),
            Ok(json!({"a": [{}, {"b": 3}]})),
        );
        assert_eq!(same_group, original);
        let elsewhere = wrong_way(
            Ok(json!({"a": [{"b": 1}], "c": 1})),
            Ok(json!({"a": [{"b": 1}], "c": 2})),
        );
        assert_ne!(elsewhere, original);
        let another_kind = wrong_way(Ok(json!({"a": [{"b": 1}]})), Ok(json!({"a": [{"b": "1"}]})));
        assert_ne!(another_kind, original);

        let erring = wrong_way(Err("x".into()), Ok(json!(1)));
        assert!(erring.is_some());
        assert_eq!(wrong_way(Err("x".into()), Ok(json!(2))), erring);
        assert_ne!(wrong_way(Err("y".into()), Ok(json!(1))), erring);
        let both_erring = wrong_way(Err("x".into()), Err("y".into()));
        assert_eq!(wrong_way(Err("x".into()), Err("y".into())), both_erring);
        assert_ne!(wrong_way(Err("x".into()), Err("z".into())), both_erring);
        assert_ne!(wrong_way(Err("w".into()), Err("y".into())), both_erring);

        assert_eq!(wrong_way(Err("too long".into()), Ok(json!(1))), None);
    }

    #[test]
    fn shrinks_while_it_keeps() {
        assert_eq!(shrink(json!("abc"), Value::is_string), json!(""));
        assert_eq!(
            shrink(json!({"markdown": "a\nbc\nd", "other": [1]}), |value| {
                value["markdown"]
                    .as_str()
                    .is_some_and(|text| text.contains('b'))
            }),
            json!({"markdown": "b"})
        );
    }

    #[test]
    fn reads_records_with_and_without_when_the_worker_answered() {
        let record = Record::read(json!({
            "operation": "parse", "input": "x", "output": {"a": 1}, "answered": [5, 7],
        }));
        assert!(record.theirs.when == Some(5..=7));
        assert!(record.theirs.result == Ok(json!({"a": 1})));
        assert_eq!(
            record.into_json(),
            json!({"operation": "parse", "input": "x", "output": {"a": 1}, "answered": [5, 7]})
        );
        let old = Record::read(json!({"operation": "parse", "input": "x", "error": "no"}));
        assert!(old.theirs.when.is_none());
        assert_eq!(
            old.into_json(),
            json!({"operation": "parse", "input": "x", "error": "no"})
        );
    }
}
