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

/* Compares the library's result, or its error, with the text expected, and frees both. */
static void expect(const char *test, const char *what, const char *expected, char *actual, char *error) {
    if (!actual)
        fail(test, what, expected, error);
    else if (strcmp(actual, expected) != 0)
        fail(test, what, expected, actual);
    tarnish_free(actual);
    tarnish_free(error);
}

/* Checks that a call failed with an error starting with `prefix`, and frees the error. */
static void expect_error(const char *test, bool failed, char *error, const char *prefix) {
    if (!failed || !error || strncmp(error, prefix, strlen(prefix)) != 0)
        fail(test, "the error", prefix, failed ? error : "(no error)");
    tarnish_free(error);
}

static void run_transform(const char *test, const TarnishSchema *schema, const char *start, const char *steps,
                          const char *result, char *mapping) {
    char *error = NULL;
    expect(test, "the document", result, tarnish_apply_steps(schema, start, steps, &error), error);

    error = NULL;
    char *inverted = tarnish_invert_steps(schema, start, steps, &error);
    if (!inverted) {
        fail(test, "the inverted steps", "steps", error);
        tarnish_free(error);
    } else {
        error = NULL;
        expect(test, "the document the inverted steps give", start,
               tarnish_apply_steps(schema, result, inverted, &error), error);
        tarnish_free(inverted);
    }

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

static void run_errors(const TarnishSchema *schema) {
    char *error = NULL;
    TarnishSchema *missing = tarnish_schema_new("{\"nodes\":{\"text\":{}}}", &error);
    expect_error("a schema without its top node", missing == NULL, error,
                 "RangeError: Schema is missing its top node type ('doc')");

    error = NULL;
    TarnishSchema *unparsed =
        tarnish_schema_new("{\"nodes\":{\"doc\":{\"content\":\"paragraph+\"},\"text\":{}}}", &error);
    expect_error("a content expression that doesn't parse", unparsed == NULL, error,
                 "SyntaxError: No node type or group 'paragraph' found (in content expression 'paragraph+')");

    error = NULL;
    bool valid = tarnish_check(schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"text\",\"text\":\"loose\"}]}", &error);
    expect_error("a document that doesn't fit the schema", !valid, error, "RangeError: Invalid content for node doc");

    error = NULL;
    char *applied = tarnish_apply_steps(schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\"}]}",
                                        "[{\"stepType\":\"replace\",\"from\":0,\"to\":1}]", &error);
    expect_error("a step that doesn't apply", applied == NULL, error, "TransformError: ");

    error = NULL;
    valid = tarnish_check(schema, "{\"type\":", &error);
    expect_error("text that isn't JSON", !valid, error, "Error: Invalid JSON");

    error = NULL;
    valid = tarnish_map_position(schema, "[]", 0, 1, NULL, &error);
    expect_error("nowhere to map into", !valid, error, "Error: ");

    if (tarnish_apply_steps(schema, "null", "[]", NULL) != NULL)
        fail("an error with nowhere to report it", "the result", "(NULL)", "a document");
}

int main(int argc, char **argv) {
    if (argc != 2 || !(fixtures = fopen(argv[1], "r"))) {
        fprintf(stderr, "Usage: %s <fixtures written by harness/test-c.mjs>\n", argv[0]);
        return 2;
    }

    size_t schema_count = read_count();
    TarnishSchema **schemas = calloc(schema_count, sizeof *schemas);
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

    for (size_t index = 0; index < schema_count; index++)
        tarnish_schema_free(schemas[index]);
    free(schemas);
    fclose(fixtures);

    if (failures) {
        fprintf(stderr, "%d failed\n", failures);
        return 1;
    }
    printf("%zu transforms and the errors: all passed\n", test_count);
    return 0;
}
