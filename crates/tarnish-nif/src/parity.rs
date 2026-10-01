//! The tools that hold an application's NIF to its Node worker, as tarnish's README describes:
//! `record`, `replay`, `reduce` and `speed`. An application runs them from an example of its own,
//! whose `main` is [`main`] of its [`Parity`].

use std::cmp::Reverse;
use std::hint::black_box;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio};
use std::time::Instant;

use tarnish::js::json::{from_str, json, stringify, stringify_entries};
use tarnish::{Map, Value};

use crate::convert::{Answer, Conversions, Request, answer_json};

/// An application's two makings of its conversions.
pub struct Parity<'a> {
    /// The NIF's.
    pub conversions: &'a dyn Conversions,
    /// The worker's: the module `config :tarnish, Tarnish.Bridge, conversions:` names.
    pub module: &'a Path,
    /// Whether two values, where the worker's answer to a request of `input` and the NIF's
    /// differ, are the same answer all the same, as two ids each made up anew are.
    pub same: Same,
}

pub type Same = fn(input: &Value, theirs: &Value, ours: &Value) -> bool;

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
    answer_json(conversions, request).map(Answer::into_json)
}

/// A request and the worker's answer to it. A record file is a JSON array of them, each
/// `{"operation", "input", "options"}`, the options left out when there are none, with the
/// worker's `"output"` or its `"error"`.
struct Record {
    request: Request,
    answer: Result<Value, String>,
}

