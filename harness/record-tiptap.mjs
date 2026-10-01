// Records what @tiptap/markdown's MarkdownManager and ProseMirror's DOMParser and DOMSerializer do
// with Tiptap's own extensions, to fixtures/tiptap.json, for tarnish-tiptap's tests to expect:
// - Each Markdown input records the document the manager parses it to.
// - Each document records the Markdown the manager serializes it to. Some hold text whose `text`
//   or `marks` is something other than what the manager calls a method of, so that they record
//   V8's error for each.
// - Each HTML input records the document DOMParser parses the body of a linkedom document
//   holding it to.
// - Each document records the innerHTML of an element DOMSerializer fills in linkedom.
// The version of @tiptap/markdown is recorded too, for the tests to check that tarnish-tiptap
// ports the one installed.
import { getSchema } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Highlight } from "@tiptap/extension-highlight"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Text } from "@tiptap/extension-text"
import { TextStyle } from "@tiptap/extension-text-style"
import { Underline } from "@tiptap/extension-underline"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { installed, outcome, writeFixture } from "./fixture.mjs"

const extensions = [
  Document,
  Paragraph,
  Text,
  HardBreak,
  Bold,
  Italic,
  Strike,
  Underline,
  Subscript,
  Superscript,
  Highlight,
  TextStyle,
]
const marked = new Marked()
marked.use(moreLists())
const manager = new MarkdownManager({ extensions, marked })
const schema = getSchema(extensions)
const parser = DOMParser.fromSchema(schema)
const serializer = DOMSerializer.fromSchema(schema)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")

const withMarks = (text, ...marks) => ({ type: "text", text, marks: marks.map(type => ({ type })) })
// tarnish-tiptap's Underline and Highlight leave out their Markdown, which an application gives.
const markTypes = markdown => [
  "bold",
  "italic",
  "strike",
  ...(markdown ? [] : ["underline", "highlight"]),
  "subscript",
  "superscript",
  "textStyle",
]
const docs = markdown => [
  { type: "doc", content: [{ type: "paragraph" }] },
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "plain " },
          ...markTypes(markdown).map(type => withMarks(type, type)),
          { type: "hardBreak" },
          withMarks("both", "bold", "italic"),
        ],
      },
      { type: "paragraph", content: [withMarks("a ", "bold"), withMarks("b", "bold", "italic"), withMarks(" c", "italic")] },
      { type: "paragraph", content: [{ type: "text", text: "*not em* <b>not bold</b> & 1. two" }] },
    ],
  },
]

const text = node => ({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", ...node }] }] })
const values = [1, 0, 1.5, "x", 'a"b', "", true, false, null, {}, [], [1], { some: 1 }]
const serializesMarkdown = [
  ...docs(true),
  ...values.flatMap(value => [
    text({ text: "a", marks: { some: value } }),
    text({ text: { replace: value } }),
    text({ text: "a", marks: [{ type: { localeCompare: value } }, { type: "unknown" }] }),
    text({ text: "a", marks: [{ type: "unknown" }, { type: { localeCompare: value } }] }),
  ]),
  text({ text: "a", marks: { other: 1 } }),
  text({ text: { other: 1 } }),
  text({ text: [1] }),
  text({ text: 5 }),
].map(doc => ({ doc, ...outcome(() => ({ markdown: manager.serialize(doc) })) }))

const parsesMarkdown = [
  "",
  "plain",
  "**bold** *italic* ~~strike~~ `code`",
  "***both*** **a *b* c**",
  "one  \ntwo\n\nthree",
  "&nbsp;",
  "# heading\n\n- list\n\n> quote",
].map(markdown => ({ markdown, ...outcome(() => ({ doc: manager.parse(markdown) })) }))

const parsesHTML = [
  "<p>plain</p>",
  "<p><strong>b</strong><b>b</b><em>i</em><i>i</i><s>s</s><del>d</del><u>u</u><sub>x</sub><sup>y</sup><mark>h</mark></p>",
  "<p><span style='color: red'>c</span><span>none</span><br>z</p>",
  "<p><b style='font-weight: normal'>not bold</b><i style='font-style: normal'>not italic</i></p>",
  "<p><span style='font-weight: bold; font-style: italic; text-decoration: underline line-through'>styled</span></p>",
  "<h1>heading</h1><ul><li>item</li></ul>",
  "loose text",
].map(html => ({
  html,
  ...outcome(() => {
    const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
    return { doc: parser.parse(document.body).toJSON() }
  }),
}))

const serializesHTML = [
  ...docs(false),
  // `doc` has no toDOM, so the serializer has nothing to call for a doc inside the doc.
  { type: "doc", content: [{ type: "doc" }] },
].map(doc => ({
  doc,
  ...outcome(() => {
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

writeFixture("tiptap", {
  tiptap: installed("@tiptap/markdown"),
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
