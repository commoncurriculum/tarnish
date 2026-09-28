// Sends every import of the ProseMirror packages under test to one target, including the
// imports inside the test helpers, so that the tests and the helpers share one implementation.
const PACKAGES = new Set(["prosemirror-model", "prosemirror-transform"])
const root = new URL("../", import.meta.url)
const target = process.env.TARNISH_TARGET

export async function resolve(specifier, context, nextResolve) {
  if (!PACKAGES.has(specifier)) return nextResolve(specifier, context)
  if (target === "rust") {
    return { url: new URL(`crates/tarnish-node/js/${specifier}.mjs`, root).href, shortCircuit: true }
  }
  return nextResolve(specifier, { ...context, parentURL: root.href })
}
