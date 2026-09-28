// Sends every import of the ProseMirror packages under test to one target, including the
// imports inside the test helpers, so that the tests and the helpers share one implementation,
// and gives the tests that import jsdom linkedom's documents in its place.
const PACKAGES = new Set(["prosemirror-model", "prosemirror-transform"])
const root = new URL("../", import.meta.url)
const target = process.env.TARNISH_TARGET

export async function resolve(specifier, context, nextResolve) {
  if (specifier === "jsdom") return { url: new URL("harness/jsdom.mjs", root).href, shortCircuit: true }
  if (!PACKAGES.has(specifier)) return nextResolve(specifier, context)
  if (target === "rust") {
    return { url: new URL(`crates/tarnish-node/js/${specifier}.mjs`, root).href, shortCircuit: true }
  }
  return nextResolve(specifier, { ...context, parentURL: root.href })
}
