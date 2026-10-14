//! What a schema keeps of its spec, and the order of the parse rules its types give.

use std::sync::Arc;

use tarnish::dom::{ParseRule, Rule, StyleRule, TagRule, schema_rules};
use tarnish::js::json::stringify;
use tarnish::json::{self, Map, Value};
use tarnish::{Node, Schema, api, js};
use tarnish_fixtures::{assert_same, read, records};

fn schema(spec: &str) -> Schema {
    api::schema(&json::from_str(spec).expect("JSON")).expect("a schema")
}

/// A document of one item with no attributes given, of a type whose attributes are `attrs`, read
/// and written back as JSON.
fn item_read_back(attrs: &str) -> String {
    let schema = schema(&format!(
        r#"{{"nodes": {{"doc": {{"content": "item*"}}, "item": {{"attrs": {attrs}}}, "text": {{}}}}}}"#
    ));
    let doc = json::from_str(r#"{"type": "doc", "content": [{"type": "item", "attrs": {}}]}"#)
        .expect("JSON");
    Node::from_json(&schema, &doc)
        .map_or_else(|error| error.to_string(), |doc| stringify(&doc.to_json()))
}

#[test]
fn reads_has_default_with_no_default_as_a_default_of_undefined() {
    // As ProseMirror reads `{default: undefined}`, `{}` and `{default: 0}`.
    assert_eq!(
        item_read_back(r#"{"indent": {"hasDefault": true}}"#),
        r#"{"type":"doc","content":[{"type":"item","attrs":{}}]}"#
    );
    assert_eq!(
        item_read_back(r#"{"indent": {}}"#),
        "RangeError: No value supplied for attribute indent"
    );
    assert_eq!(
        item_read_back(r#"{"indent": {"hasDefault": true, "default": 0}}"#),
        r#"{"type":"doc","content":[{"type":"item","attrs":{"indent":0}}]}"#
    );
}

#[test]
fn keeps_the_properties_prosemirror_does_not_read() {
    let schema = schema(
        r#"{"nodes": {"doc": {"content": "cell+"}, "cell": {"content": "text*", "tableRole": "cell"}, "text": {}},
            "marks": {"em": {"inclusive": false, "rank": {"x": 1}}}}"#,
    );
    let cell = schema.expect_node_type("cell").expect("a cell type");
    assert_eq!(
        stringify(&Value::Object(cell.spec().extra.clone())),
        r#"{"tableRole":"cell"}"#
    );
    let em = schema.mark_type_at(0);
    assert_eq!(
        stringify(&Value::Object(em.spec().extra.clone())),
        r#"{"rank":{"x":1}}"#
    );
}

/// A rule as `harness/record-dom.mjs` writes one, `clearMark` a hook that clears every mark.
fn rule(written: &Value) -> ParseRule<()> {
    let text = |field: &str| {
        written
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let mut rule = match (text("tag"), text("style")) {
        (Some(tag), _) => {
            let mut tag = TagRule::new(tag);
            tag.element.node = text("node");
            ParseRule::Tag(Rule::new(tag))
        }
        (None, style) => {
            let mut style = StyleRule::new(style.expect("a tag or a style"));
            if written.get("clearMark").is_some() {
                style.clear_mark = Some(Arc::new(|_| Ok(true)));
            }
            ParseRule::Style(Rule::new(style))
        }
    };
    let (priority, ignore, mark) = match &mut rule {
        ParseRule::Tag(rule) => (&mut rule.priority, &mut rule.ignore, &mut rule.mark),
        ParseRule::Style(rule) => (&mut rule.priority, &mut rule.ignore, &mut rule.mark),
    };
    *priority = written.get("priority").and_then(Value::as_f64);
    *ignore = written.get("ignore").is_some();
    *mark = text("mark");
    rule
}

/// A rule as the recorder writes one: the fields it has, in one order.
fn written(rule: &ParseRule<()>) -> Value {
    let mut fields = Map::new();
    let (priority, ignore, mark) = match rule {
        ParseRule::Tag(rule) => {
            fields.insert("tag".into(), rule.kind.tag.as_str().into());
            (rule.priority, rule.ignore, &rule.mark)
        }
        ParseRule::Style(rule) => {
            fields.insert("style".into(), rule.kind.style.as_str().into());
            (rule.priority, rule.ignore, &rule.mark)
        }
    };
    if let Some(priority) = priority {
        fields.insert("priority".into(), js::number(priority));
    }
    if ignore {
        fields.insert("ignore".into(), true.into());
    }
    if let ParseRule::Style(rule) = rule
        && rule.kind.clear_mark.is_some()
    {
        fields.insert("clearMark".into(), true.into());
    }
    if let Some(mark) = mark {
        fields.insert("mark".into(), mark.as_str().into());
    }
    if let ParseRule::Tag(rule) = rule
        && let Some(node) = &rule.kind.element.node
    {
        fields.insert("node".into(), node.as_str().into());
    }
    Value::Object(fields)
}

/// `DOMParser.schemaRules` of a mark type's and a node type's rules, which say what they make in
/// each way a tag rule and a style rule can, at each priority: the order it puts them in, and the
/// type it names in each that says nothing.
#[test]
fn orders_and_names_rules_as_schema_rules_does() {
    let fixtures = read("dom");
    let schema = api::schema(&fixtures["ruleSchema"]).expect("a schema");
    let rules_of = |name: &str| -> Vec<ParseRule<()>> {
        let given = records(&fixtures, "parseRules").iter();
        given
            .filter(|given| given["of"] == name)
            .map(rule)
            .collect()
    };
    let marks = schema
        .mark_types()
        .map(|mark| rules_of(mark.name()))
        .collect();
    let nodes = schema
        .node_types()
        .map(|node| rules_of(node.name()))
        .collect();
    let ordered = schema_rules(&schema, marks, nodes)
        .iter()
        .map(written)
        .collect();
    assert_same(
        &Value::Array(records(&fixtures, "schemaRules").to_vec()),
        &Value::Array(ordered),
    );
}
