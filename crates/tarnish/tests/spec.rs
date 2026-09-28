//! What a schema keeps of its spec, and the order of the parse rules its types give.

use tarnish::dom::{ParseRule, Rule, StyleRule, TagRule, schema_rules};
use tarnish::js::json::stringify;
use tarnish::json::{self, Value};
use tarnish::{Schema, api};

fn schema(spec: &str) -> Schema {
    api::schema(&json::from_str(spec).expect("JSON")).expect("a schema")
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

fn tag(tag: &str, priority: Option<f64>) -> ParseRule<()> {
    let mut rule = Rule::new(TagRule::new(tag));
    rule.priority = priority;
    ParseRule::Tag(rule)
}

fn tags(rules: &[ParseRule<()>]) -> Vec<String> {
    rules
        .iter()
        .map(|rule| match rule {
            ParseRule::Tag(rule) => rule.kind.tag.clone(),
            ParseRule::Style(rule) => rule.kind.style.clone(),
        })
        .collect()
}

const INLINE: &str = r#"{"marks": {"em": {}},
    "nodes": {"doc": {"content": "inline*"}, "text": {"group": "inline"},
              "foo": {"group": "inline", "inline": true}, "bar": {"group": "inline", "inline": true}}}"#;

#[test]
fn orders_rules_by_priority_then_schema_order() {
    let schema = schema(INLINE);
    let marks = vec![vec![tag("i", None), tag("em", None)]];
    let nodes = vec![
        vec![],
        vec![],
        vec![tag("foo", None)],
        vec![tag("bar", None)],
    ];
    assert_eq!(
        tags(&schema_rules(&schema, marks, nodes)),
        ["i", "em", "foo", "bar"]
    );

    let marks = vec![vec![tag("i", Some(40.0)), tag("em", Some(70.0))]];
    let nodes = vec![
        vec![],
        vec![],
        vec![tag("foo", None)],
        vec![tag("bar", Some(60.0))],
    ];
    assert_eq!(
        tags(&schema_rules(&schema, marks, nodes)),
        ["em", "bar", "foo", "i"]
    );
}

#[test]
fn names_each_rule_for_its_type_unless_it_says_what_it_makes() {
    let schema = schema(INLINE);
    let mut ignored = Rule::new(TagRule::<()>::new("script"));
    ignored.ignore = true;
    let mut other = Rule::new(TagRule::<()>::new("b"));
    other.kind.element.node = Some("bar".into());
    let marks = vec![vec![
        ParseRule::Tag(Rule::new(TagRule::new("i"))),
        ParseRule::Style(Rule::new(StyleRule::new("font-style=italic"))),
    ]];
    let nodes = vec![
        vec![],
        vec![],
        vec![ParseRule::Tag(ignored), ParseRule::Tag(other)],
        vec![],
    ];
    let made: Vec<(Option<String>, Option<String>)> = schema_rules(&schema, marks, nodes)
        .into_iter()
        .map(|rule| match rule {
            ParseRule::Tag(rule) => (rule.kind.element.node, rule.mark),
            ParseRule::Style(rule) => (None, rule.mark),
        })
        .collect();
    assert_eq!(
        made,
        [
            (None, Some("em".into())),
            (None, Some("em".into())),
            (None, None),
            (Some("bar".into()), None),
        ]
    );
}
