//! Parse rules, the DOM parser, and the order of a schema's rules.

use std::sync::Arc;

use napi::bindgen_prelude::{ClassInstance, FromNapiValue, Object, Unknown};
use napi::{Env, JsValue, Result, ValueType};
use napi_derive::napi;
use tarnish::dom::{
    ClearMarkHook, ContentElement, DomParser, FindPosition, GetAttrs, GetContentHook, ParseOptions,
    ParseRule, PreserveWhitespace, RuleFromNode, RuleKind, Skip, TagRule,
};
use tarnish::{Attrs, Mark};

use super::{JsDom, JsNode, dom_node};
use crate::content::ContentMatchHandle;
use crate::fragment::FragmentHandle;
use crate::js::{self, Hook, OrThrow};
use crate::mark;
use crate::node::{self, NodeHandle};
use crate::position::ResolvedPosHandle;
use crate::schema::SchemaHandle;
use crate::slice;

fn truthy_string(object: &Object, key: &str) -> Result<Option<String>> {
    Ok(js::get_string(object, key)?.filter(|value| !value.is_empty()))
}

fn preserve_whitespace(value: Unknown) -> Result<Option<PreserveWhitespace>> {
    if js::is_nullish(&value)? {
        return Ok(None);
    }
    Ok(Some(match value.get_type()? {
        ValueType::String if String::from_unknown(value)? == "full" => PreserveWhitespace::Full,
        _ if value.coerce_to_bool()? => PreserveWhitespace::Yes,
        _ => PreserveWhitespace::No,
    }))
}

/// What `getAttrs` gave: `false` to not match, otherwise the attributes, falsy for the
/// defaults.
fn attrs_result(result: Unknown) -> Result<Option<Option<Attrs>>> {
    if js::is_false(&result)? {
        return Ok(None);
    }
    if !result.coerce_to_bool()? {
        return Ok(Some(None));
    }
    js::attrs_from_js(result).map(Some)
}

/// A parse rule of JavaScript's, `None` when it is neither a tag rule nor a style rule.
fn parse_rule(rule: &Object) -> Result<Option<ParseRule<JsNode>>> {
    let (tag, style) = (js::get(rule, "tag")?, js::get(rule, "style")?);
    let kind = if !js::is_nullish(&tag)? {
        RuleKind::Tag(tag_rule(rule, js::coerce_to_string(&tag)?)?)
    } else if !js::is_nullish(&style)? {
        RuleKind::Style(js::coerce_to_string(&style)?)
    } else {
        return Ok(None);
    };
    rule_with_kind(rule, kind).map(Some)
}

fn tag_rule(rule: &Object, selector: String) -> Result<TagRule<JsNode>> {
    let namespace = js::get(rule, "namespace")?;
    let content_element = js::get(rule, "contentElement")?;
    Ok(TagRule {
        selector,
        namespace: match namespace.get_type()? {
            ValueType::Undefined => None,
            ValueType::Null => Some(None),
            _ => Some(Some(js::coerce_to_string(&namespace)?)),
        },
        content_element: match content_element.get_type()? {
            ValueType::String => Some(ContentElement::Selector(String::from_unknown(
                content_element,
            )?)),
            ValueType::Function => Hook::method(rule, "contentElement")?.map(|hook| {
                ContentElement::Hook(Arc::new(move |node: &JsNode| {
                    js::host(|env| JsNode::new(hook.call(env, node.value(env)?)?))
                }))
            }),
            _ if content_element.coerce_to_bool()? => {
                Some(ContentElement::Node(JsNode::new(content_element)?))
            }
            _ => None,
        },
        get_content: Hook::method(rule, "getContent")?.map(|hook| {
            Arc::new(move |node: &JsNode, _: &tarnish::Schema| {
                js::host(|env| {
                    let content = hook.call(env, node.value(env)?)?;
                    Ok(ClassInstance::<FragmentHandle>::from_unknown(content)?
                        .fragment
                        .clone())
                })
            }) as GetContentHook<JsNode>
        }),
        preserve_whitespace: preserve_whitespace(js::get(rule, "preserveWhitespace")?)?,
    })
}

