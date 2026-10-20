// Times real ProseMirror on the documents bench/document.mjs writes to target/bench/, once the
// JIT has warmed up: each row of the README's table, and fromJSON, the steps and toJSON together,
// the work tarnish's apply_steps does. document.json is read in prosemirror-schema-basic, as the
// README's table has it, and the others in the schema each file holds.
//
// TARNISH_BENCH_DOC may name a card stack's fixture, `{doc, changes: [{steps}]}`, with each
// change's steps applying to `doc`; its schema's spec is the fixture's `schema`, or the JSON file
// TARNISH_BENCH_SCHEMA names.
//
// Every case of every document runs in each of TARNISH_BENCH_ROUNDS rounds (7 unless given), in
// turn, so that the machine's drift lands on all of them alike. A round times a case's calls for
// TARNISH_BENCH_MS milliseconds (150 unless given) and takes their median; a row gives the median
// of its rounds and the range they span.
//
// The "kept node" rows are a socket process's message, as elixir/bench/apply_steps.exs times it:
// the node read earlier, kept between messages, takes one change's steps and is written back. Each
// call takes the next change in turn; the row for one step takes each step in turn, applied alone
// to the document the steps before it in its change made. The "typed" rows keep the document that
// 100 messages each typed "x" into, in one place or in a place of its own, or that 10 did in 10
// places, and time one message more, typing in the first place.
import { existsSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node, Schema } from "prosemirror-model"
import { schema as basic } from "prosemirror-schema-basic"
import { Step, Transform } from "prosemirror-transform"

const rounds = Number(process.env.TARNISH_BENCH_ROUNDS ?? 7)
const milliseconds = Number(process.env.TARNISH_BENCH_MS ?? 150)
const bench = (name) => fileURLToPath(new URL(`../target/bench/${name}`, import.meta.url))

// A spec as data says an attribute's default is undefined with `hasDefault` and no `default`, as
// tarnish reads it, since JSON has no undefined. ProseMirror reads `default: undefined`.
const fromData = (spec) => {
  const types = (types = {}) =>
    Object.fromEntries(
      Object.entries(types).map(([name, type]) => [
        name,
        type.attrs
          ? {
              ...type,
              attrs: Object.fromEntries(
                Object.entries(type.attrs).map(([attr, { hasDefault, ...rest }]) => [
                  attr,
                  hasDefault && !Object.hasOwn(rest, "default") ? { ...rest, default: undefined } : rest,
                ]),
              ),
            }
          : type,
      ]),
    )
  return new Schema({ ...spec, nodes: types(spec.nodes), marks: types(spec.marks) })
}

// Each document: a name, its schema, its JSON, and the step lists that each apply to it.
const documents = []
const load = (name, path, schemaFor) => {
  const file = JSON.parse(readFileSync(path, "utf8"))
  const schema = schemaFor(file)
  if (!schema) return
  const changes = file.changes ? file.changes.map((change) => change.steps) : [file.steps]
  documents.push({ name, schema, doc: file.doc, changes })
}
load("document.json: 200 paragraphs", bench("document.json"), () => basic)
for (const [name, about] of [
  ["nested.json", "a long list and a table"],
  ["cards.json", "a 50-card stack"],
]) {
  load(`${name}: ${about}`, bench(name), (file) => fromData(file.schema))
}
if (process.env.TARNISH_BENCH_DOC) {
  const path = process.env.TARNISH_BENCH_DOC
  load(`TARNISH_BENCH_DOC: ${path}`, path, (file) => {
    const given = process.env.TARNISH_BENCH_SCHEMA
    const spec = file.schema ?? (given && existsSync(given) ? JSON.parse(readFileSync(given, "utf8")) : undefined)
    if (spec) return fromData(spec)
    console.log(`Skipping ${path}: it holds no schema, and TARNISH_BENCH_SCHEMA names no spec.\n`)
  })
}

