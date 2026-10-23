// Writes target/bench/document.json: a 200-paragraph document in prosemirror-schema-basic, each
// paragraph with bold text and a link, and ten steps that each type one character near the
// middle, as an editor sends them.
//
// Then writes two more, each with the schema's spec as JSON, as `new Schema(spec)` and
// `Tarnish.Schema.new` read it:
// - target/bench/nested.json: most of the content in one long bullet list and one table, in
//   prosemirror-schema-basic with prosemirror-schema-list and prosemirror-tables' nodes;
// - target/bench/cards.json: a card stack, 50 cards that carry ids, comment ids and link
//   metadata, in a schema of the same node types as a card stack's.
// Their steps are what ProseMirror's Transform makes for an editor's edits near the middle.
//
// TARNISH_BENCH_DOC may name a card stack's fixture, `{doc, changes: [{steps}]}`, with each
// change's steps applying to `doc`; its schema's spec is the fixture's `schema`, or the JSON file
// TARNISH_BENCH_SCHEMA names. Its changes are replayed, and it is written as
// target/bench/given.json, which target/ keeps out of the repo.
//
// Each file is `{schema?, doc, changes: [{steps}], scenarios}`, document.json's without a schema,
// since each bench reads it in a basic schema of its own. Each change applies to `doc`. The
// scenarios are the messages the benches' "typed" and "chained" rows apply, each a list of steps,
// in the order they're applied:
// - typed_once_100: 100 messages typing "x" in the first place one applies;
// - typed_10, typed_100: 10 or 100 messages typing "x", spread evenly, the last place first so
//   that each place is where it was read;
// - chained_10, chained_100: the 100 messages after typed_100, each typing in the next of 10 or
//   100 places spread evenly, where that place is by then.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node, Schema } from "prosemirror-model"
import { schema as basic } from "prosemirror-schema-basic"
import { addListNodes } from "prosemirror-schema-list"
import { tableNodes } from "prosemirror-tables"
import { Step, Transform } from "prosemirror-transform"
import { fromData, specOf } from "./schema.mjs"

// The scenarios' messages on `doc`, a node of `schema`.
const scenarios = (schema, node) => {
  // A message that types "x" at a position, and the positions where one applies, in order.
  const typing = (at) => [{ stepType: "replace", from: at, to: at, slice: { content: [{ type: "text", text: "x" }] } }]
  const places = []
  for (let at = 0; at <= node.content.size; at++) {
    if (!Step.fromJSON(schema, typing(at)[0]).apply(node).failed) places.push(at)
  }
  // `count` messages, each typing "x" in the first place or, spread evenly, in a place of its own,
  // the last first.
  const typed = (count, spread) => {
    const messages = []
    for (let index = count - 1; index >= 0; index--) {
      messages.push(typing(spread ? places[Math.floor((index * places.length) / count)] : places[0]))
    }
    return messages
  }
  // The 100 messages after typed(100, true), each typing in the next of `count` places spread
  // evenly, where that place is by then.
  const chained = (count) => {
    const typedAt = Array.from({ length: 100 }, (_, index) => places[Math.floor((index * places.length) / 100)])
    const chain = []
    for (let index = 0; index < 100; index++) {
      const place = places[Math.floor((Math.floor((index * count) / 100) * places.length) / count)]
      chain.push(typing(place + typedAt.filter((at) => at < place).length))
      typedAt.push(place)
    }
    return chain
  }
  return {
    typed_once_100: typed(100, false),
    typed_10: typed(10, true),
    typed_100: typed(100, true),
    chained_10: chained(10),
    chained_100: chained(100),
  }
}

const paragraph = (index) => ({
  type: "paragraph",
  content: [
    { type: "text", text: `Paragraph ${index}: the quick brown fox jumps over the lazy dog, ` },
    { type: "text", marks: [{ type: "strong" }], text: "twice as fast" },
    { type: "text", text: " as the one before it, and " },
    {
      type: "text",
      marks: [{ type: "link", attrs: { href: `https://example.com/${index}`, title: null } }],
      text: "a link",
    },
    { type: "text", text: " to end on." },
  ],
})
const doc = { type: "doc", content: Array.from({ length: 200 }, (_, index) => paragraph(index)) }
const steps = Array.from({ length: 10 }, (_, index) => ({
  stepType: "replace",
  from: 5000 + index,
  to: 5000 + index,
  slice: { content: [{ type: "text", text: "x" }] },
}))
const out = fileURLToPath(new URL("../target/bench/", import.meta.url))
mkdirSync(out, { recursive: true })
const scenariosOf = (schema, json) => scenarios(schema, Node.fromJSON(schema, json))
writeFileSync(
  `${out}document.json`,
  JSON.stringify({ doc, changes: [{ steps }], scenarios: scenariosOf(basic, doc) }),
)
// Where the text of the `index`th node of `type` starts, in document order.
const textStart = (doc, type, index) => {
  let seen = 0
  let found
  doc.descendants((node, pos) => {
    if (found !== undefined) return false
    if (node.type.name === type && seen++ === index) found = pos + 1
  })
  return found
}
// Types `text` at `at`, as an editor does in unmarked text.
const type = (tr, text, at) => tr.insert(at, tr.doc.type.schema.text(text))
// Writes `name`.json from the document and the steps `edit` makes on a Transform of it, which
// ProseMirror applies again from their JSON, so that the file holds steps that apply.
const write = (name, schema, json, edit) => {
  const tr = new Transform(Node.fromJSON(schema, json))
  edit(tr)
  const steps = tr.steps.map((step) => step.toJSON())
  const spec = specOf(schema)
  const again = fromData(JSON.parse(JSON.stringify(spec)))
  const replayed = new Transform(Node.fromJSON(again, json))
  for (const step of tr.steps) replayed.step(step.constructor.fromJSON(again, step.toJSON()))
  if (!replayed.doc.eq(Node.fromJSON(again, tr.doc.toJSON()))) throw new Error(`${name}: steps don't replay`)
  const file = { schema: spec, doc: json, changes: [{ steps }], scenarios: scenariosOf(again, json) }
  writeFileSync(`${out}${name}.json`, JSON.stringify(file))
}

