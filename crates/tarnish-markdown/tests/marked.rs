//! The tokens marked's lexer makes, with marked-more-lists' list tokenizer, of each input
//! `harness/record-marked.mjs` records: tarnish-markdown's lexer must make the same tokens, or
//! fail with the same error.

use tarnish_fixtures::{outcome, read, records, version};
use tarnish_js::json::{Value, json};
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Marked, Token};
use tarnish_markdown::marked_more_lists::more_lists;

fn lexed(marked: &Marked, input: &str) -> Value {
    let tokens = Lexer::new(marked).lex(&utf16::from(input));
    outcome(
        tokens.map(
            |tokens| json!({ "tokens": tokens.iter().map(Token::to_json).collect::<Vec<_>>() }),
        ),
    )
}

#[test]
fn ports_the_installed_marked() {
    assert_eq!(version("marked"), tarnish_markdown::MARKED);
    assert_eq!(
        version("@tiptap/markdown > marked"),
        tarnish_markdown::MARKED
    );
    assert_eq!(
        version("marked-more-lists"),
        tarnish_markdown::MARKED_MORE_LISTS
    );
}

#[test]
fn tokens_match_marked() {
    let marked = Marked::new(more_lists());
    let fixtures = read("marked");
    let mut failures = Vec::new();
    for mut record in records(&fixtures, "lexed").to_vec() {
        let input = record.as_object_mut().expect("a record").remove("input");
        let input = input.as_ref().and_then(Value::as_str).expect("an input");
        let actual = lexed(&marked, input);
        if actual != record {
            failures.push(format!(
                "{input:?}\n  marked: {record}\n  tarnish-markdown: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
