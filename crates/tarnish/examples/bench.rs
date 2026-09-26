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
    let (doc, steps) = (&document["doc"], &document["steps"]);
    let node = Node::from_json(&schema, doc).unwrap();
    let time = |name: &str, run: &mut dyn FnMut()| println!("{name:16} {:.0} µs", median(run));
    time("fromJSON", &mut || {
        drop(Node::from_json(&schema, doc).unwrap())
    });
    time("toJSON", &mut || drop(node.to_json()));
    time("check", &mut || node.check().unwrap());
    let written = tarnish::etf::write(doc);
    time("etf::write", &mut || drop(tarnish::etf::write(doc)));
    time("etf::write_node", &mut || {
        drop(tarnish::etf::write_node(&node))
    });
    time("etf::read", &mut || {
        drop(tarnish::etf::read(&written).unwrap())
    });
    let text = tarnish::js::json::stringify(doc);
    time("json::from_str", &mut || {
        drop(json::from_str(&text).unwrap())
    });
    time("Value::clone", &mut || drop(doc.clone()));
    time("apply 10 steps", &mut || {
        drop(api::apply_steps(&node, steps).unwrap())
    });
}
