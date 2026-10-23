//! Times the model on target/bench/document.json, which `npm run bench` writes, without a
//! binding in the way: `cargo run --release -p tarnish --example bench`.

use std::time::Instant;

use tarnish::{Node, api, json};

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
        drop(changed.to_json())
    });
    time("JSON text of the changed document", &mut || {
        drop(changed.to_json_string())
    });
    time("fromJSON, apply 10 steps, toJSON", &mut || {
        let node = Node::from_json(&schema, doc).unwrap();
        drop(api::apply_steps(&node, steps).unwrap().to_json())
    });
    let steps_text = steps.to_string();
    time("JSON text in, apply 10 steps, JSON text out", &mut || {
        let node = Node::from_json_text(&schema, &text).unwrap();
        let steps = json::from_str(&steps_text).unwrap();
        drop(api::apply_steps(&node, &steps).unwrap().to_json_string())
    });
}
