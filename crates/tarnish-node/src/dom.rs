//! The DOM parser and serializer, over the host's DOM: jsdom in the tests.

use std::collections::HashMap;
use std::ptr;
use std::sync::Arc;

use napi::bindgen_prelude::FromNapiValue;
use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::dom::{
    ContentElement, Dom, DomParser, DomSerializer, DomSpec, FindPosition, GetAttrs, MarkToDom,
    NodeKind, NodeToDom, ParseOptions, ParseRule, PreserveWhitespace, Rendered, RuleKind, Skip,
    TagRule,
};
use tarnish::{Attrs, Text, Value};

use crate::content::ContentMatchArg;
use crate::fragment::FragmentArg;
use crate::js::{self, Hook, Js, JsRef, OrThrow};
use crate::mark::{self, MarkArg};
use crate::node::{self, NodeArg};
use crate::position::ResolvedPosArg;
use crate::schema::SchemaHandle;
use crate::slice;

/// A node of the host's DOM.
#[derive(Clone)]
pub struct JsNode(Arc<JsRef>);

impl JsNode {
    fn new(env: sys::napi_env, value: sys::napi_value) -> Result<JsNode> {
        Ok(JsNode(Arc::new(JsRef::new(env, value)?)))
    }

    fn value(&self) -> Result<sys::napi_value> {
        self.0.value()
    }
}

/// The host's DOM, with the document that makes its new nodes, if there is one.
pub struct JsDom {
    env: sys::napi_env,
    document: Option<sys::napi_value>,
}

impl JsDom {
    fn host<T>(&self, result: Result<T>) -> tarnish::Result<T> {
        result.map_err(|error| js::host_error(self.env, error))
    }

    fn get(&self, node: &JsNode, key: &str) -> Result<sys::napi_value> {
        js::get(self.env, node.value()?, key)
    }

    fn node_or_none(&self, value: sys::napi_value) -> Result<Option<JsNode>> {
        match js::type_of(self.env, value)? {
            sys::ValueType::napi_object => Ok(Some(JsNode::new(self.env, value)?)),
            _ => Ok(None),
        }
    }

    fn call_method(
        &self,
        object: sys::napi_value,
        name: &str,
        args: &[sys::napi_value],
    ) -> Result<sys::napi_value> {
        let method = js::get(self.env, object, name)?;
        js::call(self.env, object, method, args)
    }

    fn document(&self) -> Result<sys::napi_value> {
        self.document
            .ok_or_else(|| napi::Error::from_reason("No document to create DOM nodes with"))
    }

    fn style(&self, node: &JsNode) -> Result<Option<sys::napi_value>> {
        let style = self.get(node, "style")?;
        match js::type_of(self.env, style)? {
            sys::ValueType::napi_object => Ok(Some(style)),
            _ => Ok(None),
        }
    }
}

impl Dom for JsDom {
    type Node = JsNode;

    fn kind(&self, node: &JsNode) -> tarnish::Result<NodeKind> {
        self.host((|| {
            let kind = self.get(node, "nodeType")?;
            Ok(match js::type_of(self.env, kind)? {
                sys::ValueType::napi_number => {
                    match unsafe { u32::from_napi_value(self.env, kind) }? {
                        1 => NodeKind::Element,
                        3 => NodeKind::Text,
                        _ => NodeKind::Other,
                    }
                }
                _ => NodeKind::Other,
            })
        })())
    }

    fn node_name(&self, node: &JsNode) -> tarnish::Result<String> {
        self.host((|| unsafe {
            String::from_napi_value(self.env, self.get(node, "nodeName")?)
        })())
    }

    fn text(&self, node: &JsNode) -> tarnish::Result<Text> {
        self.host((|| {
            js::text_from_js(self.env, self.get(node, "nodeValue")?)
        })())
    }

    fn namespace(&self, node: &JsNode) -> tarnish::Result<Option<String>> {
        self.host((|| js::get_string(self.env, node.value()?, "namespaceURI"))())
    }

    fn parent(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        self.host((|| self.node_or_none(self.get(node, "parentNode")?))())
    }

    fn first_child(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        self.host((|| self.node_or_none(self.get(node, "firstChild")?))())
    }

    fn next_sibling(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        self.host((|| self.node_or_none(self.get(node, "nextSibling")?))())
    }

