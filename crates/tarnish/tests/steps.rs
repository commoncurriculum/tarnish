//! A step type of an application's own, recorded from the real ProseMirror packages by
//! `harness/record-steps.mjs`, which defines the same step in JavaScript.

use std::sync::{Arc, LazyLock};

use tarnish::json::{Value, json};
use tarnish::transform::{
    CustomStep, Mappable, Mapping, Step, StepMap, StepResult, Transform, register_step,
};
use tarnish::{Error, Fragment, Node, Result, Schema, Slice, Text, api};
use tarnish_fixtures::{expect, outcome, read, records};

#[derive(Debug)]
struct InsertText {
    pos: usize,
    text: String,
}

impl InsertText {
    fn len(&self) -> usize {
        Text::from(self.text.as_str()).len()
    }

    fn from_json(_: &Schema, json: &Value) -> Result<Arc<dyn CustomStep>> {
        match (json["pos"].as_f64(), json["text"].as_str()) {
            (Some(pos), Some(text)) => Ok(Arc::new(InsertText {
                pos: pos as usize,
                text: text.to_owned(),
            })),
            _ => Err(Error::Range(
                "Invalid input for InsertTextStep.fromJSON".into(),
            )),
        }
    }
}

impl CustomStep for InsertText {
    fn json_id(&self) -> &str {
        "insertText"
    }

    fn apply<'a>(&self, doc: Node<'a>) -> Result<StepResult<'a>> {
        let text = doc.schema().text(self.text.as_str(), &[])?;
        let slice = Slice::new(Fragment::from_node(text), 0, 0);
        StepResult::from_replace(doc, self.pos, self.pos, &slice)
    }

    fn get_map(&self) -> StepMap {
        StepMap::new(vec![self.pos, 0, self.len()], false)
    }

    fn invert<'a>(&self, _: &Node<'a>) -> Result<Step<'a>> {
        Ok(Step::Replace {
            from: self.pos,
            to: self.pos + self.len(),
            slice: Slice::empty(),
            structure: false,
        })
    }

    fn map<'a>(&self, mapping: &dyn Mappable) -> Option<Step<'a>> {
        let result = mapping.map_result(self.pos, 1);
        (!result.deleted_across()).then(|| {
            Step::Custom(Arc::new(InsertText {
                pos: result.pos,
                text: self.text.clone(),
            }))
        })
    }

    fn merge<'a>(&self, other: &Step<'a>) -> Option<Step<'a>> {
        let other = other.custom::<InsertText>()?;
        (other.pos == self.pos + self.len()).then(|| {
            Step::Custom(Arc::new(InsertText {
                pos: self.pos,
                text: self.text.clone() + &other.text,
            }))
        })
    }

    fn to_json(&self) -> Value {
        json!({ "stepType": "insertText", "pos": self.pos, "text": self.text.as_str() })
    }
}

/// The fixtures, with the step registered as the recorder registers it, once.
fn fixtures() -> (Schema, Value) {
    static REGISTERED: LazyLock<Result<()>> =
        LazyLock::new(|| register_step("insertText", InsertText::from_json));
    REGISTERED.as_ref().expect("the step registers");
    let fixtures = read("steps");
    let schema = api::schema(&fixtures["schema"]).expect("a schema");
    (schema, fixtures)
}

fn step(schema: &Schema, json: &Value) -> Step<'static> {
    Step::from_json(schema, json).expect("a step")
}

#[test]
fn step_ids_register_once_and_not_over_prosemirrors() {
    let (_, fixtures) = fixtures();
    let cases = records(&fixtures, "jsonID");
    // The first registration is the one `fixtures` made, which succeeded.
    expect(&cases[0], &["id"], json!({ "result": null }));
    for case in &cases[1..] {
        let id = case["id"].as_str().expect("an id");
        let registered = register_step(id, InsertText::from_json);
        let registered = registered.map(|()| json!({ "result": null }));
        expect(case, &["id"], outcome(registered));
    }
}

#[test]
fn steps_from_json_are_prosemirrors() {
    let (schema, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        let step = Step::from_json(&schema, &case["json"]);
        let step = step.map(|step| json!({ "result": step.to_json() }));
        expect(case, &["json"], outcome(step));
    }
}

#[test]
fn transforms_apply_invert_and_map_as_prosemirrors() {
    let (schema, fixtures) = fixtures();
    let doc = Node::from_json(&schema, &fixtures["start"]).expect("a document");
    for case in records(&fixtures, "transforms") {
        let mut tr = Transform::new(doc.clone());
        let mut applied = Vec::new();
        for json in case["steps"].as_array().expect("steps") {
            let step = step(&schema, json);
            let before = tr.doc().clone();
            let step_applied = tr.maybe_step(step.clone()).map(|result| match result {
                StepResult::Failed(message) => json!({ "failed": message }),
                StepResult::Ok(_) => {
                    let ranges: Vec<Value> =
                        step.get_map().ranges().iter().map(|&n| n.into()).collect();
                    let inverted = step.invert(&before).expect("an inverse").to_json();
                    json!({ "map": ranges, "inverted": inverted })
                }
            });
            let failed = step_applied.is_err();
            applied.push(outcome(step_applied));
            if failed {
                break;
            }
        }
        let positions: Vec<Value> = (0..=doc.content().size())
            .map(|pos| tr.mapping().map(pos, 1).into())
            .collect();
        let given =
            json!({ "applied": applied, "result": tr.doc().to_json(), "positions": positions });
        expect(case, &["steps"], given);
    }
}

#[test]
fn steps_map_as_prosemirrors() {
    let (schema, fixtures) = fixtures();
    for case in records(&fixtures, "mapped") {
        let mut mapping = Mapping::new();
        for json in case["over"].as_array().expect("steps") {
            mapping.append_map(step(&schema, json).get_map(), None);
        }
        let mapped = step(&schema, &case["step"]).map(&mapping);
        let mapped = mapped.map_or(Value::Null, |step| step.to_json());
        expect(case, &["step", "over"], json!({ "result": mapped }));
    }
}

#[test]
fn steps_merge_as_prosemirrors() {
    let (schema, fixtures) = fixtures();
    for case in records(&fixtures, "merged") {
        let merged = step(&schema, &case["a"]).merge(&step(&schema, &case["b"]));
        let merged = merged.map_or(Value::Null, |step| step.to_json());
        expect(case, &["a", "b"], json!({ "result": merged }));
    }
}

#[test]
fn apply_steps_reads_registered_steps() {
    let (schema, fixtures) = fixtures();
    let doc = Node::from_json(&schema, &fixtures["start"]).expect("a document");
    let case = &fixtures["transforms"][1];
    let changed = api::apply_steps(&doc, &case["steps"]).expect("the steps");
    assert_eq!(changed.to_json(), case["result"]);
}
