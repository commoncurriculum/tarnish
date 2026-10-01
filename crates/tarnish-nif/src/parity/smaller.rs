//! The values one step smaller than a request's input, which `reduce` tries in turn.

use std::collections::HashSet;
use std::ops::Range;

use tarnish::{Key, Value};

use super::Segment;

/// A value one step smaller than another: the other with something cut from one of its parts.
pub(super) struct Smaller<'a> {
    whole: &'a Value,
    /// Where the part is in the whole.
    path: &'a [Segment],
    cut: Cut<'a>,
}

/// What's cut from a part.
enum Cut<'a> {
    /// An array's item.
    Item(usize),
    /// All of an array but one item, which takes its place.
    AllBut(usize),
    /// An object's entry.
    Entry(&'a Key),
    /// A text's characters in a range of its bytes.
    Text(Range<usize>),
}

impl Smaller<'_> {
    pub(super) fn value(&self) -> Value {
        let mut whole = self.whole.clone();
        let mut part = &mut whole;
        for segment in self.path {
            part = match (segment, part) {
                (Segment::Key(key), Value::Object(map)) => map.get_mut(key),
                (Segment::Index(index), Value::Array(items)) => items.get_mut(*index),
                _ => None,
            }
            .expect("a path to a part of the value");
        }
        match (&self.cut, part) {
            (Cut::Item(index), Value::Array(items)) => {
                items.remove(*index);
            }
            (Cut::AllBut(index), part @ Value::Array(_)) => {
                let item = part[*index].take();
                *part = item;
            }
            (Cut::Entry(key), Value::Object(map)) => {
                map.shift_remove(key);
            }
            (Cut::Text(range), Value::String(text)) => text.replace_range(range.clone(), ""),
            _ => unreachable!("a cut of the part's kind"),
        }
        whole
    }
}

/// The first of `try_smaller`'s answers for the values one step smaller than `value`, tried in
/// turn from the outermost part in: each part's own cuts, then its parts'. A part's cuts take an
/// array's items one at a time, then the array but one item; an object's entries one at a time;
/// and for a text, [`text_cuts`].
pub(super) fn first<T>(
    value: &Value,
    mut try_smaller: impl FnMut(Smaller<'_>) -> Option<T>,
) -> Option<T> {
    let mut path = Vec::new();
    // The parts to look at, last first, each with the length of its parent's path.
    let mut parts: Vec<(usize, Option<Segment>, &Value)> = vec![(0, None, value)];
    while let Some((depth, segment, part)) = parts.pop() {
        path.truncate(depth);
        path.extend(segment);
        for cut in cuts(part) {
            let smaller = Smaller {
                whole: value,
                path: &path,
                cut,
            };
            if let Some(found) = try_smaller(smaller) {
                return Some(found);
            }
        }
        let depth = path.len();
        match part {
            Value::Array(items) => parts.extend(
                items
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(index, item)| (depth, Some(Segment::Index(index)), item)),
            ),
            Value::Object(map) => parts.extend(
                map.iter()
                    .rev()
                    .map(|(key, item)| (depth, Some(Segment::Key(key.clone())), item)),
            ),
            _ => {}
        }
    }
    None
}

fn cuts(part: &Value) -> Vec<Cut<'_>> {
    match part {
        Value::Array(items) => (0..items.len())
            .map(Cut::Item)
            .chain(
                (0..items.len())
                    .filter(|&index| !items[index].is_null())
                    .map(Cut::AllBut),
            )
            .collect(),
        Value::Object(map) => map.keys().map(Cut::Entry).collect(),
        Value::String(text) => text_cuts(text).into_iter().map(Cut::Text).collect(),
        _ => Vec::new(),
    }
}

/// The most pieces a text is cut into at once.
const MOST_PIECES: usize = 256;

