// Writes target/bench/document.json: a 200-paragraph document in prosemirror-schema-basic, each
// paragraph with bold text and a link, and ten steps that each type one character near the
// middle, as an editor sends them.
import { mkdirSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const paragraph = (index) => ({
  type: "paragraph",
  content: [
    { type: "text", text: `Paragraph ${index}: the quick brown fox jumps over the lazy dog, ` },
    { type: "text", marks: [{ type: "strong" }], text: "twice as fast" },
    { type: "text", text: " as the one before it, and " },
    {
      type: "text",
      marks: [{ type: "link", attrs: { href: `https://example.com/${index}`, title: null } }],
      text: "a link",
    },
    { type: "text", text: " to end on." },
  ],
})
const doc = { type: "doc", content: Array.from({ length: 200 }, (_, index) => paragraph(index)) }
const steps = Array.from({ length: 10 }, (_, index) => ({
  stepType: "replace",
  from: 5000 + index,
  to: 5000 + index,
  slice: { content: [{ type: "text", text: "x" }] },
}))
const out = fileURLToPath(new URL("../target/bench/", import.meta.url))
mkdirSync(out, { recursive: true })
writeFileSync(`${out}document.json`, JSON.stringify({ doc, steps }))
