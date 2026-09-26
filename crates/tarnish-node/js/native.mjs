// The native bridge, which harness/build.mjs builds.
import { createRequire } from "node:module"

export default createRequire(import.meta.url)("../tarnish.node")
