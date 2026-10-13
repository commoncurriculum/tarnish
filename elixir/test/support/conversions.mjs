// The conversions of the application tarnish's tests make: HTML to and from ProseMirror's JSON on a
// schema of paragraphs of text in linkedom, and JSON as "Markdown": `JSON.stringify` of a document,
// and `JSON.parse` of a text, which show a test a value as the conversion read it or made it. A
// paragraph's `data` holds any JSON, written in its `data-json` attribute. test_native/ makes them
// in Rust.
import { parseHTML as parseWindow } from "linkedom"
import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model"

// A lone surrogate, which the pool reads as U+FFFD, and a backslash before an emoji and before
// text like a surrogate's escape, which it keeps.
const NO_MARKDOWN = "No Markdown: \ud83d\\😀\\ud800"

function parsed(text, message) {
  try {
    return JSON.parse(text)
  } catch {
    throw new Error(message)
  }
}

const schema = new Schema({
  nodes: {
    doc: { content: "paragraph+" },
    paragraph: {
      content: "text*",
      attrs: { data: { default: null } },
      parseDOM: [
        {
          tag: "p",
          getAttrs: (dom) => ({
            data: parsed(dom.getAttribute("data-json") ?? "null", "data-json holds no JSON"),
          }),
        },
      ],
      toDOM: ({ attrs: { data } }) =>
        data === null ? ["p", 0] : ["p", { "data-json": JSON.stringify(data) }, 0],
    },
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

function refuseOptions(options) {
  if (options !== undefined) throw new Error("The HTML conversions take no options")
}

export function parseMarkdown(markdown) {
  return parsed(markdown, NO_MARKDOWN)
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
