/*
 * Runs every transform prosemirror-transform's own tests make through the C library, from the
 * file harness/test-c.mjs writes: a count and that many schema specs, then a count and, for
 * each transform, its schema's index, the starting document, the steps, the document they
 * give, and the positions mapped through them as "from to" pairs. The JSON is what
 * JSON.stringify writes, so the library's output must equal it byte for byte.
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "tarnish.h"

static FILE *fixtures;
static int failures;

static char *read_line(void) {
    char *line = NULL;
    size_t capacity = 0;
    ssize_t length = getline(&line, &capacity, fixtures);
    if (length < 0) {
        fprintf(stderr, "The fixtures ended early\n");
        exit(2);
    }
    if (length > 0 && line[length - 1] == '\n')
        line[length - 1] = '\0';
    return line;
}

static size_t read_count(void) {
    char *line = read_line();
    size_t count = strtoul(line, NULL, 10);
    free(line);
    return count;
}

static void fail(const char *test, const char *what, const char *expected, const char *actual) {
    failures++;
    fprintf(stderr, "%s: %s\n  expected: %s\n  actual:   %s\n", test, what, expected, actual ? actual : "(NULL)");
}

/* Checks that a call failed with an error starting with `prefix`, and frees the error. */
static void expect_error(const char *test, bool failed, char *error, const char *prefix) {
    if (!failed || !error || strncmp(error, prefix, strlen(prefix)) != 0)
        fail(test, "the error", prefix, failed ? error : "(no error)");
    tarnish_free(error);
}

/* A node read from its JSON, or NULL, having failed the test, when it can't be. */
static TarnishNode_t *read_node(const char *test, const TarnishSchema_t *schema, const char *json) {
    char *error = NULL;
    TarnishNode_t *node = tarnish_node_from_json(schema, json, &error);
    if (!node) {
        fail(test, "the node", json, error);
        tarnish_free(error);
    }
    return node;
}

/* Compares a node's JSON with the text expected. */
static void expect_json(const char *test, const char *what, const char *expected, const TarnishNode_t *node) {
    char *json = tarnish_node_to_json(node);
    if (strcmp(json, expected) != 0)
        fail(test, what, expected, json);
    tarnish_free(json);
}

static void run_transform(const char *test, const TarnishSchema_t *schema, const char *start, const char *steps,
                          const char *result, char *mapping) {
    TarnishNode_t *doc = read_node(test, schema, start);
    if (!doc)
        return;
    char *error = NULL;
    TarnishNode_t *changed = tarnish_apply_steps(doc, steps, &error);
    char *inverted = changed ? tarnish_invert_steps(doc, steps, &error) : NULL;
    if (!changed || !inverted) {
        fail(test, changed ? "the inverted steps" : "the document", "no error", error);
        tarnish_free(error);
    } else {
        expect_json(test, "the document", result, changed);
        TarnishNode_t *undone = tarnish_apply_steps(changed, inverted, &error);
        if (!undone) {
            fail(test, "the document the inverted steps give", start, error);
            tarnish_free(error);
        } else {
            expect_json(test, "the document the inverted steps give", start, undone);
        }
        tarnish_node_free(undone);
    }
    tarnish_free(inverted);
    tarnish_node_free(changed);
    tarnish_node_free(doc);

    for (char *cursor = mapping, *end; *cursor; cursor = end) {
        size_t from = strtoul(cursor, &end, 10);
        size_t to = strtoul(end, &end, 10);
        size_t mapped = 0;
        error = NULL;
        if (!tarnish_map_position(schema, steps, from, 1, &mapped, &error)) {
            fail(test, "the mapped position", "a position", error);
            tarnish_free(error);
        } else if (mapped != to) {
            char expected[32], actual[32];
            snprintf(expected, sizeof expected, "%zu", to);
            snprintf(actual, sizeof actual, "%zu", mapped);
            fail(test, "the mapped position", expected, actual);
        }
    }
}

