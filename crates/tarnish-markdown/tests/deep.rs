//! Markdown nested far deeper than a small stack could recurse through, lexed on a thread with
//! such a stack: each of marked's recursions over nesting has to grow the stack. The lexer takes
//! time that grows with the square of the depth (the cube, when a lazy continuation line re-lexes
//! every nested blockquote), so they nest only as deep as that needs.

use tarnish_fixtures::{assert_same, read, records, same};
use tarnish_js::json::{Value, json};
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

/// Markdown nested `depth` deep, and the tokens marked makes of it, which the test folds from
/// the depths below and holds to marked's at the depths `harness/record-marked.mjs` records.
struct Nested {
    what: &'static str,
    depth: usize,
    markdown: fn(usize) -> String,
    tokens: fn(usize) -> Value,
}

fn text(text: &str) -> Value {
    json!({"type": "text", "raw": text, "text": text})
}

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "raw": text, "text": text, "tokens": [self::text(text)]})
}

fn quoted(depth: usize) -> String {
    ">".repeat(depth) + " x"
}

/// Each blockquote's text is its raw without its first `>`, and holds the one inside it.
fn blockquotes(depth: usize) -> Value {
    let mut tokens = paragraph("x");
    for level in 1..=depth {
        let text = match level {
            1 => "x".into(),
            _ => quoted(level - 1),
        };
        tokens =
            json!({"type": "blockquote", "raw": quoted(level), "text": text, "tokens": [tokens]});
    }
    json!([tokens])
}

fn lazy(depth: usize) -> String {
    "> ".repeat(depth) + "x\ny"
}

/// The line after the blockquotes goes on the paragraph at the bottom of every one of them.
fn lazy_blockquotes(depth: usize) -> Value {
    let mut tokens = paragraph("x\ny");
    for level in 1..=depth {
        tokens = json!({"type": "blockquote", "raw": lazy(level), "text": lazy(level - 1), "tokens": [tokens]});
    }
    json!([tokens])
}

fn listed(depth: usize) -> String {
    "1. ".repeat(depth) + "x"
}

/// Each list holds an item, whose text is the list's raw without its marker, and which holds
/// the list inside it.
fn lists(depth: usize) -> Value {
    let mut tokens = json!({"type": "text", "raw": "x", "text": "x", "tokens": [text("x")]});
    for level in 1..=depth {
        let item = json!({"type": "list_item", "raw": listed(level), "text": listed(level - 1),
            "tokens": [tokens], "task": false, "checked": null, "loose": false});
        tokens = json!({"type": "list", "raw": listed(level), "ordered": true, "start": 1,
            "loose": false, "items": [item]});
    }
    json!([tokens])
}

fn delimited(open: &str, close: &str, depth: usize) -> String {
    open.repeat(depth) + "x" + &close.repeat(depth)
}

/// A paragraph of `kind`, whose text, between its delimiters, holds the next one in.
fn delimiteds(kind: &str, open: &str, close: &str, depth: usize) -> Value {
    let mut tokens = vec![text("a x a")];
    for level in 1..=depth {
        let inner = delimited(open, close, level - 1);
        let token = json!({"type": kind, "raw": delimited(open, close, level),
            "text": format!("a {inner} a"), "tokens": tokens});
        tokens = vec![text("a "), token, text(" a")];
    }
    let outer = delimited(open, close, depth);
    json!([{"type": "paragraph", "raw": &outer, "text": outer, "tokens": [tokens.swap_remove(1)]}])
}

const NESTED: [Nested; 6] = [
    Nested {
        what: "blockquotes",
        depth: BLOCKS,
        markdown: quoted,
        tokens: blockquotes,
    },
    Nested {
        what: "lazy continuations",
        depth: LAZY,
        markdown: lazy,
        tokens: lazy_blockquotes,
    },
    Nested {
        what: "list items",
        depth: BLOCKS,
        markdown: listed,
        tokens: lists,
    },
    Nested {
        what: "strong",
        depth: INLINES,
        markdown: |depth| delimited("**a ", " a**", depth),
        tokens: |depth| delimiteds("strong", "**a ", " a**", depth),
    },
    Nested {
        what: "em",
        depth: INLINES,
        markdown: |depth| delimited("*a ", " a*", depth),
        tokens: |depth| delimiteds("em", "*a ", " a*", depth),
    },
    Nested {
        what: "del",
        depth: INLINES,
        markdown: |depth| delimited("~~a ", " a~~", depth),
        tokens: |depth| delimiteds("del", "~~a ", " a~~", depth),
    },
];

fn lexed(markdown: &str) -> Vec<Token> {
    let marked = Marked::new(more_lists());
    let tokens = Lexer::new(&marked).lex(&utf16::from(markdown));
    tokens.unwrap_or_else(|error| panic!("{markdown:?}: {}", error.message()))
}

fn to_json(tokens: &[Token]) -> Value {
    Value::Array(tokens.iter().map(Token::to_json).collect())
}

#[test]
fn lexes_markdown_nested_as_deeply_as_memory_allows() {
    let fixtures = read("marked");
    let recorded = records(&fixtures, "lexed");
    for nested in &NESTED {
        for depth in 1..=3 {
            let markdown = (nested.markdown)(depth);
            let record = recorded
                .iter()
                .find(|record| record["input"] == markdown.as_str());
            let record = record.unwrap_or_else(|| panic!("no record of {markdown:?}"));
            assert_same(&record["tokens"], &(nested.tokens)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        for nested in &NESTED {
            let tokens = to_json(&lexed(&(nested.markdown)(nested.depth)));
            let expected = (nested.tokens)(nested.depth);
            assert!(same(&expected, &tokens), "{}", nested.what);
        }
    });
}

/// Tokens nested far deeper than a small stack could recurse through, formatted with `{:?}`
/// and cloned there.
#[test]
fn formats_and_clones_tokens_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let tokens = lexed(&quoted(FORMATTED));
        let formatted = format!("{tokens:?}");
        let cloned = tokens.clone();
        assert!(format!("{cloned:?}") == formatted);
        assert!(same(&blockquotes(FORMATTED), &to_json(&cloned)));
    });
}
