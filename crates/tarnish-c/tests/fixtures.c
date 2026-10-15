/*
 * Runs the fixtures harness/test-c.mjs writes through the C library. The JSON is what
 * JSON.stringify writes, so the library's output must equal it byte for byte. The file is
 * sections of a count and that many records, and ends with "end":
 *
 * - schema specs, of fixtures/transform.json then fixtures/model.json;
 * - transforms, prosemirror-transform's own tests' then fixtures/model.json's: the schema's
 *   index, the starting document and the steps, then "ok" with the document they give, the steps
 *   undoing them, and the position each position maps to with assoc -1 and then 1, or "error"
 *   with the error;
 * - schema specs, each with "ok" or "error" and the error;
 * - documents to check: the schema's index, the document, and "ok" or "error" with the error;
 *
 * then fixtures/ops.json:
 *
 * - schema specs;
 * - op lists: the schema's index, the starting document and the ops, then "ok" with the document
 *   and steps they give and the positions mapped through the steps, "-" where the steps aren't
 *   mapped, or "error" with the error;
 * - textBetween: the schema's index, the document, "from to", the block separator and leaf text,
 *   and "ok" with the text or "error" with the error;
 * - textBetween from every position to every one after it: the schema's index, the document, the
 *   block separator and leaf text, the count of positions, and each text, `to` faster;
 * - textContent: the schema's index, the document and the text.
 *
 * Text is the hex of its UTF-8, "-" being text not given.
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "tarnish.h"

static FILE *fixtures;
static int failures;

static _Noreturn void malformed(const char *what) {
    fprintf(stderr, "The fixtures are malformed: %s\n", what);
    exit(2);
}

static char *read_line(void) {
    char *line = NULL;
    size_t capacity = 0;
    ssize_t length = getline(&line, &capacity, fixtures);
    if (length < 0)
        malformed("they ended early");
    if (length > 0 && line[length - 1] == '\n')
        line[length - 1] = '\0';
    return line;
}

static size_t read_number(void) {
    char *line = read_line(), *end;
    size_t number = strtoul(line, &end, 10);
    if (end == line || *end != '\0')
        malformed("a number expected");
    free(line);
    return number;
}

/* A section's count, of which there must be some: a loop over none passes. */
static size_t read_count(const char *section) {
    size_t count = read_number();
    if (count == 0) {
        fprintf(stderr, "No %s in the fixtures\n", section);
        exit(2);
    }
    return count;
}

/* The index of one of `count` schemas. */
static size_t read_schema(size_t count) {
    size_t index = read_number();
    if (index >= count)
        malformed("a schema's index past the schemas");
    return index;
}

/* Whether the record that follows is "ok", or "error". */
static bool read_ok(void) {
    char *line = read_line();
    bool ok = strcmp(line, "ok") == 0;
    if (!ok && strcmp(line, "error") != 0)
        malformed("\"ok\" or \"error\" expected");
    free(line);
    return ok;
}

static void read_end(void) {
    char *line = read_line();
    if (strcmp(line, "end") != 0)
        malformed("\"end\" expected");
    free(line);
    char *more = NULL;
    size_t capacity = 0;
    if (getline(&more, &capacity, fixtures) >= 0)
        malformed("more after \"end\"");
    free(more);
}

static void fail(const char *test, const char *what, const char *expected, const char *actual) {
    failures++;
    fprintf(stderr, "%s: %s\n  expected: %s\n  actual:   %s\n", test, what, expected, actual ? actual : "(NULL)");
}

/* Checks that a call failed with the error `expected`, and frees the error. */
static void expect_error(const char *test, bool failed, char *error, const char *expected) {
    if (!failed || !error || strcmp(error, expected) != 0)
        fail(test, "the error", expected, failed ? error : "(no error)");
    tarnish_free(error);
}

/* Checks that a call gave `expected` when `ok`, and otherwise failed with `expected` as its error.
 * Frees what it gave and the error. */
