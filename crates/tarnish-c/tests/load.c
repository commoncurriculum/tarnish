/*
 * Loads the library with dlopen, after the program has started, and applies a step through it.
 * On musl a library loaded after start can't have initial-exec thread-locals, which a library
 * linked at start, as fixtures.c links it, can.
 */
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

#include "tarnish.h"

#define LOAD(name) \
    __typeof__(&name) name##_loaded; \
    *(void **)&name##_loaded = dlsym(library, #name); \
    if (!name##_loaded) { \
        fprintf(stderr, "%s\n", dlerror()); \
        return 1; \
    }

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: load <library>\n");
        return 2;
    }
    void *library = dlopen(argv[1], RTLD_NOW);
    if (!library) {
        fprintf(stderr, "%s\n", dlerror());
        return 1;
    }
    LOAD(tarnish_schema_new)
    LOAD(tarnish_node_from_json)
    LOAD(tarnish_apply_steps)
    LOAD(tarnish_node_to_json)

    char *error = NULL;
    TarnishSchema_t *schema = tarnish_schema_new_loaded(
        "{\"nodes\": {\"doc\": {\"content\": \"text*\"}, \"text\": {}}}", &error);
    TarnishNode_t *doc = schema ? tarnish_node_from_json_loaded(
        schema, "{\"type\": \"doc\", \"content\": [{\"type\": \"text\", \"text\": \"Hi\"}]}", &error)
                                : NULL;
    TarnishNode_t *changed = doc ? tarnish_apply_steps_loaded(
        doc,
        "[{\"stepType\": \"replace\", \"from\": 2, \"to\": 2, "
        "\"slice\": {\"content\": [{\"type\": \"text\", \"text\": \"!\"}]}}]",
        &error)
                                 : NULL;
    if (!changed) {
        fprintf(stderr, "%s\n", error);
        return 1;
    }
    char *json = tarnish_node_to_json_loaded(changed);
    const char *expected = "{\"type\":\"doc\",\"content\":[{\"type\":\"text\",\"text\":\"Hi!\"}]}";
    if (strcmp(json, expected) != 0) {
        fprintf(stderr, "Expected %s, got %s\n", expected, json);
        return 1;
    }
    printf("%s\n", json);
    return 0;
}
