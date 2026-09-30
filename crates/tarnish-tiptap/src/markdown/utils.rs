//! `src/utils.ts`.

use std::borrow::Cow;

use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;

use crate::attrs_equal;
use tarnish::json::Value;
use tarnish_js as js;
use tarnish_js::Error;
use tarnish_js::utf16;
use tarnish_js::value::{self, SameValueKey};
use tarnish_markdown::marked::Token;

/// A `Map` of marks by their type.
pub type Marks<'a> = IndexMap<SameValueKey<'a>, &'a Value, FxBuildHasher>;

/// How long the match of `/\n[^\S\n]*(?:\n[^\S\n]*)+$/` in `raw` is: from the first newline of
/// the whitespace `raw` ends with, when another newline follows it.
fn trailing_blank_lines(raw: &[u16]) -> Option<usize> {
    let newline = utf16::unit(b'\n');
    let whitespace = raw
        .iter()
        .rposition(|&unit| !js::is_whitespace_unit(unit))
        .map_or(0, |last| last + 1);
    let first = whitespace + raw[whitespace..].iter().position(|&unit| unit == newline)?;
    let newlines = raw[first..].iter().filter(|&&unit| unit == newline).count();
    (newlines >= 2).then_some(raw.len() - first)
}

/// `extractAbsorbedBlankLines`: the blank lines a token's `raw` ends with become a space token
/// of their own, unless a space token is next to it.
pub fn extract_absorbed_blank_lines(tokens: &[Token]) -> Vec<Cow<'_, Token>> {
    let mut normalized = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        let next_is_space = tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == "space");
        let trailing = if token.kind == "space" || next_is_space {
            None
        } else {
            trailing_blank_lines(&token.raw)
        };
        match trailing {
            None => normalized.push(Cow::Borrowed(token)),
            Some(length) => {
                let kept = token.raw.len() - length;
                normalized.push(Cow::Owned(Token {
                    raw: token.raw.slice(0..kept),
                    ..token.clone()
                }));
                normalized.push(Cow::Owned(Token::new("space", token.raw.substring(kept))));
            }
        }
    }
    normalized
}

/// `node.marks || []`.
pub fn marks_of(node: Option<&Value>) -> &[Value] {
    match node.and_then(|node| node.get("marks")) {
        Some(Value::Array(marks)) => marks,
        _ => &[],
    }
}

/// `mark.type`.
pub fn mark_type<'a>(mark: &'a Value) -> Result<SameValueKey<'a>, Error> {
    Ok(SameValueKey::of(value::get(Some(mark), "type")?))
}

/// `new Map(marks.map((mark) => [mark.type, mark]))`: a later mark of a type replaces the value
/// but keeps the first one's position.
pub fn marks_by_type(marks: &[Value]) -> Result<Marks<'_>, Error> {
    let mut by_type = Marks::with_capacity_and_hasher(marks.len(), FxBuildHasher);
    for mark in marks {
        by_type.insert(mark_type(mark)?, mark);
    }
    Ok(by_type)
}

/// `marks.find((m) => m.type === kind && attrsEqual(m.attrs, attrs))`.
fn find_mark<'a>(
    marks: &'a [Value],
    kind: SameValueKey,
    attrs: Option<&Value>,
) -> Result<Option<&'a Value>, Error> {
    for mark in marks {
        if mark_type(mark)? == kind && attrs_equal(value::optional(Some(mark), "attrs"), attrs) {
            return Ok(Some(mark));
        }
    }
    Ok(None)
}

/// `findMarksToClose`.
pub fn find_marks_to_close<'a>(
    current_marks: &Marks<'a>,
    next_node: Option<&Value>,
) -> Result<Vec<SameValueKey<'a>>, Error> {
    let mut marks_to_close = Vec::new();
    for (kind, current_mark) in current_marks {
        if !js::truthy(next_node) {
            marks_to_close.push(*kind);
            continue;
        }
        let next_marks = value::array_method(
            value::or_empty_array(value::optional(next_node, "marks")),
            "(nextNode.marks || []).find",
        )?;
        let attrs = value::optional(Some(current_mark), "attrs");
        if !js::truthy(find_mark(next_marks, *kind, attrs)?) {
            marks_to_close.push(*kind);
        }
    }
    Ok(marks_to_close)
}