// Nested: a heading, a 120-item bullet list whose every fifth item holds a list of two, and a
// 60-row table of three columns, with a paragraph on either side.
const nestedSchema = new Schema({
  nodes: addListNodes(basic.spec.nodes, "paragraph block*", "block").append(
    tableNodes({ tableGroup: "block", cellContent: "block+", cellAttributes: {} }),
  ),
  marks: basic.spec.marks,
})
const line = (text, index) => ({
  type: "paragraph",
  content: [
    { type: "text", text: `${text} ${index}: the quick brown fox jumps over ` },
    { type: "text", marks: [{ type: "strong" }], text: "the lazy dog" },
    { type: "text", text: ", then " },
    { type: "text", marks: [{ type: "link", attrs: { href: `https://example.com/${index}`, title: null } }], text: "a link" },
    { type: "text", text: "." },
  ],
})
const item = (index) => ({
  type: "list_item",
  content:
    index % 5 === 0
      ? [
          line("Item", index),
          { type: "bullet_list", content: [0, 1].map((sub) => ({ type: "list_item", content: [line(`Item ${index}.`, sub)] })) },
        ]
      : [line("Item", index)],
})
const cell = (row, column) => ({
  type: row === 0 ? "table_header" : "table_cell",
  attrs: { colspan: 1, rowspan: 1, colwidth: null },
  content: [row === 0 ? { type: "paragraph", content: [{ type: "text", text: `Column ${column}` }] } : line(`Cell ${row}.${column}`, row)],
})
const nested = {
  type: "doc",
  content: [
    { type: "heading", attrs: { level: 1 }, content: [{ type: "text", text: "A nested document" }] },
    { type: "paragraph", content: [{ type: "text", text: "Most of what follows is one list and one table." }] },
    { type: "bullet_list", content: Array.from({ length: 120 }, (_, index) => item(index)) },
    { type: "table", content: Array.from({ length: 61 }, (_, row) => ({ type: "table_row", content: [0, 1, 2].map((column) => cell(row, column)) })) },
    { type: "paragraph", content: [{ type: "text", text: "The end." }] },
  ],
}
// Types into the 60th item, splits it, types into the new item, then types into a cell halfway
// down the table and bolds a word of it.
write("nested", nestedSchema, nested, (tr) => {
  // The intro, then 60 items, every fifth with two more paragraphs.
  let at = textStart(tr.doc, "paragraph", 1 + 60 + 2 * 12) + 10
  for (const char of "abcd") type(tr, char, at++)
  tr.split(at, 2)
  at += 4
  for (const char of "ef") type(tr, char, at++)
  // The intro, the list's 168 and the one the split made, the header row and 29 rows: row 30's
  // second cell.
  const cellAt = textStart(tr.doc, "paragraph", 1 + 168 + 1 + 3 + 29 * 3 + 1) + 4
  for (const char of "gh") type(tr, char, cellAt + "gh".indexOf(char))
  tr.addMark(cellAt + 20, cellAt + 25, nestedSchema.marks.strong.create())
})