// What a call returns is kept, so that the JIT can't drop the work.
let kept
const cases = []
for (const { name, schema, doc, changes } of documents) {
  // The document and steps as JSON text, as a server receives them.
  const docText = JSON.stringify(doc)
  const changesText = changes.map((steps) => JSON.stringify(steps))
  const node = Node.fromJSON(schema, doc)
  const step = (from, steps) => {
    const tr = new Transform(from)
    for (const json of steps) tr.step(Step.fromJSON(schema, json))
    return tr.doc
  }
  // Each change applies to the document as read, and the last one's result is what's written.
  const applyAll = (from, lists = changes) => {
    let last
    for (const steps of lists) last = step(from, steps)
    return last
  }
  const changed = applyAll(node)
  const count = changes.reduce((sum, steps) => sum + steps.length, 0)
  const applied = changes.length === 1 ? `apply ${count} steps` : `apply ${changes.length} changes (${count} steps)`
  // Each change with what its steps make of the kept node, and each step with the document the
  // steps before it in its change made.
  const messages = changes.map((steps) => ({ steps, changed: step(node, steps) }))
  const steps = changes.flatMap((list) => {
    let before = node
    return list.map((json) => {
      const one = { before, steps: [json] }
      before = step(before, [json])
      return one
    })
  })
  const cycle = (items) => {
    let at = 0
    return () => items[at++ % items.length]
  }
  // A message that types "x" at a position, and the positions where one applies, in order.
  const typing = (at) => [{ stepType: "replace", from: at, to: at, slice: { content: [{ type: "text", text: "x" }] } }]
  const places = []
  for (let at = 0; at <= node.content.size; at++) {
    if (!Step.fromJSON(schema, typing(at)[0]).apply(node).failed) places.push(at)
  }
  // The kept node after `count` messages, each typing "x" in the first place or, spread evenly,
  // in a place of its own, the last first so that each place is where it was read.
  const typed = (count, spread) => {
    let kept = node
    for (let index = count - 1; index >= 0; index--) {
      kept = step(kept, typing(spread ? places[Math.floor((index * places.length) / count)] : places[0]))
    }
    return kept
  }
  const messageAfter = (kept) => () => step(kept, typing(places[0])).toJSON()
  const inOnePlace = typed(100, false)
  const in10Places = typed(10, true)
  const in100Places = typed(100, true)
  const typedOnceMore = step(in100Places, typing(places[0]))
  const message = cycle(messages)
  const oneStep = cycle(steps)
  const add = (row, run) => cases.push({ document: name, row, run, times: [] })
  add("fromJSON", () => Node.fromJSON(schema, doc))
  add("JSON.parse, then fromJSON", () => Node.fromJSON(schema, JSON.parse(docText)))
  add("check", () => node.check())
  add(applied, () => applyAll(node))
  add("toJSON of the changed document", () => changed.toJSON())
  add("JSON.stringify of its toJSON", () => JSON.stringify(changed.toJSON()))
  add(`fromJSON, ${applied}, toJSON`, () => applyAll(Node.fromJSON(schema, doc)).toJSON())
  add(`JSON text in, ${applied}, JSON text out`, () =>
    JSON.stringify(
      applyAll(
        Node.fromJSON(schema, JSON.parse(docText)),
        changesText.map((text) => JSON.parse(text)),
      ).toJSON(),
    ),
  )
  add("kept node: one message", () => step(node, message().steps).toJSON())
  add("kept node: its apply", () => step(node, message().steps))
  add("kept node: its toJSON", () => message().changed.toJSON())
  add("kept node: apply, one step", () => {
    const { before, steps } = oneStep()
    return step(before, steps)
  })
  add("kept node, typed in 1 place 100 times: a message", messageAfter(inOnePlace))
  add("kept node, typed in 10 places: a message", messageAfter(in10Places))
  add("kept node, typed in 100 places: a message", messageAfter(in100Places))
  add("kept node, typed in 100 places: its toJSON", () => typedOnceMore.toJSON())
}

for (const { run } of cases) for (let call = 0; call < 2000; call++) kept = run()
for (let round = 0; round < rounds; round++) {
  for (const { run, times } of cases) {
    const calls = []
    const deadline = performance.now() + milliseconds
    while (performance.now() < deadline || calls.length < 50) {
      const start = process.hrtime.bigint()
      kept = run()
      calls.push(Number(process.hrtime.bigint() - start) / 1000)
    }
    calls.sort((a, b) => a - b)
    times.push(calls[calls.length >> 1])
  }
}

const format = (µs) => (µs < 10 ? µs.toFixed(1) : µs.toFixed(0))
console.log(`µs per call: the median of ${rounds} rounds' medians, and the range of the rounds\n`)
let section
for (const { document, row, times } of cases) {
  if (document !== section) {
    console.log(`${section === undefined ? "" : "\n"}${document}`)
    section = document
  }
  times.sort((a, b) => a - b)
  const median = times[times.length >> 1]
  const range = `${format(times[0])}..${format(times[times.length - 1])}`
  console.log(`  ${row.padEnd(58)} ${format(median).padStart(6)}   ${range}`)
}
