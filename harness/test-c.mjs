// Builds the C library, then compiles crates/tarnish-c/tests/fixtures.c against its header and
// runs it over the transforms in fixtures/transform.json and fixtures/model.json, the schemas
// and checks in fixtures/model.json, and the op lists and text in fixtures/ops.json. The C
// program can't read JSON, so this writes each fixture as lines of JSON.stringify output: the
// text the library must give back, byte for byte.
import { execFileSync } from "node:child_process"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../", import.meta.url))
const target = process.env.CARGO_TARGET_DIR ?? `${root}target`
const out = `${target}/c-test/`
mkdirSync(out, { recursive: true })

const read = (name) => JSON.parse(readFileSync(`${root}fixtures/${name}.json`, "utf8"))
const transform = read("transform")
const model = read("model")
const lines = []

// The library writes a NUL in a message as `\u0000`.
const outcome = (record, ...given) =>
  record.error
    ? ["error", `${record.error.class}: ${record.error.message.replaceAll("\0", "\\u0000")}`]
    : ["ok", ...given]
// Where every position maps with assoc -1, then 1: "-" for each where the steps aren't mapped.
const positions = (mapped) => (mapped ? mapped.map((assoc) => assoc.pos.join(" ")) : ["-", "-"])

const schemas = [...transform.schemas, ...model.schemas]
lines.push(String(schemas.length), ...schemas.map((schema) => JSON.stringify(schema)))
if (transform.tests.length !== model.transformTests.length) throw new Error("A test without its record")
// model.json records the result of its transforms as text already.
const tests = [
  ...transform.tests.map((test, index) => ({
    ...test,
    ...model.transformTests[index],
    result: JSON.stringify(test.result),
  })),
  ...model.transforms.map((test) => ({ ...test, schema: test.schema + transform.schemas.length })),
]
lines.push(String(tests.length))
for (const test of tests) {
  lines.push(
    String(test.schema),
    JSON.stringify(test.start),
    JSON.stringify(test.steps),
    ...outcome(test, test.result, JSON.stringify(test.inverted), ...positions(test.mapped)),
  )
}
lines.push(String(model.newSchema.length))
for (const record of model.newSchema) lines.push(JSON.stringify(record.spec), ...outcome(record))
lines.push(String(model.check.length))
for (const record of model.check) {
  lines.push(String(record.schema + transform.schemas.length), JSON.stringify(record.json), ...outcome(record))
}

// Text is written as the hex of its UTF-8, since it may hold line breaks, "-" being text not
// given. UTF-8 has U+FFFD for a lone surrogate, as the library gives it.
const ops = read("ops")
const hex = (text) => (text == null ? "-" : Buffer.from(text, "utf8").toString("hex"))
const text = (written) =>
  hex(typeof written === "string" ? written : written.resultJSON ? JSON.parse(written.resultJSON) : written.result)
lines.push(String(ops.schemas.length), ...ops.schemas.map((schema) => JSON.stringify(schema)))
lines.push(String(ops.transforms.length))
for (const record of ops.transforms) {
  lines.push(
    String(record.schema),
    JSON.stringify(record.doc),
    JSON.stringify(record.ops),
    ...outcome(record, JSON.stringify(record.result), JSON.stringify(record.steps), ...positions(record.mapped)),
  )
}
lines.push(String(ops.textBetween.length))
for (const record of ops.textBetween) {
  lines.push(
    String(record.schema),
    JSON.stringify(record.doc),
    `${record.from} ${record.to}`,
    hex(record.blockSeparator),
    hex(record.leafText),
    ...outcome(record, text(record)),
  )
}
lines.push(String(ops.textsBetween.length))
for (const record of ops.textsBetween) {
  lines.push(
    String(record.schema),
    JSON.stringify(record.doc),
    hex(record.blockSeparator),
    hex(record.leafText),
    String(record.texts.length),
    ...record.texts.flat().map(text),
  )
}
lines.push(String(ops.textContent.length))
for (const record of ops.textContent) {
  lines.push(String(record.schema), JSON.stringify(record.doc), text(record))
}
lines.push("end")
if (lines.some((line) => line.includes("\n"))) throw new Error("A fixture's line holds a line break")
writeFileSync(`${out}fixtures.txt`, lines.join("\n") + "\n")

execFileSync("cargo", ["build", "-p", "tarnish-c"], { cwd: root, stdio: "inherit" })
const library = `${target}/debug`
execFileSync(
  process.env.CC ?? "cc",
  [
    "-std=c11",
    "-Wall",
    "-Wextra",
    "-Werror",
    "-I",
    `${root}crates/tarnish-c/include`,
    `${root}crates/tarnish-c/tests/fixtures.c`,
    "-o",
    `${out}fixtures`,
    `-L${library}`,
    `-Wl,-rpath,${library}`,
    "-ltarnish_c",
  ],
  { stdio: "inherit" },
)
execFileSync(`${out}fixtures`, [`${out}fixtures.txt`], { stdio: "inherit" })
