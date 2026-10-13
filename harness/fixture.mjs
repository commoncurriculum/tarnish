// What the recorders share: how a run is recorded, and how a fixture is written.
import { writeFileSync } from "node:fs"

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

// Writes fixtures/<name>.json: an object of `fields`, each on a line of its own, and each record
// of a list on a line of its own, so that a diff shows which records changed.
export function writeFixture(name, fields) {
  const list = records => `[\n${records.map(record => `    ${JSON.stringify(record)}`).join(",\n")}\n  ]`
  const field = value => (Array.isArray(value) && value.length > 0 ? list(value) : JSON.stringify(value))
  const lines = Object.entries(fields).map(([key, value]) => `  ${JSON.stringify(key)}: ${field(value)}`)
  writeFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), `{\n${lines.join(",\n")}\n}\n`)
}
