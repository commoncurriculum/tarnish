//! HTML nested far deeper than a small stack could recurse through, parsed into a document and
//! written back on a thread with such a stack.

mod basic;

use tarnish::api;
use tarnish::dom::ParseOptions;
use tarnish::json;
use tarnish_html::{HtmlDom, HtmlNode, parse_html, to_html};

const DEPTH: usize = 3_000;

#[test]
fn deep_html_round_trips_on_a_small_stack() {
    let html = format!(
        "{}<p>deep</p>{}",
        "<blockquote>".repeat(DEPTH),
        "</blockquote>".repeat(DEPTH)
    );
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(move || {
            let fixtures = std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/dom.json"
            ))
            .expect("the fixtures");
            let fixtures = json::from_str(&fixtures).expect("JSON");
            let schema = api::schema(&fixtures["schema"]).expect("the schema");
            let parser = basic::parser(&schema);

            let fragment = HtmlDom::new().parse_fragment(&html);
            assert_eq!(fragment.inner_html(), html);

            let doc = parse_html(&parser, &html, ParseOptions::<HtmlNode>::default())
                .expect("a document");
            let mut depth = 0;
            let mut node = doc.clone();
            while let Some(child) = node.first_child() {
                depth += 1;
                node = child;
            }
            assert_eq!(depth, DEPTH + 2);
            assert_eq!(
                to_html(&basic::serializer(), doc.content()).expect("HTML"),
                html
            );
        })
        .expect("a thread")
        .join()
        .expect("the test");
}