/// `findMarksToOpen`.
pub fn find_marks_to_open<'a>(
    active_marks: &Marks,
    current_marks: &Marks<'a>,
) -> Vec<(SameValueKey<'a>, &'a Value)> {
    current_marks
        .iter()
        .filter(|(kind, mark)| match active_marks.get(*kind) {
            Some(active) if js::truthy(Some(active)) => !attrs_equal(
                value::optional(Some(active), "attrs"),
                value::optional(Some(mark), "attrs"),
            ),
            _ => true,
        })
        .map(|(kind, mark)| (*kind, *mark))
        .collect()
}

/// `findMarksToCloseAtEnd`.
pub fn find_marks_to_close_at_end<'a>(
    active_marks: &Marks<'a>,
    current_marks: &Marks,
    next_node: Option<&Value>,
) -> Result<Vec<SameValueKey<'a>>, Error> {
    let is_last_node = !js::truthy(next_node);
    let next_marks = value::optional(next_node, "marks").filter(|_| !is_last_node);
    let next_node_has_no_marks =
        !is_last_node && (!js::truthy(next_marks) || next_marks.is_some_and(has_zero_length));
    let next_node_has_different_marks = !is_last_node
        && js::truthy(next_marks)
        && !mark_sets_equal(
            current_marks,
            &marks_by_type(value::array_method(next_marks, "nextNode.marks.map")?)?,
        );
    let mut marks_to_close_at_end = Vec::new();
    if is_last_node || next_node_has_no_marks || next_node_has_different_marks {
        if !is_last_node && js::truthy(next_marks) {
            let next_marks = value::array_method(next_marks, "nextNode.marks.find")?;
            for (kind, active_mark) in active_marks.iter().rev() {
                let attrs = value::optional(Some(active_mark), "attrs");
                if !js::truthy(find_mark(next_marks, *kind, attrs)?) {
                    marks_to_close_at_end.push(*kind);
                }
            }
        } else if is_last_node || next_node_has_no_marks {
            marks_to_close_at_end.extend(active_marks.keys().rev());
        }
    }
    Ok(marks_to_close_at_end)
}

/// `marks.length === 0` for truthy `marks`.
fn has_zero_length(marks: &Value) -> bool {
    match marks {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map
            .get("length")
            .and_then(Value::as_f64)
            .is_some_and(|length| length == 0.0),
        _ => false,
    }
}

/// `markdownManager.markSetsEqual`.
fn mark_sets_equal(a: &Marks, b: &Marks) -> bool {
    a.len() == b.len()
        && a.iter().all(|(kind, mark)| {
            b.get(kind).is_some_and(|other| {
                js::truthy(Some(other))
                    && attrs_equal(
                        value::optional(Some(mark), "attrs"),
                        value::optional(Some(other), "attrs"),
                    )
            })
        })
}

/// `closeMarksBeforeNode`.
pub fn close_marks_before_node<'a>(
    active_marks: &mut Marks<'a>,
    mut closing: impl FnMut(SameValueKey<'a>, &'a Value) -> Result<String, Error>,
) -> Result<String, Error> {
    let mut before = String::new();
    for (kind, mark) in active_marks.iter().rev() {
        before.insert_str(0, &closing(*kind, mark)?);
    }
    active_marks.clear();
    Ok(before)
}

/// `reopenMarksAfterNode`.
pub fn reopen_marks_after_node<'a>(
    marks_to_reopen: Marks<'a>,
    active_marks: &mut Marks<'a>,
    mut opening: impl FnMut(SameValueKey<'a>, &'a Value) -> Result<String, Error>,
) -> Result<String, Error> {
    let mut after = String::new();
    for (kind, mark) in marks_to_reopen {
        after += &opening(kind, mark)?;
        active_marks.insert(kind, mark);
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use tarnish_js::random::strings;
    use tarnish_js::regexp::RegExp;

    #[test]
    fn trailing_blank_lines_matches_its_regex() {
        let regex = RegExp::new(r"\n[^\S\n]*(?:\n[^\S\n]*)+$", "");
        for raw in strings(
            &[
                "a", "\n", "\n", " ", "\t", "\r", "\u{A0}", "\u{2028}", "x\n",
            ],
            200_000,
        ) {
            assert_eq!(
                super::trailing_blank_lines(&raw),
                regex.exec(&raw).map(|found| found.all().len()),
                "{:?}",
                String::from_utf16_lossy(&raw)
            );
        }
    }
}
