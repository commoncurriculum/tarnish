//! `generateText` and `getText`: a document's text, with each node type's `renderText` for the
//! nodes of the type, as `getTextSerializersFromSchema` reads them from the schema.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use tarnish::Node;
use tarnish_js::json::Value;
use tarnish_js::{Error, Result};

use crate::{Extension, get_schema};

/// What a node's `renderText`, or a text serializer, is given: the node, where it is, its parent
/// and its index there, and the range whose text is read.
pub struct TextSerializerProps<'a> {
    pub node: &'a Node<'static>,
    pub pos: usize,
    pub parent: &'a Node<'static>,
    pub index: usize,
    pub range: Range<usize>,
}

/// A `TextSerializer`, or a node's `renderText`: the node's text.
pub type TextSerializer =
    Arc<dyn Fn(&TextSerializerProps) -> std::result::Result<String, Error> + Send + Sync>;

/// `getText`'s options.
#[derive(Clone)]
pub struct TextOptions {
    /// `blockSeparator`: what goes before each block that doesn't start the text.
    pub block_separator: String,
    /// `textSerializers`: the text of the nodes of each type they name, whose content isn't read.
    pub text_serializers: HashMap<String, TextSerializer>,
}

impl Default for TextOptions {
    fn default() -> Self {
        TextOptions {
            block_separator: "\n\n".to_owned(),
            text_serializers: HashMap::new(),
        }
    }
}

/// `generateText(doc, extensions, options)`: the text of the document, read with the schema the
/// extensions make, whose node types' `renderText` the options' serializers override.
pub fn generate_text(
    doc: &Value,
    extensions: &[Extension],
    options: &TextOptions,
) -> Result<String> {
    let schema = get_schema(extensions)?;
    let node = Node::from_json(&schema.schema, doc)?;
    let mut text_serializers = schema.text_serializers.clone();
    text_serializers.extend(options.text_serializers.clone());
    get_text(
        &node,
        &TextOptions {
            block_separator: options.block_separator.clone(),
            text_serializers,
        },
    )
}

/// `getText(node, options)`: the text of the node's content. A text node's text with a lone
/// surrogate has U+FFFD in its place.
pub fn get_text(node: &Node<'static>, options: &TextOptions) -> Result<String> {
    let range = 0..node.content().size();
    let mut text = String::new();
    node.nodes_between(
        range.start,
        range.end,
        &mut |child, pos, parent, index| {
            if child.is_block() && pos > range.start {
                text += &options.block_separator;
            }
            if let Some(serializer) = options.text_serializers.get(child.node_type().name()) {
                if let Some(parent) = parent {
                    text += &serializer(&TextSerializerProps {
                        node: child,
                        pos,
                        parent,
                        index,
                        range: range.clone(),
                    })?;
                }
                return Ok(false);
            }
            if let Some(own) = child.text() {
                text += &own.to_string_lossy();
            }
            Ok(true)
        },
        0,
    )?;
    Ok(text)
}
