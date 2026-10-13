// Records what @tiptap/markdown's MarkdownManager and ProseMirror's DOMParser and DOMSerializer do
// with Tiptap's extensions, to fixtures/tiptap.json, for tarnish-tiptap's tests to expect:
// - Each Markdown input records the document the manager parses it to: fixtures/marked.json's,
//   and Tiptap's own syntax.
// - Each document records the Markdown the manager serializes it to: the documents the parses
//   make, and some that hold text whose `text` or `marks` is something other than what the manager
//   calls a method of, so that they record V8's error for each.
// - Each HTML input records the document DOMParser parses the body of a linkedom document holding
//   it to: fixtures/dom.json's, and Tiptap's own elements and styles.
// - Each document records the innerHTML of an element DOMSerializer fills in linkedom.
// A record is of Tiptap's own extensions, or names the others it's of in `extensions`: `colors`,
// with Highlight's colors and Color's, or `code`, an application's code mark and code block, and
// a figure whose attribute renders under a name it makes.
import { readFileSync } from "node:fs"
import { Mark, Node as TiptapNode, getSchema } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Text } from "@tiptap/extension-text"
import { Color, TextStyle } from "@tiptap/extension-text-style"
import { Underline } from "@tiptap/extension-underline"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { outcome, writeFixture } from "./fixture.mjs"

const own = [
  Document,
  Paragraph,
  Text,
  Heading,
  Image,
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
const Code = Mark.create({
  name: "code",
  code: true,
  renderMarkdown: (node, helpers) => "`" + helpers.renderChildren(node) + "`",
})
const CodeBlock = TiptapNode.create({
  name: "codeBlock",
  code: true,
  group: "block",
  content: "text*",
  parseHTML: () => [{ tag: "pre" }],
  renderMarkdown: (node, helpers) => "```\n" + helpers.renderChildren(node) + "\n```",
})
const Figure = TiptapNode.create({
  name: "figure",
  group: "block",
  parseHTML: () => [{ tag: "figure" }],
  renderHTML: ({ HTMLAttributes }) => ["figure", HTMLAttributes],
  addAttributes: () => ({ made: { default: null, renderHTML: attributes => ({ "data-made": attributes.made }) } }),
})
const configurations = {
  own,
  colors: [...own.filter(extension => extension !== Highlight), Highlight.configure({ multicolor: true }), Color],
  code: [Document, Paragraph, Text, Code, CodeBlock, Figure],
}
const converters = Object.fromEntries(
  Object.entries(configurations).map(([name, extensions]) => {
    const marked = new Marked()
    marked.use(moreLists())
    const schema = getSchema(extensions)
    const parser = DOMParser.fromSchema(schema)
    const serializer = DOMSerializer.fromSchema(schema)
    return [name, { manager: new MarkdownManager({ extensions, marked }), schema, parser, serializer }]
  }),
)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))
const named = name => (name === "own" ? {} : { extensions: name })
const unique = values => [...new Set(values)]

// `type` inside itself, `depth` deep, around a text node.
const nested = (type, depth) => {
  let node = { type: "text", text: "x" }
  for (let level = 0; level < depth; level++) node = { type, content: [node] }
  return { type: "doc", content: [node] }
}
// Nesting one to three deep, which tests/deep.rs folds into the conversions of any depth.
const depths = [1, 2, 3]
const delimited = (open, close, depth) => open.repeat(depth) + "x" + close.repeat(depth)
const deepMarkdown = depths.flatMap(depth => [
  ">".repeat(4 * depth) + " x",
  "> ".repeat(depth) + "x\ny",
  "1. ".repeat(depth) + "x",
  ...[["**a ", " a**"], ["*a ", " a*"], ["~~a ", " a~~"]].map(
    ([open, close]) => ">".repeat(4 * depth) + " " + delimited(open, close, depth),
  ),
  "# " + delimited("*a ", " a*", depth) + "\n\ny",
])
const deepHTML = depths.flatMap(depth => ["div", "p", "b", "span style=color:red"].map(tag => `<${tag}>`.repeat(depth) + "x"))
const deepDocs = depths.flatMap(depth => ["doc", "paragraph", "bold", "italic", "strike"].map(type => nested(type, depth)))

