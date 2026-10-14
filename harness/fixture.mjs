// What the recorders share: how a run is recorded, and how a fixture is written.
import { writeFileSync } from "node:fs"

// HTMLAttributes of values that aren't strings, which `mergeAttributes` keeps as they are: an
// element gets each as `setAttribute` makes a string of it, but `null` and `undefined`.
export const otherAttributes = {
  class: 5,
  "data-null": null,
  "data-undefined": undefined,
  "data-zero": 0,
  "data-false": false,
  "data-number": 1.5,
  "data-object": {},
  "data-array": [1, "a", null],
}

// The fields `run` gives, or its error: its class and message, and a DOMException's name.
export function outcome(run) {
  try {
    return run()
  } catch (error) {
    const name = error.constructor.name === "DOMException" ? { name: error.name } : {}
    return { error: { class: error.constructor.name, ...name, message: error.message } }
  }
}

// Where every position of a document `size` long maps through `mapping`, with `assoc` -1 and 1:
// the positions mapped to, in order, and the positions whose `MapResult` has each flag.
export function mapped(mapping, size) {
  return [-1, 1].map(assoc => {
    const results = Array.from({ length: size + 1 }, (_, pos) => mapping.mapResult(pos, assoc))
    const flagged = flag => results.flatMap((result, pos) => (result[flag] ? [pos] : []))
    return {
      assoc,
      pos: results.map(result => result.pos),
      deleted: flagged("deleted"),
      deletedBefore: flagged("deletedBefore"),
      deletedAfter: flagged("deletedAfter"),
      deletedAcross: flagged("deletedAcross"),
    }
  })
}

// The fields of a node's and a mark's spec that ProseMirror or Tiptap reads, in NodeSpec's and
// MarkSpec's order, each with the value ProseMirror takes where the spec leaves it out: false for
// a flag, true for a flag only a `false` turns off, and null for a string.
const nodeFields = [
  ["content", null],
  ["marks", null],
  ["group", null],
  ["inline", false],
  ["atom", false],
  ["selectable", true],
  ["draggable", false],
  ["code", false],
  ["whitespace", null],
  ["definingAsContext", false],
  ["definingForContent", false],
  ["defining", false],
  ["isolating", false],
  ["linebreakReplacement", false],
]
const markFields = [
  ["inclusive", true],
  ["excludes", null],
  ["group", null],
  ["spanning", true],
  ["code", false],
]
// The fields of a spec that hold functions and parse rules, which the conversions' records hold
// to what they do.
const functions = new Set(["parseDOM", "toDOM", "toText", "leafText", "toDebugString"])

// `schema` as ProseMirror holds it: its top node's type, and each node and mark type in its order
// with each field `nodeFields` or `markFields` names, whether its spec sets it or leaves it out,
// then the fields an extension adds to the spec, such as `tableRole`, and each attribute: whether
// it's required, and its default, which JSON leaves out where it's `undefined`.
export function schemaRecord(schema) {
  const types = (types, fields) => {
    const known = new Set(fields.map(([field]) => field))
    return Object.values(types).map(({ name, spec }) => {
      const { attrs = {}, ...rest } = spec
      return {
        name,
        ...Object.fromEntries(fields.map(([field, left]) => [field, spec[field] ?? left])),
        ...Object.fromEntries(Object.entries(rest).filter(([field]) => !known.has(field) && !functions.has(field))),
        attrs: Object.entries(attrs).map(([name, attr]) => ({
          name,
          required: !Object.hasOwn(attr, "default"),
          default: attr.default,
        })),
      }
    })
  }
  return {
    topNode: schema.topNodeType.name,
    nodes: types(schema.nodes, nodeFields),
    marks: types(schema.marks, markFields),
  }
}

// Writes fixtures/<name>.json: an object of `fields`, each on a line of its own, and each record
// of a list on a line of its own, so that a diff shows which records changed.
export function writeFixture(name, fields) {
  const list = records => `[\n${records.map(record => `    ${JSON.stringify(record)}`).join(",\n")}\n  ]`
  const field = value => (Array.isArray(value) && value.length > 0 ? list(value) : JSON.stringify(value))
  const lines = Object.entries(fields).map(([key, value]) => `  ${JSON.stringify(key)}: ${field(value)}`)
  writeFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), `{\n${lines.join(",\n")}\n}\n`)
}
