//! tarnish as a C library: schemas and documents as handles, and ProseMirror's JSON in and out
//! as UTF-8 strings. `include/tarnish.h`, which the `header` example writes, declares it.

#![forbid(unsafe_code)]

use std::ffi::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};

use safer_ffi::prelude::*;
use tarnish::js::json::stringify;
use tarnish::{Error, Node, Result, Schema, Value, api};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// A schema, built once from its spec.
#[derive_ReprC]
#[repr(opaque)]
pub struct TarnishSchema(Schema);

/// A document, or any node, read once from its JSON.
#[derive_ReprC]
#[repr(opaque)]
pub struct TarnishNode(Node<'static>);

/// Where a function that can fail puts its error, as `include/banner.h` describes.
type ErrorOut<'a> = Option<Out<'a, Option<char_p::Box>>>;

/// `JSON.parse` of the string.
fn parse(json: Option<char_p::Ref<'_>>) -> Result<Value> {
    let json = json.ok_or_else(|| Error::Other("A JSON string was NULL".into()))?;
    let text = std::str::from_utf8(json.to_bytes())
        .map_err(|_| Error::Other("A JSON string wasn't UTF-8".into()))?;
    tarnish::json::from_str(text).map_err(|_| Error::Syntax("Invalid JSON".into()))
}

/// The text as a C string. JSON never holds a raw NUL, but an error's message or a document's
/// text may.
fn string(text: String) -> char_p::Box {
    char_p::Box::try_from(text.replace('\0', "\\u0000")).expect("text without NUL")
}

/// A string that may be NULL, which must be UTF-8.
fn text<'a>(text: Option<char_p::Ref<'a>>, what: &str) -> Result<Option<&'a str>> {
    let utf8 = |text: char_p::Ref<'a>| std::str::from_utf8(text.to_bytes());
    let invalid = |_| Error::Other(format!("The {what} wasn't UTF-8"));
    text.map(|text| utf8(text).map_err(invalid)).transpose()
}

fn given<'a, T>(pointer: Option<&'a T>, what: &str) -> Result<&'a T> {
    pointer.ok_or_else(|| Error::Other(format!("The {what} was NULL")))
}

/// What `f` gives, or `None`, having put the error, or a panic's, in `error`.
fn run<T>(error: ErrorOut<'_>, f: impl FnOnce() -> Result<T>) -> Option<T> {
    let outcome = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(Error::Other(
            "tarnish panicked; this is a bug in tarnish".into(),
        ))
    });
    match outcome {
        Ok(result) => Some(result),
        Err(failure) => {
            if let Some(error) = error {
                error.write(Some(string(failure.to_string())));
            }
            None
        }
    }
}

/// A schema from its spec: an object of "nodes" and "marks", each an object of the types' specs
/// in order or an array of `[name, spec]` pairs, and "topNode". Free it with
/// `tarnish_schema_free`.
#[ffi_export]
fn tarnish_schema_new(
    spec_json: Option<char_p::Ref<'_>>,
    error: ErrorOut<'_>,
) -> Option<repr_c::Box<TarnishSchema>> {
    run(error, || {
        let schema = api::schema(&parse(spec_json)?)?;
        Ok(Box::new(TarnishSchema(schema)).into())
    })
}

/// Frees a schema. A node read with it keeps what it needs of it.
#[ffi_export]
fn tarnish_schema_free(schema: Option<repr_c::Box<TarnishSchema>>) {
    drop(schema);
}

/// A node read from its JSON, as `Node.fromJSON` reads it. Free it with `tarnish_node_free`.
#[ffi_export]
fn tarnish_node_from_json(
    schema: Option<&TarnishSchema>,
    json: Option<char_p::Ref<'_>>,
    error: ErrorOut<'_>,
) -> Option<repr_c::Box<TarnishNode>> {
    run(error, || {
        let node = Node::from_json(&given(schema, "schema")?.0, &parse(json)?)?;
        Ok(Box::new(TarnishNode(node)).into())
    })
}

/// The node's JSON, which is what `JSON.stringify` writes for it, byte for byte.
#[ffi_export]
fn tarnish_node_to_json(node: &TarnishNode) -> char_p::Box {
    string(node.0.to_json_string())
}

/// Frees a node.
#[ffi_export]
fn tarnish_node_free(node: Option<repr_c::Box<TarnishNode>>) {
    drop(node);
}

/// Whether the node and its descendants conform to the schema.
#[ffi_export]
fn tarnish_check(node: Option<&TarnishNode>, error: ErrorOut<'_>) -> bool {
    run(error, || given(node, "node")?.0.check()).is_some()
}

