// Builds tarnish-css to WebAssembly and writes it into a checkout of the linkedom fork, as
// the engine behind its `element.style`:
//
//   node harness/css-wasm.mjs ../linkedom
//
// Needs the wasm32-unknown-unknown target and wasm-bindgen-cli of the version the crate pins.

import { execFileSync } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

const root = new URL("..", import.meta.url).pathname
const fork = process.argv[2]
if (!fork) throw new Error("usage: node harness/css-wasm.mjs <linkedom checkout>")

const run = (command, args) => execFileSync(command, args, { cwd: root, encoding: "utf8" })

const pinned = readFileSync(join(root, "crates/tarnish-css-wasm/Cargo.toml"), "utf8").match(
  /wasm-bindgen = "=([^"]+)"/,
)[1]
const installed = run("wasm-bindgen", ["--version"]).trim()
if (installed !== `wasm-bindgen ${pinned}`)
  throw new Error(`the crate pins wasm-bindgen ${pinned}, but ${installed} is installed`)

run("cargo", ["build", "-p", "tarnish-css-wasm", "--target", "wasm32-unknown-unknown", "--profile", "wasm"])
const out = mkdtempSync(join(tmpdir(), "tarnish-css-"))
try {
  run("wasm-bindgen", [
    "--target",
    "web",
    "--weak-refs",
    "--out-dir",
    out,
    join(root, "target/wasm32-unknown-unknown/wasm/tarnish_css_wasm.wasm"),
  ])
  // The glue's default export fetches the module from a URL; the fork instantiates it from
  // the bytes with initSync, and ascjs would make a default export the whole CommonJS module.
  const ending = "export { initSync, __wbg_init as default };\n"
  const glue = readFileSync(join(out, "tarnish_css_wasm.js"), "utf8")
  if (!glue.endsWith(ending)) throw new Error(`the glue no longer ends with ${JSON.stringify(ending)}`)
  const bytes = readFileSync(join(out, "tarnish_css_wasm_bg.wasm")).toString("base64")

  const sources = ["crates/tarnish-css", "crates/tarnish-css-wasm", "Cargo.lock"]
  const commit = run("git", ["log", "-1", "--format=%h", "--", ...sources]).trim()
  const dirty = run("git", ["status", "--porcelain", "--", ...sources]).trim() ? "-dirty" : ""
  const header =
    `// Generated from tarnish-css (tarnish ${commit}${dirty}) by tarnish's harness/css-wasm.mjs: do not edit.\n` +
    "// tarnish-css is stylo, Servo's CSS engine (MPL-2.0), compiled to WebAssembly.\n"
  writeFileSync(
    join(fork, "esm/shared/css/engine.js"),
    header + glue.slice(0, -ending.length) + "export { initSync };\n",
  )
  writeFileSync(join(fork, "esm/shared/css/wasm.js"), `${header}export default ${JSON.stringify(bytes)};\n`)
} finally {
  rmSync(out, { recursive: true, force: true })
}
