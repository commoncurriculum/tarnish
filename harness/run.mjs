// Runs ProseMirror's own test files, unedited, from the pinned submodules, against the target
// TARNISH_TARGET names: `js` for the real packages, `rust` for tarnish.
//
// Against the real packages, prosemirror-transform's test file also records every transform it
// checks with steps to fixtures/transform.json, through its own EMIT_JSON option. The Elixir and
// C tests read that file, so their expected outputs all come from JavaScript.
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const target = process.env.TARNISH_TARGET
if (target !== "js" && target !== "rust") {
  console.error("Set TARNISH_TARGET to js or rust")
  process.exit(2)
}
const root = fileURLToPath(new URL("../", import.meta.url))
const suites = (process.argv[2] ? process.argv.slice(2) : ["prosemirror-model", "prosemirror-transform"])
let failed = false
for (const suite of suites) {
  console.log(`\n== ${suite} (${target})`)
  const records = target === "js" && suite === "prosemirror-transform"
  const result = spawnSync(
    "npx",
    // A skipped or focused test fails the run, so a pass means every test ran.
    ["mocha", "--reporter", "dot", "--forbid-only", "--forbid-pending", `upstream/${suite}/test/test-*.ts`],
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
