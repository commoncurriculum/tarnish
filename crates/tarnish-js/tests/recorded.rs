//! What V8's built-ins that tarnish-js ports give, recorded by `harness/record-js.mjs`: tarnish-js
//! must give the same values and the same errors, and ask a sort's comparator for the same
//! comparisons.

use std::borrow::Cow;

use tarnish_fixtures::{expect, outcome, read, records, same};
use tarnish_js::json::{self, Event, Map, Value, json};
use tarnish_js::value::{self, Iterating};

fn text(value: &Value) -> &str {
    value.as_str().expect("text")
}

/// A number as the recorder writes it: as `String` does, but -0 as "-0".
fn number_text(number: f64) -> String {
    match number == 0.0 && number.is_sign_negative() {
        true => "-0".into(),
        false => tarnish_js::number_to_string(number),
    }
}

/// `JSON.parse(text)` as the recorder records it.
fn parsed(text: &str) -> Value {
    let Ok(value) = json::from_str(text) else {
        return json!({"error": {"class": "SyntaxError"}});
    };
    let numbers: Vec<Value> = json::events(&value)
        .filter_map(|event| match event {
            Event::Scalar(Value::Number(number)) => Some(number_text(number.as_f64()).into()),
            _ => None,
        })
        .collect();
    let mut fields: Map = [
        ("written".into(), json::stringify(&value).into()),
        ("numbers".into(), numbers.into()),
    ]
    .into_iter()
    .collect();
    let string = outcome(tarnish_js::to_string(&value).map(|string| json!({ "string": string })));
    fields.extend(string.as_object().expect("fields").clone());
    fields.into()
}

#[test]
fn parses_json_as_v8_does() {
    let fixtures = read("js");
    for record in records(&fixtures, "parses") {
        expect(record, &["text"], parsed(text(&record["text"])));
    }
    let drawn = records(&fixtures, "drawn");
    for (record, text) in drawn.iter().zip(tarnish_js::random::json_texts()) {
        assert_eq!(record["text"].as_str(), Some(text.as_str()), "a text drawn");
        expect(record, &["text"], parsed(&text));
    }
}

/// Each record of a section that isn't, without its string and radix, the number `convert` gives
/// of it, as [`number_text`] writes it.
fn numbers_differing(fixtures: &Value, section: &str, convert: impl Fn(&Value) -> f64) -> String {
    let differing = records(fixtures, section).iter().filter_map(|record| {
        let number = json!({ "number": number_text(convert(record)) });
        let mut expected = record.as_object().expect("a record").clone();
        expected.retain(|key, _| key != "string" && key != "radix");
        (!same(&expected.into(), &number)).then(|| format!("{record}: {}", number["number"]))
    });
    differing.collect::<Vec<_>>().join("\n")
}

#[test]
fn converts_numbers_as_v8_does() {
    let fixtures = read("js");
    let differing = numbers_differing(&fixtures, "numbers", |record| {
        tarnish_js::string_to_number(text(&record["string"]))
    });
    assert!(differing.is_empty(), "Number(string):\n{differing}");
    let differing = numbers_differing(&fixtures, "parseInts", |record| {
        let string = text(&record["string"]);
        match record.get("radix") {
            Some(radix) => {
                let radix = radix.as_u64().expect("a radix") as u32;
                tarnish_js::parse_int_radix(string, radix)
            }
            None => tarnish_js::parse_int(string),
        }
    });
    assert!(
        differing.is_empty(),
        "parseInt(string, radix):\n{differing}"
    );
}

