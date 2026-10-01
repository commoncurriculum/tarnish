//! What zod's `safeParse` gives each schema tarnish-zod ports for each input, recorded by
//! `harness/record-zod.mjs`: tarnish-zod must give the same data or the same issues, and fail
//! where zod throws as it makes the schema.

use std::panic::{self, AssertUnwindSafe};

use tarnish_fixtures::{read, records, version};
use tarnish_js::json::{self, Map, Value, json};
use tarnish_zod::{Issues, Schema};

#[test]
fn ports_the_installed_zod() {
    assert_eq!(version("zod"), tarnish_zod::ZOD);
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.into())
}

fn text(value: &Value) -> &str {
    value.as_str().expect("text")
}

fn shape(entries: &Value) -> Vec<(&'static str, Schema)> {
    let entries = entries.as_array().expect("a shape");
    let entry = |pair: &Value| (leak(text(&pair[0])), build(&pair[1]));
    entries.iter().map(entry).collect()
}

fn keys(keys: &Value) -> Vec<&'static str> {
    let keys = keys.as_array().expect("keys");
    keys.iter().map(|key| leak(text(key))).collect()
}

/// The schema the recorder writes as JSON.
fn build(schema: &Value) -> Schema {
    if let Some(kind) = schema.as_str() {
        return match kind {
            "string" => Schema::String,
            "number" => Schema::Number,
            "int" => Schema::Int,
            "boolean" => Schema::Boolean,
            _ => panic!("no schema {kind}"),
        };
    }
    let (inner, argument) = (&schema[1], schema.get(&2));
    match text(&schema[0]) {
        "enum" => Schema::Enum(Box::leak(keys(inner).into_boxed_slice())),
        "array" => Schema::array(build(inner)),
        "record" => Schema::record(build(inner)),
        "object" => Schema::Object(shape(inner)),
        "strictObject" => Schema::StrictObject(shape(inner)),
        "nullable" => build(inner).nullable(),
        "optional" => build(inner).optional(),
        "nullish" => build(inner).nullish(),
        "strict" => build(inner).strict(),
        "partial" => build(inner).partial(),
        "default" => build(inner).default(argument.expect("a default").clone()),
        "catch" => build(inner).catch(argument.cloned()),
        "pick" => build(inner).pick(&keys(argument.expect("a mask"))),
        "omit" => build(inner).omit(&keys(argument.expect("a mask"))),
        "extend" => build(inner).extend(shape(argument.expect("a shape"))),
        kind => panic!("no schema of kind {kind}"),
    }
}

/// The schema, or the error zod throws making it: tarnish-zod panics where zod throws.
fn made(schema: &Value) -> Result<Schema, Value> {
    panic::catch_unwind(AssertUnwindSafe(|| build(schema))).map_err(|panic| {
        let message = panic.downcast_ref::<String>().expect("a message");
        json!({"error": {"class": "Error", "message": message.as_str()}})
    })
}

/// A parse as the recorder records it.
fn recorded(parsed: Result<Option<Value>, Issues>) -> Value {
    match parsed {
        Ok(Some(data)) => json!({ "data": data }),
        Ok(None) => json!({}),
        Err(issues) => json!({ "message": issues.to_string() }),
    }
}

/// The record without its inputs.
fn expected(record: &Value) -> Value {
    let mut fields = record.as_object().expect("a record").clone();
    fields.remove("schema");
    fields.remove("json");
    Value::Object(fields)
}

/// The first issue's message, as the record's message holds it.
fn first_message(expected: &Value) -> Option<String> {
    let issues = json::from_str(expected.get("message")?.as_str()?).expect("the issues");
    issues[0]["message"].as_str().map(str::to_owned)
}

#[test]
fn parses_as_zod_does() {
    let fixtures = read("zod");
    let schemas: Vec<_> = records(&fixtures, "schemas").iter().map(made).collect();
    let mut failures = Vec::new();
    for record in records(&fixtures, "parses") {
        let index = record["schema"].as_u64().expect("a schema's index") as usize;
        let input = record
            .get("json")
            .map(|text| json::from_str(self::text(text)).expect("JSON"));
        let expected = expected(record);
        let actual = match &schemas[index] {
            Ok(schema) => {
                let parsed = schema.parse(input.as_ref());
                if let Err(issues) = &parsed
                    && Some(issues.first_message().to_owned()) != first_message(&expected)
                {
                    failures.push(format!(
                        "{record}\n  first message: {}",
                        issues.first_message()
                    ));
                }
                recorded(parsed)
            }
            Err(thrown) => thrown.clone(),
        };
        if actual != expected {
            let schema = &fixtures["schemas"][index];
            failures.push(format!(
                "{schema} of {input:?}\n  zod: {expected}\n  tarnish-zod: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// An object schema's other parses give what `parse` gives: `parse_fields` of the value, and
/// those that read an object's properties by key, of an object.
#[test]
fn parses_an_objects_fields_as_zod_does() {
    let fixtures = read("zod");
    let mut failures = Vec::new();
    for record in records(&fixtures, "parses") {
        let index = record["schema"].as_u64().expect("a schema's index") as usize;
        let Ok(schema @ Schema::Object(_)) = made(&fixtures["schemas"][index]) else {
            continue;
        };
        let input = record
            .get("json")
            .map(|text| json::from_str(self::text(text)).expect("JSON"));
        let expected = expected(record);
        let fields = |fields: tarnish_zod::Fields| Some(Value::Object(fields.into_map()));
        let mut parses = vec![(
            "parse_fields",
            recorded(schema.parse_fields(input.as_ref()).map(fields)),
        )];
        if let Some(Value::Object(object)) = &input {
            let property = |key: &str| object.get(key);
            parses.push((
                "parse_properties",
                recorded(schema.parse_properties(property).map(fields)),
            ));
            let safe = schema
                .safe_parse_properties(property)
                .map(|parsed| json!({"data": parsed.into_map()}));
            let unsafe_parse = expected.get("data").map(|_| expected.clone());
            if safe != unsafe_parse {
                failures.push(format!("safe_parse_properties of {record}: {safe:?}"));
            }
            // The fallback is the empty object, which the record of "{}" holds the parse of.
            let empty = Map::new();
            let or_empty = schema.parse_properties_or(property, |key| empty.get(key));
            let fallback = records(&fixtures, "parses").iter().find(|other| {
                other["schema"] == record["schema"]
                    && other.get("json").and_then(Value::as_str) == Some("{}")
            });
            let expected_or = match expected.get("data") {
                Some(_) => expected.clone(),
                None => self::expected(fallback.expect("the parse of {}")),
            };
            let actual_or = recorded(or_empty.map(fields));
            if actual_or != expected_or {
                failures.push(format!("parse_properties_or of {record}\n  zod: {expected_or}\n  tarnish-zod: {actual_or}"));
            }
        }
        for (how, actual) in parses {
            if actual != expected {
                failures.push(format!(
                    "{how} of {record}\n  zod: {expected}\n  tarnish-zod: {actual}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
