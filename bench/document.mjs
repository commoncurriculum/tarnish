// Writes target/bench/document.json: a 200-paragraph document in prosemirror-schema-basic, each
// paragraph with bold text and a link, and ten steps that each type one character near the
// middle, as an editor sends them.
//
// Then writes two more, each `{schema, doc, steps}` with the schema's spec as JSON, as
// `new Schema(spec)` and `Tarnish.Schema.new` read it:
// - target/bench/nested.json: most of the content in one long bullet list and one table, in
//   prosemirror-schema-basic with prosemirror-schema-list and prosemirror-tables' nodes;
// - target/bench/cards.json: a card stack, 50 cards that carry ids, comment ids and link
//   metadata, in a schema of the same node types as a card stack's.
// Their steps are what ProseMirror's Transform makes for an editor's edits near the middle.
import { mkdirSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Node, Schema } from "prosemirror-model"
import { schema as basic } from "prosemirror-schema-basic"
import { addListNodes } from "prosemirror-schema-list"
import { tableNodes } from "prosemirror-tables"
import { Transform } from "prosemirror-transform"

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
writeFileSync(`${out}document.json`, JSON.stringify({ doc, steps }))

// A schema's spec as JSON, which drops its functions: toDOM, getAttrs and the like.
const specOf = (schema) => ({
  topNode: schema.topNodeType.name,
  nodes: schema.spec.nodes.toObject(),
  marks: schema.spec.marks.toObject(),
})
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
  const again = new Schema(JSON.parse(JSON.stringify(spec)))
  const replayed = new Transform(Node.fromJSON(again, json))
  for (const step of tr.steps) replayed.step(step.constructor.fromJSON(again, step.toJSON()))
  if (!replayed.doc.eq(Node.fromJSON(again, tr.doc.toJSON()))) throw new Error(`${name}: steps don't replay`)
  writeFileSync(`${out}${name}.json`, JSON.stringify({ schema: spec, doc: json, steps }))
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