const parsesMarkdown = [
  ...unique([
    "",
    "plain",
    "**bold** *italic* ~~strike~~ `code`",
    "***both*** **a *b* c**",
    "one  \ntwo\n\nthree",
    "&nbsp;",
    "# heading\n\n- list\n\n> quote",
    "### three **bold**\n\nSetext\n===",
    "++under++ ==high== ++==both==++",
    '![alt](a.png "title")\n\ntext ![](b.png) more',
    ...fixture("marked").lexed.map(record => record.input),
    ...deepMarkdown,
  ]).map(markdown => ["own", markdown]),
  ...["==high==", "==a **b** c==", "a ==b== ++==c==++", '<span style="color: red">x</span>'].map(markdown => [
    "colors",
    markdown,
  ]),
  ...["`*a* <b>` \\*c\\*\n\n```\n*d* <e>\n```", "`code` *em*", "```\nblock\n```"].map(markdown => ["code", markdown]),
].map(([name, markdown]) => ({
  ...named(name),
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

// The inputs html5ever builds another tree from than the fork's parse5 does, which tarnish-html's
// tests/recorded.rs holds to html5ever's tree.
const otherTrees = new Set([
  "<select><option>a<b>x</b></option></select>",
  "<select><option>a<p>b</p></select>",
  "<p><span>a<isindex>b</span>c</p>",
  "<math><mi><![CDATA[x<y]]></mi></math>",
  "<math><mi><![CDATA[x]]></mi></math>",
  "<svg><desc><![CDATA[x]]></desc></svg>",
  "<svg><foreignObject><![CDATA[x]]></foreignObject></svg>",
  "<math><annotation-xml encoding=text/html><![CDATA[x]]></annotation-xml></math>",
])
// A span without a color, `depth` deep in elements that aren't spans inside a span with one.
const uncolored = depth =>
  `<p><span style="color: red">${"<b>".repeat(depth)}<span style="font-size: 9px">x</span>${"</b>".repeat(depth)}</span></p>`
const parsesHTML = [
  ...unique([
    "<p>plain</p>",
    "<p><strong>b</strong><b>b</b><em>i</em><i>i</i><s>s</s><del>d</del><u>u</u><sub>x</sub><sup>y</sup><mark>h</mark></p>",
    "<p><span style='color: red'>c</span><span>none</span><br>z</p>",
    "<p><b style='font-weight: normal'>not bold</b><i style='font-style: normal'>not italic</i></p>",
    "<p><span style='font-weight: bold; font-style: italic; text-decoration: underline line-through'>styled</span></p>",
    "<h1>heading</h1><ul><li>item</li></ul>",
    "<h3>three</h3><h6>six</h6>",
    "<img src='a.png' alt='A' title='T' width='10' height='5'><img alt='no src'><img src='data:image/png;base64,AA'>",
    "loose text",
    ...fixture("dom")
      .parses.map(record => record.html)
      .filter(html => !otherTrees.has(html)),
    ...deepHTML,
  ]).map(html => ["own", html]),
  ...[
    "<p><mark>plain</mark></p>",
    '<p><mark data-color="red">x</mark></p>',
    '<p><mark style="background-color: #FF0000">x</mark></p>',
    '<p><mark style="background-color: rgb(1, 2, 3); color: inherit">x</mark></p>',
    '<p><mark data-color="red" style="background-color: blue">x</mark></p>',
    '<p><mark data-color="" style="background-color: blue">x</mark></p>',
    '<p><mark style="background: yellow">x</mark></p>',
    '<p><mark style="BACKGROUND-COLOR: Blue">x</mark></p>',
    '<p><mark style="background-color: ">x</mark></p>',
    '<p><mark style="background-color: red; background-color: blue">x</mark></p>',
    '<p><mark data-color="red"><span style="color: blue">x</span></mark></p>',
    '<p><span style="color: red">x</span></p>',
    `<p><span style="color: 'red'">x</span></p>`,
    '<p><span style="color: #00F; color: ">x</span></p>',
    '<p><span style="font-weight: bold">x</span></p>',
    '<p><span style="color: red"><span>x</span></span></p>',
    '<p><span style="color: red"><span style="font-weight: bold">x</span><span style="color: blue">y</span></span></p>',
    '<p><span style="color: red">a<span style="color: blue">b<span style="color: green">c</span></span>d</span></p>',
    '<p><span style="color: red"><span style="color: blue"><span style="color: green">x</span></span></span></p>',
    // Each span gets the style of the one around it ahead of its own, through elements that aren't
    // spans as deep as 20.
    ...[0, 1, 2, 3, 19, 20, 21, 22].map(uncolored),
    `<p>${[...Array(22).keys()].map(level => `<span style="color: #${level}">`).join("")}x${"</span>".repeat(22)}</p>`,
  ].map(html => ["colors", html]),
  ...[
    "<pre>a  \n b</pre>",
    '<figure made="x"></figure>',
    '<figure made="1"></figure>',
    '<figure made="true"></figure>',
    '<figure made=""></figure>',
    "<figure></figure><p>after</p>",
  ].map(html => ["code", html]),
].map(([name, html]) => ({
  ...named(name),
  html,
  ...outcome(() => {
    const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
    return { doc: converters[name].parser.parse(document.body).toJSON() }
  }),
}))

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it.
const parsed = name =>
  [...parsesMarkdown, ...parsesHTML]
    .filter(record => (record.extensions ?? "own") === name && record.doc)
    .map(record => record.doc)
const documents = (name, docs) =>
  unique([...docs, ...parsed(name)].map(doc => JSON.stringify(doc))).map(text => [name, JSON.parse(text)])

const withMarks = (text, ...marks) => ({ type: "text", text, marks: marks.map(type => ({ type })) })
const markTypes = ["bold", "italic", "strike", "underline", "highlight", "subscript", "superscript", "textStyle"]
const docs = [
  { type: "doc", content: [{ type: "paragraph" }] },
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "plain " },
          ...markTypes.map(type => withMarks(type, type)),
          { type: "hardBreak" },
          withMarks("both", "bold", "italic"),
        ],
      },
      { type: "paragraph", content: [withMarks("a ", "bold"), withMarks("b", "bold", "italic"), withMarks(" c", "italic")] },
      { type: "paragraph", content: [{ type: "text", text: "*not em* <b>not bold</b> & 1. two" }] },
    ],
  },
  {
    type: "doc",
    content: [
      { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "two" }] },
      { type: "heading", attrs: { level: "3" }, content: [withMarks("three", "bold")] },
      { type: "heading", content: [{ type: "text", text: "no level" }] },
      { type: "heading", attrs: { level: 4 } },
    ],
  },
  {
    type: "doc",
    content: [
      { type: "image", attrs: { src: "a.png", alt: "A", title: "T", width: 10 } },
      { type: "image", attrs: { src: "b.png" } },
      { type: "image" },
    ],
  },
]
const paragraph = (...content) => ({ type: "doc", content: [{ type: "paragraph", content }] })
const colors = ["red", "", null, 0, 1, "#FF0000", 'a"b']
const colorDocs = [
  ...colors.map(color => paragraph({ type: "text", text: "x", marks: [{ type: "highlight", attrs: { color } }] })),
  ...colors.map(color => paragraph({ type: "text", text: "x", marks: [{ type: "textStyle", attrs: { color } }] })),
  paragraph({
    type: "text",
    text: "x",
    marks: [
      { type: "highlight", attrs: { color: "yellow" } },
      { type: "textStyle", attrs: { color: "blue" } },
    ],
  }),
]
const codeDocs = [
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "*a* <b>", marks: [{ type: "code" }] },
          { type: "text", text: " *c*" },
        ],
      },
      { type: "codeBlock", content: [{ type: "text", text: "*d* <e>" }] },
    ],
  },
  ...["x", 1, true, "", null].map(made => ({ type: "doc", content: [{ type: "figure", attrs: { made } }] })),
]

