//! Attributes as `addAttributes` and `addGlobalAttributes` declare them, and
//! `getRenderedAttributes`.

use std::borrow::Cow;
use std::sync::Arc;

use tarnish::chunk::ValueRef;
use tarnish_html::HtmlNode;
use tarnish_js::Error;
use tarnish_js::json::Value;

use super::merge_attributes::merge_entry;
use crate::{AttrValue, SpecAttrs};

/// An attribute's `renderHTML`, given the node's or mark's attributes: it hands each entry of the
/// object it returns to [`Rendered::merge`], in the object's order.
pub type RenderAttribute =
    Arc<dyn for<'a> Fn(ValueRef<'a>, &mut Rendered<'a>) -> Result<(), Error> + Send + Sync>;
/// An attribute's `parseHTML`, given the element: `None` for `null` or `undefined`.
pub type ParseAttribute = Arc<dyn Fn(&HtmlNode) -> Option<Value> + Send + Sync>;

/// The HTML attributes `getRenderedAttributes` has merged so far.
pub struct Rendered<'a>(SpecAttrs<'a>);

impl<'a> Rendered<'a> {
    /// Merges an entry of an attribute's rendering as `mergeAttributes` does. An attribute's
    /// rendering is an object, whose keys are distinct.
    pub fn merge(
        &mut self,
        key: impl Into<Cow<'a, str>>,
        value: impl Into<AttrValue<'a>>,
    ) -> Result<(), Error> {
        merge_entry(&mut self.0, key.into(), value.into())
    }
}

#[derive(Clone)]
pub struct ExtensionAttribute {
    pub name: &'static str,
    /// `None` for a required attribute.
    pub default: Option<Value>,
    /// `default: undefined`, which leaves the attribute out of a node or mark not given it.
    pub undefined: bool,
    pub rendered: bool,
    pub render_html: Option<RenderAttribute>,
    pub parse_html: Option<ParseAttribute>,
}

impl ExtensionAttribute {
    /// `{ default }`, or a required attribute for `None`.
    pub fn new(name: &'static str, default: impl Into<Option<Value>>) -> Self {
        ExtensionAttribute {
            name,
            default: default.into(),
            undefined: false,
            rendered: true,
            render_html: None,
            parse_html: None,
        }
    }

    /// `{ default: undefined }`.
    pub fn undefined(name: &'static str) -> Self {
        ExtensionAttribute {
            undefined: true,
            ..ExtensionAttribute::new(name, None)
        }
    }

    pub fn default(mut self, default: impl Into<Option<Value>>) -> Self {
        self.default = default.into();
        self.undefined = false;
        self
    }

    pub fn not_rendered(mut self) -> Self {
        self.rendered = false;
        self
    }

    pub fn render_html(
        mut self,
        render: impl for<'a> Fn(ValueRef<'a>, &mut Rendered<'a>) -> Result<(), Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.render_html = Some(Arc::new(render));
        self
    }

    pub fn parse_html(
        mut self,
        parse: impl Fn(&HtmlNode) -> Option<Value> + Send + Sync + 'static,
    ) -> Self {
        self.parse_html = Some(Arc::new(parse));
        self
    }
}

/// One entry of `addGlobalAttributes`: attributes for the types it names.
pub struct GlobalAttributes {
    pub types: GlobalTypes,
    pub attributes: Vec<ExtensionAttribute>,
}

/// The `types` of an entry of `addGlobalAttributes`.
#[derive(Clone, Copy, Debug)]
pub enum GlobalTypes {
    /// An array of type names.
    Named(&'static [&'static str]),
    /// `'*'`: every node type but `text`, and every mark type.
    All,
    /// `'nodes'`: every node type but `text`.
    Nodes,
    /// `'marks'`.
    Marks,
    /// An array of the node types but those named, as an extension makes one of the node
    /// extensions `addGlobalAttributes` is given.
    NodesBut(&'static [&'static str]),
}

impl GlobalTypes {
    /// The names of the types, of the extensions' node types and mark types, in their order.
    pub(crate) fn resolve<'a>(&self, nodes: &[&'a str], marks: &[&'a str]) -> Vec<&'a str> {
        let nodes_but = |excluded: &[&str]| -> Vec<&'a str> {
            let kept = nodes.iter().filter(|name| !excluded.contains(name));
            kept.copied().collect()
        };
        match *self {
            GlobalTypes::Named(names) => names.to_vec(),
            GlobalTypes::All => [nodes_but(&["text"]), marks.to_vec()].concat(),
            GlobalTypes::Nodes => nodes_but(&["text"]),
            GlobalTypes::Marks => marks.to_vec(),
            GlobalTypes::NodesBut(excluded) => nodes_but(excluded),
        }
    }
}

/// `getRenderedAttributes(nodeOrMark, extensionAttributes)`, given the attributes of a node or
/// mark, which hold every one of its type's but those left `undefined`.
pub fn get_rendered_attributes<'a>(
    attrs: ValueRef<'a>,
    attributes: &[ExtensionAttribute],
) -> Result<SpecAttrs<'a>, Error> {
    let mut rendered = Rendered(SpecAttrs::new());
    for attribute in attributes.iter().filter(|attribute| attribute.rendered) {
        match &attribute.render_html {
            Some(render) => render(attrs, &mut rendered)?,
            // An attribute left `undefined` gives the element nothing.
            None => match attrs.get(attribute.name) {
                Some(value) => rendered.merge(attribute.name, value)?,
                None => debug_assert!(attribute.undefined, "{} is left out", attribute.name),
            },
        }
    }
    Ok(rendered.0)
}
