// The one runner of the Tiptap recorders, harness/record-tiptap*.mjs, and what they share.
//
// `record(name, configurations, others)` writes fixtures/<name>.json. Each configuration gives
// the extensions of a `getSchema` and of a `MarkdownManager`, whose marked uses
// marked-more-lists, the manager's `indentation` where it isn't the default, and the inputs of
// each section its options reach, which the section records:
// - `parsesMarkdown`: the document the manager parses each Markdown input to;
// - `serializesMarkdown`: the Markdown the manager serializes each document to;
// - `parsesHTML`: the document DOMParser parses the body of a linkedom document holding each
//   input to;
// - `serializesHTML`: the innerHTML of an element DOMSerializer fills with each document's
//   content in linkedom;
// - `generatesText`: the text `generateText` makes of each `{ doc, blockSeparator, serializers }`,
//   `serializers` naming one of `textSerializers`;
// - `generatesUniqueIds`: in `ids`, the document `generateUniqueIds` gives ids.
// A serialization's documents may be a function of the documents its configuration's parses
// make. Each input is recorded once, as its JSON text reads, as the tests read it. A record names
// its configuration in `extensions`, then holds its input, then what the conversion made or the
// error it threw: its class and message. The fixture's `schemas` holds each configuration's
// schema, as harness/fixture.mjs's schemaRecord writes it, and `others` holds the recorder's own
// sections.
import { readFileSync } from "node:fs"
import { generateText, getSchema } from "@tiptap/core"
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
import { generateUniqueIds } from "@tiptap/extension-unique-id"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { outcome, schemaRecord, writeFixture } from "./fixture.mjs"

// Tiptap's own extensions.
export const own = [
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

// The text serializers a record of `generatesText` may name: `places` names each paragraph and
// chip and where it is.
export const textSerializers = {
  places: {
    paragraph: ({ node, pos, index }) => `[${node.type.name} ${pos} ${index}]`,
    chip: ({ node }) => `(${node.attrs.label})`,
  },
}

export const unique = values => [...new Set(values)]
export const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))

// The inputs html5ever builds another tree from than the fork's parse5 does, which tarnish-html's
// tests/recorded.rs holds to html5ever's tree.
const otherTrees = new Set([
  "<select><option>a<b>x</b></option></select>",
  "<select><option>a<p>b</p></select>",
  "<p><span>a<isindex>b</span>c</p>",
  "<math><mi><![CDATA[x<y]]></mi></math>",
  "<math><mi><![CDATA[x]]></mi></math>",
  "<svg><desc><![CDATA[x]]></desc></svg>",
  "<svg><foreignObject><![CDATA[x]]></foreignObject></svg>",
  "<math><annotation-xml encoding=text/html><![CDATA[x]]></annotation-xml></math>",
])
// fixtures/marked.json's Markdown inputs that `pattern` matches.
export const markedInputs = (pattern = /^/) =>
  fixture("marked")
    .lexed.map(record => record.input)
    .filter(markdown => pattern.test(markdown))
// fixtures/dom.json's HTML inputs that `pattern` matches, but those `otherTrees` holds.
export const domInputs = (pattern = /^/) =>
  fixture("dom")
    .parses.map(record => record.html)
    .filter(html => !otherTrees.has(html) && pattern.test(html))

// The documents given, then those the configuration's parses make.
export const withParsed = docs => parsed => [...docs, ...parsed]

// The JSON of a document, of a paragraph, and of text with marks, each a type's name or a mark.
export const doc = (...content) => ({ type: "doc", content })
export const paragraph = (...content) => ({ type: "paragraph", content })
export const text = (text, ...marks) => ({
  type: "text",
  text,
  ...(marks.length ? { marks: marks.map(mark => (typeof mark === "string" ? { type: mark } : mark)) } : {}),
})

// RubyText's parse rule makes its content element with the page's global `document`, which is
// linkedom's here, as a browser's would be its own.
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
globalThis.document = document
export const body = html => parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`).document.body

function converter({ extensions, indentation }) {
  const marked = new Marked()
  marked.use(moreLists())
  const schema = getSchema(extensions)
  const manager = new MarkdownManager({ extensions, marked, ...(indentation ? { indentation } : {}) })
  return { extensions, schema, manager, parser: DOMParser.fromSchema(schema), serializer: DOMSerializer.fromSchema(schema) }
}

function serialize({ schema, serializer }, doc) {
  const container = document.createElement("div")
  serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
  return { html: container.innerHTML }
}

// The HTML DOMSerializer makes of each of the documents that `extensions` serialize.
export function rendered(extensions, docs) {
  const made = converter({ extensions })
  return docs.flatMap(doc => outcome(() => serialize(made, doc)).html || [])
}

// What each section makes of an input, with a configuration's converter.
const sections = {
  parsesMarkdown: ({ manager }, markdown) => ({ markdown, ...outcome(() => ({ doc: manager.parse(markdown) })) }),
  serializesMarkdown: ({ manager }, doc) => ({ doc, ...outcome(() => ({ markdown: manager.serialize(doc) })) }),
  parsesHTML: ({ parser }, html) => ({ html, ...outcome(() => ({ doc: parser.parse(body(html)).toJSON() })) }),
  serializesHTML: (made, doc) => ({ doc, ...outcome(() => serialize(made, doc)) }),
  generatesText: ({ extensions }, input) => {
    const { doc, blockSeparator, serializers } = input
    const options = { blockSeparator, textSerializers: textSerializers[serializers] }
    return { ...input, ...outcome(() => ({ text: generateText(doc, extensions, options) })) }
  },
  generatesUniqueIds: ({ extensions }, doc) => ({ doc, ...outcome(() => ({ ids: generateUniqueIds(doc, extensions) })) }),
}

export function record(name, configurations, others = {}) {
  const made = Object.fromEntries(Object.entries(configurations).map(([name, given]) => [name, converter(given)]))
  const records = (section, inputs) =>
    Object.entries(configurations).flatMap(([name, given]) =>
      unique((inputs(given, name) ?? []).map(input => JSON.stringify(input))).map(input => ({
        extensions: name,
        ...sections[section](made[name], JSON.parse(input)),
      })),
    )
  const parsesMarkdown = records("parsesMarkdown", given => given.parsesMarkdown)
  const parsesHTML = records("parsesHTML", given => given.parsesHTML)
  const parsed = name => [...parsesMarkdown, ...parsesHTML].filter(record => record.extensions === name && record.doc)
  const documents = section => (given, name) =>
    typeof given[section] === "function" ? given[section](parsed(name).map(record => record.doc)) : given[section]
  const recorded = {
    schemas: Object.entries(made).map(([name, { schema }]) => ({ extensions: name, ...schemaRecord(schema) })),
    parsesMarkdown,
    serializesMarkdown: records("serializesMarkdown", documents("serializesMarkdown")),
    parsesHTML,
    serializesHTML: records("serializesHTML", documents("serializesHTML")),
    generatesText: records("generatesText", given => given.generatesText),
    generatesUniqueIds: records("generatesUniqueIds", given => given.generatesUniqueIds),
    ...others,
  }
  writeFixture(name, Object.fromEntries(Object.entries(recorded).filter(([, records]) => records.length > 0)))
}
