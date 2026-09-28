/*
 * tarnish: ProseMirror's document model and transforms, as a C library.
 *
 * Schema specs, steps and documents are ProseMirror's JSON, as UTF-8 strings, and JSON the
 * library returns is what JSON.stringify writes for the same value, byte for byte. A schema is
 * built once from its spec and a node read once from its JSON; calls take them as handles.
 *
 * A function that can fail takes `char **error`. On failure it returns NULL or false and, when
 * `error` isn't NULL, sets `*error` to "Class: message", the class naming the error ProseMirror
 * throws (RangeError, SyntaxError, ReplaceError, TransformError, TypeError or Error). Free
 * every string the library returns, errors included, with tarnish_free, a schema with
 * tarnish_schema_free and a node with tarnish_node_free. Documents may nest as deeply as memory
 * allows.
 *
 * `cargo run -p tarnish-c --example header --features headers` writes this file.
 */

#ifndef TARNISH_H
#define TARNISH_H
#ifdef __cplusplus
extern "C" {
#endif

/** \brief
 *  A document, or any node, read once from its JSON.
 */
typedef struct TarnishNode TarnishNode_t;

/** \brief
 *  The document with the steps, a JSON array, applied in order. Free it with
 *  `tarnish_node_free`.
 */
TarnishNode_t *
tarnish_apply_steps (
    TarnishNode_t const * node,
    char const * steps_json,
    char * * error);


#include <stdbool.h>

/** \brief
 *  Whether the node and its descendants conform to the schema.
 */
bool
tarnish_check (
    TarnishNode_t const * node,
    char * * error);

/** \brief
 *  Frees a string the library returned.
 */
void
tarnish_free (
    char * string);

/** \brief
 *  The steps, as a JSON array, that undo the steps applied to the document, last first.
 */
char *
tarnish_invert_steps (
    TarnishNode_t const * node,
    char const * steps_json,
    char * * error);

/** \brief
 *  A schema, built once from its spec.
 */
typedef struct TarnishSchema TarnishSchema_t;


#include <stddef.h>
#include <stdint.h>

/** \brief
 *  Maps a position through the changes the steps make, into `mapped`. With `assoc` below zero,
 *  a position where content is inserted stays before it; otherwise it moves after it.
 */
bool
tarnish_map_position (
    TarnishSchema_t const * schema,
    char const * steps_json,
    size_t pos,
    int32_t assoc,
    size_t * mapped,
    char * * error);

/** \brief
 *  Frees a node.
 */
void
tarnish_node_free (
    TarnishNode_t * node);

/** \brief
 *  A node read from its JSON, as `Node.fromJSON` reads it. Free it with `tarnish_node_free`.
 */
TarnishNode_t *
tarnish_node_from_json (
    TarnishSchema_t const * schema,
    char const * json,
    char * * error);

/** \brief
 *  The node's JSON, which is what `JSON.stringify` writes for it, byte for byte.
 */
char *
tarnish_node_to_json (
    TarnishNode_t const * node);

/** \brief
 *  Frees a schema. A node read with it keeps what it needs of it.
 */
void
tarnish_schema_free (
    TarnishSchema_t * schema);

/** \brief
 *  A schema from its spec: an object of "nodes" and "marks", each an object of the types' specs
 *  in order or an array of `[name, spec]` pairs, and "topNode". Free it with
 *  `tarnish_schema_free`.
 */
TarnishSchema_t *
tarnish_schema_new (
    char const * spec_json,
    char * * error);

/** \brief
 *  The text between `from` and `to`, as `textBetween` gives it: `block_separator` goes between
 *  blocks, and `leaf_text` stands for each leaf node that isn't text; either may be NULL. A lone
 *  surrogate, where a position splits a pair, is U+FFFD, and a NUL is written `\u0000`. Free it
 *  with `tarnish_free`.
 */
char *
tarnish_text_between (
    TarnishNode_t const * node,
    size_t from,
    size_t to,
    char const * block_separator,
    char const * leaf_text,
    char * * error);

/** \brief
 *  All the text in the node, as `textContent` gives it, with a lone surrogate as U+FFFD and a
 *  NUL written `\u0000`. Free it with `tarnish_free`.
 */
char *
tarnish_text_content (
    TarnishNode_t const * node,
    char * * error);

/** \brief
 *  Makes changes on the server: applies the ops, a JSON array, in order to one `Transform` of
 *  the document, and gives the changed document. Free it with `tarnish_node_free`. An op is an
 *  object naming a `Transform` method in "op", with the method's arguments by the names
 *  ProseMirror gives them, as the README describes. When `steps_json` isn't NULL, a call that
 *  succeeds sets it to the JSON array of the steps the transform made, for editors to apply;
 *  free it with `tarnish_free`.
 */
TarnishNode_t *
tarnish_transform (
    TarnishNode_t const * node,
    char const * ops_json,
    char * * steps_json,
    char * * error);


#ifdef __cplusplus
} /* extern \"C\" */
#endif

#endif /* TARNISH_H */
