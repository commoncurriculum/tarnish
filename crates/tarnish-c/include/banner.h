/*
 * tarnish: ProseMirror's document model and transforms, as a C library.
 *
 * Schema specs, steps and documents are ProseMirror's JSON, as UTF-8 strings, and JSON the
 * library returns is what JSON.stringify writes for the same value, byte for byte. A schema is
 * built once from its spec and a node read once from its JSON; calls take them as handles.
 *
 * A function that can fail takes `char **error`. On failure it returns NULL or false and, when
 * `error` isn't NULL, sets `*error` to "Class: message", the class naming the error ProseMirror
 * throws (RangeError, SyntaxError, ReplaceError, TransformError or Error). Free every string
 * the library returns, errors included, with tarnish_free, a schema with tarnish_schema_free and
 * a node with tarnish_node_free. Documents may nest as deeply as memory allows.
 *
 * `cargo run -p tarnish-c --example header --features headers` writes this file.
 */
