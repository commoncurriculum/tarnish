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

/// The schemas of the specs a fixture's `schemas` lists.
pub fn schemas(fixture: &Value) -> Vec<Schema> {
    fixture["schemas"]
        .as_array()
        .expect("schemas")
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

/// Asserts that `record` holds each field of `outcome` as `outcome` has it.
#[track_caller]
pub fn expect(record: &Value, outcome: Value) {
    for (field, value) in outcome.as_object().expect("an outcome's fields") {
        assert!(
            record.get(field.as_str()) == Some(value),
            "{field}: {value} in {record}"
        );
    }
}
