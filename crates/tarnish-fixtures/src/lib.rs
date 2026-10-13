//! The fixtures `npm run test:js` records from the JavaScript tarnish ports, as
//! `harness/fixture.mjs` writes them, for tarnish's tests to expect. And [`recursion`], which
//! holds a crate's source to growing the stack wherever it recurses.

pub mod recursion;

use std::collections::BTreeMap;

use tarnish::json::{self, Map, Value, json};
use tarnish::transform::{MapResult, Mappable, Mapping, Step};
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

/// Where every position of a document `size` long maps through the steps, with `assoc` -1 and 1,
/// as `harness/fixture.mjs`'s `mapped` records it.
pub fn mapped(schema: &Schema, steps: &Value, size: usize) -> Value {
    let mut mapping = Mapping::new();
    for step in steps.as_array().expect("steps") {
        let step = Step::from_json(schema, step).expect("a step");
        mapping.append_map(step.get_map(), None);
    }
    let assocs = [-1, 1].map(|assoc| {
        let results: Vec<MapResult> = (0..=size)
            .map(|pos| mapping.map_result(pos, assoc))
            .collect();
        let flagged = |flag: fn(&MapResult) -> bool| -> Value {
            let at = results
                .iter()
                .enumerate()
                .filter(|(_, result)| flag(result));
            Value::Array(at.map(|(pos, _)| pos.into()).collect())
        };
        let pos: Vec<Value> = results.iter().map(|result| result.pos.into()).collect();
        json!({
            "assoc": assoc,
            "pos": pos,
            "deleted": flagged(MapResult::deleted),
            "deletedBefore": flagged(MapResult::deleted_before),
            "deletedAfter": flagged(MapResult::deleted_after),
            "deletedAcross": flagged(MapResult::deleted_across),
        })
    });
    Value::Array(assocs.into())
}

/// Whether two values are the same as `JSON.stringify` writes them: the order of an object's keys
/// counts, as it does in what JavaScript writes, where `Value`'s `==` takes any order.
pub fn same(expected: &Value, actual: &Value) -> bool {
    json::stringify(expected) == json::stringify(actual)
}

/// Asserts that `actual` is `expected`, as [`same`] compares them.
#[track_caller]
pub fn assert_same(expected: &Value, actual: &Value) {
    let (expected, actual) = (json::stringify(expected), json::stringify(actual));
    assert!(expected == actual, "{actual} where {expected} was expected");
}

/// Asserts that `record`, without the fields `inputs` names, is `outcome`: each field the run
/// gave the same, as [`same`] compares them, as the record has it, and no field the record has
/// that the run didn't give. The order of the fields themselves, which a test assembles, doesn't
/// count.
#[track_caller]
pub fn expect(record: &Value, inputs: &[&str], outcome: Value) {
    let written = |fields: &Map, skipped: &[&str]| -> BTreeMap<String, String> {
        let kept = fields
            .iter()
            .filter(|(key, _)| !skipped.contains(&key.as_str()));
        kept.map(|(key, value)| (key.to_string(), json::stringify(value)))
            .collect()
    };
    let recorded = written(record.as_object().expect("a record's fields"), inputs);
    let ran = written(outcome.as_object().expect("a run's fields"), &[]);
    assert!(recorded == ran, "{ran:?} where {recorded:?} was recorded");
}