    fn previous_sibling(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        self.host((|| self.node_or_none(self.get(node, "previousSibling")?))())
    }

    fn child(&self, node: &JsNode, index: usize) -> tarnish::Result<Option<JsNode>> {
        self.host((|| {
            let children = self.get(node, "childNodes")?;
            let mut child = ptr::null_mut();
            js::check(unsafe {
                sys::napi_get_element(self.env, children, index as u32, &mut child)
            })?;
            self.node_or_none(child)
        })())
    }

    fn matches(&self, node: &JsNode, selector: &str) -> tarnish::Result<bool> {
        self.host((|| {
            let selector = js::string(self.env, selector)?;
            let result = self.call_method(node.value()?, "matches", &[selector])?;
            js::truthy(self.env, result)
        })())
    }

    fn query_selector(&self, node: &JsNode, selector: &str) -> tarnish::Result<Option<JsNode>> {
        self.host((|| {
            let selector = js::string(self.env, selector)?;
            let found = self.call_method(node.value()?, "querySelector", &[selector])?;
            self.node_or_none(found)
        })())
    }

    fn style_count(&self, node: &JsNode) -> tarnish::Result<usize> {
        self.host((|| match self.style(node)? {
            Some(style) => {
                let length = js::get(self.env, style, "length")?;
                Ok(unsafe { u32::from_napi_value(self.env, length) }? as usize)
            }
            None => Ok(0),
        })())
    }

    fn style_value(&self, node: &JsNode, property: &str) -> tarnish::Result<String> {
        self.host((|| match self.style(node)? {
            Some(style) => {
                let property = js::string(self.env, property)?;
                let value = self.call_method(style, "getPropertyValue", &[property])?;
                unsafe { String::from_napi_value(self.env, value) }
            }
            None => Ok(String::new()),
        })())
    }

    fn same(&self, a: &JsNode, b: &JsNode) -> tarnish::Result<bool> {
        self.host((|| {
            let mut same = false;
            js::check(unsafe {
                sys::napi_strict_equals(self.env, a.value()?, b.value()?, &mut same)
            })?;
            Ok(same)
        })())
    }

    fn contains(&self, ancestor: &JsNode, node: &JsNode) -> tarnish::Result<bool> {
        self.host((|| {
            let result = self.call_method(ancestor.value()?, "contains", &[node.value()?])?;
            js::truthy(self.env, result)
        })())
    }

    fn compare_document_position(&self, a: &JsNode, b: &JsNode) -> tarnish::Result<u16> {
        self.host((|| {
            let result = self.call_method(a.value()?, "compareDocumentPosition", &[b.value()?])?;
            Ok(unsafe { u32::from_napi_value(self.env, result) }? as u16)
        })())
    }

    fn append_child(&self, parent: &JsNode, child: &JsNode) -> tarnish::Result<()> {
        self.host((|| {
            self.call_method(parent.value()?, "appendChild", &[child.value()?])
                .map(|_| ())
        })())
    }

    fn create_element(&self, namespace: Option<&str>, name: &str) -> tarnish::Result<JsNode> {
        self.host((|| {
            let document = self.document()?;
            let name = js::string(self.env, name)?;
            let element = match namespace {
                Some(namespace) => {
                    let namespace = js::string(self.env, namespace)?;
                    self.call_method(document, "createElementNS", &[namespace, name])?
                }
                None => self.call_method(document, "createElement", &[name])?,
            };
            JsNode::new(self.env, element)
        })())
    }

    fn create_text(&self, text: &Text) -> tarnish::Result<JsNode> {
        self.host((|| {
            let text = js::text_to_js(self.env, text)?;
            let node = self.call_method(self.document()?, "createTextNode", &[text])?;
            JsNode::new(self.env, node)
        })())
    }

    fn create_fragment(&self) -> tarnish::Result<JsNode> {
        self.host((|| {
            let fragment = self.call_method(self.document()?, "createDocumentFragment", &[])?;
            JsNode::new(self.env, fragment)
        })())
    }

