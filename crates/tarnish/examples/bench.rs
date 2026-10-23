//! Times the model on target/bench/document.json, which `npm run bench` writes, without a
//! binding in the way: `cargo run --release -p tarnish --example bench`.

use std::time::Instant;

use tarnish::{JsonKind, JsonRef, Node, api, json};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn median(run: &mut dyn FnMut()) -> f64 {
    for _ in 0..300 {
        run();
    }
    let mut times: Vec<f64> = (0..1000)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[500]
}

/// Reads every value in `json`, as a caller of the view that wants all of it would.
// bounded: as deep as the bench document
fn visit(json: JsonRef) -> usize {
    match json.kind() {
        JsonKind::Array(_) => json.items().map(visit).sum(),
        JsonKind::Object(_) => json
            .entries()
            .map(|(key, value)| key.len() + visit(value))
            .sum(),
        JsonKind::String(string) => string.len(),
        _ => 1,
    }
}

fn read(path: &str) -> json::Value {
    json::from_str(&std::fs::read_to_string(path).expect(path)).expect(path)
}

fn main() {
    let fixtures = read("fixtures/transform.json");
    let schema = api::schema(&fixtures["schemas"][0]).unwrap();
    let document = read("target/bench/document.json");
    let (doc, steps) = (&document["doc"], &document["changes"][0]["steps"]);
    let node = Node::from_json(&schema, doc).unwrap();
    let changed = api::apply_steps(&node, steps).unwrap();
    let time = |name: &str, run: &mut dyn FnMut()| println!("{name:36} {:.0} µs", median(run));
    time("fromJSON", &mut || {
        drop(Node::from_json(&schema, doc).unwrap())
    });
    let text = json::stringify(doc);
    time("fromJSON of JSON text", &mut || {
        drop(Node::from_json_text(&schema, &text).unwrap())
    });
    time("JSON.parse, then fromJSON", &mut || {
        drop(Node::from_json(&schema, &json::from_str(&text).unwrap()).unwrap())
    });
    time("check", &mut || node.check().unwrap());
    time("apply 10 steps", &mut || {
        drop(api::apply_steps(&node, steps).unwrap())
    });
    time("toJSON of the changed document", &mut || {
        std::hint::black_box(changed.to_json().len());
    });
    time("toJSON, every value read", &mut || {
        std::hint::black_box(visit(changed.to_json()));
    });
    time("toJSON, copied to a value", &mut || {
        drop(changed.to_json().to_value())
    });
    time("JSON text of the changed document", &mut || {
        drop(changed.to_json_string())
    });
    time("fromJSON, apply 10 steps, toJSON", &mut || {
        let node = Node::from_json(&schema, doc).unwrap();
        let changed = api::apply_steps(&node, steps).unwrap();
        std::hint::black_box(changed.to_json().len());
    });
    time("fromJSON, apply 10 steps, toJSON read", &mut || {
        let node = Node::from_json(&schema, doc).unwrap();
        std::hint::black_box(visit(api::apply_steps(&node, steps).unwrap().to_json()));
    });
    let steps_text = steps.to_string();
    time("JSON text in, apply 10 steps, JSON text out", &mut || {
        let node = Node::from_json_text(&schema, &text).unwrap();
        let steps = json::from_str(&steps_text).unwrap();
        drop(api::apply_steps(&node, &steps).unwrap().to_json_string())
    });
}