/// The document with the steps, a JSON array, applied in order. Free it with
/// `tarnish_node_free`.
#[ffi_export]
fn tarnish_apply_steps(
    node: Option<&TarnishNode>,
    steps_json: Option<char_p::Ref<'_>>,
    error: ErrorOut<'_>,
) -> Option<repr_c::Box<TarnishNode>> {
    run(error, || {
        let applied = api::apply_steps(&given(node, "node")?.0, &parse(steps_json)?)?;
        Ok(Box::new(TarnishNode(applied)).into())
    })
}

/// The steps, as a JSON array, that undo the steps applied to the document, last first.
#[ffi_export]
fn tarnish_invert_steps(
    node: Option<&TarnishNode>,
    steps_json: Option<char_p::Ref<'_>>,
    error: ErrorOut<'_>,
) -> Option<char_p::Box> {
    run(error, || {
        let inverted = api::invert_steps(&given(node, "node")?.0, &parse(steps_json)?)?;
        Ok(string(stringify(&inverted)))
    })
}

/// Maps a position through the changes the steps make, into `mapped`. With `assoc` below zero,
/// a position where content is inserted stays before it; otherwise it moves after it.
#[ffi_export]
fn tarnish_map_position(
    schema: Option<&TarnishSchema>,
    steps_json: Option<char_p::Ref<'_>>,
    pos: usize,
    assoc: c_int,
    mapped: Option<Out<'_, usize>>,
    error: ErrorOut<'_>,
) -> bool {
    run(error, || {
        let mapped = mapped.ok_or_else(|| Error::Other("The place to map into was NULL".into()))?;
        let pos = api::map_position(&given(schema, "schema")?.0, &parse(steps_json)?, pos, assoc)?;
        mapped.write(pos);
        Ok(())
    })
    .is_some()
}

/// Makes changes on the server: applies the ops, a JSON array, in order to one `Transform` of
/// the document, and gives the changed document. Free it with `tarnish_node_free`. An op is an
/// object naming a `Transform` method in "op", with the method's arguments by the names
/// ProseMirror gives them, as the README describes. When `steps_json` isn't NULL, a call that
/// succeeds sets it to the JSON array of the steps the transform made, for editors to apply;
/// free it with `tarnish_free`.
#[ffi_export]
fn tarnish_transform(
    node: Option<&TarnishNode>,
    ops_json: Option<char_p::Ref<'_>>,
    steps_json: Option<Out<'_, Option<char_p::Box>>>,
    error: ErrorOut<'_>,
) -> Option<repr_c::Box<TarnishNode>> {
    run(error, || {
        let (doc, steps) = api::transform(&given(node, "node")?.0, &parse(ops_json)?)?;
        if let Some(steps_json) = steps_json {
            steps_json.write(Some(string(stringify(&steps))));
        }
        Ok(Box::new(TarnishNode(doc)).into())
    })
}

/// The text between `from` and `to`, as `textBetween` gives it: `block_separator` goes between
/// blocks, and `leaf_text` stands for each leaf node that isn't text; either may be NULL. A lone
/// surrogate, where a position splits a pair, is U+FFFD, and a NUL is written `\u0000`. Free it
/// with `tarnish_free`.
#[ffi_export]
fn tarnish_text_between(
    node: Option<&TarnishNode>,
    from: usize,
    to: usize,
    block_separator: Option<char_p::Ref<'_>>,
    leaf_text: Option<char_p::Ref<'_>>,
    error: ErrorOut<'_>,
) -> Option<char_p::Box> {
    run(error, || {
        let node = &given(node, "node")?.0;
        let block_separator = text(block_separator, "block separator")?;
        let leaf_text = text(leaf_text, "leaf text")?;
        let between = api::text_between(node, from, to, block_separator, leaf_text)?;
        Ok(string(between.to_string_lossy().into_owned()))
    })
}

/// All the text in the node, as `textContent` gives it, with a lone surrogate as U+FFFD and a
/// NUL written `\u0000`. Free it with `tarnish_free`.
#[ffi_export]
fn tarnish_text_content(node: Option<&TarnishNode>, error: ErrorOut<'_>) -> Option<char_p::Box> {
    run(error, || {
        let content = api::text_content(&given(node, "node")?.0)?;
        Ok(string(content.to_string_lossy().into_owned()))
    })
}

/// Frees a string the library returned.
#[ffi_export]
fn tarnish_free(string: Option<char_p::Box>) {
    drop(string);
}

/// Writes `include/tarnish.h`.
#[cfg(feature = "headers")]
pub fn write_header(path: &str) -> std::io::Result<()> {
    safer_ffi::headers::builder()
        .with_guard("TARNISH_H")
        .with_banner(include_str!("../include/banner.h"))
        .to_file(path)?
        .generate()
}