    fn set_attribute(
        &self,
        element: &JsNode,
        namespace: Option<&str>,
        name: &str,
        value: &Value,
    ) -> tarnish::Result<()> {
        self.host((|| {
            let (name, value) = (
                js::string(self.env, name)?,
                js::value_to_js(self.env, value)?,
            );
            match namespace {
                Some(namespace) => {
                    let namespace = js::string(self.env, namespace)?;
                    self.call_method(
                        element.value()?,
                        "setAttributeNS",
                        &[namespace, name, value],
                    )?
                }
                None => self.call_method(element.value()?, "setAttribute", &[name, value])?,
            };
            Ok(())
        })())
    }

    fn set_style(&self, element: &JsNode, css: &Value) -> tarnish::Result<bool> {
        self.host((|| {
            let style = self.get(element, "style")?;
            if !js::truthy(self.env, style)? {
                return Ok(false);
            }
            let (key, css) = (
                js::string(self.env, "cssText")?,
                js::value_to_js(self.env, css)?,
            );
            js::check(unsafe { sys::napi_set_property(self.env, style, key, css) })?;
            Ok(true)
        })())
    }
}

/// A value that may be a DOM node, as one, when it is an object with a `nodeType`.
fn dom_node(env: sys::napi_env, value: sys::napi_value) -> Result<Option<JsNode>> {
    if js::type_of(env, value)? != sys::ValueType::napi_object {
        return Ok(None);
    }
    let kind = js::get(env, value, "nodeType")?;
    match js::type_of(env, kind)? {
        sys::ValueType::napi_undefined | sys::ValueType::napi_null => Ok(None),
        _ => Ok(Some(JsNode::new(env, value)?)),
    }
}

fn truthy_string(env: sys::napi_env, object: sys::napi_value, key: &str) -> Result<Option<String>> {
    Ok(js::get_string(env, object, key)?.filter(|value| !value.is_empty()))
}

fn preserve_whitespace(
    env: sys::napi_env,
    value: sys::napi_value,
) -> Result<Option<PreserveWhitespace>> {
    Ok(match js::type_of(env, value)? {
        sys::ValueType::napi_undefined | sys::ValueType::napi_null => None,
        sys::ValueType::napi_string
            if unsafe { String::from_napi_value(env, value) }? == "full" =>
        {
            Some(PreserveWhitespace::Full)
        }
        _ if js::truthy(env, value)? => Some(PreserveWhitespace::Yes),
        _ => Some(PreserveWhitespace::No),
    })
}

/// What `getAttrs` gave: `false` to not match, otherwise the attributes, falsy for the
/// defaults.
fn attrs_result(env: sys::napi_env, result: sys::napi_value) -> Result<Option<Option<Attrs>>> {
    if js::is_false(env, result)? {
        return Ok(None);
    }
    if !js::truthy(env, result)? {
        return Ok(Some(None));
    }
    let value = js::value_from_js(env, result)?;
    Ok(Some(Some(Arc::new(
        tarnish::js::attrs(value.as_ref())
            .cloned()
            .unwrap_or_default(),
    ))))
}

fn attrs(env: sys::napi_env, rule: sys::napi_value) -> Result<Option<Attrs>> {
    let value = js::value_from_js(env, js::get(env, rule, "attrs")?)?;
    Ok(tarnish::js::attrs(value.as_ref()).map(|attrs| Arc::new(attrs.clone())))
}

/// A parse rule of JavaScript's, `None` when it is neither a tag rule nor a style rule.
fn parse_rule(
    env: sys::napi_env,
    rule: sys::napi_value,
    schema: &SchemaHandle,
) -> Result<Option<ParseRule<JsNode>>> {
    let tag = js::get(env, rule, "tag")?;
    let style = js::get(env, rule, "style")?;
    let is_set = |value| -> Result<bool> {
        Ok(!matches!(
            js::type_of(env, value)?,
            sys::ValueType::napi_undefined | sys::ValueType::napi_null
        ))
    };
    let kind = if is_set(tag)? {
        RuleKind::Tag(tag_rule(
            env,
            rule,
            unsafe { String::from_napi_value(env, js::coerce_to_string(env, tag)?) }?,
            schema,
        )?)
    } else if is_set(style)? {
        RuleKind::Style(unsafe { String::from_napi_value(env, js::coerce_to_string(env, style)?) }?)
    } else {
        return Ok(None);
    };
    rule_with_kind(env, rule, kind).map(Some)
}

