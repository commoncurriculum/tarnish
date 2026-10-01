// Records the installed version of each package a tarnish crate ports or is held to, to
// fixtures/versions.json: for the crates' tests to check the versions they name, and for an
// application to check that the packages its JavaScript runs are these (v4's
// backend/bridge/tools/versions.ts does).
//
// Each key names a package as a dependent resolves it: "linkedom > parse5" is the parse5 linkedom
// loads. The linkedom fork keeps upstream's version, so it is recorded as the SHA-256 of its
// files. "CSS engine" is what the fork's element.style runs: tarnish-css, as its engine() names it.
import { createHash } from "node:crypto"
import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { parseHTML } from "linkedom"
import { writeFixture } from "./fixture.mjs"

const root = fileURLToPath(new URL("../", import.meta.url))

// The directory of the package `name` as Node resolves it from `from`.
function resolved(name, from) {
  for (let dir = from; ; dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", name)
    if (existsSync(join(candidate, "package.json"))) return realpathSync(candidate)
    if (dirname(dir) === dir) throw new Error(`${name} isn't installed for ${from}`)
  }
}

// The SHA-256 of a package's files, each its path and its bytes, in the order of their paths.
function filesDigest(dir) {
  const hash = createHash("sha256")
  const walk = relative => {
    const entries = readdirSync(join(dir, relative), { withFileTypes: true })
    for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
      if (entry.name === "node_modules") continue
      const path = relative ? `${relative}/${entry.name}` : entry.name
      if (entry.isDirectory()) {
        walk(path)
      } else {
        hash.update(`${path}\0`)
        hash.update(readFileSync(join(dir, path)))
        hash.update("\0")
      }
    }
  }
  walk("")
  return `sha256:${hash.digest("hex")}`
}

function version(key) {
  const dir = key.split(" > ").reduce((from, name) => resolved(name, from), root)
  if (key === "linkedom") return filesDigest(dir)
  return JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version
}

const tiptap = [
  "core",
  "extension-bold",
  "extension-document",
  "extension-hard-break",
  "extension-heading",
  "extension-highlight",
  "extension-image",
  "extension-italic",
  "extension-paragraph",
  "extension-strike",
  "extension-subscript",
  "extension-superscript",
  "extension-text",
  "extension-text-style",
  "extension-underline",
  "markdown",
  "pm",
].map(name => `@tiptap/${name}`)

const packages = [
  "@tiptap/pm > prosemirror-model",
  "@tiptap/pm > prosemirror-transform",
  ...tiptap,
  "tiptap-extension-flat-list",
  "marked",
  "@tiptap/markdown > marked",
  "marked-more-lists",
  "zod",
  "linkedom",
  "linkedom > parse5",
  "linkedom > parse5 > entities",
  "linkedom > css-select",
  "linkedom > css-select > css-what",
]

// The recorders load ProseMirror as the harness resolves it, which must be the copy Tiptap's
// @tiptap/pm loads.
for (const name of ["prosemirror-model", "prosemirror-transform"]) {
  if (version(name) !== version(`@tiptap/pm > ${name}`)) {
    throw new Error(`The harness loads another ${name} than @tiptap/pm does`)
  }
}

// linkedom loads its CSS engine once element.style is first read.
if (parseHTML('<p style="color: red">').document.querySelector("p").style.color !== "red") {
  throw new Error("linkedom's element.style doesn't read styles")
}
const { engine } = await import(new URL("shared/css/engine.js", import.meta.resolve("linkedom")))

writeFixture("versions", {
  ...Object.fromEntries(packages.map(key => [key, version(key)])),
  "CSS engine": engine(),
})
