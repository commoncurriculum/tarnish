//! The fixtures `npm run test:js` records from the JavaScript tarnish ports, as
//! `harness/fixture.mjs` writes them, for tarnish's tests to expect.

use tarnish::json::{self, Value, json};
use tarnish::{Error, Schema, api};

/// `fixtures/<name>.json`.
pub fn read(name: &str) -> Value {
    let path = format!("{}/../../fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    json::from_str(&text).unwrap_or_else(|_| panic!("{path} is JSON"))
}

/// The installed version of a package `fixtures/versions.json` names, as a dependent resolves it:
/// `"@tiptap/pm > prosemirror-model"` is the prosemirror-model `@tiptap/pm` loads.
#[track_caller]
pub fn version(package: &str) -> String {
    let version = read("versions")[package].as_str().map(str::to_owned);
    version.unwrap_or_else(|| panic!("no version of {package}"))
}

/// The records of a fixture's `section`, of which there must be some: a loop over none passes.
#[track_caller]
pub fn records<'f>(fixture: &'f Value, section: &str) -> &'f [Value] {
    let records = fixture[section].as_array();
    let records = records.unwrap_or_else(|| panic!("no section {section}"));
    assert!(!records.is_empty(), "no records in {section}");
    records
}

/// The schemas of the specs a fixture's `schemas` lists.
pub fn schemas(fixture: &Value) -> Vec<Schema> {
    records(fixture, "schemas")
        .iter()
        .map(|spec| api::schema(spec).expect("a schema"))
        .collect()
}

/// A run as the fixtures record one: the fields it gives, or its error's class and message.
pub fn outcome<T: Into<Value>>(result: Result<T, Error>) -> Value {
    match result {
        Ok(fields) => fields.into(),
        Err(error) => json!({"error": {"class": error.class(), "message": error.message()}}),
    }
}

/// Asserts that `record`, without the fields `inputs` names, is `outcome`: each field the run
/// gave as the record has it, and no field the record has that the run didn't give.
#[track_caller]
pub fn expect(record: &Value, inputs: &[&str], outcome: Value) {
    let mut recorded = record.as_object().expect("a record's fields").clone();
    for input in inputs {
        recorded.remove(input);
    }
    assert!(
        Value::Object(recorded) == outcome,
        "{outcome} where {record} was recorded"
    );
}