fn tag_rule(
    env: sys::napi_env,
    rule: sys::napi_value,
    selector: String,
    schema: &SchemaHandle,
) -> Result<TagRule<JsNode>> {
    let namespace = js::get(env, rule, "namespace")?;
    let content_element = js::get(env, rule, "contentElement")?;
    Ok(TagRule {
        selector,
        namespace: match js::type_of(env, namespace)? {
            sys::ValueType::napi_undefined => None,
            sys::ValueType::napi_null => Some(None),
            _ => Some(Some(unsafe {
                String::from_napi_value(env, js::coerce_to_string(env, namespace)?)
            }?)),
        },
        content_element: match js::type_of(env, content_element)? {
            sys::ValueType::napi_string => Some(ContentElement::Selector(unsafe {
                String::from_napi_value(env, content_element)
            }?)),
            sys::ValueType::napi_function => {
                let hook = Hook::method(env, rule, "contentElement")?.expect("a function");
                Some(ContentElement::Hook(Arc::new(move |node: &JsNode| {
                    let result = hook.call(|_| Ok(vec![node.value()?]))?;
                    hook.read(|env| JsNode::new(env, result))
                })))
            }
            _ if js::truthy(env, content_element)? => {
                Some(ContentElement::Node(JsNode::new(env, content_element)?))
            }
            _ => None,
        },
        get_content: Hook::method(env, rule, "getContent")?.map(|hook| {
            let schema_id = schema.id();
            Arc::new(move |node: &JsNode, _: &tarnish::Schema| {
                let result = hook.call(|env| {
                    let schema =
                        js::call_registered(env, "wrapSchema", &[js::number(env, schema_id)?])?;
                    Ok(vec![node.value()?, schema])
                })?;
                hook.read(|env| {
                    unsafe { FragmentArg::from_napi_value(env, result) }.map(|fragment| fragment.0)
                })
            }) as tarnish::dom::GetContentHook<JsNode>
        }),
        preserve_whitespace: preserve_whitespace(env, js::get(env, rule, "preserveWhitespace")?)?,
    })
}

fn rule_with_kind(
    env: sys::napi_env,
    rule: sys::napi_value,
    kind: RuleKind<JsNode>,
) -> Result<ParseRule<JsNode>> {
    let is_tag = matches!(kind, RuleKind::Tag(_));
    let priority = js::get(env, rule, "priority")?;
    let skip = js::get(env, rule, "skip")?;
    let mut parsed = ParseRule::new(kind);
    parsed.priority = match js::type_of(env, priority)? {
        sys::ValueType::napi_number => Some(unsafe { f64::from_napi_value(env, priority) }?),
        _ => None,
    };
    parsed.consuming = !js::is_false(env, js::get(env, rule, "consuming")?)?;
    parsed.context = truthy_string(env, rule, "context")?;
    parsed.node = truthy_string(env, rule, "node")?;
    parsed.mark = truthy_string(env, rule, "mark")?;
    parsed.ignore = js::truthy(env, js::get(env, rule, "ignore")?)?;
    parsed.close_parent = js::truthy(env, js::get(env, rule, "closeParent")?)?;
    parsed.skip = match dom_node(env, skip)? {
        Some(node) => Skip::Node(node),
        None if js::truthy(env, skip)? => Skip::Yes,
        None => Skip::No,
    };
    parsed.attrs = attrs(env, rule)?;
    parsed.get_attrs = Hook::method(env, rule, "getAttrs")?.map(|hook| {
        if is_tag {
            GetAttrs::Tag(Arc::new(move |node: &JsNode| {
                let result = hook.call(|_| Ok(vec![node.value()?]))?;
                hook.read(|env| attrs_result(env, result))
            }))
        } else {
            GetAttrs::Style(Arc::new(move |value: &str| {
                let result = hook.call(|env| Ok(vec![js::string(env, value)?]))?;
                hook.read(|env| attrs_result(env, result))
            }))
        }
    });
    parsed.clear_mark = Hook::method(env, rule, "clearMark")?.map(|hook| {
        Arc::new(move |mark: &tarnish::Mark| {
            let result = hook.call(|env| Ok(vec![mark::wrap(env, mark)?]))?;
            hook.read(|env| js::truthy(env, result))
        }) as tarnish::dom::ClearMarkHook
    });
    Ok(parsed)
}

