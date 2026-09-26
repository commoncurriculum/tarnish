// Builds the C library, then compiles crates/tarnish-c/tests/fixtures.c against its header and
// runs it over fixtures/transform.json. The C program can't read JSON, so this writes each
// fixture as lines of JSON.stringify output: the text the library must give back, byte for byte.
import { execFileSync } from "node:child_process"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../", import.meta.url))
const out = `${root}target/c-test/`
mkdirSync(out, { recursive: true })

const { schemas, tests } = JSON.parse(readFileSync(`${root}fixtures/transform.json`, "utf8"))
const lines = [String(schemas.length), ...schemas.map((schema) => JSON.stringify(schema)), String(tests.length)]
for (const test of tests) {
  lines.push(
    String(test.schema),
    JSON.stringify(test.start),
    JSON.stringify(test.steps),
    JSON.stringify(test.result),
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
