// Times real ProseMirror on the documents bench/document.mjs writes to target/bench/, once the
// JIT has warmed up: each row of the README's table, and fromJSON, the steps and toJSON together,
// the work tarnish's apply_steps does. document.json is read in prosemirror-schema-basic, as the
// README's table has it, and the others in the schema each file holds: given.json too, the card
// stack TARNISH_BENCH_DOC names, when bench/document.mjs has written it.
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
// places, and time one message more, typing in the first place. The "chained" rows are the 100
// messages a socket takes after those 100: each types "x" in one of 10 or 100 places spread evenly,
// in turn, into the document the message before it made, and each call takes the next message.
// Their messages are the scenarios bench/document.mjs writes.
import { existsSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node } from "prosemirror-model"
import { schema as basic } from "prosemirror-schema-basic"
import { Step, Transform } from "prosemirror-transform"
import { fromData } from "./schema.mjs"

const rounds = Number(process.env.TARNISH_BENCH_ROUNDS ?? 7)
const milliseconds = Number(process.env.TARNISH_BENCH_MS ?? 150)
const bench = (name) => fileURLToPath(new URL(`../target/bench/${name}`, import.meta.url))

// Each document: a name, its schema, its JSON, the step lists that each apply to it, and its
// scenarios.
const documents = []
for (const [file, name] of [
  ["document.json", "document.json: 200 paragraphs"],
  ["nested.json", "nested.json: a long list and a table"],
  ["cards.json", "cards.json: a 50-card stack"],
  ["given.json", `TARNISH_BENCH_DOC: ${process.env.TARNISH_BENCH_DOC}`],
]) {
  const path = bench(file)
  if (file === "given.json" && !(process.env.TARNISH_BENCH_DOC && existsSync(path))) continue
  const { schema, doc, changes, scenarios } = JSON.parse(readFileSync(path, "utf8"))
  documents.push({
    name,
    schema: schema ? fromData(schema) : basic,
    doc,
    changes: changes.map((change) => change.steps),
    scenarios,
  })
}

// What a call returns is kept, so that the JIT can't drop the work.
let kept
const cases = []
for (const { name, schema, doc, changes, scenarios } of documents) {
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
  // The node each message of `messages` makes of the one the message before it made, from
  // `start`, each with the node it was applied to and its steps.
  const fold = (start, messages) => {
    let before = start
    return messages.map((steps) => {
      const changed = step(before, steps)
      const one = { before, steps, changed }
      before = changed
      return one
    })
  }
  const last = (folded) => folded[folded.length - 1].changed
  // The first message of typed_once_100 types in the first place.
  const first = scenarios.typed_once_100[0]
  const messageAfter = (kept) => () => step(kept, first).toJSON()
  const inOnePlace = last(fold(node, scenarios.typed_once_100))
  const in10Places = last(fold(node, scenarios.typed_10))
  const in100Places = last(fold(node, scenarios.typed_100))
  const typedOnceMore = step(in100Places, first)
  const chained = (count) => cycle(fold(in100Places, scenarios[`chained_${count}`]))
  const chainedIn10 = chained(10)
  const chainedIn100 = chained(100)
  const chainedIn100Written = chained(100)
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
  add("kept node, chained in 10 places: a message", () => {
    const { before, steps } = chainedIn10()
    return step(before, steps).toJSON()
  })
  add("kept node, chained in 100 places: a message", () => {
    const { before, steps } = chainedIn100()
    return step(before, steps).toJSON()
  })
  add("kept node, chained in 100 places: its toJSON", () => chainedIn100Written().changed.toJSON())
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
