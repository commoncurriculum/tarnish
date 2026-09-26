// Sends every import of the ProseMirror packages under test to one target, including the
// imports inside the test helpers, so that the tests and the helpers share one implementation.
import { pathToFileURL } from "node:url"

const PACKAGES = new Set(["prosemirror-model", "prosemirror-transform"])
// The packages tarnish implements. The others resolve to the real package, which then uses
// tarnish's for its own imports of these.
const PORTED = new Set(["prosemirror-model"])
const root = new URL("../", import.meta.url)
const target = process.env.TARNISH_TARGET

export async function resolve(specifier, context, nextResolve) {
  if (!PACKAGES.has(specifier)) return nextResolve(specifier, context)
  if (target === "rust" && PORTED.has(specifier)) {
    return { url: new URL(`crates/tarnish-node/js/${specifier}.mjs`, root).href, shortCircuit: true }
  }
  return nextResolve(specifier, { ...context, parentURL: pathToFileURL(root.pathname).href + "/" })
}
