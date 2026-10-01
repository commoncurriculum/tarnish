//! What ProseMirror's `DOMParser` and `DOMSerializer` do in the linkedom fork, recorded by
//! `harness/record-dom.mjs`: over this crate's DOM, tarnish's must build the same trees, parse
//! the same documents, and write the same HTML and strings.

mod basic;

use tarnish::dom::{Dom, DomParser, DomSpec, ParseOptions, PreserveWhitespace, render_spec};
use tarnish::json::{self, Value};
use tarnish::{Node, Result, Schema, api};
use tarnish_fixtures::{outcome, read};
use tarnish_html::{HtmlDom, HtmlNode, parse_html, parse_html_slice, to_html};

/// Inputs html5ever builds another tree from than the fork's parse5 8 does, and why. The test
/// prints both trees, and fails when they come to match.
const DIFFERENT_TREES: &[(&[&str], &str)] = &[
    (
        &[
            "<select><option>a<b>x</b></option></select>",
            "<select><option>a<p>b</p></select>",
        ],
        "html5ever 0.40 parses <select> as the standard now does, keeping the elements inside \
         it. parse5 8 predates that, and drops their tags.",
    ),
    (
        &["<p><span>a<isindex>b</span>c</p>"],
        "html5ever still counts <isindex> among the special elements, which an end tag doesn't \
         close past, as the standard did before it dropped <isindex>. parse5 knows no \
         <isindex>, so </span> closes it.",
    ),
    (
        &[
            "<math><mi><![CDATA[x<y]]></mi></math>",
            "<math><mi><![CDATA[x]]></mi></math>",
            "<svg><desc><![CDATA[x]]></desc></svg>",
            "<svg><foreignObject><![CDATA[x]]></foreignObject></svg>",
            "<math><annotation-xml encoding=text/html><![CDATA[x]]></annotation-xml></math>",
        ],
        "The standard reads a CDATA section as text wherever the current element isn't HTML's, \
         the integration points among them. parse5 reads one as a comment in a MathML text \
         integration point such as <mi> and in an HTML integration point such as <desc>, as it \
         does in HTML.",
    ),
];

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

fn report(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

struct Check<'r> {
    html: &'r str,
    failures: Vec<String>,
}

impl Check<'_> {
    fn equal(&mut self, what: &str, actual: &Value, recorded: &Value) {
        let expected = expected(recorded);
        if *actual != expected {
            self.failures.push(format!(
                "{what} of {:?}\n  recorded: {expected}\n  tarnish-html: {actual}",
                self.html
            ));
        }
    }

    /// Whether the trees match. Where they are known to differ, check that they still do.
    fn tree(&mut self, what: &str, actual: String, expected: &Value) -> bool {
        let known = DIFFERENT_TREES
            .iter()
            .find(|(inputs, _)| inputs.contains(&self.html));
        match (known, expected.as_str() == Some(&actual)) {
            (None, true) => true,
            (Some(_), false) => {
                println!(
                    "{what} of {:?}\n  recorded: {expected}\n  html5ever: {actual:?}",
                    self.html
                );
                false
            }
            (None, false) => {
                self.equal(what, &Value::String(actual), expected);
                false
            }
            (Some(_), true) => {
                let message = format!("{what} of {:?} is no longer different", self.html);
                self.failures.push(message);
                false
            }
        }
    }
}

#[test]
fn parses_match_the_fork() {
    let (schema, fixtures) = fixtures();
    let parser: DomParser<HtmlNode> = basic::parser(&schema);
    let mut failures = Vec::new();
    for record in fixtures["parses"].as_array().expect("parses") {
        let html = record["html"].as_str().expect("HTML");
        let mut check = Check {
            html,
            failures: Vec::new(),
        };

        let (template, document) = (&record["template"], &record["document"]);
        let dom = HtmlDom::new();
        let tree = dom.parse_fragment(html).inner_html();
        if check.tree("The template's tree", tree, &template["tree"]) {
            let doc = parse_html(&parser, html, options(record));
            let doc = outcome(doc.map(|doc| doc.to_json()));
            check.equal("The template's document", &doc, &template["doc"]);
            let slice = parse_html_slice(&parser, html, options(record));
            let slice = outcome(slice.map(|slice| slice.to_json()));
            check.equal("The template's slice", &slice, &template["slice"]);
        }

        let body = |dom: &HtmlDom| dom.body().expect("a body");
        let tree = body(&HtmlDom::parse_document(html)).inner_html();
        if check.tree("The document's tree", tree, &document["tree"]) {
            let dom = HtmlDom::parse_document(html);
            let doc = parser.parse(&dom, &body(&dom), options(record));
            let doc = outcome(doc.map(|doc| doc.to_json()));
            check.equal("The document's document", &doc, &document["doc"]);
        }
        failures.extend(check.failures);
    }
    report(failures);
}

#[test]
fn serializations_match_the_fork() {
    let (schema, fixtures) = fixtures();
    let serializer = basic::serializer();
    let mut failures = Vec::new();
    for record in fixtures["serializes"].as_array().expect("serializations") {
        let doc = Node::from_json(&schema, &record["doc"]);
        let html = doc.and_then(|doc| to_html(&serializer, doc.content()));
        let html = outcome(html.map(Value::String));
        let recorded = match record.get("html") {
            Some(html) => html.clone(),
            None => json::json!({"error": record["error"].clone()}),
        };
        if html != expected(&recorded) {
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
    let properties = fixtures["styleProperties"].as_array().expect("properties");
    let dom = HtmlDom::new();
    let mut failures = Vec::new();
    for record in fixtures["styles"].as_array().expect("styles") {
        let css = Value::String(record["css"].as_str().expect("CSS").to_owned());
        let element = dom.create_element(None, "p")?;
        dom.set_attribute(&element, None, "style", &css)?;
        let mut values = json::Map::new();
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
    for record in fixtures["renders"].as_array().expect("renders") {
        let json = &record["spec"];
        let holder = json::json!({"type": "holder", "attrs": {"spec": json}});
        let holder = Node::from_json(&holders, &holder).expect("a holder");
        let attribute = holder.attrs_view().get("spec").expect("its spec");
        let mut specs = vec![("As an attribute's value", DomSpec::Attr(attribute))];
        if json.is_array() {
            specs.push(("As a spec", DomSpec::from(json.clone())));
        }
        let mut recorded = record.clone();
        recorded.as_object_mut().expect("a record").remove("spec");
        for (how, spec) in specs {
            let dom = HtmlDom::new();
            let rendered = render_spec(&dom, &spec, None).map(|rendered| {
                let hole = rendered.content_dom.map(|hole| hole.outer_html());
                json::json!({"html": rendered.dom.outer_html(), "hole": hole})
            });
            let actual = outcome(rendered);
            if actual != expected(&recorded) {
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
    for record in fixtures["strings"].as_array().expect("strings") {
        let dom = match record.get("document").and_then(Value::as_str) {
            Some(html) => HtmlDom::parse_document(html),
            None => HtmlDom::new(),
        };
        let string = dom.attribute_value(&node_of(&dom, record)?)?;
        if record["string"].as_str() != Some(&string) {
            failures.push(format!("  recorded: {record}\n  tarnish-html: {string:?}"));
        }
    }
    report(failures);
    Ok(())
}
