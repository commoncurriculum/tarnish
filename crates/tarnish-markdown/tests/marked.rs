//! The tokens marked's lexer makes, with marked-more-lists' list tokenizer, of each input
//! `harness/record-marked.mjs` records: tarnish-markdown's lexer must make the same tokens, or
//! fail with the same error.

use tarnish_js::json::{self, Value, json};
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Marked, Token};
use tarnish_markdown::marked_more_lists::more_lists;

fn fixtures() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/marked.json");
    json::from_str(&std::fs::read_to_string(path).expect("the fixtures")).expect("JSON")
}

fn lexed(marked: &Marked, input: &str) -> Value {
    match Lexer::new(marked).lex(&utf16::from(input)) {
        Ok(tokens) => json!({ "tokens": tokens.iter().map(Token::to_json).collect::<Vec<_>>() }),
        Err(error) => json!({ "error": error.message() }),
    }
}

#[test]
fn ports_the_installed_marked() {
    assert_eq!(
        fixtures()["marked"].as_str(),
        Some(tarnish_markdown::MARKED)
    );
}

#[test]
fn tokens_match_marked() {
    let marked = Marked::new(more_lists());
    let mut fixtures = fixtures();
    let mut failures = Vec::new();
    for mut record in fixtures["lexed"].take().into_array().expect("the inputs") {
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
