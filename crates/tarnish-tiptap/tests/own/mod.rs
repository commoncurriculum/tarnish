//! The extensions `harness/record-tiptap.mjs` gives the `MarkdownManager` and `getSchema`: Tiptap's
//! own, and the others a record names, and the conversions it records with them.

use tarnish::Node;
use tarnish::json::{Value, json};
use tarnish_fixtures::outcome;
use tarnish_js::Error;
use tarnish_markdown::marked::Marked;
use tarnish_markdown::marked_more_lists::more_lists;
use tarnish_tiptap::extensions::bold::bold;
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::hard_break::hard_break;
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::highlight::{HighlightOptions, highlight};
use tarnish_tiptap::extensions::image::{ImageOptions, image};
use tarnish_tiptap::extensions::italic::italic;
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::strike::strike;
use tarnish_tiptap::extensions::subscript::subscript;
use tarnish_tiptap::extensions::superscript::superscript;
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::text_style::{color, text_style};
use tarnish_tiptap::extensions::underline::underline;
use tarnish_tiptap::markdown::MarkdownManager;
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, MarkExtension, NodeExtension, ParseHtml, TiptapSchema,
    get_schema, html,
};

/// Tiptap's own extensions, but Highlight.
fn own_but_highlight() -> Vec<Extension> {
    vec![
        document().into(),
        paragraph().into(),
        text().into(),
        heading(HeadingOptions::default()).into(),
        image(ImageOptions::default()).into(),
        hard_break().into(),
        bold().into(),
        italic().into(),
        strike().into(),
        underline().into(),
        subscript().into(),
        superscript().into(),
    ]
}

pub fn extensions() -> Vec<Extension> {
    let mut own = own_but_highlight();
    own.push(highlight(HighlightOptions { multicolor: false }).into());
    own.push(text_style().into());
    own
}

/// Tiptap's own extensions, Highlight with its colors and after TextStyle, and Color.
fn colors() -> Vec<Extension> {
    let mut colors = own_but_highlight();
    colors.push(text_style().into());
    colors.push(highlight(HighlightOptions { multicolor: true }).into());
    colors.push(color().into());
    colors
}

/// An application's code mark and code block, whose text stays as it is, and a figure whose
/// attribute renders under a name it makes.
fn code() -> Vec<Extension> {
    let code = MarkExtension::create("code")
        .code()
        .render_markdown(|node, helpers, _| {
            Ok(["`", &helpers.render_children(node, "")?, "`"].concat())
        });
    let block = NodeExtension::create("codeBlock")
        .code()
        .group("block")
        .content("text*")
        .parse_html([ParseHtml::tag("pre")])
        .render_markdown(|node, helpers, _| {
            Ok(["```\n", &helpers.render_children(node, "")?, "\n```"].concat())
        });
    let name = ["data", "made"].join("-");
    let figure = NodeExtension::create("figure")
        .group("block")
        .parse_html([ParseHtml::tag("figure")])
        .render_html(|_, html| Ok(DomSpec::element("figure", html, Vec::new())))
        .add_attributes(vec![
            ExtensionAttribute::new("made", Value::Null).render_html(move |attrs, rendered| {
                rendered.merge(name.clone(), attrs.get("made").expect("its attribute"))
            }),
        ]);
    vec![
        document().into(),
        paragraph().into(),
        text().into(),
        code.into(),
        block.into(),
        figure.into(),
    ]
}

/// The schema and the Markdown manager, `new MarkdownManager({ extensions, marked })` with a
/// marked that uses marked-more-lists, of the extensions a record names.
pub struct Converter {
    schema: TiptapSchema,
    markdown: MarkdownManager,
}

/// The extensions a record names in `extensions`, `None` for Tiptap's own.
pub fn named(record: &Value) -> Option<&str> {
    record.get("extensions").and_then(Value::as_str)
}

impl Converter {
    pub fn new(extensions: Option<&str>) -> Converter {
        let extensions = match extensions {
            None => self::extensions(),
            Some("colors") => colors(),
            Some("code") => code(),
            Some(other) => panic!("no extensions {other}"),
        };
        Converter {
            schema: get_schema(&extensions).expect("the schema"),
            markdown: MarkdownManager::new(&extensions, Marked::new(more_lists())),
        }
    }

    /// What the conversion a `section` records makes of an input, as a record of it holds it.
    pub fn convert(&self, section: &str, input: &Value) -> Value {
        fn record<T: Into<Value>>(to: &str, result: Result<T, Error>) -> Value {
            outcome(result.map(|output| json!({ to: output.into() })))
        }
        let text = || input.as_str().expect("text");
        match section {
            "parsesMarkdown" => record("doc", self.markdown.parse(text())),
            "serializesMarkdown" => record("markdown", self.markdown.serialize(input)),
            "parsesHTML" => record(
                "doc",
                html::parse(&self.schema, text()).map(|doc| doc.to_json()),
            ),
            "serializesHTML" => record(
                "html",
                Node::from_json(&self.schema.schema, input)
                    .and_then(|doc| html::serialize(&self.schema, &doc)),
            ),
            _ => panic!("no section {section}"),
        }
    }
}

/// Each section's input field.
pub fn input_of(section: &str) -> &'static str {
    match section {
        "parsesMarkdown" => "markdown",
        "parsesHTML" => "html",
        _ => "doc",
    }
}
