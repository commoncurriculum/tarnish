// Runs ProseMirror's suites and test/ against TARNISH_TARGET: `js` for the real packages, which
// also records fixtures/transform.json, or `rust` for tarnish. The README's "How it's proven"
// says why.
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const target = process.env.TARNISH_TARGET
if (target !== "js" && target !== "rust") {
  console.error("Set TARNISH_TARGET to js or rust")
  process.exit(2)
}
const root = fileURLToPath(new URL("../", import.meta.url))
const directories = {
  "prosemirror-model": "upstream/prosemirror-model/test",
  "prosemirror-transform": "upstream/prosemirror-transform/test",
  tarnish: "test",
}
const suites = process.argv[2] ? process.argv.slice(2) : Object.keys(directories)
let failed = false
for (const suite of suites) {
  console.log(`\n== ${suite} (${target})`)
  const records = target === "js" && suite === "prosemirror-transform"
  const result = spawnSync(
    "npx",
    // A skipped or focused test fails the run, so a pass means every test ran.
    ["mocha", "--reporter", "dot", "--forbid-only", "--forbid-pending", `${directories[suite]}/test-*.ts`],
    {
      cwd: root,
      stdio: "inherit",
      env: {
        ...process.env,
        ...(records ? { EMIT_JSON: `${root}fixtures/transform.json` } : {}),
        NODE_OPTIONS: "--import tsx --import ./harness/register.mjs",
      },
    },
  )
  failed ||= result.status !== 0
}
process.exit(failed ? 1 : 0)