static void run_errors(const TarnishSchema_t *schema) {
    char *error = NULL;
    TarnishSchema_t *missing = tarnish_schema_new("{\"nodes\":{\"text\":{}}}", &error);
    expect_error("a schema without its top node", missing == NULL, error,
                 "RangeError: Schema is missing its top node type ('doc')");

    error = NULL;
    TarnishSchema_t *unparsed =
        tarnish_schema_new("{\"nodes\":{\"doc\":{\"content\":\"paragraph+\"},\"text\":{}}}", &error);
    expect_error("a content expression that doesn't parse", unparsed == NULL, error,
                 "SyntaxError: No node type or group 'paragraph' found (in content expression 'paragraph+')");

    error = NULL;
    TarnishSchema_t *nul = tarnish_schema_new("{\"nodes\":{\"doc\":{\"content\":\"para\\u0000graph+\"},\"text\":{}}}", &error);
    expect_error("an error whose message holds a NUL", nul == NULL, error,
                 "SyntaxError: No node type or group 'para' found (in content expression 'para\\u0000graph+')");

    error = NULL;
    TarnishNode_t *loose =
        tarnish_node_from_json(schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"text\",\"text\":\"loose\"}]}", &error);
    bool valid = loose && tarnish_check(loose, &error);
    expect_error("a document that doesn't fit the schema", !valid, error, "RangeError: Invalid content for node doc");
    tarnish_node_free(loose);

    error = NULL;
    TarnishNode_t *doc = tarnish_node_from_json(schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\"}]}", &error);
    TarnishNode_t *applied = tarnish_apply_steps(doc, "[{\"stepType\":\"replace\",\"from\":0,\"to\":1}]", &error);
    expect_error("a step that doesn't apply", applied == NULL, error, "TransformError: ");

    error = NULL;
    TarnishNode_t *unread = tarnish_node_from_json(schema, "{\"type\":", &error);
    expect_error("text that isn't JSON", unread == NULL, error, "SyntaxError: Invalid JSON");

    error = NULL;
    valid = tarnish_map_position(schema, "[]", 0, 1, NULL, &error);
    expect_error("nowhere to map into", !valid, error, "Error: ");

    if (tarnish_apply_steps(doc, "null", NULL) != NULL)
        fail("an error with nowhere to report it", "the result", "(NULL)", "a document");
    tarnish_node_free(doc);
}

/* A document whose attribute nests far deeper than a thread's stack could recurse through. */
static void run_deep(void) {
    enum { DEPTH = 200000 };
    char *error = NULL;
    TarnishSchema_t *schema =
        tarnish_schema_new("{\"nodes\":[[\"doc\",{\"attrs\":{\"data\":{\"default\":null}}}],[\"text\",{}]]}", &error);
    if (!schema) {
        fail("a deep attribute", "the schema", "a schema", error);
        tarnish_free(error);
        return;
    }
    const char *head = "{\"type\":\"doc\",\"attrs\":{\"data\":", *tail = "}}";
    size_t length = strlen(head) + 2 * DEPTH + strlen(tail);
    char *json = malloc(length + 1);
    strcpy(json, head);
    memset(json + strlen(head), '[', DEPTH);
    memset(json + strlen(head) + DEPTH, ']', DEPTH);
    strcpy(json + strlen(head) + 2 * DEPTH, tail);
    TarnishNode_t *doc = read_node("a deep attribute", schema, json);
    if (doc)
        expect_json("a deep attribute", "the document", json, doc);
    tarnish_node_free(doc);
    free(json);
    tarnish_schema_free(schema);
}

int main(int argc, char **argv) {
    if (argc != 2 || !(fixtures = fopen(argv[1], "r"))) {
        fprintf(stderr, "Usage: %s <fixtures written by harness/test-c.mjs>\n", argv[0]);
        return 2;
    }

    size_t schema_count = read_count();
    TarnishSchema_t **schemas = calloc(schema_count, sizeof *schemas);
    for (size_t index = 0; index < schema_count; index++) {
        char *spec = read_line(), *error = NULL;
        if (!(schemas[index] = tarnish_schema_new(spec, &error))) {
            fprintf(stderr, "Schema %zu: %s\n", index, error);
            return 1;
        }
        free(spec);
    }

    size_t test_count = read_count();
    for (size_t index = 0; index < test_count; index++) {
        size_t schema = read_count();
        char *start = read_line(), *steps = read_line(), *result = read_line(), *mapping = read_line();
        char test[32];
        snprintf(test, sizeof test, "transform %zu", index);
        run_transform(test, schemas[schema], start, steps, result, mapping);
        free(start);
        free(steps);
        free(result);
        free(mapping);
    }

    run_errors(schemas[0]);
    run_deep();

    for (size_t index = 0; index < schema_count; index++)
        tarnish_schema_free(schemas[index]);
    free(schemas);
    fclose(fixtures);

    if (failures) {
        fprintf(stderr, "%d failed\n", failures);
        return 1;
    }
    printf("%zu transforms, the errors and a deep document: all passed\n", test_count);
    return 0;
}
