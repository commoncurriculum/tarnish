//! What ProseMirror's `DOMParser` and `DOMSerializer` do in the linkedom fork, recorded by
//! `harness/record-dom.mjs`: over this crate's DOM, tarnish's must build the same trees, parse
//! the same documents, and write the same HTML and strings.

mod basic;

use tarnish::dom::{Dom, DomParser, DomSpec, ParseOptions, PreserveWhitespace, render_spec};
use tarnish::json::{self, Map, Value};
use tarnish::{Node, Result, Schema, api};
use tarnish_fixtures::{outcome, read, records};
use tarnish_html::{HtmlDom, HtmlNode, parse_html, parse_html_slice, to_html};

/// An input html5ever builds another tree from than the fork's parse5 8 does, and what
/// tarnish-html makes of it: html5ever's tree, which a template and a document's body hold
/// alike, the document ProseMirror's parser reads from either, and the template's slice, as
/// JSON.
struct Different {
    html: &'static str,
    tree: &'static str,
    doc: &'static str,
    slice: &'static str,
}

const fn different(
    html: &'static str,
    tree: &'static str,
    doc: &'static str,
    slice: &'static str,
) -> Different {
    Different {
        html,
        tree,
        doc,
        slice,
    }
}

/// The inputs html5ever builds another tree from, and why. The test fails when the fork's tree
/// comes to match html5ever's.
const DIFFERENT_TREES: &[(&[Different], &str)] = &[
    (
        &[
            different(
                "<select><option>a<b>x</b></option></select>",
                "<select><option>a<b>x</b></option></select>",
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"text","marks":[{"type":"strong"}],"text":"x"}]}]}"#,
                r#"{"content":[{"type":"text","text":"a"},{"type":"text","marks":[{"type":"strong"}],"text":"x"}]}"#,
            ),
            different(
                "<select><option>a<p>b</p></select>",
                "<select><option>a<p>b</p></option></select>",
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]},{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}"#,
                r#"{"content":[{"type":"text","text":"a"},{"type":"paragraph","content":[{"type":"text","text":"b"}]}],"openEnd":1}"#,
            ),
        ],
        "html5ever 0.40 parses <select> as the standard now does, keeping the elements inside \
         it. parse5 8 predates that, and drops their tags.",
    ),
    (
        &[different(
            "<p><span>a<isindex>b</span>c</p>",
            "<p><span>a<isindex>bc</isindex></span></p>",
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"abc"}]}]}"#,
            r#"{"content":[{"type":"paragraph","content":[{"type":"text","text":"abc"}]}],"openStart":1,"openEnd":1}"#,
        )],
        "html5ever still counts <isindex> among the special elements, which an end tag doesn't \
         close past, as the standard did before it dropped <isindex>. parse5 knows no \
         <isindex>, so </span> closes it.",
    ),
    (
        &[
            different(
                "<math><mi><![CDATA[x<y]]></mi></math>",
                "<math><mi>x&lt;y</mi></math>",
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x<y"}]}]}"#,
                r#"{"content":[{"type":"text","text":"x<y"}]}"#,
            ),
            different(
                "<math><mi><![CDATA[x]]></mi></math>",
                "<math><mi>x</mi></math>",
                X_DOC,
                X_SLICE,
            ),
            different(
                "<svg><desc><![CDATA[x]]></desc></svg>",
                "<svg><desc>x</desc></svg>",
                X_DOC,
                X_SLICE,
            ),
            different(
                "<svg><foreignObject><![CDATA[x]]></foreignObject></svg>",
                "<svg><foreignObject>x</foreignObject></svg>",
                X_DOC,
                X_SLICE,
            ),
            different(
                "<math><annotation-xml encoding=text/html><![CDATA[x]]></annotation-xml></math>",
                r#"<math><annotation-xml encoding="text/html">x</annotation-xml></math>"#,
                X_DOC,
                X_SLICE,
            ),
        ],
        "The standard reads a CDATA section as text wherever the current element isn't HTML's, \
         the integration points among them. parse5 reads one as a comment in a MathML text \
         integration point such as <mi> and in an HTML integration point such as <desc>, as it \
         does in HTML.",
    ),
];

const X_DOC: &str =
    r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}"#;
const X_SLICE: &str = r#"{"content":[{"type":"text","text":"x"}]}"#;

fn fixtures() -> (Schema, Value) {
    let fixtures = read("dom");
    let schema = api::schema(&fixtures["schema"]).expect("the schema");
    (schema, fixtures)
}

