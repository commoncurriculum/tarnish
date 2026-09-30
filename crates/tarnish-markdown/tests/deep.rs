//! Markdown nested far deeper than a small stack could recurse through, lexed on a thread with
//! such a stack: each of marked's recursions over nesting has to grow the stack. The lexer takes
//! time that grows with the square of the depth, so they nest only as deep as that needs.

use tarnish_js::json::Value;
use tarnish_js::stack::on_dirty_scheduler_stack;
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Marked, Token};
use tarnish_markdown::marked_more_lists::more_lists;

const BLOCKS: usize = 3_000;
const INLINES: usize = 1_000;

/// How many tokens deep the JSON of the tokens nests, walked without recursing.
fn depth(tokens: &[Value]) -> usize {
    let mut deepest = 0;
    let mut walk: Vec<(&Value, usize)> = tokens.iter().map(|token| (token, 1)).collect();
    while let Some((token, depth)) = walk.pop() {
        deepest = deepest.max(depth);
        for key in ["tokens", "items"] {
            let children = token.get(key).and_then(Value::as_array);
            walk.extend(
                children
                    .into_iter()
                    .flatten()
                    .map(|child| (child, depth + 1)),
            );
        }
    }
    deepest
}

#[test]
fn lexes_markdown_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let delimited =
            |open: &str, close: &str| open.repeat(INLINES) + "x" + &close.repeat(INLINES);
        let nested = [
            ("blockquotes", BLOCKS, ">".repeat(BLOCKS) + " x"),
            ("list items", BLOCKS, "1. ".repeat(BLOCKS) + "x"),
            ("strong", INLINES, delimited("**a ", " a**")),
            ("em", INLINES, delimited("*a ", " a*")),
            ("del", INLINES, delimited("~~a ", " a~~")),
        ];
        let marked = Marked::new(more_lists());
        for (what, levels, markdown) in nested {
            let tokens = Lexer::new(&marked)
                .lex(&utf16::from(&markdown))
                .unwrap_or_else(|error| panic!("{what}: {}", error.message()));
            let json: Vec<Value> = tokens.iter().map(Token::to_json).collect();
            assert!(depth(&json) > levels, "{what}: {}", depth(&json));
        }
    });
}