static void expect_outcome(const char *test, const char *what, bool ok, const char *expected, char *given,
                           char *error) {
    bool same = ok ? given && strcmp(given, expected) == 0 : !given && error && strcmp(error, expected) == 0;
    if (!same)
        fail(test, ok ? what : "the error", expected, given ? given : error);
    tarnish_free(given);
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

static TarnishSchema_t **read_schemas(const char *section, size_t *count) {
    *count = read_count(section);
    TarnishSchema_t **schemas = calloc(*count, sizeof *schemas);
    for (size_t index = 0; index < *count; index++) {
        char *spec = read_line(), *error = NULL;
        if (!(schemas[index] = tarnish_schema_new(spec, &error))) {
            fprintf(stderr, "%s %zu: %s\n", section, index, error);
            exit(1);
        }
        free(spec);
    }
    return schemas;
}

static void free_schemas(TarnishSchema_t **schemas, size_t count) {
    for (size_t index = 0; index < count; index++)
        tarnish_schema_free(schemas[index]);
    free(schemas);
}

/* Checks that each position maps through the steps with `assoc` to the one `expected` has for it,
 * in order: none when it's "-". */
static void expect_mapped(const char *test, const TarnishSchema_t *schema, const char *steps, int assoc,
                          const char *expected) {
    if (strcmp(expected, "-") == 0)
        return;
    size_t pos = 0;
    for (const char *cursor = expected; *cursor; pos++) {
        char *end;
        size_t to = strtoul(cursor, &end, 10);
        if (end == cursor)
            malformed("a position expected");
        cursor = end;
        size_t mapped = 0;
        char *error = NULL, what[80];
        snprintf(what, sizeof what, "position %zu mapped with assoc %d", pos, assoc);
        if (!tarnish_map_position(schema, steps, pos, assoc, &mapped, &error)) {
            fail(test, what, "a position", error);
            tarnish_free(error);
        } else if (mapped != to) {
            char wanted[32], actual[32];
            snprintf(wanted, sizeof wanted, "%zu", to);
            snprintf(actual, sizeof actual, "%zu", mapped);
            fail(test, what, wanted, actual);
        }
    }
    if (pos == 0)
        malformed("no positions");
}

static void read_mapped(const char *test, const TarnishSchema_t *schema, const char *steps) {
    for (int assoc = -1; assoc <= 1; assoc += 2) {
        char *expected = read_line();
        expect_mapped(test, schema, steps, assoc, expected);
        free(expected);
    }
}

static void run_transform(const char *test, const TarnishSchema_t *schema) {
    char *start = read_line(), *steps = read_line();
    bool ok = read_ok();
    char *expected = read_line(), *expected_inverted = ok ? read_line() : NULL;
    TarnishNode_t *doc = read_node(test, schema, start);
    if (doc) {
        char *error = NULL;
        TarnishNode_t *changed = tarnish_apply_steps(doc, steps, &error);
        if (!ok) {
            expect_error(test, changed == NULL, error, expected);
        } else if (changed) {
            expect_json(test, "the document", expected, changed);
            error = NULL;
            char *inverted = tarnish_invert_steps(doc, steps, &error);
            expect_outcome(test, "the inverted steps", true, expected_inverted, inverted, error);
        } else {
            fail(test, "the document", expected, error);
            tarnish_free(error);
        }
        tarnish_node_free(changed);
        tarnish_node_free(doc);
    }
    if (ok)
        read_mapped(test, schema, steps);
    free(start);
    free(steps);
    free(expected);
    free(expected_inverted);
}

static void run_schema(const char *test) {
    char *spec = read_line();
    bool ok = read_ok();
    char *expected = ok ? NULL : read_line(), *error = NULL;
    TarnishSchema_t *schema = tarnish_schema_new(spec, &error);
    if (ok && !schema)
        fail(test, "the schema", "a schema", error);
    if (!ok)
        expect_error(test, schema == NULL, error, expected);
    else
        tarnish_free(error);
    tarnish_schema_free(schema);
    free(spec);
    free(expected);
}

static void run_check(const char *test, const TarnishSchema_t *schema) {
    char *json = read_line();
    bool ok = read_ok();
    char *expected = ok ? NULL : read_line(), *error = NULL;
    TarnishNode_t *node = tarnish_node_from_json(schema, json, &error);
    bool valid = node && tarnish_check(node, &error);
    if (ok && !valid)
        fail(test, "the check", "no error", error);
    if (!ok)
        expect_error(test, !valid, error, expected);
    else
        tarnish_free(error);
    tarnish_node_free(node);
    free(json);
    free(expected);
}

/* What only the library refuses: what isn't JSON, and NULL where it writes. */
static void run_errors(const TarnishSchema_t *schema) {
    char *error = NULL;
    TarnishNode_t *unread = tarnish_node_from_json(schema, "{\"type\":", &error);
    expect_error("text that isn't JSON", unread == NULL, error, "SyntaxError: Invalid JSON");

    error = NULL;
    bool valid = tarnish_map_position(schema, "[]", 0, 1, NULL, &error);
    expect_error("nowhere to map into", !valid, error, "Error: The place to map into was NULL");

    TarnishNode_t *doc = read_node("an error", schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\"}]}");
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

/* A line of hex, as harness/test-c.mjs writes text, decoded in place; NULL for "-". */
static char *read_text(void) {
    char *line = read_line();
    if (strcmp(line, "-") == 0) {
        free(line);
        return NULL;
    }
    size_t length = strlen(line) / 2;
    for (size_t index = 0; index < length; index++) {
        char pair[3] = {line[2 * index], line[2 * index + 1], '\0'};
        line[index] = (char)strtoul(pair, NULL, 16);
    }
    line[length] = '\0';
    return line;
}

static void run_op_list(const char *test, const TarnishSchema_t *schema) {
    char *start = read_line(), *ops = read_line();
    bool ok = read_ok();
    char *expected = read_line(), *expected_steps = ok ? read_line() : NULL;
    TarnishNode_t *doc = read_node(test, schema, start);
    if (doc) {
        char *steps = NULL, *error = NULL;
        TarnishNode_t *changed = tarnish_transform(doc, ops, &steps, &error);
        if (changed)
            expect_json(test, "the document", ok ? expected : "an error", changed);
        expect_outcome(test, "the steps", ok, ok ? expected_steps : expected, steps, error);
        tarnish_node_free(changed);
        tarnish_node_free(doc);
    }
    if (ok)
        read_mapped(test, schema, expected_steps);
    free(start);
    free(ops);
    free(expected);
    free(expected_steps);
}

static void run_text_between(const char *test, const TarnishSchema_t *schema) {
    char *start = read_line(), *range = read_line(), *separator = read_text(), *leaf = read_text();
    bool ok = read_ok();
    char *expected = ok ? read_text() : read_line();
    char *end;
    size_t from = strtoul(range, &end, 10), to = strtoul(end, NULL, 10);
    TarnishNode_t *doc = read_node(test, schema, start);
    if (doc) {
        char *error = NULL;
        char *text = tarnish_text_between(doc, from, to, separator, leaf, &error);
        expect_outcome(test, "the text", ok, expected, text, error);
        tarnish_node_free(doc);
    }
    free(start);
    free(range);
    free(separator);
    free(leaf);
    free(expected);
}

static void run_texts_between(const char *test, const TarnishSchema_t *schema) {
    char *start = read_line(), *separator = read_text(), *leaf = read_text();
    size_t positions = read_count("positions");
    TarnishNode_t *doc = read_node(test, schema, start);
    for (size_t from = 0; from < positions; from++) {
        for (size_t to = from; to < positions; to++) {
            char *expected = read_text(), *error = NULL, what[80];
            snprintf(what, sizeof what, "the text from %zu to %zu", from, to);
            if (doc)
                expect_outcome(test, what, true, expected,
                               tarnish_text_between(doc, from, to, separator, leaf, &error), error);
            free(expected);
        }
    }
    tarnish_node_free(doc);
    free(start);
    free(separator);
    free(leaf);
}

static void run_text_content(const char *test, const TarnishSchema_t *schema) {
    char *start = read_line(), *expected = read_text();
    TarnishNode_t *doc = read_node(test, schema, start);
    if (doc) {
        char *error = NULL;
        char *text = tarnish_text_content(doc, &error);
        expect_outcome(test, "the text", true, expected, text, error);
        tarnish_node_free(doc);
    }
    free(start);
    free(expected);
}

/* What the library itself refuses, or takes, in the calls fixtures/ops.json's records make. */
static void run_op_errors(const TarnishSchema_t *schema) {
    TarnishNode_t *doc = read_node("ops", schema, "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\"}]}");
    char *error = NULL;
    TarnishNode_t *same = tarnish_transform(doc, "[]", NULL, &error);
    if (!same)
        fail("ops with nowhere to put their steps", "the document", "a document", error);
    tarnish_free(error);
    tarnish_node_free(same);

    error = NULL;
    TarnishNode_t *none = tarnish_transform(NULL, "[]", NULL, &error);
    expect_error("ops on no document", none == NULL, error, "Error: The node was NULL");
    tarnish_node_free(none);

    error = NULL;
    char *text = tarnish_text_between(doc, 0, 0, "\xff", NULL, &error);
    expect_error("a block separator that isn't UTF-8", text == NULL, error,
                 "Error: The block separator wasn't UTF-8");
    tarnish_free(text);
    tarnish_node_free(doc);
}

/* Runs a section of records that each start with a schema's index, and gives how many it ran. */
static size_t run_section(const char *section, TarnishSchema_t **schemas, size_t schema_count,
                          void (*run)(const char *, const TarnishSchema_t *)) {
    size_t count = read_count(section);
    for (size_t index = 0; index < count; index++) {
        size_t schema = read_schema(schema_count);
        char test[64];
        snprintf(test, sizeof test, "%s %zu", section, index);
        run(test, schemas[schema]);
    }
    return count;
}

int main(int argc, char **argv) {
    if (argc != 2 || !(fixtures = fopen(argv[1], "r"))) {
        fprintf(stderr, "Usage: %s <fixtures written by harness/test-c.mjs>\n", argv[0]);
        return 2;
    }

    size_t schema_count;
    TarnishSchema_t **schemas = read_schemas("schemas", &schema_count);
    size_t records = run_section("transforms", schemas, schema_count, run_transform);
    size_t specs = read_count("specs");
    for (size_t index = 0; index < specs; index++) {
        char test[32];
        snprintf(test, sizeof test, "spec %zu", index);
        run_schema(test);
    }
    records += specs;
    records += run_section("checks", schemas, schema_count, run_check);

    size_t ops_schema_count;
    TarnishSchema_t **ops_schemas = read_schemas("ops schemas", &ops_schema_count);
    records += run_section("op lists", ops_schemas, ops_schema_count, run_op_list);
    records += run_section("textBetween", ops_schemas, ops_schema_count, run_text_between);
    records += run_section("textBetween ranges", ops_schemas, ops_schema_count, run_texts_between);
    records += run_section("textContent", ops_schemas, ops_schema_count, run_text_content);
    read_end();

    run_errors(schemas[0]);
    run_op_errors(ops_schemas[0]);
    run_deep();

    free_schemas(schemas, schema_count);
    free_schemas(ops_schemas, ops_schema_count);
    fclose(fixtures);

    if (failures) {
        fprintf(stderr, "%d failed\n", failures);
        return 1;
    }
    printf("%zu records, the library's own errors and a deep document: all passed\n", records);
    return 0;
}
