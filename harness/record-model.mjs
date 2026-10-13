// Records what the real ProseMirror packages do with inputs their own tests don't give them, to
// fixtures/model.json, for the Rust, Elixir and C tests to expect: `Node.fromJSON` of attributes
// that aren't objects, of numbers JavaScript can't hold as written, and of JSON ProseMirror
// writes otherwise; `createAndFill` of types that need themselves or that nothing fills; the
// errors of `new Schema(spec)` and of `node.check()`; steps that split surrogate pairs, and steps
// that fail; and, for each test fixtures/transform.json holds, the steps that undo its steps and
// where each position maps through them.
//
// JSON whose numbers JSON.stringify wouldn't write as they're spelled is recorded as its `text`.
// What steps give is recorded as its `JSON.stringify` text, since some JSON readers, Jason among
// them, refuse a lone surrogate, and where it has one, as `wellFormed` JSON too, each lone
// surrogate U+FFFD, as JSON without lone surrogates gives it. The steps undoing them are recorded
// that way.
import { readFileSync } from "node:fs"
import { Node, Schema } from "prosemirror-model"
import { Step, Transform } from "prosemirror-transform"
import { mapped, outcome, writeFixture } from "./fixture.mjs"

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
  {
    nodes: {
      doc: { content: "block+" },
      paragraph: { content: "inline*", group: "block" },
      heading: { attrs: { level: { default: 1, validate: "number" } }, content: "inline*", group: "block" },
      blockquote: { content: "block+", group: "block" },
      code_block: { content: "text*", marks: "", group: "block" },
      image: {
        attrs: { src: { validate: "string" }, alt: { default: null, validate: "string|null" } },
        inline: true,
        group: "inline",
      },
      text: { group: "inline" },
    },
    marks: {
      em: {},
      strong: {},
      link: { attrs: { href: { validate: "string" } } },
      comment: { attrs: { id: {} }, excludes: "" },
      code: { excludes: "_" },
    },
  },
]
const built = schemas.map(spec => new Schema(spec))

const fromJSON = []
const read = (schema, given) => {
  const json = given.json ?? JSON.parse(given.text)
  fromJSON.push({ schema, ...given, ...outcome(() => ({ result: Node.fromJSON(built[schema], json).toJSON() })) })
}
for (const attrs of [undefined, null, false, 0, "", 5, "text", [1], {}, { a: "A" }]) {
  const given = attrs === undefined ? {} : { attrs }
  for (const json of [
    { type: "item", ...given },
    { type: "plain", ...given },
    { type: "doc", content: [{ type: "item", attrs: { a: 1 }, marks: [{ type: "m", ...given }] }] },
    { type: "doc", content: [{ type: "item", attrs: { a: 1 }, marks: [{ type: "n", ...given }] }] },
  ]) {
    read(0, { json })
  }
}
// Integers past a double's precision, past 64 bits and past a double's range, and doubles at
// the edges of how JavaScript writes them: in a node given every attribute, whose JSON a binding
// may share where it's what ProseMirror writes, and in one it writes anew.
const numbers = [
  "9007199254740993",
  "-9007199254740993",
  "9223372036854775808",
  "18446744073709551615",
  "-9223372036854775809",
  "1e20",
  "1e21",
  "0.1",
  "5e-324",
  "1.0",
  "-0",
  "-0.0",
  String(2n ** 1024n),
]
for (const number of numbers) {
  read(0, { text: `{"type":"item","attrs":{"a":${number},"b":1}}` })
  read(0, { text: `{"type":"item","attrs":{"a":[${number}]}}` })
}
// A node ProseMirror writes otherwise than it was read, after one written as it was read.
const same = '{"type":"paragraph","content":[{"type":"text","text":"same"}]}'
const text = '{"type":"text","text":"a"}'
for (const written of [
  '{"type":"paragraph"}',
  '{"type":"paragraph","extra":1}',
  '{"type":"paragraph","attrs":{}}',
  '{"type":"paragraph","content":[]}',
  '{"type":"paragraph","content":[{"type":"text","text":"a","marks":[]}]}',
  `{"type":"paragraph","content":[${text},{"type":"text","text":"b"}]}`,
  '{"type":"paragraph","content":[{"type":"text","text":"a","marks":[{"type":"strong"},{"type":"em"}]}]}',
  '{"type":"paragraph","content":[{"type":"text","text":"a","marks":[{"type":"em","attrs":null}]}]}',
  `{"type":"heading","content":[${text}]}`,
  `{"type":"heading","attrs":{"level":1.0},"content":[${text}]}`,
  `{"type":"heading","attrs":{"level":${2n ** 1024n}},"content":[${text}]}`,
  `{"type":"heading","attrs":{"level":2,"extra":1},"content":[${text}]}`,
]) {
  read(5, { text: `{"type":"doc","content":[${same},${written}]}` })
}

