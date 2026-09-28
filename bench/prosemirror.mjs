// Times real ProseMirror on target/bench/document.json: fromJSON, the steps, and toJSON, the
// work tarnish's apply_steps does, once the JIT has warmed up.
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node } from "prosemirror-model"
import { schema } from "prosemirror-schema-basic"
import { Step, Transform } from "prosemirror-transform"

const path = fileURLToPath(new URL("../target/bench/document.json", import.meta.url))
const { doc, steps } = JSON.parse(readFileSync(path, "utf8"))
const apply = () => {
  const tr = new Transform(Node.fromJSON(schema, doc))
  for (const step of steps) tr.step(Step.fromJSON(schema, step))
  return tr.doc.toJSON()
}
for (let round = 0; round < 2000; round++) apply()
const times = []
for (let round = 0; round < 2000; round++) {
  const start = process.hrtime.bigint()
  apply()
  times.push(Number(process.hrtime.bigint() - start) / 1000)
}
times.sort((a, b) => a - b)
console.log(`ProseMirror in Node, apply 10 steps: ${times[1000].toFixed(0)} µs (median)`)