const text = node => ({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", ...node }] }] })
const values = [1, 0, 1.5, "x", 'a"b', "", true, false, null, {}, [], [1], { some: 1 }]
const serializesMarkdown = [
  ...documents("own", [...docs, ...deepDocs]),
  ...values
    .flatMap(value => [
      text({ text: "a", marks: { some: value } }),
      text({ text: { replace: value } }),
      text({ text: "a", marks: [{ type: { localeCompare: value } }, { type: "unknown" }] }),
      text({ text: "a", marks: [{ type: "unknown" }, { type: { localeCompare: value } }] }),
    ])
    .map(doc => ["own", doc]),
  ...[text({ text: "a", marks: { other: 1 } }), text({ text: { other: 1 } }), text({ text: [1] }), text({ text: 5 })].map(
    doc => ["own", doc],
  ),
  ...documents("colors", colorDocs),
  ...documents("code", codeDocs),
].map(([name, doc]) => ({ ...named(name), doc, ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })) }))

const serializesHTML = [
  ...documents("own", [...docs, ...depths.map(depth => nested("paragraph", depth))]),
  // `doc` has no toDOM, so the serializer has nothing to call for a doc inside the doc.
  ["own", { type: "doc", content: [{ type: "doc" }] }],
  ...documents("colors", colorDocs),
  ...documents("code", codeDocs),
].map(([name, doc]) => ({
  ...named(name),
  doc,
  ...outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

writeFixture("tiptap", {
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
