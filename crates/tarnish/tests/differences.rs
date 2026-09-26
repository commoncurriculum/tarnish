//! Input ProseMirror takes and tarnish refuses: a step's position that isn't a whole number from
//! zero up, or a range that runs backwards, which gives JavaScript negative sizes.

use tarnish::json::{self, Value};
use tarnish::transform::Step;
use tarnish::{Schema, api};

fn schema() -> Schema {
    let fixtures: Value = json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/transform.json"
        ))
        .expect("the fixtures"),
    )
    .expect("JSON");
    api::schema(&fixtures["schemas"][0]).expect("a schema")
}

#[test]
fn steps_out_of_order_or_off_whole_positions_are_invalid_input() {
    let schema = schema();
    let doc = json::from_str(
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdef"}]}]}"#,
    )
    .expect("JSON");
    let doc = tarnish::Node::from_json(&schema, &doc).expect("a document");
    for (step, class) in [
        (r#"{"stepType":"replace","from":5,"to":3}"#, "ReplaceStep"),
        (r#"{"stepType":"replace","from":1.5,"to":3}"#, "ReplaceStep"),
        (r#"{"stepType":"replace","from":-1,"to":3}"#, "ReplaceStep"),
        (
            r#"{"stepType":"replaceAround","from":0,"to":8,"gapFrom":6,"gapTo":4,"insert":0}"#,
            "ReplaceAroundStep",
        ),
        (
            r#"{"stepType":"replaceAround","from":0,"to":8,"gapFrom":1,"gapTo":7,"insert":1}"#,
            "ReplaceAroundStep",
        ),
        (
            r#"{"stepType":"addMark","from":5,"to":3,"mark":{"type":"em"}}"#,
            "AddMarkStep",
        ),
    ] {
        let step = json::from_str(step).expect("JSON");
        let expected = format!("RangeError: Invalid input for {class}.fromJSON");
        let refused = Step::from_json(&schema, &step).map(drop);
        assert_eq!(refused.expect_err("invalid").to_string(), expected);
        let steps = Value::Array(vec![step]);
        let mapped = api::map_position(&schema, &steps, 4, 1).map(drop);
        assert_eq!(mapped.expect_err("invalid").to_string(), expected);
        let applied = api::apply_steps(&doc, &steps).map(drop);
        assert_eq!(applied.expect_err("invalid").to_string(), expected);
    }
}
