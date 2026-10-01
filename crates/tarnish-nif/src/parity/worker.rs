//! A Node worker, as `Tarnish.Bridge.Pool` runs one.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use tarnish::Value;
use tarnish::js::json::{from_str, json, stringify_entries};

use super::Answered;
use crate::convert::Request;

/// tarnish's `worker.mjs`, of the commit this crate is, on the application's conversions, on the
/// `node` the pool runs by default.
pub(super) struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    sent: u64,
}

impl Worker {
    pub(super) fn start(module: &Path) -> Worker {
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

    pub(super) fn answer(&mut self, request: &Request) -> Answered {
        self.sent += 1;
        let id = Value::from(self.sent);
        let frame = [
            ("id", &id),
            ("operation", &request.operation),
            ("input", &request.input),
        ]
        .into_iter()
        .chain(request.options.as_ref().map(|options| ("options", options)));
        Answered::timed(|| {
            writeln!(self.stdin, "{}", stringify_entries(frame))
                .expect("the worker reads a request");
            let mut response = self.line();
            assert!(
                response["id"] == id,
                "the answer to request {id}: {response}"
            );
            match response.get_mut("error") {
                Some(error) => Err(error.take().into_string().expect("an error's message")),
                None => Ok(response["result"].take()),
            }
        })
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