/// The ranges of `text` to cut, each once, biggest first: all of it, each half, each line, then
/// each of its quarters, eighths and so on, down to single characters for a text of at most
/// `MOST_PIECES` characters. `reduce` starts over on what's left after each cut it keeps, so a
/// long text is cut by halves before it's cut finer, and a text no cut keeps costs at most
/// `2 * MOST_PIECES` tries beside its lines.
fn text_cuts(text: &str) -> Vec<Range<usize>> {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .collect();
    let length = bounds.len() - 1;
    let pieces = |count: usize| {
        let bounds = &bounds;
        (0..count)
            .map(move |piece| bounds[piece * length / count]..bounds[(piece + 1) * length / count])
    };
    let mut cuts = Vec::new();
    let mut seen = HashSet::new();
    let mut cut = |range: Range<usize>| {
        if !range.is_empty() && seen.insert(range.clone()) {
            cuts.push(range);
        }
    };
    cut(0..text.len());
    pieces(2).for_each(&mut cut);
    let mut line = 0;
    for (newline, _) in text.match_indices('\n') {
        cut(line..newline + 1);
        line = newline + 1;
    }
    cut(line..text.len());
    let mut count = 4;
    while count <= MOST_PIECES && count / 2 < length {
        pieces(count).for_each(&mut cut);
        count *= 2;
    }
    cuts
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarnish::js::json::{from_str, json};
    use tarnish::js::stack;

    fn all(value: &Value) -> Vec<Value> {
        let mut found = Vec::new();
        first(value, |smaller| {
            found.push(smaller.value());
            None::<()>
        });
        found
    }

    #[test]
    fn makes_values_one_step_smaller_outermost_first() {
        assert_eq!(
            all(&json!([{"a": "xy"}, null])),
            [
                json!([null]),
                json!([{"a": "xy"}]),
                json!({"a": "xy"}),
                json!([{}, null]),
                json!([{"a": ""}, null]),
                json!([{"a": "y"}, null]),
                json!([{"a": "x"}, null]),
            ]
        );
    }

    fn texts(text: &str) -> Vec<String> {
        all(&json!(text))
            .into_iter()
            .map(|value| value.into_string().unwrap())
            .collect()
    }

    #[test]
    fn cuts_a_text_whole_then_by_halves_then_by_lines_then_finer() {
        assert_eq!(texts("a"), [""]);
        assert_eq!(
            texts("ab\ncd\nef"),
            [
                // all of it, then each half
                "",
                "d\nef",
                "ab\nc",
                // each line
                "cd\nef",
                "ab\nef",
                "ab\ncd\n",
                // each quarter but the last, which is a line
                "\ncd\nef",
                "abd\nef",
                "ab\ncef",
                // each character
                "b\ncd\nef",
                "a\ncd\nef",
                "abcd\nef",
                "ab\nd\nef",
                "ab\nc\nef",
                "ab\ncdef",
                "ab\ncd\nf",
                "ab\ncd\ne",
            ]
        );
        assert_eq!(texts("ü😀"), ["", "😀", "ü"]);
    }

    #[test]
    fn cuts_a_long_text_into_no_more_than_so_many_pieces() {
        let cuts = text_cuts(&"x".repeat(MOST_PIECES));
        assert_eq!(
            cuts.iter().filter(|cut| cut.len() == 1).count(),
            MOST_PIECES
        );
        let cuts = text_cuts(&"x".repeat(100_000));
        assert!(cuts.len() < 2 * MOST_PIECES, "{} cuts", cuts.len());
        assert!(cuts.iter().all(|cut| cut.len() >= 100_000 / MOST_PIECES));
    }

    #[test]
    fn cuts_a_value_as_deep_as_it_nests() {
        const DEPTH: usize = 100_000;
        fn nested(leaf: &str) -> Value {
            from_str(&format!("{}{leaf}{}", "[".repeat(DEPTH), "]".repeat(DEPTH))).unwrap()
        }
        stack::on_dirty_scheduler_stack(|| {
            let value = nested(r#""xy""#);
            let innermost = first(&value, |smaller| {
                (smaller.path.len() == DEPTH).then(|| smaller.value())
            });
            assert!(innermost == Some(nested(r#""""#)));
        });
    }
}