fn rules(
    env: sys::napi_env,
    rules: &[Js],
    schema: &SchemaHandle,
) -> Result<Vec<ParseRule<JsNode>>> {
    let mut parsed = Vec::with_capacity(rules.len());
    for rule in rules {
        if let Some(rule) = parse_rule(env, rule.0, schema)? {
            parsed.push(rule);
        }
    }
    Ok(parsed)
}

#[napi]
pub struct DomParserHandle {
    parser: DomParser<JsNode>,
}

#[napi]
impl DomParserHandle {
    #[napi(constructor)]
    pub fn new(env: Env, schema: &SchemaHandle, rule_list: Vec<Js>) -> Result<Self> {
        let parsed = rules(env.raw(), &rule_list, schema)?;
        Ok(DomParserHandle {
            parser: DomParser::new(schema.schema.clone(), parsed).or_throw(&env)?,
        })
    }

    #[napi]
    pub fn parse(&self, env: Env, dom: Js, options: Js) -> Result<Js> {
        let parsed = with_options(env, options, |js_dom, options| {
            let root = JsNode::new(env.raw(), dom.0)?;
            Ok(self.parser.parse(js_dom, &root, options))
        })?;
        node::wrap(env.raw(), &parsed.or_throw(&env)?).map(Js)
    }

    #[napi]
    pub fn parse_slice(&self, env: Env, dom: Js, options: Js) -> Result<Js> {
        let parsed = with_options(env, options, |js_dom, options| {
            let root = JsNode::new(env.raw(), dom.0)?;
            Ok(self.parser.parse_slice(js_dom, &root, options))
        })?;
        slice::wrap(env.raw(), &parsed.or_throw(&env)?).map(Js)
    }
}

