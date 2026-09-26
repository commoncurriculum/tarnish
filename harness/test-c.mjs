// Builds the C library, then compiles crates/tarnish-c/tests/fixtures.c against its header and
// runs it over the transforms in fixtures/transform.json and fixtures/model.json. The C program
// can't read JSON, so this writes each fixture as lines of JSON.stringify output: the text the
// library must give back, byte for byte.
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