fn rule_with_kind(rule: &Object, kind: RuleKind<JsNode>) -> Result<ParseRule<JsNode>> {
    let is_tag = matches!(kind, RuleKind::Tag(_));
    let (priority, skip) = (js::get(rule, "priority")?, js::get(rule, "skip")?);
    let mut parsed = ParseRule::new(kind);
    parsed.priority = match priority.get_type()? {
        ValueType::Number => Some(f64::from_unknown(priority)?),
        _ => None,
    };
    parsed.consuming = !js::is_false(&js::get(rule, "consuming")?)?;
    parsed.context = truthy_string(rule, "context")?;
    parsed.node = truthy_string(rule, "node")?;
    parsed.mark = truthy_string(rule, "mark")?;
    parsed.ignore = js::get(rule, "ignore")?.coerce_to_bool()?;
    parsed.close_parent = js::get(rule, "closeParent")?.coerce_to_bool()?;
    parsed.skip = match dom_node(skip)? {
        Some(node) => Skip::Node(node),
        None if skip.coerce_to_bool()? => Skip::Yes,
        None => Skip::No,
    };
    parsed.attrs = js::attrs_from_js(js::get(rule, "attrs")?)?;
    parsed.get_attrs = Hook::method(rule, "getAttrs")?.map(|hook| {
        if is_tag {
            GetAttrs::Tag(Arc::new(move |node: &JsNode| {
                js::host(|env| attrs_result(hook.call(env, node.value(env)?)?))
            }))
        } else {
            GetAttrs::Style(Arc::new(move |value: &str| {
                js::host(|env| attrs_result(hook.call(env, value)?))
            }))
        }
    });
    parsed.clear_mark = Hook::method(rule, "clearMark")?.map(|hook| {
        Arc::new(move |mark: &Mark| {
            js::host(|env| hook.call(env, mark::wrap(env, mark)?)?.coerce_to_bool())
        }) as ClearMarkHook
    });
    Ok(parsed)
}

#[napi]
pub struct DomParserHandle {
    parser: DomParser<JsNode>,
}

#[napi]
impl DomParserHandle {
    /// A parser of the rules, whose `getContent` gives the handle of the fragment it makes.
    #[napi(constructor)]
    pub fn new(env: &Env, schema: &SchemaHandle, rules: Vec<Object>) -> Result<Self> {
        let mut parsed = Vec::with_capacity(rules.len());
        for rule in &rules {
            parsed.extend(parse_rule(rule)?);
        }
        Ok(DomParserHandle {
            parser: DomParser::new(schema.schema.clone(), parsed).or_throw(env)?,
        })
    }

    /// Parse with JavaScript's options, which hold handles in place of wrappers.
    #[napi]
    pub fn parse<'env>(
        &self,
        env: &'env Env,
        dom: Unknown,
        options: Object,
    ) -> Result<Unknown<'env>> {
        let root = JsNode::new(dom)?;
        let parsed = with_options(options, |dom, options| {
            self.parser.parse(dom, &root, options)
        })?;
        node::wrap(env, &parsed.or_throw(env)?)
    }

    #[napi]
    pub fn parse_slice<'env>(
        &self,
        env: &'env Env,
        dom: Unknown,
        options: Object,
    ) -> Result<Unknown<'env>> {
        let root = JsNode::new(dom)?;
        let parsed = with_options(options, |dom, options| {
            self.parser.parse_slice(dom, &root, options)
        })?;
        slice::wrap(env, &parsed.or_throw(env)?)
    }
}

