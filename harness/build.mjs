// Builds the native bridge and puts it where crates/tarnish-node/js loads it from.
import { execFileSync } from "node:child_process"
import { copyFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../", import.meta.url))
execFileSync("cargo", ["build", "-p", "tarnish-node"], { cwd: root, stdio: "inherit" })
const library = { darwin: "libtarnish_node.dylib", win32: "tarnish_node.dll" }[process.platform] ?? "libtarnish_node.so"
copyFileSync(`${root}target/debug/${library}`, `${root}crates/tarnish-node/tarnish.node`)