#[test]
fn reads_and_builds_as_v8_does() {
    let fixtures = read("js");
    for record in records(&fixtures, "reads") {
        let input = record
            .get("json")
            .map(|json| json::from_str(text(json)).expect("JSON"));
        let input = input.as_ref();
        let found = match text(&record["read"]) {
            "get" => value::get(input, text(&record["key"])).map(Option::<&Value>::cloned),
            "index" => {
                let index = record["index"].as_u64().expect("an index") as usize;
                value::index(input, index).map(|found| found.map(Cow::into_owned))
            }
            "arrayMethod" => value::array_method(input, text(&record["callee"]))
                .map(|items| Some(items.to_vec().into())),
            "iterate" => {
                let how = match record.get("expression") {
                    Some(expression) => Iterating::Expression(text(expression)),
                    None => Iterating::Value,
                };
                value::iterate(input, how).map(|items| Some(items.into_owned().into()))
            }
            "string" => tarnish_js::string(input).map(|string| Some(string.into_owned().into())),
            read => panic!("no read {read}"),
        };
        let found = found.map(|found| match found {
            Some(result) => json!({ "result": result }),
            None => json!({}),
        });
        let inputs = ["read", "key", "index", "callee", "expression", "json"];
        expect(record, &inputs, outcome(found));
    }
    for record in records(&fixtures, "sums") {
        let [a, b] = ["a", "b"].map(|key| json::from_str(text(&record[key])).expect("JSON"));
        let sum = tarnish_js::plus(&a, &b).map(|result| json!({ "result": result }));
        expect(record, &["a", "b"], outcome(sum));
    }
    for record in records(&fixtures, "maxima") {
        let [a, b] = ["a", "b"].map(|key| text(&record[key]).parse().expect("a number"));
        let max = json!({ "max": number_text(tarnish_js::math_max(a, b)) });
        expect(record, &["a", "b"], max);
    }
    for record in records(&fixtures, "repeats") {
        let count: f64 = text(&record["count"]).parse().expect("a count");
        let repeated = tarnish_js::repeat(text(&record["string"]), count);
        let repeated = repeated.map(|repeated| json!({ "repeated": repeated }));
        expect(record, &["string", "count"], outcome(repeated));
    }
    for record in records(&fixtures, "replaces") {
        let [string, pattern, replacement] =
            ["string", "pattern", "replacement"].map(|key| text(&record[key]));
        let replaced = tarnish_js::replace_all(string, pattern, replacement);
        let replaced = json!({ "replaced": replaced.expect("a string") });
        expect(record, &["string", "pattern", "replacement"], replaced);
    }
}

/// Each comparison V8's sort asks of the comparator, as the places the two items had before
/// sorting, and where the sort puts each item.
#[test]
fn sorts_as_v8_does() {
    let fixtures = read("js");
    for record in records(&fixtures, "sorts") {
        let items = record["items"].as_array().expect("items");
        let comparator = text(&record["comparator"]);
        let mut places: Vec<usize> = (0..items.len()).collect();
        let mut comparisons = Vec::new();
        tarnish_js::array::sort(&mut places, |a, b| {
            comparisons.push(format!("{a}:{b}"));
            Ok(match comparator {
                "difference" => items[a].as_f64().expect("a number") - items[b].as_f64().unwrap(),
                "contrary" => ((a * 31 + b * 17) % 3) as f64 - 1.0,
                comparator => panic!("no comparator {comparator}"),
            })
        })
        .expect("no comparison throws");
        let recorded: Vec<&str> = text(&record["comparisons"]).split(' ').collect();
        let length = comparisons.len().max(recorded.len());
        let differs = (0..length)
            .find(|&at| comparisons.get(at).map(String::as_str) != recorded.get(at).copied());
        if let Some(at) = differs {
            panic!(
                "comparison {at} of {} items by {comparator}: {:?} where {:?} was recorded",
                items.len(),
                comparisons.get(at),
                recorded.get(at),
            );
        }
        let sorted = json!({ "comparisons": comparisons.join(" "), "sorted": places });
        expect(record, &["items", "comparator"], sorted);
    }
}

#[cfg(feature = "collation")]
#[test]
fn collates_as_node_does() {
    let fixtures = read("js");
    for record in records(&fixtures, "collations") {
        let order = tarnish_js::locale_compare(text(&record["a"]), text(&record["b"]));
        expect(record, &["a", "b"], json!({ "order": order }));
    }
    for record in records(&fixtures, "collated") {
        let words = record["words"].as_array().expect("words");
        let mut sorted: Vec<&str> = words.iter().map(text).collect();
        tarnish_js::array::sort(&mut sorted, |a, b| Ok(tarnish_js::locale_compare(a, b)))
            .expect("no comparison throws");
        expect(record, &["words"], json!({ "sorted": sorted }));
    }
}
