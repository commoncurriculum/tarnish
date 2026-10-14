// Records what Tiptap's inline extensions, and the extensions that give other types attributes,
// make of documents, to fixtures/tiptap-inline.json, for tarnish-tiptap's tests/inline.rs to
// expect. Each record names the configuration of extensions it's of in `extensions`:
// - `schemas` records each configuration's node and mark types as ProseMirror reads their specs.
// - `parsesMarkdown`, `serializesMarkdown`, `parsesHTML` and `serializesHTML` record what
//   harness/record-tiptap.mjs records, of each configuration.
import { readFileSync } from "node:fs"
import { Extension, Mark, Node as TiptapNode, getSchema, splitExtensions } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Text } from "@tiptap/extension-text"
import { TextStyle } from "@tiptap/extension-text-style"
import { Underline } from "@tiptap/extension-underline"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { outcome, writeFixture } from "./fixture.mjs"

// Tiptap's own extensions, as harness/record-tiptap.mjs gives them.
const own = [
  Document,
  Paragraph,
  Text,
  Heading,
  Image,
  HardBreak,
  Bold,
  Italic,
  Strike,
  Underline,
  Subscript,
  Superscript,
  Highlight,
  TextStyle,
]

// Global attributes for each kind of `types`: `'*'`, `'nodes'`, `'marks'`, an array that names a
// type twice and one there isn't, and an array an extension makes of the node extensions it's given.
const Globals = Extension.create({
  name: "globals",
  addGlobalAttributes() {
    const { nodeExtensions } = splitExtensions(this.extensions)
    return [
      { types: "*", attributes: { every: { default: "*" } } },
      { types: "nodes", attributes: { node: { default: "n" }, every: { default: "node" } } },
      { types: "marks", attributes: { mark: { default: null } } },
      { types: ["paragraph", "bold", "paragraph", "missing"], attributes: { named: { default: 1 } } },
      {
        types: nodeExtensions.map(extension => extension.name).filter(name => name !== "doc" && name !== "text"),
        attributes: { made: { default: false, rendered: false } },
      },
    ]
  },
})

// A node and marks that set the fields of a spec that only an editor reads.
const Chip = TiptapNode.create({
  name: "chip",
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  draggable: true,
})
const Rule = TiptapNode.create({ name: "rule", group: "block", atom: true, selectable: true })
const Tag = Mark.create({ name: "tag", inclusive: false })
const Free = Mark.create({ name: "free", inclusive: true })

const configurations = {
  own,
  globals: [Document, Paragraph, Text, Heading, HardBreak, Bold, Italic, Globals],
  specs: [Document, Paragraph, Text, Chip, Rule, Tag, Free],
}

const converters = Object.fromEntries(
  Object.entries(configurations).map(([name, extensions]) => {
    const marked = new Marked()
    marked.use(moreLists())
    const schema = getSchema(extensions)
    const parser = DOMParser.fromSchema(schema)
    const serializer = DOMSerializer.fromSchema(schema)
    return [name, { extensions, manager: new MarkdownManager({ extensions, marked }), schema, parser, serializer }]
  }),
)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))
const unique = values => [...new Set(values)]
// The inputs of each configuration in `inputs`, each once.
const each = inputs => Object.entries(inputs).flatMap(([name, list]) => unique(list).map(input => [name, input]))

const schemas = Object.entries(converters).map(([name, { schema }]) => ({
  extensions: name,
  nodes: Object.values(schema.nodes).map(type => [
    type.name,
    {
      atom: type.isAtom,
      inline: type.isInline,
      selectable: type.spec.selectable !== false,
      draggable: !!type.spec.draggable,
      groups: type.groups,
      attrs: type.defaultAttrs,
    },
  ]),
  marks: Object.values(schema.marks).map(type => [
    type.name,
    { inclusive: type.spec.inclusive !== false, attrs: type.instance ? type.instance.attrs : null },
  ]),
}))

const parsesMarkdown = each({
  globals: ["plain **bold**"],
}).map(([name, markdown]) => ({
  extensions: name,
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

const parsesHTML = each({
  globals: [
    "<p>plain</p>",
    '<p every="x" node="1" named="true" made="1.5">a<strong every="" mark="false" named="-2">b</strong></p>',
    '<h2 every node="+.5" named="1e3">x</h2><p named="0x1"><br every="y" node="2."></p>',
  ],
}).map(([name, html]) => ({
  extensions: name,
  html,
  ...outcome(() => {
    const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
    return { doc: converters[name].parser.parse(document.body).toJSON() }
  }),
}))

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it, and those `docs` gives.
const documents = docs =>
  Object.keys(converters).flatMap(name => {
    const parsed = [...parsesMarkdown, ...parsesHTML]
      .filter(record => record.extensions === name && record.doc)
      .map(record => record.doc)
    return unique([...(docs[name] ?? []), ...parsed].map(doc => JSON.stringify(doc))).map(text => [
      name,
      JSON.parse(text),
    ])
  })

const paragraph = (...content) => ({ type: "doc", content: [{ type: "paragraph", content }] })
const docs = {
  globals: [
    {
      type: "doc",
      content: [
        { type: "paragraph", attrs: { every: null, node: 0, named: "", made: "not rendered" }, content: [] },
        { type: "heading", attrs: { level: 1, every: { an: "object" }, node: [1, 2] } },
      ],
    },
    paragraph({ type: "text", text: "x", marks: [{ type: "bold", attrs: { every: 1, mark: "m", named: true } }] }),
  ],
}

const serializesMarkdown = documents(docs).map(([name, doc]) => ({
  extensions: name,
  doc,
  ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })),
}))

const serializesHTML = documents(docs).map(([name, doc]) => ({
  extensions: name,
  doc,
  ...outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

writeFixture("tiptap-inline", {
  schemas,
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
