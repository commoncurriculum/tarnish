//! Rendering specs into a DOM held in memory.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use super::{DomSerializer, DomSpec, NodeToDom, Rendered, render_spec};
use crate::dom::{Dom, NodeKind};
use crate::error::Result;
use crate::json::{Value, json};
use crate::model::Node;
use crate::text::Text;

/// A DOM of elements and texts, with each node its index. It answers only what rendering asks.
#[derive(Default)]
struct Tree {
    nodes: RefCell<Vec<TreeNode>>,
}

struct TreeNode {
    name: String,
    text: Option<String>,
    attrs: Vec<(String, String)>,
    children: Vec<usize>,
}

impl Tree {
    fn add(&self, name: &str, text: Option<String>) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(TreeNode {
            name: name.into(),
            text,
            attrs: Vec::new(),
            children: Vec::new(),
        });
        nodes.len() - 1
    }

    /// The node as HTML, its attributes in the order they were set.
    fn html(&self, node: usize) -> String {
        let nodes = self.nodes.borrow();
        let node = &nodes[node];
        if let Some(text) = &node.text {
            return text.clone();
        }
        let attrs: String = node
            .attrs
            .iter()
            .map(|(name, value)| format!(" {name}=\"{value}\""))
            .collect();
        let children: String = node
            .children
            .iter()
            .map(|&child| self.html(child))
            .collect();
        format!("<{0}{attrs}>{children}</{0}>", node.name)
    }
}

impl Dom for Tree {
    type Node = usize;

    fn kind(&self, node: &usize) -> Result<NodeKind> {
        Ok(match self.nodes.borrow()[*node].text {
            Some(_) => NodeKind::Text,
            None => NodeKind::Element,
        })
    }

    fn append_child(&self, parent: &usize, child: &usize) -> Result<()> {
        self.nodes.borrow_mut()[*parent].children.push(*child);
        Ok(())
    }

    fn create_element(&self, _: Option<&str>, name: &str) -> Result<usize> {
        Ok(self.add(name, None))
    }

    fn create_text(&self, text: &Text) -> Result<usize> {
        Ok(self.add("#text", Some(text.to_string())))
    }

    fn create_fragment(&self) -> Result<usize> {
        Ok(self.add("#fragment", None))
    }

    fn set_attribute(
        &self,
        element: &usize,
        _: Option<&str>,
        name: &str,
        value: &Value,
    ) -> Result<()> {
        let value = match value {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        };
        self.nodes.borrow_mut()[*element]
            .attrs
            .push((name.into(), value));
        Ok(())
    }

    fn set_style(&self, _: &usize, _: &Value) -> Result<bool> {
        Ok(false)
    }

    fn stringify(&self, node: &usize) -> Result<String> {
        Ok(format!("[node {node}]"))
    }

    fn node_name(&self, _: &usize) -> Result<String> {
        unreachable!("rendering reads no names")
    }
    fn text(&self, _: &usize) -> Result<Text> {
        unreachable!("rendering reads no text")
    }
    fn namespace(&self, _: &usize) -> Result<Option<String>> {
        unreachable!("rendering reads no namespaces")
    }
    fn parent(&self, _: &usize) -> Result<Option<usize>> {
        unreachable!("rendering walks no tree")
    }
    fn first_child(&self, _: &usize) -> Result<Option<usize>> {
        unreachable!("rendering walks no tree")
    }
    fn next_sibling(&self, _: &usize) -> Result<Option<usize>> {
        unreachable!("rendering walks no tree")
    }
    fn previous_sibling(&self, _: &usize) -> Result<Option<usize>> {
        unreachable!("rendering walks no tree")
    }
    fn child(&self, _: &usize, _: usize) -> Result<Option<usize>> {
        unreachable!("rendering walks no tree")
    }
    fn matches(&self, _: &usize, _: &str) -> Result<bool> {
        unreachable!("rendering matches no selectors")
    }
    fn query_selector(&self, _: &usize, _: &str) -> Result<Option<usize>> {
        unreachable!("rendering matches no selectors")
    }
    fn style_count(&self, _: &usize) -> Result<usize> {
        unreachable!("rendering reads no styles")
    }
    fn style_value(&self, _: &usize, _: &str) -> Result<String> {
        unreachable!("rendering reads no styles")
    }
    fn same(&self, _: &usize, _: &usize) -> Result<bool> {
        unreachable!("rendering compares no nodes")
    }
    fn contains(&self, _: &usize, _: &usize) -> Result<bool> {
        unreachable!("rendering compares no nodes")
    }
    fn compare_document_position(&self, _: &usize, _: &usize) -> Result<u16> {
        unreachable!("rendering compares no nodes")
    }
}

