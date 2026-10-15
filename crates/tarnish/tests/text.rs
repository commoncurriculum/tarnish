//! A document read from JSON text is the one read from the value `JSON.parse` gives for it, or
//! fails the same way: every document the recordings hold, and values made to probe what the
//! text reader handles itself.

use tarnish::json::{self, Value, json};
use tarnish::{Node, Schema};
use tarnish_fixtures::{read, records, schemas};

/// What reading gives, as text: the node's JSON, or its error.
fn outcome(read: tarnish::Result<Node>) -> String {
    match read {
        Ok(node) => node.to_json_string(),
        Err(error) => format!("{}: {}", error.class(), error.message()),
    }
}

fn assert_reads_alike(schema: &Schema, text: &str) {
    let from_value = match json::from_str(text) {
        Ok(value) => outcome(Node::from_json(schema, &value)),
        Err(_) => "SyntaxError: Invalid JSON".to_string(),
    };
    assert_eq!(
        outcome(Node::from_json_text(schema, text)),
        from_value,
        "{text}"
    );
}

/// Every document in a recording's records, under `key`.
fn documents<'f>(fixture: &'f Value, section: &str, key: &str) -> Vec<(usize, &'f Value)> {
    records(fixture, section)
        .iter()
        .filter_map(|case| {
            let schema = case.get("schema").and_then(Value::as_u64).unwrap_or(0) as usize;
            Some((schema, case.get(key)?))
        })
        .collect()
}

#[test]
fn recorded_documents_read_alike() {
    for (name, sections) in [
        ("transform", &[("tests", "start"), ("tests", "result")][..]),
        (
            "ops",
            &[
                ("transforms", "doc"),
                ("textContent", "doc"),
                ("textsBetween", "doc"),
            ],
        ),
        (
            "model",
            &[
                ("transforms", "start"),
                ("fromJSON", "json"),
                ("check", "json"),
            ],
        ),
    ] {
        let fixture = read(name);
        let schemas = schemas(&fixture);
        for &(section, key) in sections {
            for (schema, doc) in documents(&fixture, section, key) {
                assert_reads_alike(&schemas[schema], &json::stringify(doc));
            }
        }
    }
}

#[test]
fn edge_cases_read_alike() {
    let fixture = read("model");
    let schemas = schemas(&fixture);
    let paragraph = |attrs: Value| json!({"type": "doc", "content": [{"type": "heading", "attrs": attrs, "content": [{"type": "text", "text": "x"}]}]});
    let mut texts: Vec<String> = [
        json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": ""}]}]}),
        json!({"type": "doc", "content": [{"type": "nope"}]}),
        json!({"type": "doc", "content": [{"type": "paragraph", "marks": [{"type": "nope"}]}]}),
        json!({"type": "doc", "content": "no"}),
        json!({"type": "doc", "content": [null]}),
        json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "a", "marks": [{"type": "em"}, {"type": "strong"}]}, {"type": "text", "text": "b", "marks": [{"type": "strong"}, {"type": "em"}]}]}]}),
        json!({"type": 1}),
        json!(null),
        json!(0),
        json!([]),
        paragraph(json!({"level": 2})),
        paragraph(json!({"level": "2"})),
        paragraph(json!({"level": 2.5})),
        paragraph(json!({"level": null})),
        paragraph(json!({"level": {"1": 1, "b": 2, "0": 3}})),
        paragraph(json!({"level": [1, [2, {"a": true}]]})),
        paragraph(json!(null)),
        paragraph(json!(false)),
        paragraph(json!(1)),
        paragraph(json!("attrs")),
        paragraph(json!({})),
    ]
    .iter()
    .map(json::stringify)
    .collect();
    texts.extend(
        [
            r#"{"type":"doc","type":"nope","content":[]}"#,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"\ud800"}]}]}"#,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"😀 é"}]}]}"#,
            r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":1e400},"content":[]}]}"#,
            r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":123456789012345678901234567890}}]}"#,
            r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":-0}}]}"#,
            "\u{FEFF}{\"type\":\"doc\"}",
            r#"{"type":"doc",}"#,
            r#"{"type":"doc"} x"#,
            "",
        ]
        .map(String::from),
    );
    for schema in &schemas {
        for text in &texts {
            assert_reads_alike(schema, text);
        }
    }
}