fn options(record: &Value) -> ParseOptions<'static, HtmlNode> {
    let preserve_whitespace = match record
        .get("options")
        .map(|options| &options["preserveWhitespace"])
    {
        Some(Value::Bool(true)) => Some(PreserveWhitespace::Yes),
        Some(Value::String(full)) if full == "full" => Some(PreserveWhitespace::Full),
        _ => None,
    };
    ParseOptions {
        preserve_whitespace,
        ..ParseOptions::default()
    }
}

/// A recorded outcome as tarnish gives it: a `DOMException` as an error of the class its name
/// names.
fn expected(recorded: &Value) -> Value {
    let Some(error) = recorded.get("error") else {
        return recorded.clone();
    };
    match error.get("name") {
        Some(name) => json::json!({"error": {"class": name, "message": &error["message"]}}),
        None => json::json!({"error": error.clone()}),
    }
}

/// `record` without the fields `inputs` names.
fn recorded(record: &Value, inputs: &[&str]) -> Value {
    let mut fields = record.as_object().expect("a record").clone();
    for input in inputs {
        fields.remove(input);
    }
    Value::Object(fields)
}

fn report(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// What a template and a document's body parse to, as the recorder records it: each one's
/// tree, the document the parser reads from it, and the template's slice, each outcome as
/// tarnish gives it. A tree is read before the parser reads it, since the parser moves a list
/// nested straight in a list into the item before it.
fn parsed(parser: &DomParser<HtmlNode>, html: &str, record: &Value) -> Value {
    let template_tree = HtmlDom::new().parse_fragment(html).inner_html();
    let template_doc = parse_html(parser, html, options(record)).map(|doc| doc.to_json());
    let slice = parse_html_slice(parser, html, options(record)).map(|slice| slice.to_json());
    let dom = HtmlDom::parse_document(html);
    let body = dom.body().expect("a body");
    let document_tree = body.inner_html();
    let document_doc = parser.parse(&dom, &body, options(record));
    json::json!({
        "template": {
            "tree": template_tree,
            "doc": outcome(template_doc),
            "slice": outcome(slice),
        },
        "document": {
            "tree": document_tree,
            "doc": outcome(document_doc.map(|doc| doc.to_json())),
        },
    })
}

/// A parse record as tarnish-html must give it: the record, where the trees match, or what
/// html5ever's tree gives, where they don't.
fn expected_parse(record: &Value, failures: &mut Vec<String>) -> Value {
    let html = record["html"].as_str().expect("HTML");
    let known = DIFFERENT_TREES
        .iter()
        .flat_map(|(inputs, _)| inputs.iter())
        .find(|different| different.html == html);
    let Some(different) = known else {
        let mut parse = recorded(record, &["html", "options"]);
        for part in parse.as_object_mut().expect("a parse").values_mut() {
            for outcome in part.as_object_mut().expect("a part").values_mut() {
                *outcome = expected(outcome);
            }
        }
        return parse;
    };
    for part in ["template", "document"] {
        if record[part]["tree"].as_str() == Some(different.tree) {
            failures.push(format!(
                "The {part}'s tree of {html:?} is no longer different"
            ));
        }
    }
    let read = |text: &str| json::from_str(text).expect("JSON");
    json::json!({
        "template": {"tree": different.tree, "doc": read(different.doc), "slice": read(different.slice)},
        "document": {"tree": different.tree, "doc": read(different.doc)},
    })
}

#[test]
fn parses_match_the_fork() {
    let (schema, fixtures) = fixtures();
    let parser: DomParser<HtmlNode> = basic::parser(&schema);
    let mut failures = Vec::new();
    for record in records(&fixtures, "parses") {
        let html = record["html"].as_str().expect("HTML");
        let actual = parsed(&parser, html, record);
        let expected = expected_parse(record, &mut failures);
        if actual != expected {
            failures.push(format!(
                "{html:?}\n  expected: {expected}\n  tarnish-html: {actual}"
            ));
        }
    }
    report(failures);
}

#[test]
fn serializations_match_the_fork() {
    let (schema, fixtures) = fixtures();
    let serializer = basic::serializer();
    let mut failures = Vec::new();
    for record in records(&fixtures, "serializes") {
        let doc = Node::from_json(&schema, &record["doc"]);
        let html = doc.and_then(|doc| to_html(&serializer, doc.content()));
        let html = outcome(html.map(|html| json::json!({"html": html})));
        let recorded = expected(&recorded(record, &["doc"]));
        if html != recorded {
            failures.push(format!(
                "{}\n  recorded: {recorded}\n  tarnish-html: {html}",
                record["doc"]
            ));
        }
    }
    report(failures);
}

#[test]
fn inline_styles_match_the_fork() -> Result<()> {
    let (_, fixtures) = fixtures();
    let properties = records(&fixtures, "styleProperties");
    let dom = HtmlDom::new();
    let mut failures = Vec::new();
    for record in records(&fixtures, "styles") {
        let css = Value::String(record["css"].as_str().expect("CSS").to_owned());
        let element = dom.create_element(None, "p")?;
        dom.set_attribute(&element, None, "style", &css)?;
        let mut values = Map::new();
        for property in properties {
            let property = property.as_str().expect("a property");
            let value = dom.style_value(&element, property)?;
            if !value.is_empty() {
                values.insert(property.into(), Value::String(value));
            }
        }
        let written = dom.create_element(None, "p")?;
        dom.set_style(&written, &css)?;
        let actual = json::json!({
            "css": css,
            "length": dom.style_count(&element)?,
            "values": values,
            "cssText": written.attribute("style"),
        });
        if actual != *record {
            failures.push(format!("  recorded: {record}\n  tarnish-html: {actual}"));
        }
    }
    report(failures);
    Ok(())
}

#[test]
fn the_forks_css_engine_is_this_one() {
    let (_, fixtures) = fixtures();
    assert_eq!(fixtures["engine"].as_str(), Some(tarnish_css::ENGINE));
}

/// Each spec rendered as JSON reaches `renderSpec`: an array as a spec's own value, and any spec
/// as the value of a node's attribute.
#[test]
fn rendered_specs_match_the_fork() {
    let (_, fixtures) = fixtures();
    let holders = json::json!({"nodes": [
        ["doc", {"content": "holder*"}],
        ["holder", {"attrs": {"spec": {}}}],
        ["text", {}],
    ]});
    let holders = api::schema(&holders).expect("the holders' schema");
    let mut failures = Vec::new();
    for record in records(&fixtures, "renders") {
        let json = &record["spec"];
        let holder = json::json!({"type": "holder", "attrs": {"spec": json}});
        let holder = Node::from_json(&holders, &holder).expect("a holder");
        let attribute = holder.attrs_view().get("spec").expect("its spec");
        let mut specs = vec![("As an attribute's value", DomSpec::Attr(attribute))];
        if json.is_array() {
            specs.push(("As a spec", DomSpec::from(json.clone())));
        }
        let recorded = expected(&recorded(record, &["spec"]));
        for (how, spec) in specs {
            let dom = HtmlDom::new();
            let rendered = render_spec(&dom, &spec, None).map(|rendered| {
                let hole = rendered.content_dom.map(|hole| hole.outer_html());
                json::json!({"html": rendered.dom.outer_html(), "hole": hole})
            });
            let actual = outcome(rendered);
            if actual != recorded {
                failures.push(format!(
                    "{how}: {json}\n  recorded: {recorded}\n  tarnish-html: {actual}"
                ));
            }
        }
    }
    report(failures);
}

/// The node a `strings` record names, made in `dom` as the recorder made it.
fn node_of(dom: &HtmlDom, record: &Value) -> Result<HtmlNode> {
    let first_child =
        |dom: &HtmlDom, node| dom.first_child(&node).map(|child| child.expect("a child"));
    match record.get("node").and_then(Value::as_str) {
        Some("text") => dom.create_text(&"x".into()),
        Some("comment") => first_child(dom, dom.parse_fragment("<!--x-->")),
        Some("fragment") => dom.create_fragment(),
        Some("document") => Ok(dom.document()),
        Some("doctype") => {
            let doctype = HtmlDom::parse_document("<!DOCTYPE html>");
            first_child(&doctype, doctype.document())
        }
        _ => {
            let element = record["element"].as_str().expect("an element");
            let element = match element.find(' ') {
                Some(space) => dom.create_element(Some(&element[..space]), &element[space + 1..]),
                None => dom.create_element(None, element),
            }?;
            if let Some(href) = record.get("href") {
                dom.set_attribute(&element, None, "href", href)?;
            }
            Ok(element)
        }
    }
}

#[test]
fn attribute_values_of_nodes_match_the_fork() -> Result<()> {
    let (_, fixtures) = fixtures();
    let mut failures = Vec::new();
    for record in records(&fixtures, "strings") {
        let dom = match record.get("document").and_then(Value::as_str) {
            Some(html) => HtmlDom::parse_document(html),
            None => HtmlDom::new(),
        };
        let string = dom.attribute_value(&node_of(&dom, record)?)?;
        let actual = json::json!({"string": string});
        if actual != recorded(record, &["document", "node", "element", "href"]) {
            failures.push(format!("  recorded: {record}\n  tarnish-html: {actual}"));
        }
    }
    report(failures);
    Ok(())
}