/// Run a parse with JavaScript's options, then write the positions it found back into the
/// `findPositions` objects.
fn with_options<T>(
    env: Env,
    options: Js,
    parse: impl FnOnce(&JsDom, ParseOptions<'_, JsNode>) -> Result<tarnish::Result<T>>,
) -> Result<tarnish::Result<T>> {
    let raw = env.raw();
    let options = options.0;
    let js_dom = JsDom {
        env: raw,
        document: None,
    };
    let optional = |key: &str| -> Result<Option<sys::napi_value>> {
        let value = js::get(raw, options, key)?;
        Ok(match js::type_of(raw, value)? {
            sys::ValueType::napi_undefined | sys::ValueType::napi_null => None,
            _ => Some(value),
        })
    };
    let index = |key: &str| -> Result<Option<usize>> {
        optional(key)?
            .map(|value| unsafe { u32::from_napi_value(raw, value) }.map(|index| index as usize))
            .transpose()
    };

    let find_array = optional("findPositions")?;
    let find_objects: Vec<sys::napi_value> = match find_array {
        Some(array) => unsafe { Vec::<Js>::from_napi_value(raw, array) }?
            .into_iter()
            .map(|js| js.0)
            .collect(),
        None => Vec::new(),
    };
    let mut find = Vec::with_capacity(find_objects.len());
    for &object in &find_objects {
        let pos = js::get(raw, object, "pos")?;
        find.push(FindPosition {
            node: JsNode::new(raw, js::get(raw, object, "node")?)?,
            offset: unsafe { u32::from_napi_value(raw, js::get(raw, object, "offset")?) }? as usize,
            pos: match js::type_of(raw, pos)? {
                sys::ValueType::napi_number => {
                    Some(unsafe { u32::from_napi_value(raw, pos) }? as usize)
                }
                _ => None,
            },
        });
    }
    let rule_from_node = optional("ruleFromNode")?
        .map(|function| Hook::function(raw, function))
        .transpose()?;
    let from_node = rule_from_node.as_ref().map(|hook| {
        move |node: &JsNode| -> tarnish::Result<Option<ParseRule<JsNode>>> {
            let rule = hook.call(|_| Ok(vec![node.value()?]))?;
            hook.read(|env| {
                if !js::truthy(env, rule)? {
                    return Ok(None);
                }
                let tag = TagRule {
                    selector: String::new(),
                    namespace: None,
                    content_element: None,
                    get_content: None,
                    preserve_whitespace: preserve_whitespace(
                        env,
                        js::get(env, rule, "preserveWhitespace")?,
                    )?,
                };
                rule_with_kind(env, rule, RuleKind::Tag(tag)).map(Some)
            })
        }
    });
    let parse_options = ParseOptions {
        preserve_whitespace: preserve_whitespace(
            raw,
            js::get(raw, options, "preserveWhitespace")?,
        )?,
        find_positions: find_array.is_some().then_some(&mut find),
        from: index("from")?,
        to: index("to")?,
        top_node: optional("topNode")?
            .map(|value| unsafe { NodeArg::from_napi_value(raw, value) }.map(|node| node.0))
            .transpose()?,
        top_match: optional("topMatch")?
            .map(|value| {
                unsafe { ContentMatchArg::from_napi_value(raw, value) }.map(|found| found.0)
            })
            .transpose()?,
        context: optional("context")?
            .map(|value| unsafe { ResolvedPosArg::from_napi_value(raw, value) }.map(|pos| pos.0))
            .transpose()?,
        rule_from_node: from_node
            .as_ref()
            .map(|f| f as tarnish::dom::RuleFromNode<'_, JsNode>),
        top_open: js::truthy(raw, js::get(raw, options, "topOpen")?)?,
    };
    let result = parse(&js_dom, parse_options)?;
    for (object, found) in find_objects.iter().zip(&find) {
        if let Some(pos) = found.pos {
            let (key, pos) = (js::string(raw, "pos")?, js::number(raw, pos as f64)?);
            js::check(unsafe { sys::napi_set_property(raw, *object, key, pos) })?;
        }
    }
    Ok(result)
}

/// `DOMParser.schemaRules`: the schema's rules in parse order, each as `[ofMark, typeIndex,
/// ruleIndex, named]`.
#[napi]
pub fn schema_rules(
    env: Env,
    schema: &SchemaHandle,
    marks: Vec<Vec<Js>>,
    nodes: Vec<Vec<Js>>,
) -> Result<Vec<(bool, u32, u32, bool)>> {
    let raw = env.raw();
    let named = |types: Vec<Vec<Js>>,
                 names: Vec<String>|
     -> Result<Vec<(String, Vec<ParseRule<JsNode>>)>> {
        types
            .into_iter()
            .zip(names)
            .map(|(type_rules, name)| {
                let mut parsed = Vec::with_capacity(type_rules.len());
                for rule in type_rules {
                    // A rule of neither kind still takes its place in the order, which its
                    // kind doesn't decide.
                    let rule = match parse_rule(raw, rule.0, schema)? {
                        Some(rule) => rule,
                        None => rule_with_kind(raw, rule.0, RuleKind::Style(String::new()))?,
                    };
                    parsed.push(rule);
                }
                Ok((name, parsed))
            })
            .collect()
    };
    let mark_names = schema
        .schema
        .mark_types()
        .map(|mark| mark.name().to_owned())
        .collect();
    let node_names = schema
        .schema
        .node_types()
        .map(|node| node.name().to_owned())
        .collect();
    let ordered = tarnish::dom::schema_rules(named(marks, mark_names)?, named(nodes, node_names)?);
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

/// A DOM spec of JavaScript's.
fn spec(env: sys::napi_env, value: sys::napi_value) -> Result<DomSpec<JsNode>> {
    if let Some(node) = dom_node(env, value)? {
        return Ok(DomSpec::Node(node));
    }
    if js::type_of(env, value)? == sys::ValueType::napi_object {
        let mut is_array = false;
        js::check(unsafe { sys::napi_is_array(env, value, &mut is_array) })?;
        if is_array {
            let items = unsafe { Vec::<Js>::from_napi_value(env, value) }?
                .into_iter()
                .map(|item| spec(env, item.0))
                .collect::<Result<Vec<_>>>()?;
            return Ok(DomSpec::Array {
                items,
                origin: js::array_origin(env, value),
            });
        }
        let dom = js::get(env, value, "dom")?;
        if let Some(dom) = dom_node(env, dom)? {
            let content_dom = js::get(env, value, "contentDOM")?;
            return Ok(DomSpec::Rendered(Rendered {
                dom,
                content_dom: dom_node(env, content_dom)?,
            }));
        }
    }
    Ok(DomSpec::Value(
        js::value_from_js(env, value)?.unwrap_or(Value::Null),
    ))
}

#[napi(object)]
pub struct RenderedJs {
    pub dom: Js,
    #[napi(js_name = "contentDOM")]
    pub content_dom: Option<Js>,
}

fn rendered_js(rendered: Rendered<JsNode>) -> Result<RenderedJs> {
    Ok(RenderedJs {
        dom: Js(rendered.dom.value()?),
        content_dom: rendered
            .content_dom
            .map(|dom| dom.value().map(Js))
            .transpose()?,
    })
}

#[napi]
pub struct DomSerializerHandle {
    serializer: DomSerializer<JsNode>,
}

#[napi]
impl DomSerializerHandle {
    /// A serializer of the `toDOM` functions in `nodes` and `marks`, by type name. A mark type
    /// whose entry isn't a function isn't serialized.
    #[napi(constructor)]
    pub fn new(env: Env, nodes: Js, marks: Js) -> Result<Self> {
        let raw = env.raw();
        let mut node_hooks: HashMap<String, NodeToDom<JsNode>> = HashMap::new();
        for name in js::property_names(raw, nodes.0)? {
            if let Some(hook) = Hook::method(raw, nodes.0, &name)? {
                let to_dom: NodeToDom<JsNode> = Arc::new(move |node| {
                    let result = hook.call(|env| Ok(vec![node::wrap(env, node)?]))?;
                    hook.read(|env| spec(env, result))
                });
                node_hooks.insert(name, to_dom);
            }
        }
        let mut mark_hooks: HashMap<String, MarkToDom<JsNode>> = HashMap::new();
        for name in js::property_names(raw, marks.0)? {
            let function = js::get(raw, marks.0, &name)?;
            if js::type_of(raw, function)? != sys::ValueType::napi_function {
                continue;
            }
            let hook = Hook::function(raw, function)?;
            let to_dom: MarkToDom<JsNode> = Arc::new(move |mark, inline| {
                let result = hook.call(|env| {
                    let mut inline_value = ptr::null_mut();
                    js::check(unsafe { sys::napi_get_boolean(env, inline, &mut inline_value) })?;
                    Ok(vec![mark::wrap(env, mark)?, inline_value])
                })?;
                hook.read(|env| spec(env, result))
            });
            mark_hooks.insert(name, to_dom);
        }
        Ok(DomSerializerHandle {
            serializer: DomSerializer::new(node_hooks, mark_hooks),
        })
    }

    #[napi]
    pub fn serialize_fragment(
        &self,
        env: Env,
        fragment: FragmentArg,
        document: Js,
        target: Option<Js>,
    ) -> Result<Js> {
        let dom = JsDom {
            env: env.raw(),
            document: Some(document.0),
        };
        let target = target
            .map(|target| JsNode::new(env.raw(), target.0))
            .transpose()?;
        let result = self
            .serializer
            .serialize_fragment(&dom, &fragment.0, target)
            .or_throw(&env)?;
        result.value().map(Js)
    }

    #[napi]
    pub fn serialize_node(&self, env: Env, node: NodeArg, document: Js) -> Result<Js> {
        let dom = JsDom {
            env: env.raw(),
            document: Some(document.0),
        };
        let result = self
            .serializer
            .serialize_node(&dom, &node.0)
            .or_throw(&env)?;
        result.value().map(Js)
    }

    #[napi]
    pub fn serialize_mark(
        &self,
        env: Env,
        mark: MarkArg,
        inline: bool,
        document: Js,
    ) -> Result<Option<RenderedJs>> {
        let dom = JsDom {
            env: env.raw(),
            document: Some(document.0),
        };
        let rendered = self
            .serializer
            .serialize_mark(&dom, &mark.0, inline)
            .or_throw(&env)?;
        rendered.map(rendered_js).transpose()
    }
}

#[napi]
pub fn render_spec(
    env: Env,
    document: Js,
    structure: Js,
    xml_ns: Option<String>,
) -> Result<RenderedJs> {
    let dom = JsDom {
        env: env.raw(),
        document: Some(document.0),
    };
    let structure = spec(env.raw(), structure.0)?;
    let rendered = tarnish::dom::render_spec(&dom, &structure, xml_ns.as_deref()).or_throw(&env)?;
    rendered_js(rendered)
}
