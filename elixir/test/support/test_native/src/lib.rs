//! The NIF of the application tarnish's Elixir tests make: tarnish-nif's, with the conversions
//! of `test/support/conversions.mjs`.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use tarnish::dom::{
    DomParser, DomSerializer, DomSpec, MarkToDom, NodeToDom, ParseOptions, ParseRule, Rule, TagRule,
};
use tarnish::{Error, Node, Schema, Value, api, json};
use tarnish_html::{HtmlNode, parse_html, to_html};
use tarnish_nif::convert::{Conversions, Document};

/// `NO_MARKDOWN`, with U+FFFD for its lone surrogate, as the pool reads it.
const NO_MARKDOWN: &str = "No Markdown: \u{FFFD}\\😀\\ud800";

struct Html {
    schema: Schema,
    parser: DomParser<HtmlNode>,
    serializer: DomSerializer<HtmlNode>,
}

static HTML: LazyLock<Html> = LazyLock::new(|| {
    let spec = r#"{"nodes": {"doc": {"content": "paragraph+"}, "paragraph": {"content": "text*"},
                             "text": {}},
                   "marks": {"em": {}, "strong": {}}}"#;
    let schema = api::schema(&json::from_str(spec).expect("JSON")).expect("a schema");
    let tags = |selectors: &[&str]| {
        let tag = |selector: &&str| ParseRule::Tag(Rule::new(TagRule::new(*selector)));
        selectors.iter().map(tag).collect()
    };
    let marks = vec![tags(&["i", "em"]), tags(&["b", "strong"])];
    let nodes = vec![vec![], tags(&["p"]), vec![]];
    let parser = DomParser::from_schema(schema.clone(), marks, nodes).expect("a parser");
    let paragraph: NodeToDom<HtmlNode> = Arc::new(|_| Ok(json::json!(["p", 0]).into()));
    let mark = |tag: &'static str| -> MarkToDom<HtmlNode> {
        Arc::new(move |_, _| Ok(DomSpec::from(json::json!([tag, 0]))))
    };
    let serializer = DomSerializer::new(
        HashMap::from([("paragraph".to_owned(), paragraph)]),
        HashMap::from([
            ("em".to_owned(), mark("em")),
            ("strong".to_owned(), mark("strong")),
        ]),
    );
    Html {
        schema,
        parser,
        serializer,
    }
});

/// `refuseOptions`.
fn refuse_options(options: Option<&Value>) -> Result<(), Error> {
    match options {
        None => Ok(()),
        Some(_) => Err(Error::Other("The HTML conversions take no options".into())),
    }
}

impl Conversions for Html {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn parse_markdown(&self, _: &str, _: Option<&Value>) -> Result<Value, Error> {
        Err(Error::Other(NO_MARKDOWN.into()))
    }

    fn serialize_markdown(&self, _: &Value, _: Option<&Value>) -> Result<String, Error> {
        Err(Error::Other(NO_MARKDOWN.into()))
    }

    fn parse_html(&self, html: &str, options: Option<&Value>) -> Result<Node<'static>, Error> {
        refuse_options(options)?;
        parse_html(&self.parser, html, ParseOptions::default())
    }

    fn serialize_html(&self, json: Document, options: Option<&Value>) -> Result<String, Error> {
        refuse_options(options)?;
        to_html(&self.serializer, json.read(&self.schema)?.content())
    }
}

fn load(env: rustler::Env, threads: rustler::Term) -> bool {
    tarnish_nif::load(env, threads, Some(&*HTML))
}

rustler::init!("Elixir.Tarnish.TestNative", load = load);
