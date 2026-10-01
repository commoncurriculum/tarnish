// The conversions of the application tarnish's tests make: HTML to and from ProseMirror's JSON on a
// schema of paragraphs of text in linkedom, a document's JSON as its "Markdown", which shows a test
// the document as the conversion read it, and no Markdown to parse. test_native/ makes them in Rust.
import { parseHTML as parseWindow } from "linkedom"
import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model"

const schema = new Schema({
  nodes: {
    doc: { content: "paragraph+" },
    paragraph: { content: "text*", parseDOM: [{ tag: "p" }], toDOM: () => ["p", 0] },
    text: {},
  },
  marks: {
    em: { parseDOM: [{ tag: "i" }, { tag: "em" }], toDOM: () => ["em", 0] },
    strong: { parseDOM: [{ tag: "b" }, { tag: "strong" }], toDOM: () => ["strong", 0] },
  },
})
const parser = DOMParser.fromSchema(schema)
const serializer = DOMSerializer.fromSchema(schema)
const { document } = parseWindow("")

// A lone surrogate, which the pool reads as U+FFFD, and a backslash before an emoji and before
// text like a surrogate's escape, which it keeps.
const NO_MARKDOWN = "No Markdown: \ud83d\\😀\\ud800"

function refuseOptions(options) {
  if (options !== undefined) throw new Error("The HTML conversions take no options")
}

export function parseMarkdown() {
  throw new Error(NO_MARKDOWN)
}

export function serializeMarkdown(json) {
  return JSON.stringify(json)
}

export function parseHTML(html, options) {
  refuseOptions(options)
  const template = document.createElement("template")
  template.innerHTML = html
  return parser.parse(template.content).toJSON()
}

export function serializeHTML(json, options) {
  refuseOptions(options)
  const { content } = Node.fromJSON(schema, json)
  const element = document.createElement("div")
  element.appendChild(serializer.serializeFragment(content, { document }))
  return element.innerHTML
}
