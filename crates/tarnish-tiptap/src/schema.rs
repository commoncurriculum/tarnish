//! `getSchema` and `sortExtensions`, and the `DOMParser` and `DOMSerializer` of the schema they
//! build.

use std::collections::HashMap;
use std::sync::Arc;

use tarnish::dom::{DomParser, DomSerializer, MarkToDom, NodeToDom, ParseRule};
use tarnish::model::{AttributeDefault, AttributeSpec, MarkSpec, NodeSpec, Schema, SchemaSpec};
use tarnish_html::HtmlNode;
use tarnish_js::Result;

use super::attributes::{ExtensionAttribute, get_rendered_attributes};
use super::extension::{Extension, Kind};
use super::parse_html::parse_rules;

/// The schema `getSchema` builds from extensions, with the `DOMParser` and `DOMSerializer`
/// `fromSchema` make of its specs' `parseDOM` and `toDOM`.
pub struct TiptapSchema {
    pub schema: Schema,
    pub parser: DomParser<HtmlNode>,
    pub serializer: DomSerializer<HtmlNode>,
}

/// `getSchema(extensions)`: the extensions sorted by priority, their node and mark types in that
/// order, and each type's attributes those `addGlobalAttributes` gives it, then its own.
pub fn get_schema(extensions: &[Extension]) -> Result<TiptapSchema> {
    let extensions = sort_extensions(extensions);
    let globals: Vec<(&'static str, &ExtensionAttribute)> = extensions
        .iter()
        .flat_map(|extension| &extension.global_attributes)
        .flat_map(|global| {
            global.types.iter().flat_map(|name| {
                global
                    .attributes
                    .iter()
                    .map(move |attribute| (*name, attribute))
            })
        })
        .collect();
    let mut spec = SchemaSpec::default();
    let (mut node_rules, mut mark_rules) = (Vec::new(), Vec::new());
    let (mut to_doms, mut mark_to_doms) = (HashMap::new(), HashMap::new());
    for extension in extensions {
        let attributes: Vec<ExtensionAttribute> = globals
            .iter()
            .filter(|(name, _)| *name == extension.name)
            .map(|(_, attribute)| *attribute)
            .chain(&extension.attributes)
            .cloned()
            .collect();
        let attrs = attribute_specs(&attributes);
        let attributes = Arc::new(attributes);
        let rules: Vec<ParseRule<HtmlNode>> =
            parse_rules(extension.name, &extension.parse_html, &attributes);
        match &extension.kind {
            Kind::Node(node) => {
                spec.nodes.push((
                    extension.name.to_owned(),
                    NodeSpec {
                        content: non_empty(&node.content),
                        group: non_empty(node.group),
                        inline: node.inline,
                        marks: node.marks.map(str::to_owned),
                        linebreak_replacement: node.linebreak_replacement,
                        code: node.code,
                        attrs,
                        ..NodeSpec::default()
                    },
                ));
                node_rules.push(rules);
                if let Some(render) = node.render_html.clone() {
                    let attributes = Arc::clone(&attributes);
                    let to_dom: NodeToDom<HtmlNode> = Arc::new(move |node| {
                        render(
                            node,
                            get_rendered_attributes(node.attrs_view(), &attributes)?,
                        )
                    });
                    to_doms.insert(extension.name.to_owned(), to_dom);
                }
            }
            Kind::Mark(mark) => {
                spec.marks.push((
                    extension.name.to_owned(),
                    MarkSpec {
                        attrs,
                        code: mark.code,
                        ..MarkSpec::default()
                    },
                ));
                mark_rules.push(rules);
                if let Some(render) = mark.render_html.clone() {
                    let attributes = Arc::clone(&attributes);
                    let to_dom: MarkToDom<HtmlNode> = Arc::new(move |mark, _inline| {
                        render(
                            mark,
                            get_rendered_attributes(mark.attrs_view(), &attributes)?,
                        )
                    });
                    mark_to_doms.insert(extension.name.to_owned(), to_dom);
                }
            }
            Kind::Extension => {}
        }
    }
    let schema = Schema::new(spec)?;
    let parser = DomParser::from_schema(schema.clone(), mark_rules, node_rules)?;
    Ok(TiptapSchema {
        schema,
        parser,
        serializer: DomSerializer::new(to_doms, mark_to_doms),
    })
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

/// `sortExtensions`: highest priority first, and equal priorities in the order given.
pub fn sort_extensions(extensions: &[Extension]) -> Vec<&Extension> {
    let mut sorted: Vec<&Extension> = extensions.iter().collect();
    sorted.sort_by_key(|extension| std::cmp::Reverse(extension.priority));
    sorted
}

/// `Object.fromEntries` of the attributes' defaults: a name given twice, as a global attribute
/// and the extension's own, keeps its first place and takes its last default.
fn attribute_specs(attributes: &[ExtensionAttribute]) -> Vec<(String, AttributeSpec)> {
    let mut specs: Vec<(String, AttributeSpec)> = Vec::with_capacity(attributes.len());
    for attribute in attributes {
        let spec = AttributeSpec {
            default: attribute
                .default
                .clone()
                .map_or(AttributeDefault::Required, AttributeDefault::Value),
            ..AttributeSpec::default()
        };
        match specs.iter_mut().find(|(name, _)| name == attribute.name) {
            Some((_, existing)) => *existing = spec,
            None => specs.push((attribute.name.to_owned(), spec)),
        }
    }
    specs
}
