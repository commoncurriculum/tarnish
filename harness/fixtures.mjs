// Records fixtures/transform.json from the real prosemirror-transform: its own test file
// trans.ts writes every transform the suite makes to the file EMIT_JSON names. The Elixir and
// C tests read it, so their expected outputs all come from JavaScript.
import { spawnSync } from "node:child_process"
import { mkdirSync } from "node:fs"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../", import.meta.url))
mkdirSync(`${root}fixtures`, { recursive: true })
const result = spawnSync(
  "npx",
  ["mocha", "--reporter", "dot", "--forbid-only", "--forbid-pending", "upstream/prosemirror-transform/test/test-*.ts"],
  {
    cwd: root,
    stdio: "inherit",
    env: {
      ...process.env,
      TARNISH_TARGET: "js",
      EMIT_JSON: `${root}fixtures/transform.json`,
      NODE_OPTIONS: "--import tsx --import ./harness/register.mjs",
    },
  },
)
process.exit(result.status ?? 1)
