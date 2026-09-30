//! The tree builder under tarnish-js's deadline.

use std::time::Duration;

use tarnish::js::deadline;
use tarnish_html::HtmlDom;

/// End tags that close nothing still take the tree builder through the elements open, so the
/// deadline counts them as well as the elements made. Without a deadline, the HTML parses.
#[test]
fn stops_a_parse_at_its_deadline() {
    let divs = "<div>".repeat(40);
    let html = divs.clone() + &"</li>".repeat(100);
    assert!(deadline::within(Duration::ZERO, || HtmlDom::parse_document(&html)).is_none());
    let parsed = HtmlDom::parse_document(&html);
    let body = parsed.body().expect("a body").inner_html();
    assert_eq!(body, divs + &"</div>".repeat(40));
}
