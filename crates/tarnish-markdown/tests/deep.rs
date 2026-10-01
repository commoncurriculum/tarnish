//! Markdown nested far deeper than a small stack could recurse through, lexed on a thread with
//! such a stack: each of marked's recursions over nesting has to grow the stack. The lexer takes
//! time that grows with the square of the depth (the cube, when a lazy continuation line re-lexes
//! every nested blockquote), so they nest only as deep as that needs.

use tarnish_js::json::Value;
use tarnish_js::stack::on_dirty_scheduler_stack;
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Marked, Token};
use tarnish_markdown::marked_more_lists::more_lists;

const BLOCKS: usize = 3_000;
const INLINES: usize = 1_000;
const LAZY: usize = 500;
/// How deep the formatted tokens nest, whose text, each a slice of the one below, is as long as
/// the square of the depth.
const FORMATTED: usize = 1_000;

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
            ("lazy continuations", LAZY, "> ".repeat(LAZY) + "x\ny"),
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

/// Tokens nested far deeper than a small stack could recurse through, formatted with `{:?}`
/// and cloned there.
#[test]
fn formats_and_clones_tokens_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let marked = Marked::new(more_lists());
        let markdown = ">".repeat(FORMATTED) + " x";
        let tokens = Lexer::new(&marked).lex(&utf16::from(&markdown));
        let tokens = tokens.unwrap_or_else(|error| panic!("{}", error.message()));
        let formatted = format!("{tokens:?}");
        assert_eq!(formatted.matches("kind: \"blockquote\"").count(), FORMATTED);
        let cloned = tokens.clone();
        assert_eq!(format!("{cloned:?}"), formatted);
    });
}
