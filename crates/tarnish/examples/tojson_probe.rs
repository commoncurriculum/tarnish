use tarnish::{Node, api, json};
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
fn main() {
    let fixtures =
        json::from_str(&std::fs::read_to_string("fixtures/transform.json").unwrap()).unwrap();
    let schema = api::schema(&fixtures["schemas"][0]).unwrap();
    let document =
        json::from_str(&std::fs::read_to_string("target/bench/document.json").unwrap()).unwrap();
    let node = Node::from_json(&schema, &document["doc"]).unwrap();
    for _ in 0..200 {
        drop(std::hint::black_box(node.to_json()));
    }
}
