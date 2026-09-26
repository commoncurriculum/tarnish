// Records what the real ProseMirror packages do with inputs their own tests don't give them, to
// fixtures/model.json, for the Rust, Elixir and C tests to expect: `Node.fromJSON` of attributes
// that aren't objects, `createAndFill` of types that need themselves or that nothing fills, and
// a step that splits a surrogate pair. What that step gives is recorded as its `JSON.stringify`
// text, since some JSON readers, Jason among them, refuse a lone surrogate.
import { writeFileSync } from "node:fs"
import { Node, Schema } from "prosemirror-model"
import { Transform } from "prosemirror-transform"

const schemas = [
  {
    nodes: {
      doc: { content: "item*" },
      item: { attrs: { a: {}, b: { default: 1 } } },
      plain: { attrs: { c: { default: 2 } } },
      text: {},
    },
    marks: { m: { attrs: { h: {} } }, n: { attrs: { i: { default: 3 } } } },
  },
  { nodes: { doc: { content: "block+" }, quote: { content: "block+", group: "block" }, text: {} } },
  {
    nodes: {
      doc: { content: "wrap" },
      wrap: { content: "paragraph* image" },
      paragraph: { content: "text*" },
      image: { attrs: { src: {} } },
      text: {},
    },
  },
  { nodes: { doc: { content: "paragraph{3} quote?" }, paragraph: {}, quote: { content: "paragraph+" }, text: {} } },
  { nodes: { doc: { content: "paragraph+" }, paragraph: { content: "text*" }, text: {} } },
]
const built = schemas.map(spec => new Schema(spec))

function outcome(run) {
  try {
    return { result: run() }
  } catch (error) {
    return { error: { class: error.constructor.name, message: error.message } }
  }
}

const fromJSON = []
for (const attrs of [undefined, null, false, 0, "", 5, "text", [1], {}, { a: "A" }]) {
  const given = attrs === undefined ? {} : { attrs }
  for (const json of [
    { type: "item", ...given },
    { type: "plain", ...given },
    { type: "doc", content: [{ type: "item", attrs: { a: 1 }, marks: [{ type: "m", ...given }] }] },
    { type: "doc", content: [{ type: "item", attrs: { a: 1 }, marks: [{ type: "n", ...given }] }] },
  ]) {
    fromJSON.push({ schema: 0, json, ...outcome(() => Node.fromJSON(built[0], json).toJSON()) })
  }
}

const createAndFill = [1, 2, 3].map(schema => ({
  schema,
  ...outcome(() => built[schema].topNodeType.createAndFill()?.toJSON() ?? null),
}))

const start = { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "a😀b" }] }] }
const changes = [tr => tr.insert(3, built[4].text("x"))]
const transforms = changes.map(change => {
  const tr = change(new Transform(Node.fromJSON(built[4], start)))
  return { schema: 4, start, steps: tr.steps.map(step => step.toJSON()), result: JSON.stringify(tr.doc.toJSON()) }
})

writeFileSync(
  new URL("../fixtures/model.json", import.meta.url),
  JSON.stringify({ schemas, fromJSON, createAndFill, transforms }, null, 2) + "\n",
)
