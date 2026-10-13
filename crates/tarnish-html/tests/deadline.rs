//! The tree builder under tarnish-js's deadline.

use std::time::Duration;

use tarnish::js::deadline;
use tarnish_html::HtmlDom;

/// Whether the HTML parses under a deadline that has passed: whether it's done before the parse
/// has done enough work to read the clock.
fn in_time(html: &str) -> bool {
    deadline::within(Duration::ZERO, || HtmlDom::parse_document(html)).is_some()
}

/// Each tag is a turn of the parse, and an end tag that closes nothing takes the tree builder
/// through the elements open, so the deadline counts those as well. Of as many tags as parse
/// in time when each looks through few elements, end tags that look through many don't.
#[test]
fn stops_a_parse_at_its_deadline() {
    let breaks = (1..10_000).take_while(|&count| in_time(&"<br>".repeat(count)));
    let tags = breaks.last().expect("a tag in time");
    let opened = tags / 2;
    let looking = "<div>".repeat(opened) + &"</li>".repeat(tags - opened);
    assert!(!in_time(&looking), "{tags} tags");
}
