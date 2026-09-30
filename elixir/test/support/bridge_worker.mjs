// A worker for Tarnish.Bridge: ProseMirror's DOMParser and DOMSerializer on the basic schema, in
// linkedom, answering the bridge's line protocol. It converts HTML only.
import { parseHTML } from "linkedom"
import { DOMParser, DOMSerializer, Node } from "prosemirror-model"
import { schema } from "prosemirror-schema-basic"
import { createInterface } from "node:readline"

const { document } = parseHTML("<!doctype html><html><body></body></html>")
const parser = DOMParser.fromSchema(schema)
const serializer = DOMSerializer.fromSchema(schema)

function convert({ operation, input }) {
  const element = document.createElement("div")
  switch (operation) {
    case "parseHTML":
      element.innerHTML = input
      return parser.parse(element).toJSON()
    case "serializeHTML":
      element.appendChild(serializer.serializeFragment(Node.fromJSON(schema, input).content, { document }))
      return element.innerHTML
    default:
      throw new Error(`Unknown operation ${operation}`)
  }
}

// A frame past the limit isn't read, so its answer has no id.
const MAX_FRAME_BYTES = 4096

process.stdout.write(`${JSON.stringify({ ready: true })}\n`)
for await (const line of createInterface({ input: process.stdin })) {
  if (Buffer.byteLength(line) > MAX_FRAME_BYTES) {
    process.stdout.write(`${JSON.stringify({ id: null, error: "Request exceeds 4096 bytes" })}\n`)
    continue
  }
  const { id, ...request } = JSON.parse(line)
  let response
  try {
    response = { id, result: convert(request) }
  } catch (error) {
    response = { id, error: error.message }
  }
  process.stdout.write(`${JSON.stringify(response)}\n`)
}