// A card stack: the document is cards, a titled card a title and blocks, an untitled one blocks.
// Cards carry their id, the card they nest under and the ids of their comments; a link inline
// carries the metadata of what it links to, and a standard the standard it aligns to.
const cardAttrs = { id: {}, parentCardId: { default: null }, commentIds: { default: [] }, isPublic: { default: false } }
const nullable = (names) => Object.fromEntries(names.map((name) => [name, { default: null }]))
const cardsSchema = new Schema({
  topNode: "doc",
  nodes: {
    doc: { content: "card*" },
    paragraph: { content: "inline*", group: "block" },
    cardTitle: { content: "text*", marks: "", attrs: { color: { default: "yellow" } } },
    text: { group: "inline" },
    hardBreak: { group: "inline", inline: true, selectable: false },
    linkInline: {
      group: "inline",
      inline: true,
      atom: true,
      attrs: {
        url: {},
        ...nullable(["attachmentId", "title"]),
        type: { default: "link" },
        ...nullable(["mimetype", "size"]),
        isWriteable: { default: false },
        icon: { default: null },
        source: { default: "link" },
        renderAs: { default: "hyperlink" },
        ...nullable(["previewTitle", "previewDescription", "previewImageUrl", "previewFaviconUrl"]),
      },
    },
    titledCard: { content: "cardTitle block+", group: "card", attrs: cardAttrs },
    untitledCard: { content: "block+", group: "card", attrs: cardAttrs },
    standard: { group: "block", atom: true, attrs: { standard: {}, emphasisLevel: { default: null } } },
  },
  marks: {
    bold: {},
    italic: {},
    underline: { attrs: { color: { default: null } } },
    textStyle: { attrs: { color: { default: null } } },
  },
})
const colors = ["yellow", "blue", "green", "pink"]
const link = (index) => ({
  type: "linkInline",
  attrs: {
    url: `https://example.com/resource/${index}`,
    attachmentId: index % 3 === 0 ? `attachment-${index}` : null,
    title: `Resource ${index}`,
    type: index % 3 === 0 ? "file" : "link",
    mimetype: index % 3 === 0 ? "application/pdf" : null,
    size: index % 3 === 0 ? 1024 * (index + 1) : null,
    isWriteable: false,
    icon: null,
    source: index % 3 === 0 ? "drive" : "link",
    renderAs: "hyperlink",
    previewTitle: `Resource ${index}`,
    previewDescription: `What resource ${index} is about, in a sentence or so.`,
    previewImageUrl: `https://example.com/resource/${index}.png`,
    previewFaviconUrl: "https://example.com/favicon.ico",
  },
})
const prose = (card, index) => ({
  type: "paragraph",
  content: [
    { type: "text", text: `Card ${card}, paragraph ${index}: students work in pairs to ` },
    { type: "text", marks: [{ type: "bold" }], text: "measure the angles" },
    { type: "text", text: " of each shape, then compare what they found with " },
    link(card * 10 + index),
    { type: "text", text: " before they write it up." },
  ],
})
const standard = (card) => ({
  type: "standard",
  attrs: {
    standard: {
      id: `standard-${card}`,
      code: `MATH.${card}.G.${card % 7}`,
      statement: `Understand that shapes in different categories may share attributes (${card}).`,
      jurisdiction: "Common Core",
      grades: "3",
      standardSetId: "set-math-3",
      subject: "Math",
    },
    emphasisLevel: card % 4 === 0 ? "major" : null,
  },
})
const card = (index) => {
  const attrs = {
    id: `card-${index}`,
    parentCardId: index % 6 === 5 ? `card-${index - 1}` : null,
    commentIds: Array.from({ length: index % 4 }, (_, comment) => `comment-${index}-${comment}`),
    isPublic: index % 2 === 0,
  }
  const blocks = [prose(index, 0), prose(index, 1), ...(index % 3 === 0 ? [standard(index)] : []), prose(index, 2)]
  return index % 5 === 4
    ? { type: "untitledCard", attrs, content: blocks }
    : {
        type: "titledCard",
        attrs,
        content: [{ type: "cardTitle", attrs: { color: colors[index % 4] }, content: [{ type: "text", text: `Card ${index}` }] }, ...blocks],
      }
}
const cards = { type: "doc", content: Array.from({ length: 50 }, (_, index) => card(index)) }
// In card 25, types into a paragraph, bolds a word, splits the paragraph and types into the new
// one, then recolors the card's title and makes the card public.
write("cards", cardsSchema, cards, (tr) => {
  let cardAt = 0
  for (let index = 0; index < 25; index++) cardAt += tr.doc.child(index).nodeSize
  // Each card has three paragraphs.
  let at = textStart(tr.doc, "paragraph", 25 * 3) + 12
  for (const char of "abcd") type(tr, char, at++)
  tr.addMark(at + 30, at + 34, cardsSchema.marks.bold.create())
  tr.split(at + 20)
  at += 22
  for (const char of "ef") type(tr, char, at++)
  tr.setNodeAttribute(cardAt + 1, "color", "blue")
  tr.setNodeAttribute(cardAt, "isPublic", true)
})

// The card stack TARNISH_BENCH_DOC names, whose changes must each apply to its document.
if (process.env.TARNISH_BENCH_DOC) {
  const path = process.env.TARNISH_BENCH_DOC
  const file = JSON.parse(readFileSync(path, "utf8"))
  const given = process.env.TARNISH_BENCH_SCHEMA
  const spec = file.schema ?? (given && existsSync(given) ? JSON.parse(readFileSync(given, "utf8")) : undefined)
  if (!spec) throw new Error(`${path} holds no schema, and TARNISH_BENCH_SCHEMA names no spec`)
  const schema = fromData(spec)
  const node = Node.fromJSON(schema, file.doc)
  for (const { steps } of file.changes) {
    const tr = new Transform(node)
    for (const json of steps) tr.step(Step.fromJSON(schema, json))
  }
  const written = { schema: spec, doc: file.doc, changes: file.changes, scenarios: scenarios(schema, node) }
  writeFileSync(`${out}given.json`, JSON.stringify(written))
}
