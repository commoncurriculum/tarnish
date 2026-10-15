// Times real ProseMirror on target/bench/document.json, once the JIT has warmed up: each row of
// the README's table, and fromJSON, the steps and toJSON together, the work tarnish's
// apply_steps does.
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node } from "prosemirror-model"
import { schema } from "prosemirror-schema-basic"
import { Step, Transform } from "prosemirror-transform"

const path = fileURLToPath(new URL("../target/bench/document.json", import.meta.url))
const { doc, steps } = JSON.parse(readFileSync(path, "utf8"))
// The document and steps as JSON text, as a server receives them.
const docText = JSON.stringify(doc)
const stepsText = JSON.stringify(steps)
const node = Node.fromJSON(schema, doc)
const step = (from, list = steps) => {
  const tr = new Transform(from)
  for (const json of list) tr.step(Step.fromJSON(schema, json))
  return tr.doc
}
const changed = step(node)

// What a call returns is kept, so that the JIT can't drop the work.
let kept
const median = (name, run) => {
  for (let round = 0; round < 2000; round++) kept = run()
  const times = []
  for (let round = 0; round < 2000; round++) {
    const start = process.hrtime.bigint()
    kept = run()
    times.push(Number(process.hrtime.bigint() - start) / 1000)
  }
  times.sort((a, b) => a - b)
  console.log(`${name.padEnd(36)} ${times[1000].toFixed(0)} µs (median)`)
}

median("fromJSON", () => Node.fromJSON(schema, doc))
median("JSON.parse, then fromJSON", () => Node.fromJSON(schema, JSON.parse(docText)))
median("check", () => node.check())
median("apply 10 steps", () => step(node))
median("toJSON of the changed document", () => changed.toJSON())
median("JSON.stringify of its toJSON", () => JSON.stringify(changed.toJSON()))
median("fromJSON, apply 10 steps, toJSON", () => step(Node.fromJSON(schema, doc)).toJSON())
median("JSON text in, apply 10 steps, JSON text out", () =>
  JSON.stringify(step(Node.fromJSON(schema, JSON.parse(docText)), JSON.parse(stepsText)).toJSON()),
)
