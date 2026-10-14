//! The tokens marked's lexer makes, with marked-more-lists' list tokenizer, of each input
//! `harness/record-marked.mjs` records: tarnish-markdown's lexer must make the same tokens, or
//! fail with the same error.

use std::sync::LazyLock;

use tarnish_fixtures::{outcome, read, records, same, version};
use tarnish_js::json::{Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::utf16;
use tarnish_markdown::marked::{BlockExtension, Lexer, Marked, Token, Tokens};
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

/// The inputs of each kind of block and inline, and lists of each marker marked-more-lists reads.
#[test]
fn tokens_match_marked() {
    check(&Marked::new(more_lists()), &["lexed", "markers"]);
}

static CONTAINER: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"^:::([^\n]*)\n([\s\S]*?)\n:::(?:\n|$)", ""));
static LINE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^:::x[^\n]*(?:\n|$)", ""));
static FENCED_START: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\+\+\+", "m"));
static FENCED: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"^\+\+\+(\w*)\n([\s\S]*?)\n\+\+\+(?:\n|$)", ""));

/// `harness/record-marked.mjs`'s three block extensions: containers, whose start ends paragraphs,
/// `:::x` lines, which are tried first, and `+++` fenced blocks, whose blocks are their `tokens`.
#[test]
fn block_extensions_lex_as_in_marked() {
    let mut marked = Marked::new(more_lists());
    marked.use_block_extension(BlockExtension {
        start: Some(Box::new(|src| utf16::index_of_str(src, ":::", 0))),
        tokenizer: Box::new(|lexer, src, _| {
            let Some(cap) = CONTAINER.exec(src) else {
                return Ok(None);
            };
            let name = lexer.inline_tokens(&src.slice_of(cap.get(1).unwrap_or_default()))?;
            let mut body = Vec::new();
            let body_src = src.slice_of(cap.get(2).unwrap_or_default());
            lexer.block_tokens(&body_src, &mut body, false)?;
            Ok(Some(Token {
                child_tokens: Some(Box::new(vec![
                    ("name", Tokens::from(name)),
                    ("body", Tokens::from(body)),
                ])),
                ..Token::new("container", src.slice_of(cap.all()))
            }))
        }),
    });
    marked.use_block_extension(BlockExtension {
        start: None,
        tokenizer: Box::new(|_, src, _| {
            Ok(LINE
                .exec(src)
                .map(|cap| Token::new("line", src.slice_of(cap.all()))))
        }),
    });
    marked.use_block_extension(BlockExtension {
        start: Some(Box::new(|src| {
            FENCED_START.exec(src).map(|found| found.index())
        })),
        tokenizer: Box::new(|lexer, src, _| {
            let Some(found) = FENCED.exec(src) else {
                return Ok(None);
            };
            let mut tokens = Vec::new();
            let content = src.slice_of(found.get(2).unwrap_or_default());
            lexer.block_tokens(&content, &mut tokens, false)?;
            Ok(Some(Token {
                text: Some(src.slice_of(found.get(1).unwrap_or_default())),
                tokens: Some(tokens.into()),
                ..Token::new("fenced", src.slice_of(found.all()))
            }))
        }),
    });
    check(&marked, &["extended"]);
}

/// Holds each record of the sections to what `marked` lexes of its input.
fn check(marked: &Marked, sections: &[&str]) {
    let fixtures = read("marked");
    let mut failures = Vec::new();
    let sections: Vec<&[Value]> = sections
        .iter()
        .map(|section| records(&fixtures, section))
        .collect();
    for mut record in sections.concat() {
        let input = record.as_object_mut().expect("a record").remove("input");
        let input = input.as_ref().and_then(Value::as_str).expect("an input");
        let actual = lexed(marked, input);
        if !same(&record, &actual) {
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