const createAndFill = [1, 2, 3].map(schema => ({
  schema,
  ...outcome(() => ({ result: built[schema].topNodeType.createAndFill()?.toJSON() ?? null })),
}))

const newSchema = [
  { nodes: { text: {} } },
  { topNode: "page", nodes: { doc: {}, text: {} } },
  { nodes: { doc: {} } },
  { nodes: { doc: {}, text: { attrs: { a: {} } } } },
  { nodes: { doc: {}, text: {}, em: {} }, marks: { em: {} } },
  { nodes: { doc: {}, a: { linebreakReplacement: true }, text: {} } },
  {
    nodes: {
      doc: { content: "inline*" },
      a: { linebreakReplacement: true, inline: true, group: "inline" },
      b: { linebreakReplacement: true, inline: true, group: "inline" },
      text: { group: "inline" },
    },
  },
  { nodes: { doc: {}, text: {} }, marks: { em: { excludes: "nope" } } },
  { nodes: { doc: { content: "text*", marks: "nope" }, text: {} } },
  ...[
    "paragraph+",
    "para\u0000graph+",
    "text* (",
    "text | ",
    "text{2",
    "text{x}",
    "text{3,1}",
    "(text | doc)*",
    "text+?",
    "",
  ].map(content => ({ nodes: { doc: { content }, text: {} } })),
  {
    nodes: {
      doc: { content: "block+" },
      p: { group: "block", content: "text*" },
      q: { group: "block", content: "text* p" },
      text: { group: "inline" },
    },
  },
].map(spec => ({ spec, ...outcome(() => (new Schema(spec), { result: null })) }))

const p = (...content) => ({ type: "paragraph", content })
const t = (text, marks) => (marks ? { type: "text", text, marks } : { type: "text", text })
const image = attrs => p({ type: "image", attrs })
const check = [
  [t("loose")],
  [t("a text that runs on past the fifty characters of content a message shows")],
  [p(t("a")), t("b"), p(t("c"))],
  [],
  [{ type: "paragraph", marks: [{ type: "em" }] }],
  [{ type: "code_block", content: [t("x", [{ type: "em" }])] }],
  [p(t("x", [{ type: "em" }, { type: "em" }]))],
  [p(t("x", [{ type: "em" }, { type: "code" }]))],
  [p(t("x", [{ type: "comment", attrs: { id: 1 } }, { type: "comment", attrs: { id: 2 } }]))],
  [p(t("x", [{ type: "link", attrs: { href: 5 } }]))],
  [{ type: "heading", attrs: { level: "2" }, content: [t("x")] }],
  [image({ src: 5 })],
  [image({ src: "a", alt: 5 })],
  [image({ src: "a", alt: null })],
].map(content => {
  const json = { type: "doc", content }
  return { schema: 5, json, ...outcome(() => (Node.fromJSON(built[5], json).check(), { result: null })) }
})

const madeWellFormed = json =>
  JSON.parse(JSON.stringify(json), (_, value) => (typeof value === "string" ? value.toWellFormed() : value))