/// Run a parse with JavaScript's options, then write the positions it found back into the
/// `findPositions` objects.
fn with_options<T>(
    options: Object,
    parse: impl FnOnce(&JsDom, ParseOptions<'_, JsNode>) -> tarnish::Result<T>,
) -> Result<tarnish::Result<T>> {
    let set = |key: &str| -> Result<Option<Unknown>> {
        let value = js::get(&options, key)?;
        Ok((!js::is_nullish(&value)?).then_some(value))
    };
    let index = |key: &str| -> Result<Option<usize>> {
        set(key)?
            .map(|value| u32::from_unknown(value).map(|index| index as usize))
            .transpose()
    };
    let find_array = set("findPositions")?;
    let mut find_objects = match find_array {
        Some(array) => Vec::<Object>::from_unknown(array)?,
        None => Vec::new(),
    };
    let mut find = Vec::with_capacity(find_objects.len());
    for object in &find_objects {
        let pos = js::get(object, "pos")?;
        find.push(FindPosition {
            node: JsNode::new(js::get(object, "node")?)?,
            offset: u32::from_unknown(js::get(object, "offset")?)? as usize,
            pos: match pos.get_type()? {
                ValueType::Number => Some(u32::from_unknown(pos)? as usize),
                _ => None,
            },
        });
    }
    let rule_from_node = set("ruleFromNode")?.map(|function| {
        move |node: &JsNode| -> tarnish::Result<Option<ParseRule<JsNode>>> {
            js::host(|env| {
                let rule = js::call(function, node.value(env)?)?;
                if !rule.coerce_to_bool()? {
                    return Ok(None);
                }
                let rule = Object::from_unknown(rule)?;
                let tag = TagRule {
                    selector: String::new(),
                    namespace: None,
                    content_element: None,
                    get_content: None,
                    preserve_whitespace: preserve_whitespace(js::get(
                        &rule,
                        "preserveWhitespace",
                    )?)?,
                };
                rule_with_kind(&rule, RuleKind::Tag(tag)).map(Some)
            })
        }
    });
    let top_node = set("topNode")?
        .map(ClassInstance::<NodeHandle>::from_unknown)
        .transpose()?;
    let top_match = set("topMatch")?
        .map(ClassInstance::<ContentMatchHandle>::from_unknown)
        .transpose()?;
    let context = set("context")?
        .map(ClassInstance::<ResolvedPosHandle>::from_unknown)
        .transpose()?;
    let parse_options = ParseOptions {
        preserve_whitespace: preserve_whitespace(js::get(&options, "preserveWhitespace")?)?,
        find_positions: find_array.is_some().then_some(&mut find),
        from: index("from")?,
        to: index("to")?,
        top_node: top_node.map(|node| node.node.clone()),
        top_match: top_match.map(|found| found.content_match.clone()),
        context: context.map(|pos| pos.pos.clone()),
        rule_from_node: rule_from_node
            .as_ref()
            .map(|f| f as RuleFromNode<'_, JsNode>),
        top_open: js::get(&options, "topOpen")?.coerce_to_bool()?,
    };
    let result = parse(&JsDom { document: None }, parse_options);
    for (object, found) in find_objects.iter_mut().zip(&find) {
        if let Some(pos) = found.pos {
            object.set("pos", pos as f64)?;
        }
    }
    Ok(result)
}

/// `DOMParser.schemaRules`: the schema's rules in parse order, each as `[ofMark, typeIndex,
/// ruleIndex, named]`.
#[napi]
pub fn schema_rules(
    schema: &SchemaHandle,
    marks: Vec<Vec<Object>>,
    nodes: Vec<Vec<Object>>,
) -> Result<Vec<(bool, u32, u32, bool)>> {
    let named = |types: Vec<Vec<Object>>, names: Vec<String>| {
        types
            .into_iter()
            .zip(names)
            .map(|(rules, name)| {
                let rules = rules
                    .iter()
                    .map(|rule| match parse_rule(rule)? {
                        Some(rule) => Ok(rule),
                        // A rule of neither kind still takes its place in the order, which its
                        // kind doesn't decide.
                        None => rule_with_kind(rule, RuleKind::Style(String::new())),
                    })
                    .collect::<Result<_>>()?;
                Ok((name, rules))
            })
            .collect::<Result<Vec<_>>>()
    };
    let schema = &schema.schema;
    let mark_names = schema.mark_types().map(|mark| mark.name().to_owned());
    let node_names = schema.node_types().map(|node| node.name().to_owned());
    let ordered = tarnish::dom::schema_rules(
        named(marks, mark_names.collect())?,
        named(nodes, node_names.collect())?,
    );
    Ok(ordered
        .into_iter()
        .map(|rule| {
            (
                rule.of_mark,
                rule.type_index as u32,
                rule.rule_index as u32,
                rule.named,
            )
        })
        .collect())
}
