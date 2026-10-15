//! `@tiptap/extension-unique-id`: an id attribute for the types it names, and
//! `generateUniqueIds`, which gives the nodes of those types that have none one each, as the
//! extension's `appendTransaction` does in an editor.

use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use tarnish::Node;
use tarnish::transform::TRANSFORM_ERROR;
use tarnish_js::json::Value;
use tarnish_js::{Error, Result, stack};

use crate::schema::schema_of;
use crate::{Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, Kind, PlainExtension};

/// A `generateID` option, given the node and where it is.
pub type GenerateId =
    Arc<dyn Fn(&Node<'static>, usize) -> std::result::Result<Value, Error> + Send + Sync>;

/// The types the id is for: those named, or `'all'`, every node type but `doc` and `text`.
#[derive(Clone, Copy, Debug)]
pub enum UniqueIdTypes {
    Named(&'static [&'static str]),
    All,
}

/// The options of UniqueID, but `filterTransaction` and `updateDocument`, which only an editor
/// reads.
#[derive(Clone)]
pub struct UniqueIdOptions {
    pub attribute_name: &'static str,
    pub types: UniqueIdTypes,
    pub generate_id: GenerateId,
}

impl Default for UniqueIdOptions {
    fn default() -> Self {
        UniqueIdOptions {
            attribute_name: "id",
            types: UniqueIdTypes::Named(&[]),
            generate_id: Arc::new(|_, _| Ok(Value::String(uuid_v4().into()))),
        }
    }
}

/// A version 4 UUID, as `uuid`'s `v4()` makes one, of random bits from the keys std seeds its
/// hashers with from the system's randomness.
fn uuid_v4() -> String {
    let state = RandomState::new();
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&state.hash_one(0_u8).to_le_bytes());
    bytes[8..].copy_from_slice(&state.hash_one(1_u8).to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    [
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..],
    ]
    .join("-")
}

pub fn unique_id(options: UniqueIdOptions) -> PlainExtension {
    let name = options.attribute_name;
    let attribute = ExtensionAttribute::new(name, Value::Null)
        .html_attribute_when_truthy(format!("data-{name}"));
    PlainExtension::create("uniqueID")
        .priority(10000)
        .add_global_attributes(move |extensions| {
            vec![GlobalAttributes {
                types: GlobalTypes::Named(resolve_types(options.types, extensions)),
                attributes: vec![attribute.clone()],
            }]
        })
}

/// `resolveTypes(types, extensions)`: the types named, or every node type but `doc` and `text`.
fn resolve_types(types: UniqueIdTypes, extensions: &[&Extension]) -> Vec<&'static str> {
    match types {
        UniqueIdTypes::Named(names) => names.to_vec(),
        UniqueIdTypes::All => extensions
            .iter()
            .filter(|extension| matches!(extension.kind, Kind::Node(_)))
            .map(|extension| extension.name)
            .filter(|name| !matches!(*name, "doc" | "text"))
            .collect(),
    }
}

/// `generateUniqueIds(doc, [...extensions, UniqueID.configure(options)])`: the document, read
/// with the schema the extensions make with UniqueID's in place of any of theirs, with an id for
/// each node of the types whose id is falsy. The ids go into the document's JSON rather than
/// through Tiptap's `setNodeAttribute`, whose step per node takes time quadratic in the depth,
/// and fail where it fails: on a node whose content or whose parent's is invalid.
pub fn generate_unique_ids(
    doc: &Value,
    extensions: &[Extension],
    options: &UniqueIdOptions,
) -> Result<Value> {
    let unique_id: Extension = unique_id(options.clone()).into();
    let others = extensions
        .iter()
        .filter(|extension| extension.name != "uniqueID");
    let schema = schema_of(others.chain([&unique_id]).collect())?;
    let types: Vec<&str> = match options.types {
        UniqueIdTypes::Named(names) => names.to_vec(),
        UniqueIdTypes::All => schema
            .schema
            .node_types()
            .map(|node_type| node_type.name())
            .filter(|name| !matches!(*name, "doc" | "text"))
            .collect(),
    };
    let content = Node::from_json(&schema.schema, doc)?;
    let mut json = content.to_json();
    set_ids(&content, &mut json, 0, &types, options)?;
    Ok(json)
}

/// Gives each descendant of `parent` that needs an id one, in the order `descendants` visits
/// them, in `json`, the parent's JSON, `start` being where the parent's content starts.
fn set_ids(
    parent: &Node<'static>,
    json: &mut Value,
    start: usize,
    types: &[&str],
    options: &UniqueIdOptions,
) -> Result<()> {
    let Some(Value::Array(content)) = json.get_mut("content") else {
        return Ok(());
    };
    let name = options.attribute_name;
    for ((offset, node), json) in parent.content().children_with_offsets().zip(content) {
        let pos = start + offset;
        let id = node.attrs_view().get(name);
        if !id.is_some_and(|id| id.truthy()) && types.contains(&node.node_type().name()) {
            let id = (options.generate_id)(&node, pos)?;
            let own = (!node.is_leaf()).then_some(&node);
            for checked in own.into_iter().chain([parent]) {
                if !checked.node_type().valid_content(checked.content()) {
                    let name = checked.node_type().name();
                    let message = format!("Invalid content for node {name}");
                    return Err(Error::Of(&TRANSFORM_ERROR, message));
                }
            }
            if let Some(Value::Object(attrs)) = json.get_mut("attrs")
                && let Some(attr) = attrs.get_mut(name)
            {
                *attr = id;
            }
        }
        stack::grow(|| set_ids(&node, json, pos + 1, types, options))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    #[test]
    fn makes_version_4_uuids_that_differ() {
        let uuids: HashSet<String> = (0..10_000).map(|_| super::uuid_v4()).collect();
        assert_eq!(uuids.len(), 10_000);
        for uuid in uuids {
            let parts: Vec<&str> = uuid.split('-').collect();
            let lengths: Vec<usize> = parts.iter().map(|part| part.len()).collect();
            assert_eq!(lengths, [8, 4, 4, 4, 12], "{uuid}");
            assert!(
                uuid.bytes()
                    .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
            );
            assert!(parts[2].starts_with('4'), "{uuid}");
            assert!(parts[3].starts_with(['8', '9', 'a', 'b']), "{uuid}");
        }
    }
}
