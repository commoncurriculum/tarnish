// A Tarnish.Bridge worker: `node worker.mjs <conversions>` answers the pool's requests with the
// four conversions the module at <conversions> exports, each called with the request's input and
// options. The protocol is Tarnish.Bridge.Pool's.
import { once } from "node:events"
import { createInterface } from "node:readline"
import { pathToFileURL } from "node:url"

const INVALID = "Unknown operation or invalid input"

const conversions = await import(pathToFileURL(process.argv[2]).href)

function accepts(operation, input) {
  switch (operation) {
    case "parseMarkdown":
    case "parseHTML":
      return typeof input === "string"
    case "serializeMarkdown":
    case "serializeHTML":
      return typeof input === "object" && input !== null && !Array.isArray(input)
    default:
      return false
  }
}

function message(error) {
  return error instanceof Error ? error.message : String(error)
}

// The response's JSON. A result JSON can't hold, such as a BigInt, is answered with the error
// JSON.stringify throws for it.
async function answer(line) {
  let request
  try {
    request = JSON.parse(line)
  } catch (error) {
    return JSON.stringify({ id: null, error: message(error) })
  }
  const { id = null, operation, input, options } = request ?? {}
  if (!accepts(operation, input)) return JSON.stringify({ id, error: INVALID })
  try {
    return JSON.stringify({ id, result: await conversions[operation](input, options) })
  } catch (error) {
    return JSON.stringify({ id, error: message(error) })
  }
}

async function write(json) {
  if (!process.stdout.write(`${json}\n`)) await once(process.stdout, "drain")
}

await write(JSON.stringify({ ready: true }))
for await (const line of createInterface({ input: process.stdin })) {
  await write(await answer(line))
}
