/*
 * tarnish: ProseMirror's document model and transforms, as a C library.
 *
 * Documents, steps and schema specs are ProseMirror's JSON, as UTF-8 strings. JSON a function
 * returns is what JavaScript's JSON.stringify writes for the same value, byte for byte.
 *
 * A function that can fail takes `char **error`. On failure it returns NULL or false and, when
 * `error` isn't NULL, sets `*error` to "Class: message", the class naming the error
 * ProseMirror throws (RangeError, SyntaxError, ReplaceError, TransformError or Error). Free
 * every string the library returns, errors included, with tarnish_free.
 *
 * Each call runs on a stack of its own, so a caller's stack size doesn't matter: a document
 * nested too deeply for even that fails with "RangeError: Maximum call stack size exceeded".
 */
#ifndef TARNISH_H
#define TARNISH_H

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TarnishSchema TarnishSchema;

/*
 * A schema from its spec: an object of "nodes" and "marks", each an object of the types'
 * specs in order or an array of [name, spec] pairs, and "topNode". Free it with
 * tarnish_schema_free.
 */
TarnishSchema *tarnish_schema_new(const char *spec_json, char **error);

void tarnish_schema_free(TarnishSchema *schema);

/* Whether the document conforms to the schema. */
bool tarnish_check(const TarnishSchema *schema, const char *doc_json, char **error);

/* The document with the steps, a JSON array, applied in order. */
char *tarnish_apply_steps(const TarnishSchema *schema, const char *doc_json, const char *steps_json,
                          char **error);

/* The steps, as a JSON array, that undo the steps applied to the document, last first. */
char *tarnish_invert_steps(const TarnishSchema *schema, const char *doc_json, const char *steps_json,
                           char **error);

/*
 * Map a position through the changes the steps make, into `*mapped`. With `assoc` below zero,
 * a position where content is inserted stays before it; otherwise it moves after it.
 */
bool tarnish_map_position(const TarnishSchema *schema, const char *steps_json, size_t pos, int assoc,
                          size_t *mapped, char **error);

void tarnish_free(char *string);

#ifdef __cplusplus
}
#endif

#endif