fn value(value: Value) -> DomSpec<usize> {
    DomSpec::Value(value)
}

/// `["div", {dom: span}]` sets a `dom` attribute, as JavaScript takes any object that is
/// neither an array nor a DOM node in that place for the attributes.
#[test]
fn takes_a_rendered_object_after_the_name_for_attributes() {
    let tree = Tree::default();
    let (span, em) = (tree.add("span", None), tree.add("em", None));
    let spec = DomSpec::Array {
        items: vec![
            value("div".into()),
            DomSpec::Rendered(Rendered {
                dom: span,
                content_dom: Some(em),
            }),
            value("text".into()),
        ],
        origin: None,
    };
    let rendered = render_spec(&tree, &spec, None).expect("a rendered spec");
    assert_eq!(
        tree.html(rendered.dom),
        "<div dom=\"[node 0]\" contentDOM=\"[node 1]\">text</div>"
    );

    let child = DomSpec::Array {
        items: vec![value("p".into()), value(json!({"class": "a"}))],
        origin: None,
    };
    let spec = DomSpec::Array {
        items: vec![
            value("div".into()),
            child,
            DomSpec::Rendered(Rendered {
                dom: span,
                content_dom: None,
            }),
        ],
        origin: None,
    };
    let rendered = render_spec(&tree, &spec, None).expect("a rendered spec");
    assert_eq!(
        tree.html(rendered.dom),
        "<div><p class=\"a\"></p><span></span></div>"
    );
}

fn schema() -> crate::model::Schema {
    let spec = json!({
        "nodes": {
            "doc": {"content": "block+"},
            "block": {"group": "block", "attrs": {"spec": {"default": null}}},
            "text": {}
        }
    });
    crate::api::schema(&spec).expect("the schema")
}

/// A `toDOM` that gives back its node's attribute array is refused when the array comes with
/// its origin, and rendered when it comes as a value, whose origin can't be known.
#[test]
fn refuses_an_attribute_array_only_with_its_origin() {
    let schema = schema();
    let node = Node::from_json(
        &schema,
        &json!({"type": "block", "attrs": {"spec": ["script", "alert(1)"]}}),
    )
    .expect("the node");
    let serializer = |with_origin: bool| {
        let to_dom: NodeToDom<usize> = Arc::new(move |node: &Node| {
            let Some(Value::Array(items)) = node.attrs().get("spec") else {
                unreachable!("an array attribute")
            };
            Ok(match with_origin {
                true => DomSpec::Array {
                    items: items.iter().cloned().map(DomSpec::Value).collect(),
                    origin: Some(items.as_ptr() as usize),
                },
                false => DomSpec::Value(Value::Array(items.clone())),
            })
        });
        DomSerializer::new(
            HashMap::from([("block".to_owned(), to_dom)]),
            HashMap::new(),
        )
    };
    let tree = Tree::default();
    let refused = serializer(true).serialize_node(&tree, &node);
    assert!(refused.is_err_and(|error| error.to_string().contains("cross site scripting")));
    let rendered = serializer(false)
        .serialize_node(&tree, &node)
        .expect("a rendered node");
    assert_eq!(tree.html(rendered), "<script>alert(1)</script>");
}