const applied = (schema, start, steps) => ({
  schema,
  start,
  steps,
  ...outcome(() => {
    const tr = new Transform(Node.fromJSON(built[schema], start))
    for (const step of steps) tr.step(Step.fromJSON(built[schema], step))
    const result = JSON.stringify(tr.doc.toJSON())
    const wellFormed = madeWellFormed(tr.doc.toJSON())
    const inverted = tr.steps.map((step, i) => step.invert(tr.docs[i]).toJSON()).reverse()
    return {
      result,
      ...(JSON.stringify(wellFormed) !== result && { wellFormed }),
      inverted: madeWellFormed(inverted),
      mapped: mapped(tr.mapping, tr.before.content.size),
    }
  }),
})
const transforms = []
// 0-9 paragraph, 1-8 its text, with a surrogate pair at 2-4 and another at 5-7
const pairs = { type: "doc", content: [p(t("a😀b😀c"))] }
const madeBy = change => change(new Transform(Node.fromJSON(built[4], pairs))).steps.map(step => step.toJSON())
for (let pos = 0; pos <= 9; pos++) transforms.push(applied(4, pairs, madeBy(tr => tr.insert(pos, built[4].text("x")))))
for (let from = 0; from <= 9; from++) {
  for (let to = from + 1; to <= 9; to++) transforms.push(applied(4, pairs, madeBy(tr => tr.delete(from, to))))
}
// 0-5 paragraph, 5-12 blockquote of a paragraph 6-11, 12-15 code_block
const blocks = {
  type: "doc",
  content: [p(t("one")), { type: "blockquote", content: [p(t("two"))] }, { type: "code_block", content: [t("x")] }],
}
const replace = (from, to, content, more) => ({ stepType: "replace", from, to, ...(content && { slice: { content } }), ...more })
const around = (gapFrom, insert, more) => ({ stepType: "replaceAround", from: 5, to: 12, gapFrom, gapTo: 11, insert, ...more })
for (const steps of [
  [replace(0, 1)],
  [replace(4, 7)],
  [replace(2, 8, null, { structure: true })],
  [replace(0, 0, [t("x")])],
  [replace(1, 1, [p(t("x"))])],
  [replace(1, 99)],
  [replace(12, 12, [{ type: "heading", attrs: { level: "x" } }])],
  [replace(1, 1, [t("a")]), replace(0, 1)],
  [around(6, 0, { structure: true })],
  [around(7, 0, { structure: true })],
  [around(6, 1, { slice: { content: [{ type: "code_block" }] } })],
  [{ stepType: "addMark", from: 13, to: 14, mark: { type: "em" } }],
  [{ stepType: "addMark", from: 1, to: 99, mark: { type: "em" } }],
  [{ stepType: "attr", pos: 1, attr: "level", value: 2 }],
  [{ stepType: "attr", pos: 99, attr: "level", value: 2 }],
  [{ stepType: "addNodeMark", pos: 0, mark: { type: "em" } }],
  [{ stepType: "removeNodeMark", pos: 99, mark: { type: "em" } }],
]) {
  transforms.push(applied(5, blocks, steps))
}

const recorded = JSON.parse(readFileSync(new URL("../fixtures/transform.json", import.meta.url), "utf8"))
const upstream = recorded.schemas.map(spec => new Schema(spec))
const transformTests = recorded.tests.map((test, index) => {
  const schema = upstream[test.schema]
  const tr = new Transform(Node.fromJSON(schema, test.start))
  for (const step of test.steps) tr.step(Step.fromJSON(schema, step))
  if (JSON.stringify(tr.doc.toJSON()) !== JSON.stringify(test.result)) throw new Error(`Test ${index} gives another document`)
  const inverted = tr.steps.map((step, i) => step.invert(tr.docs[i]).toJSON()).reverse()
  if (/\\ud[89a-f]/i.test(JSON.stringify(inverted))) throw new Error(`Test ${index} has a lone surrogate`)
  return { test: index, inverted, eq: tr.doc.eq(tr.before), mapped: mapped(tr.mapping, tr.before.content.size) }
})

writeFixture("model", { schemas, fromJSON, createAndFill, newSchema, check, transforms, transformTests })
