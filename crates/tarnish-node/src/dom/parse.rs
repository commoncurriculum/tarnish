//! Parse rules, the DOM parser, and the order of a schema's rules.

use std::sync::Arc;

use napi::bindgen_prelude::{ClassInstance, FromNapiValue, Object, Unknown};
use napi::{Env, JsValue, Result, ValueType};
use napi_derive::napi;
use tarnish::Mark;
use tarnish::dom::{
    AttrsHook, ClearMarkHook, Content, ContentElement, DomParser, ElementRule, FindPosition,
    GetAttrsResult, GetContentHook, Namespace, ParseOptions, PreserveWhitespace, Rule,
    RuleFromNode, Skip, StyleAttrsHook, StyleRule, TagRule,
};

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

fn truthy(object: &Object, key: &str) -> Result<bool> {
    js::get(object, key)?.coerce_to_bool()
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
fn attrs_result(result: Unknown) -> Result<GetAttrsResult> {
    if js::is_false(&result)? {
        return Ok(GetAttrsResult::Reject);
    }
    if !result.coerce_to_bool()? {
        return Ok(GetAttrsResult::Defaults);
    }
    Ok(match js::attrs_from_js(result)? {
        Some(attrs) => GetAttrsResult::Attrs(attrs),
        None => GetAttrsResult::Defaults,
    })
}

fn priority(rule: &Object) -> Result<Option<f64>> {
    let priority = js::get(rule, "priority")?;
    match priority.get_type()? {
        ValueType::Number => f64::from_unknown(priority).map(Some),
        _ => Ok(None),
    }
}

/// A rule of this kind, with the fields a rule of either kind has.
fn rule_of<K>(object: &Object, kind: K) -> Result<Rule<K>> {
    let mut rule = Rule::new(kind);
    rule.priority = priority(object)?;
    rule.consuming = !js::is_false(&js::get(object, "consuming")?)?;
    rule.context = truthy_string(object, "context")?;
    rule.mark = truthy_string(object, "mark")?;
    rule.ignore = truthy(object, "ignore")?;
    rule.attrs = js::attrs_from_js(js::get(object, "attrs")?)?;
    Ok(rule)
}

fn tag_rule(object: &Object, tag: String) -> Result<Rule<TagRule<JsNode>>> {
    let namespace = js::get(object, "namespace")?;
    let get_attrs = Hook::method(object, "getAttrs")?.map(|hook| {
        Arc::new(move |node: &JsNode| {
            js::host(|env| attrs_result(hook.call(env, node.value(env)?)?))
        }) as AttrsHook<JsNode>
    });
    let kind = TagRule {
        tag,
        namespace: match namespace.get_type()? {
            ValueType::Undefined => Namespace::Any,
            ValueType::Null => Namespace::Null,
            _ => Namespace::Is(js::coerce_to_string(&namespace)?),
        },
        get_attrs,
        element: element_rule(object)?,
    };
    rule_of(object, kind)
}

fn element_rule(rule: &Object) -> Result<ElementRule<JsNode>> {
    let skip = js::get(rule, "skip")?;
    let content_element = js::get(rule, "contentElement")?;
    let content = match Hook::method(rule, "getContent")? {
        Some(hook) => Content::Get(Arc::new(move |node: &JsNode, _: &tarnish::Schema| {
            js::host(|env| {
                let content = hook.call(env, node.value(env)?)?;
                Ok(ClassInstance::<FragmentHandle>::from_unknown(content)?
                    .fragment
                    .clone())
            })
        }) as GetContentHook<JsNode>),
        None => match content_element.get_type()? {
            ValueType::String => Content::Element(ContentElement::Selector(String::from_unknown(
                content_element,
            )?)),
            ValueType::Function => match Hook::method(rule, "contentElement")? {
                Some(hook) => {
                    Content::Element(ContentElement::Hook(Arc::new(move |node: &JsNode| {
                        js::host(|env| JsNode::new(hook.call(env, node.value(env)?)?))
                    })))
                }
                None => Content::Children,
            },
            _ if content_element.coerce_to_bool()? => {
                Content::Element(ContentElement::Node(JsNode::new(content_element)?))
            }
            _ => Content::Children,
        },
    };
    Ok(ElementRule {
        node: truthy_string(rule, "node")?,
        skip: match dom_node(skip)? {
            Some(node) => Skip::Node(node),
            None if skip.coerce_to_bool()? => Skip::Yes,
            None => Skip::No,
        },
        close_parent: truthy(rule, "closeParent")?,
        content,
        preserve_whitespace: preserve_whitespace(js::get(rule, "preserveWhitespace")?)?,
    })
}

fn style_rule(object: &Object, style: String) -> Result<Rule<StyleRule>> {
    let get_attrs = Hook::method(object, "getAttrs")?.map(|hook| {
        Arc::new(move |value: &str| js::host(|env| attrs_result(hook.call(env, value)?)))
            as StyleAttrsHook
    });
    let clear_mark = Hook::method(object, "clearMark")?.map(|hook| {
        Arc::new(move |mark: &Mark<'static>| {
            js::host(|env| hook.call(env, mark::wrap(env, mark)?)?.coerce_to_bool())
        }) as ClearMarkHook
    });
    let kind = StyleRule {
        style,
        get_attrs,
        clear_mark,
    };
    rule_of(object, kind)
}

