//! JSON values nested far deeper than a small stack could recurse through, and the JSON and
//! regular expressions that serde_json and regress read as deeply as they allow, on a thread with
//! such a stack, or one no bigger than a level of a recursion is sure of.

use tarnish_js::json::{self, Value};
use tarnish_js::stack::on_dirty_scheduler_stack;

const DEPTH: usize = 20_000;

/// How deep `JSON.stringify(value, null, 2)` nests, which indents each line as deep as it is,
/// so that its text grows with the square of the depth.
const PRETTY: usize = 3_000;

/// `levels` objects and arrays, each inside the other, around `inside`.
fn alternating(levels: usize, inside: Value) -> Value {
    (0..levels).fold(inside, |inner, level| match level % 2 {
        0 => Value::Object([("a".into(), inner)].into_iter().collect()),
        _ => Value::Array(vec![inner]),
    })
}

fn arrays(levels: usize, inside: Value) -> Value {
    (0..levels).fold(inside, |inner, _| Value::Array(vec![inner]))
}

#[test]
fn values_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let value = alternating(DEPTH, Value::Null);
        let copy = value.clone();
        assert!(copy == value);
        assert!(copy != alternating(DEPTH, Value::Bool(true)));
        let text = json::stringify(&value);
        assert!(json::from_str(&text).expect("JSON") == value);

        let joined = tarnish_js::to_string(&arrays(DEPTH, "x".into()));
        assert_eq!(joined.expect("a string"), "x");

        let pretty = json::stringify_pretty(&alternating(PRETTY, Value::Null));
        assert_eq!(pretty.lines().count(), 2 * PRETTY + 1);
        let parsed = json::from_str(&pretty).expect("JSON");
        assert!(parsed == alternating(PRETTY, Value::Null));
    });
}

/// The room a level of a recursion that goes through `stack::grow` is sure of.
const RED_ZONE: usize = 128 << 10;

/// `JSON.parse` of JSON as deep as serde_json reads it, 128 levels, which it reads, unoptimized,
/// in more stack than that.
#[test]
fn json_parses_as_deeply_as_serde_reads_where_little_room_is_left() {
    for (open, close) in [("[", "]"), ("{\"a\":", "}")] {
        let text = open.repeat(127) + "1" + &close.repeat(127);
        let parsed = std::thread::Builder::new()
            .stack_size(RED_ZONE)
            .spawn(move || json::from_str(&text).is_ok())
            .expect("a thread")
            .join();
        assert!(parsed.expect("no overflow"), "{open}");
    }
}

/// Patterns whose groups nest as deeply as regress allows, or that have more alternatives than
/// a small stack could take a call for each of: compiling them, and matching lookarounds, goes
/// as deep.
#[cfg(feature = "regexp")]
#[test]
fn regexps_compile_and_match_as_deeply_as_regress_reads() {
    use tarnish_js::regexp::RegExp;
    use tarnish_js::utf16;

    const LEVELS: usize = 255;
    on_dirty_scheduler_stack(|| {
        let nested = |open: &str, inside: &str, close: &str| {
            open.repeat(LEVELS) + inside + &close.repeat(LEVELS)
        };
        let sources = [
            nested("(", "a", ")"),
            nested("(?:c|", "a", ")"),
            nested("(?=", "a", ")") + "a",
            "a".to_owned() + &nested("(?<=", "a", ")"),
            "c|".repeat(20_000) + "a",
        ];
        let text = utf16::from("ba");
        for source in sources {
            let regexp = RegExp::new(&source, "u");
            let found = regexp.exec(&text);
            assert_eq!(
                found.map(|found| found.index()),
                Some(1),
                "{}",
                &source[..8]
            );
        }
    });
}