impl Record {
    fn read(mut record: Value) -> Record {
        let answer = match record.get_mut("error") {
            Some(error) => Err(error.take().into_string().expect("an error's message")),
            None => Ok(record["output"].take()),
        };
        Record {
            request: read_request(record),
            answer,
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
        match self.answer {
            Ok(output) => record.push("output".into(), output),
            Err(error) => record.push("error".into(), error.into()),
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

/// A Node worker, as `Tarnish.Bridge.Pool` runs one: tarnish's `worker.mjs`, of the commit this
/// crate is, on the application's conversions, on the `node` the pool runs by default.
struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    sent: u64,
}

impl Worker {
    fn start(module: &Path) -> Worker {
        let mut child = Command::new("node")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../elixir/priv/worker.mjs"
            ))
            .arg(module)
            // A shell's, which can change an answer (a stack size, say); the VM's has none.
            .env_remove("NODE_OPTIONS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("node starts");
        let stdin = child.stdin.take().expect("the worker's stdin");
        let stdout = BufReader::new(child.stdout.take().expect("the worker's stdout"));
        let mut worker = Worker {
            child,
            stdin,
            stdout,
            sent: 0,
        };
        let ready = worker.line();
        assert!(
            ready == json!({"ready": true}),
            "the worker's first line: {ready}"
        );
        worker
    }

    fn answer(&mut self, request: &Request) -> Result<Value, String> {
        self.sent += 1;
        let id = Value::from(self.sent);
        let frame = [
            ("id", &id),
            ("operation", &request.operation),
            ("input", &request.input),
        ]
        .into_iter()
        .chain(request.options.as_ref().map(|options| ("options", options)));
        writeln!(self.stdin, "{}", stringify_entries(frame)).expect("the worker reads a request");
        let mut response = self.line();
        assert!(
            response["id"] == id,
            "the answer to request {id}: {response}"
        );
        match response.get_mut("error") {
            Some(error) => Err(error.take().into_string().expect("an error's message")),
            None => Ok(response["result"].take()),
        }
    }

    fn line(&mut self) -> Value {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("the worker writes");
        assert!(read > 0, "the worker exited");
        from_str(&line).unwrap_or_else(|_| panic!("the worker's line {line:?} is JSON"))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Prints a record file of the worker's answers to the requests in the file at `path`.
fn record(parity: &Parity, path: &str) -> ExitCode {
    let mut worker = Worker::start(parity.module);
    let records = array(path)
        .into_iter()
        .map(|request| {
            let request = read_request(request);
            let answer = worker.answer(&request);
            Record { request, answer }.into_json()
        })
        .collect();
    println!("{}", stringify(&Value::Array(records)));
    ExitCode::SUCCESS
}

/// Where the NIF's answer differs from the worker's.
#[derive(Debug, PartialEq)]
struct Difference {
    /// The path to the difference without its indexes, which differences are grouped by.
    group: String,
    shown: String,
}

/// Where `ours` differs from `theirs`, two answers to a request of `input`. An object's keys may
/// come in any order.
fn difference(
    same: Same,
    input: &Value,
    theirs: &Result<Value, String>,
    ours: &Result<Value, String>,
) -> Option<Difference> {
    let (path, shown) = match (theirs, ours) {
        (Ok(theirs), Ok(ours)) => first_difference(same, input, String::new(), theirs, ours)?,
        (Err(theirs), Err(ours)) if theirs == ours => return None,
        (Err(theirs), Err(ours)) => (
            "error".into(),
            format!("error: js {theirs:?} ours {ours:?}"),
        ),
        (Err(theirs), Ok(_)) => ("js errors".into(), theirs.clone()),
        (Ok(_), Err(ours)) => ("ours errors".into(), ours.clone()),
    };
    Some(Difference {
        group: path.chars().filter(|c| !c.is_ascii_digit()).collect(),
        shown,
    })
}

/// The path to where `ours` first differs from `theirs`, and what differs there.
fn first_difference(
    same: Same,
    input: &Value,
    path: String,
    theirs: &Value,
    ours: &Value,
) -> Option<(String, String)> {
    match (theirs, ours) {
        (Value::Object(theirs), Value::Object(ours)) => {
            for (key, value) in theirs {
                let path = format!("{path}.{key}");
                let Some(other) = ours.get(key) else {
                    let shown = format!("{path} missing");
                    return Some((path, shown));
                };
                if let Some(difference) = first_difference(same, input, path, value, other) {
                    return Some(difference);
                }
            }
            let extra = ours.keys().find(|key| theirs.get(key).is_none())?;
            let path = format!("{path}.{extra}");
            let shown = format!("{path} extra");
            Some((path, shown))
        }
        (Value::Array(theirs), Value::Array(ours)) => {
            for (index, (value, other)) in theirs.iter().zip(ours).enumerate() {
                let path = format!("{path}[{index}]");
                if let Some(difference) = first_difference(same, input, path, value, other) {
                    return Some(difference);
                }
            }
            (theirs.len() != ours.len()).then(|| {
                (
                    format!("{path} length"),
                    format!("{path} length: js {} ours {}", theirs.len(), ours.len()),
                )
            })
        }
        _ if theirs == ours || same(input, theirs, ours) => None,
        _ => {
            let shown = format!("{path}: js {theirs} ours {ours}");
            Some((path, shown))
        }
    }
}

/// Replays the records in the file at `path` through the NIF, and prints how many answers differ
/// in each way, with the first few of each. It fails if any do.
fn replay(parity: &Parity, path: &str) -> ExitCode {
    let records = array(path);
    let count = records.len();
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for Record {
        request,
        answer: theirs,
    } in records.into_iter().map(Record::read)
    {
        let input = request.input.clone();
        let ours = answer(parity.conversions, request);
        let Some(Difference { group, shown }) = difference(parity.same, &input, &theirs, &ours)
        else {
            continue;
        };
        let shown = format!("{shown}\n        in: {}", truncate(&stringify(&input), 400));
        match groups.iter_mut().find(|(named, _)| *named == group) {
            Some((_, differences)) => differences.push(shown),
            None => groups.push((group, vec![shown])),
        }
    }
    groups.sort_by_key(|(_, differences)| Reverse(differences.len()));
    for (group, differences) in &groups {
        println!("{:6}  {group}", differences.len());
        for difference in differences.iter().take(3) {
            println!("        {}", truncate(difference, 600));
        }
    }
    let differ: usize = groups
        .iter()
        .map(|(_, differences)| differences.len())
        .sum();
    println!("{} same, {differ} differ", count - differ);
    match differ {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Shrinks the input of each record in the file at `path` whose answer the NIF gets wrong to a
/// smallest input it still gets wrong the same way, asking the worker for its answer to each
/// smaller one, and prints the distinct inputs it ends at, with how many records shrank to each.
fn reduce(parity: &Parity, path: &str) -> ExitCode {
    let mut worker = Worker::start(parity.module);
    let mut reduced: Vec<(String, usize)> = Vec::new();
    for original in array(path).into_iter().map(Record::read) {
        let ours = answer(parity.conversions, original.request.clone());
        let input = &original.request.input;
        if difference(parity.same, input, &original.answer, &ours).is_none() {
            continue;
        }
        let mut request = original.request.clone();
        'shrink: loop {
            let shrinking = request.input.clone();
            for input in smaller(&shrinking) {
                let candidate = Request {
                    input,
                    ..request.clone()
                };
                if wrong_the_same_way(parity, &mut worker, &candidate, &original.answer, &ours) {
                    request = candidate;
                    continue 'shrink;
                }
            }
            break;
        }
        let shown = |answer: Result<Value, String>| match answer {
            Ok(output) => truncate(&stringify(&output), 200).to_string(),
            Err(error) => format!("error {}", truncate(&error, 200)),
        };
        let line = format!(
            "{} {}\n        js   {}\n        ours {}",
            request.operation.as_str().unwrap_or_default(),
            stringify(&request.input),
            shown(worker.answer(&request)),
            shown(answer(parity.conversions, request.clone())),
        );
        match reduced.iter_mut().find(|(reached, _)| *reached == line) {
            Some((_, count)) => *count += 1,
            None => reduced.push((line, 1)),
        }
    }
    reduced.sort_by_key(|&(_, count)| Reverse(count));
    for (line, count) in &reduced {
        println!("{count:6}  {line}");
    }
    ExitCode::SUCCESS
}

/// Whether the NIF answers `candidate` otherwise than the worker does, erring where it erred on
/// the request it's shrunk from, while the worker errs as it did there, with the same message.
/// The NIF answers first, since asking the worker costs a round trip.
fn wrong_the_same_way(
    parity: &Parity,
    worker: &mut Worker,
    candidate: &Request,
    theirs_before: &Result<Value, String>,
    ours_before: &Result<Value, String>,
) -> bool {
    let ours = answer(parity.conversions, candidate.clone());
    if ours.is_ok() != ours_before.is_ok() {
        return false;
    }
    let theirs = worker.answer(candidate);
    difference(parity.same, &candidate.input, &theirs, &ours).is_some()
        && match (&theirs, theirs_before) {
            (Err(theirs), Err(before)) => theirs == before,
            (theirs, before) => theirs.is_ok() == before.is_ok(),
        }
}

/// Values one step smaller than `value`: each with one array item or object entry dropped, or
/// with an array replaced by one of its items, then the same one level down. Each is made as it's
/// asked for, since there are as many as the value has parts.
fn smaller(value: &Value) -> Box<dyn Iterator<Item = Value> + '_> {
    match value {
        Value::Array(items) => Box::new(
            (0..items.len())
                .map(move |index| {
                    let mut fewer = items.clone();
                    fewer.remove(index);
                    Value::Array(fewer)
                })
                .chain(items.iter().filter(|item| !item.is_null()).cloned())
                .chain(items.iter().enumerate().flat_map(move |(index, item)| {
                    smaller(item).map(move |replacement| {
                        let mut changed = items.clone();
                        changed[index] = replacement;
                        Value::Array(changed)
                    })
                })),
        ),
        Value::Object(map) => Box::new(
            map.keys()
                .map(move |key| {
                    let mut fewer = map.clone();
                    fewer.shift_remove(key);
                    Value::Object(fewer)
                })
                .chain(map.iter().flat_map(move |(key, item)| {
                    smaller(item).map(move |replacement| {
                        let mut changed = map.clone();
                        changed.insert(key.clone(), replacement);
                        Value::Object(changed)
                    })
                })),
        ),
        Value::String(text) if text.chars().count() > 1 => Box::new(std::iter::once(
            Value::String(text.chars().take(1).collect()),
        )),
        _ => Box::new(std::iter::empty()),
    }
}

/// Times the NIF's conversions of the requests in the file at `path`, of each operation or only
/// `only`: the median of 5 rounds after a warm-up, per request.
fn speed(conversions: &dyn Conversions, path: &str, only: Option<&str>) -> ExitCode {
    let mut operations: Vec<(String, Vec<Request>)> = Vec::new();
    for request in array(path).into_iter().map(read_request) {
        let operation = request
            .operation
            .as_str()
            .expect("an operation")
            .to_string();
        match operations.iter_mut().find(|(named, _)| *named == operation) {
            Some((_, requests)) => requests.push(request),
            None => operations.push((operation, vec![request])),
        }
    }
    for (operation, requests) in &operations {
        if only.is_some_and(|only| only != operation) {
            continue;
        }
        let micros = median_micros_per_request(conversions, requests);
        println!("{operation:<18} {micros:>8.1} µs per request");
    }
    ExitCode::SUCCESS
}

fn median_micros_per_request(conversions: &dyn Conversions, requests: &[Request]) -> f64 {
    let run = || {
        for request in requests {
            black_box(convert(conversions, request).ok());
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
        Some("serializeHTML") => conversions.serialize_html(input.into(), options).map(used),
        Some("parseHTML") => conversions.parse_html(text(), options).map(used),
        Some("serializeMarkdown") => conversions.serialize_markdown(input, options).map(used),
        Some("parseMarkdown") => conversions.parse_markdown(text(), options).map(used),
        operation => panic!("an operation, not {operation:?}"),
    }
}

fn used<T>(output: T) {
    black_box(output);
}

fn truncate(text: &str, length: usize) -> &str {
    match text.char_indices().nth(length) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(_: &Value, _: &Value, _: &Value) -> bool {
        false
    }

    fn differs(theirs: Value, ours: Value) -> Option<Difference> {
        difference(exact, &Value::Null, &Ok(theirs), &Ok(ours))
    }

    fn shown(group: &str, shown: &str) -> Option<Difference> {
        Some(Difference {
            group: group.into(),
            shown: shown.into(),
        })
    }

    #[test]
    fn finds_the_first_difference_grouped_without_indexes() {
        assert_eq!(
            differs(json!({"a": 1, "b": 2}), json!({"b": 2, "a": 1})),
            None
        );
        assert_eq!(
            differs(json!({"a": [1, {"b": 2}]}), json!({"a": [1, {"b": 3}]})),
            shown(".a[].b", ".a[1].b: js 2 ours 3")
        );
        assert_eq!(
            differs(json!({"a": 1}), json!({"a": 1, "b": 2})),
            shown(".b", ".b extra")
        );
        assert_eq!(
            differs(json!({"a": 1}), json!({})),
            shown(".a", ".a missing")
        );
        assert_eq!(
            differs(json!([1]), json!([1, 2])),
            shown(" length", " length: js 1 ours 2")
        );
        assert_eq!(
            difference(exact, &Value::Null, &Err("x".into()), &Ok(Value::Null)),
            shown("js errors", "x")
        );
    }

    #[test]
    fn takes_values_the_application_calls_the_same_as_the_same() {
        fn made_up(input: &Value, theirs: &Value, ours: &Value) -> bool {
            *input == "id?" && *theirs == "made up" && *ours == "made up too"
        }
        let answers = |input| {
            difference(
                made_up,
                &json!(input),
                &Ok(json!({"id": "made up"})),
                &Ok(json!({"id": "made up too"})),
            )
        };
        assert_eq!(answers("id?"), None);
        assert_eq!(
            answers("no id"),
            shown(".id", r#".id: js "made up" ours "made up too""#)
        );
    }

    #[test]
    fn makes_values_one_step_smaller() {
        let smaller: Vec<Value> = smaller(&json!([{"a": "xy"}, null])).collect();
        assert_eq!(
            smaller,
            [
                json!([null]),
                json!([{"a": "xy"}]),
                json!({"a": "xy"}),
                json!([{}, null]),
                json!([{"a": "x"}, null]),
            ]
        );
    }
}
