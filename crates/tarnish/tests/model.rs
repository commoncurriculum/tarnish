//! What the real ProseMirror packages do with inputs their own tests don't give them, recorded
//! by `harness/record-model.mjs`: tarnish must do the same.

use tarnish::json::{self, Value, json};
use tarnish::transform::{Step, Transform};
use tarnish::{Fragment, JsonKind, JsonRef, Node, Schema, api};
use tarnish_fixtures::{assert_same, expect, mapped, outcome, read, records, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("model");
    (schemas(&fixtures), fixtures)
}

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

/// A record's JSON, as its `text` where it has one: numbers spelled as JSON.stringify wouldn't.
fn given(case: &Value) -> Value {
    match case.get("text") {
        Some(text) => json::from_str(text.as_str().expect("JSON text")).expect("JSON"),
        None => case["json"].clone(),
    }
}

#[test]
fn nodes_from_json_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        let node = Node::from_json(schema(&schemas, case), &given(case));
        let node = node.map(|node| json!({ "result": node.to_json() }));
        expect(case, &["schema", "json", "text"], outcome(node));
    }
}

/// The same records read from their text, as `Node.fromJSON(schema, JSON.parse(text))`.
#[test]
fn nodes_from_json_text_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        let text = match case.get("text") {
            Some(text) => text.as_str().expect("JSON text").to_string(),
            None => json::stringify(&case["json"]),
        };
        let node = Node::from_json_text(schema(&schemas, case), &text);
        let node = node.map(|node| json!({ "result": node.to_json() }));
        expect(case, &["schema", "json", "text"], outcome(node));
    }
}

#[test]
fn filled_nodes_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "createAndFill") {
        let filled =
            schema(&schemas, case)
                .top_node_type()
                .create_and_fill(None, Fragment::empty(), &[]);
        let filled = filled.map(|node| node.map_or(Value::Null, |node| node.to_json().to_value()));
        let filled = filled.map(|filled| json!({ "result": filled }));
        expect(case, &["schema"], outcome(filled));
    }
}

#[test]
fn schemas_and_their_errors_are_prosemirrors() {
    let (_, fixtures) = fixtures();
    for case in records(&fixtures, "newSchema") {
        let schema = api::schema(&case["spec"]).map(|_| json!({ "result": null }));
        expect(case, &["spec"], outcome(schema));
    }
}

#[test]
fn checks_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "check") {
        let doc = Node::from_json(schema(&schemas, case), &case["json"]);
        let checked = doc
            .and_then(|doc| doc.check())
            .map(|()| json!({ "result": null }));
        expect(case, &["schema", "json"], outcome(checked));
    }
}

/// Steps that split surrogate pairs leave ProseMirror's lone surrogates, which JSON, holding none,
/// has as U+FFFD, each in its place; steps that fail, fail with ProseMirror's errors.
#[test]
fn steps_give_prosemirrors_text_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "transforms") {
        let schema = schema(&schemas, case);
        let doc = Node::from_json(schema, &case["start"]).expect("a document");
        let inputs = ["schema", "start", "steps", "wellFormed"];
        let changed = match api::apply_steps(&doc, &case["steps"]) {
            Ok(changed) => changed,
            Err(error) => {
                expect(case, &inputs, outcome::<Value>(Err(error)));
                continue;
            }
        };
        let written = json!({
            "result": changed.to_json_string(),
            "inverted": api::invert_steps(&doc, &case["steps"]).expect("their inverse"),
            "mapped": mapped(schema, &case["steps"], doc.content().size()),
        });
        expect(case, &inputs, written);
        let text = json::from_str(case["result"].as_str().expect("JSON text")).expect("JSON");
        let read = Node::from_json(schema, &text).expect("a document");
        assert_same(
            case.get("wellFormed").unwrap_or(&text),
            &read.to_json().to_value(),
        );

        // The steps themselves, which keep their lone surrogates, undo the change exactly.
        let mut tr = Transform::new(doc.clone());
        for step in case["steps"].as_array().expect("steps") {
            tr.step(Step::from_json(schema, step).expect("a step"))
                .expect("the step");
        }
        let mut undo = Transform::new(tr.doc().clone());
        for (step, before) in tr.steps().iter().zip(tr.docs()).rev() {
            undo.step(step.invert(before).expect("an inverse"))
                .expect("the inverse");
        }
        assert!(*undo.doc() == doc, "{}", undo.doc().to_json_string());
    }
}

/// A node's JSON read in place is the value it copies out, key for key and in the same order,
/// and writes the same text.
fn assert_view(node: &Node) {
    let view = node.to_json();
    let owned = view.to_value();
    assert!(view == owned, "{view} is not {}", json::stringify(&owned));
    assert!(node.to_json() == view);
    assert_eq!(view.to_json_string(), node.to_json_string());
    walk(view, &owned);
}

// bounded: as deep as a fixture document
fn walk(view: JsonRef, value: &Value) {
    match (view.kind(), value) {
        (JsonKind::Array(len), Value::Array(items)) => {
            assert_eq!(len, items.len());
            assert_eq!(view.items().len(), len);
            for (index, item) in items.iter().enumerate() {
                walk(view.at(index).expect("an item"), item);
            }
            assert!(view.at(len).is_none());
        }
        (JsonKind::Object(len), Value::Object(map)) => {
            assert_eq!(len, map.len());
            let keys: Vec<_> = view.entries().map(|(key, _)| key).collect();
            let expected: Vec<_> = map.keys().map(|key| key.as_str()).collect();
            assert_eq!(keys, expected);
            for (key, item) in map.iter() {
                walk(view.get(key).expect("a key"), item);
            }
            assert!(view.get("no such key").is_none());
        }
        _ => assert!(view == *value, "{view} is not {}", json::stringify(value)),
    }
}

#[test]
fn json_views_read_as_to_json() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        if let Ok(node) = Node::from_json(schema(&schemas, case), &given(case)) {
            assert_view(&node);
            node.descendants(&mut |child, _, _, _| {
                assert_view(child);
                for mark in child.marks().iter() {
                    assert!(mark.to_json() == mark.to_json().to_value());
                }
                Ok(true)
            })
            .expect("a walk");
        }
    }
    for case in records(&fixtures, "transforms") {
        let doc = Node::from_json(schema(&schemas, case), &case["start"]).expect("a document");
        if let Ok(changed) = api::apply_steps(&doc, &case["steps"]) {
            assert_view(&changed);
        }
    }
}
