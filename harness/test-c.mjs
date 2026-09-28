// Builds the C library, then compiles crates/tarnish-c/tests/fixtures.c against its header and
// runs it over the transforms in fixtures/transform.json and fixtures/model.json, and the op
// lists and text in fixtures/ops.json. The C program can't read JSON, so this writes each
// fixture as lines of JSON.stringify output: the text the library must give back, byte for byte.
import { execFileSync } from "node:child_process"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../", import.meta.url))
const out = `${root}target/c-test/`
mkdirSync(out, { recursive: true })

const read = (name) => JSON.parse(readFileSync(`${root}fixtures/${name}.json`, "utf8"))
const transform = read("transform")
const model = read("model")
const schemas = [...transform.schemas, ...model.schemas]
// model.json records the result of its transforms as text already.
const tests = [
  ...transform.tests.map((test) => ({ ...test, result: JSON.stringify(test.result) })),
  ...model.transforms.map((test) => ({ ...test, schema: test.schema + transform.schemas.length, mapping: [] })),
]
const lines = [String(schemas.length), ...schemas.map((schema) => JSON.stringify(schema)), String(tests.length)]
for (const test of tests) {
  lines.push(
    String(test.schema),
    JSON.stringify(test.start),
    JSON.stringify(test.steps),
    test.result,
    test.mapping.flat().join(" "),
  )
}

// Text is written as the hex of its UTF-8, since it may hold line breaks, "-" being text not
// given. UTF-8 has U+FFFD for a lone surrogate, as the library gives it.
const ops = read("ops")
const hex = (text) => (text == null ? "-" : Buffer.from(text, "utf8").toString("hex"))
const text = (record) => hex(record.resultJSON ? JSON.parse(record.resultJSON) : record.result)
const outcome = (record, ...given) =>
  record.error ? ["error", `${record.error.class}: ${record.error.message}`] : ["ok", ...given]
lines.push(String(ops.schemas.length), ...ops.schemas.map((schema) => JSON.stringify(schema)))
lines.push(String(ops.transforms.length))
for (const record of ops.transforms) {
  lines.push(
    String(record.schema),
    JSON.stringify(record.doc),
    JSON.stringify(record.ops),
    ...outcome(record, JSON.stringify(record.result), JSON.stringify(record.steps)),
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
lines.push(String(ops.textContent.length))
for (const record of ops.textContent) {
  lines.push(String(record.schema), JSON.stringify(record.doc), text(record))
}
if (lines.some((line) => line.includes("\n"))) throw new Error("A fixture's line holds a line break")
writeFileSync(`${out}fixtures.txt`, lines.join("\n") + "\n")

execFileSync("cargo", ["build", "-p", "tarnish-c"], { cwd: root, stdio: "inherit" })
const library = `${root}target/debug`
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