#[napi]
pub struct DomParserHandle {
    parser: DomParser<JsNode>,
}

#[napi]
impl DomParserHandle {
    /// A parser of the rules, whose `getContent` gives the handle of the fragment it makes. A
    /// rule with neither a `tag` nor a `style` is left out.
    #[napi(constructor)]
    pub fn new(env: &Env, schema: &SchemaHandle, rules: Vec<Object>) -> Result<Self> {
        let (mut tags, mut styles) = (Vec::new(), Vec::new());
        for rule in &rules {
            let (tag, style) = (js::get(rule, "tag")?, js::get(rule, "style")?);
            if !js::is_nullish(&tag)? {
                tags.push(tag_rule(rule, js::coerce_to_string(&tag)?)?);
            } else if !js::is_nullish(&style)? {
                styles.push(style_rule(rule, js::coerce_to_string(&style)?)?);
            }
        }
        Ok(DomParserHandle {
            parser: DomParser::new(schema.schema.clone(), tags, styles).or_throw(env)?,
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
                ValueType::Number => Some(f64::from_unknown(pos)? as isize),
                _ => None,
            },
        });
    }
    let rule_from_node = set("ruleFromNode")?.map(|function| {
        move |node: &JsNode| -> tarnish::Result<Option<Rule<ElementRule<JsNode>>>> {
            js::host(|env| {
                let found = js::call(function, node.value(env)?)?;
                if !found.coerce_to_bool()? {
                    return Ok(None);
                }
                let found = Object::from_unknown(found)?;
                rule_of(&found, element_rule(&found)?).map(Some)
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
        find_positions: find_array.is_some().then_some(&mut find[..]),
        from: index("from")?,
        to: index("to")?,
        top_node: top_node.map(|node| node.node.clone()),
        top_match: top_match.as_ref().map(|found| found.content_match()),
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

/// `DOMParser.schemaRules`: the schema's rules, given by type as its mark and node types come,
/// in parse order, each as `[ofMark, typeIndex, ruleIndex, named]`. A rule is `named` when it
/// gets its type's name: a mark type's rule without a `mark`, `ignore` or `clearMark`, and a
/// node type's without a `node`, `ignore` or `mark`.
#[napi]
pub fn schema_rules(
    marks: Vec<Vec<Object>>,
    nodes: Vec<Vec<Object>>,
) -> Result<Vec<(bool, u32, u32, bool)>> {
    let mut rules = Vec::new();
    for (of_mark, types) in [(true, marks), (false, nodes)] {
        let own = match of_mark {
            true => ["mark", "ignore", "clearMark"],
            false => ["node", "ignore", "mark"],
        };
        for (type_index, type_rules) in types.iter().enumerate() {
            for (rule_index, rule) in type_rules.iter().enumerate() {
                let mut named = true;
                for key in own {
                    if truthy(rule, key)? {
                        named = false;
                        break;
                    }
                }
                let entry = (of_mark, type_index as u32, rule_index as u32, named);
                rules.push((priority(rule)?, entry));
            }
        }
    }
    let ordered = tarnish::dom::by_priority(rules, |(priority, _)| *priority);
    Ok(ordered.into_iter().map(|(_, entry)| entry).collect())
}
