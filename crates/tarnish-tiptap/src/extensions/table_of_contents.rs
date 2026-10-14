//! `@tiptap/extension-table-of-contents`, as far as the document holds it: the `id` and
//! `data-toc-id` of each anchor, a heading by default. The plugin that gives anchors their ids
//! as the document changes runs in an editor.

use crate::{ExtensionAttribute, GlobalAttributes, GlobalTypes, PlainExtension};
use tarnish_js::json::Value;

pub struct TableOfContentsOptions {
    /// `anchorTypes`: the node types that get ids.
    pub anchor_types: &'static [&'static str],
}

impl Default for TableOfContentsOptions {
    fn default() -> Self {
        TableOfContentsOptions {
            anchor_types: &["heading"],
        }
    }
}

pub fn table_of_contents(options: TableOfContentsOptions) -> PlainExtension {
    let attribute = |name: &'static str| {
        ExtensionAttribute::new(name, Value::Null)
            .parse_html(move |element| {
                element
                    .attribute(name)
                    .filter(|id| !id.is_empty())
                    .map(Value::String)
            })
            .render_html(move |attributes, rendered| {
                rendered.merge(name, attributes.get(name).expect("its attribute"))
            })
    };
    PlainExtension::create("tableOfContents").add_global_attributes(vec![GlobalAttributes {
        types: GlobalTypes::Named(options.anchor_types),
        attributes: vec![attribute("id"), attribute("data-toc-id")],
    }])
}
