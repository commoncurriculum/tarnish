// Records what @tiptap/markdown's MarkdownManager and ProseMirror's DOMParser and DOMSerializer do
// with @tiptap/extension-list's BulletList, OrderedList, ListItem, TaskList and TaskItem, to
// fixtures/tiptap-lists.json, for tarnish-tiptap's tests/lists.rs to expect, in the record shapes
// harness/record-tiptap.mjs writes:
// - Each Markdown input records the document the manager parses it to: every input of
//   fixtures/marked.json, and each list form the extensions' tokenizers and parsers read.
// - Each document records the Markdown the manager serializes it to: the documents the parses
//   make, and lists with each attribute and shape the renderers read.
// - Each HTML input records the document DOMParser parses the body of a linkedom document holding
//   it to: fixtures/dom.json's, and each element, attribute and style the parse rules read.
// - Each document records the innerHTML of an element DOMSerializer fills in linkedom.
// Each record names its configuration in `extensions`: `lists`, Tiptap's own extensions with the
// five lists; `nested`, with TaskItem's `nested`; `attributes`, with each list's HTMLAttributes;
// `bullets`, `ordered` and `tasks`, with only those lists; `paragraphs`, lists of paragraphs;
// `flat`, the lists beside tiptap-extension-flat-list's items, without the items' parse rules,
// which tarnish leaves to the application; and `tabs`, the lists with a manager that indents with
// tabs.
import { readFileSync } from "node:fs"
import { getSchema } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { BulletList, ListItem, OrderedList, TaskItem, TaskList } from "@tiptap/extension-list"
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
import { FlatListCore, FlatListOrdered, FlatListTask, FlatListUnordered } from "tiptap-extension-flat-list"
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
const lists = [BulletList, OrderedList, ListItem, TaskList, TaskItem]
const withoutParseRules = extension => extension.extend({ parseHTML: () => [] })
const configurations = {
  lists: { extensions: [...own, ...lists] },
  nested: { extensions: [...own, BulletList, OrderedList, ListItem, TaskList, TaskItem.configure({ nested: true })] },
  attributes: {
    extensions: [
      ...own,
      BulletList.configure({ HTMLAttributes: { class: "bullets", "data-list": "bullet" } }),
      OrderedList.configure({ HTMLAttributes: { class: "numbers", start: "9", type: "A", style: "color: red" } }),
      ListItem.configure({ HTMLAttributes: { class: "item" } }),
      TaskList.configure({ HTMLAttributes: { class: "tasks", "data-type": "mine" } }),
      TaskItem.configure({ HTMLAttributes: { class: "task", "data-checked": "maybe", style: "color: blue" } }),
    ],
  },
  bullets: { extensions: [...own, BulletList, ListItem] },
  ordered: { extensions: [...own, OrderedList, ListItem] },
  tasks: { extensions: [...own, TaskList, TaskItem] },
  paragraphs: {
    extensions: [
      ...own,
      BulletList.configure({ itemTypeName: "paragraph" }),
      OrderedList.configure({ itemTypeName: "paragraph" }),
      TaskList.configure({ itemTypeName: "paragraph" }),
    ],
  },
  flat: {
    extensions: [
      Document,
      Paragraph,
      Text,
      FlatListCore,
      withoutParseRules(FlatListOrdered),
      withoutParseRules(FlatListUnordered),
      withoutParseRules(FlatListTask),
      ...lists,
    ],
  },
  tabs: { extensions: [...own, ...lists], indentation: { style: "tab", size: 3 } },
}
const converters = Object.fromEntries(
  Object.entries(configurations).map(([name, { extensions, indentation }]) => {
    const marked = new Marked()
    marked.use(moreLists())
    const schema = getSchema(extensions)
    const parser = DOMParser.fromSchema(schema)
    const serializer = DOMSerializer.fromSchema(schema)
    const manager = new MarkdownManager({ extensions, marked, ...(indentation ? { indentation } : {}) })
    return [name, { manager, schema, parser, serializer }]
  }),
)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))
const unique = values => [...new Set(values)]
const named = (names, inputs) => names.flatMap(name => unique(inputs).map(input => [name, input]))

const text = (value, ...marks) => ({ type: "text", text: value, ...(marks.length ? { marks } : {}) })
const paragraph = (...content) => ({ type: "paragraph", content })
const p = value => paragraph(text(value))
const doc = (...content) => ({ type: "doc", content })
const item = (...content) => ({ type: "listItem", content })
const bullets = (...items) => ({ type: "bulletList", content: items })
const numbers = (attrs, ...items) => ({ type: "orderedList", ...(attrs ? { attrs } : {}), content: items })
const task = (checked, ...content) => ({ type: "taskItem", attrs: { checked }, content })
const tasks = (...items) => ({ type: "taskList", content: items })

// Lists nested one to three deep, which tests/lists.rs folds into the conversions of any depth:
// each item holding the list inside it first, or after a paragraph.
const depths = [1, 2, 3]
const nestedFirst = (list, item, depth) => {
  let node = p("x")
  for (let level = 0; level < depth; level++) node = list(item(node))
  return doc(node)
}
const nestedAfter = (list, item, depth) => {
  let node = list(item(p("x")))
  for (let level = 1; level < depth; level++) node = list(item(p("x"), node))
  return doc(node)
}
const kinds = [
  [(...items) => bullets(...items), (...content) => item(...content)],
  [(...items) => numbers(null, ...items), (...content) => item(...content)],
  [(...items) => tasks(...items), (...content) => task(false, ...content)],
]
const deepDocs = depths.flatMap(depth =>
  kinds.flatMap(([list, listItem]) => [nestedFirst(list, listItem, depth), nestedAfter(list, listItem, depth)]),
)
const indented = (indent, line, depth) =>
  [...Array(depth).keys()].map(level => indent.repeat(level) + line).join("\n")
const deepMarkdown = depths.flatMap(depth => [
  "- ".repeat(depth) + "x",
  indented("   ", "1. x", depth),
  indented("  ", "- [ ] x", depth),
  indented("  ", "- x", depth),
])
// Items opened in items, each first holding the next list, which the parser closes, or text.
const deepHTML = depths.flatMap(depth =>
  ["<ul><li>", "<ol><li>", '<ul data-type="taskList"><li data-type="taskItem">'].flatMap(open => [
    open.repeat(depth) + "x",
    (open + "x").repeat(depth),
  ]),
)

const listMarkdown = [
  // Bullets, tight and loose, each marker, nested, lazy and empty.
  "- a",
  "* a",
  "+ a",
  "- a\n- b\n- c",
  "- a\n* b\n+ c",
  "- a\n\n- b",
  "- a\n\n\n- b",
  "- a\n- b\n\n- c",
  "- a\n\n  b",
  "- a\n  b",
  "- a\nb",
  "- a\n\nb",
  "- a\n  - b\n    - c",
  "- a\n    - b",
  "- a\n - b",
  "- a\n  - b\n- c",
  "- a\n\n  - b\n\n- c",
  "- - a",
  "* - + a",
  "-",
  "- ",
  "-\n  a",
  "-\n\n  a",
  "-\n-",
  "- **bold** *em* ~~del~~",
  "- a  \n  b",
  "- a\\\n  b",
  "- `code`",
  "- # h",
  "- > q",
  "- ```\n  code\n  ```",
  "-     code",
  "- a\n  ---",
  "- a\n  ===",
  "- a\n---",
  "- <div>x</div>",
  "- a <b>x</b>",
  "-\ta",
  "- a\n\t- b",
  "- a\n\n\n\n  b",
  "- a\n  b\n\n  c\n- d",
  "- &amp; &lt;",
  "- \\- a",
  "- 1\\. a",
  "paragraph\n- interrupts",
  "paragraph\n\n- after a blank line",
  "- a\n\n  1. b",
  "- a\n  1. b\n  2. c",
  "- a\n  1. b\n\n     c",
  "- 1. a",
  "- 1) a\n  2) b",
  "- a. b",
  // Ordered, each marker and numbering, continuation and what interrupts it.
  "1. a",
  "1. a\n2. b",
  "1) a\n2) b",
  "1. a\n2) b",
  "3. a\n4. b",
  "0. a",
  "00. a",
  "01. a",
  "1234567890. a",
  "99999999999999999999. a",
  "1. a\n1. b",
  "1. a\n3. b",
  "a. a\nb. b",
  "A. a\nB. b",
  "i. a\nii. b",
  "I. a\nII. b",
  "iv. a",
  "IV. a",
  "x. a",
  "c. a",
  "m. a",
  "mm. a",
  "mcmxcix. a",
  "iiii. a",
  "ic. a",
  "aa. a",
  "zz. a",
  "ab. a",
  "Ab. a",
  "abc. a",
  "iI. a",
  "vx. a",
  "b. a\nc. b",
  "1.",
  "1. ",
  "1.a",
  "1.\ta",
  "1.  a",
  "1. a\n   b",
  "1. a\nb",
  "1. a\n\nb",
  "1. a\n\n   b",
  "1. a\n\n\n   b",
  "1. a\n b",
  "1. a\n\tb",
  "1. a\n   - b",
  "1. a\n   > q",
  "1. a\n   # h",
  "1. a\n   ```\n   code\n   ```",
  "1. a\n   ---",
  "1. a\n   ***",
  "1. a\n   $$\n   x\n   $$",
  "1. a\n- b",
  "1. a\n# h",
  "1. a\n> q",
  "1. a\n```\ncode\n```",
  "1. a\n***",
  "1. a\n---",
  "1. a\n$$",
  "1. a\n   1. b\n      1. c",
  "1. a\n  1. b",
  "1. a\n    1. b\n  1. c",
  " 1. a\n 2. b",
  "  1. a\n1. b",
  "1. a\n   a. b\n      i. c",
  "1. a\n2. b\n   a. c\n   b. d\n3. e",
  "1. a\n   b\n\n   c\n2. d",
  "1. a\n\n2. b",
  "1. a\n\n\n2. b",
  "1. [ ] a",
  "1. **bold** <b>x</b>",
  "1. a\n   b. c",
  "para\n1. a",
  "para\n2. a",
  "(216) 555-1234",
  "Call (216) 555-1234 now",
  "a\n2) b",
  "> 1. a\n> 2. b",
  // Tasks, checked and not, nested, and next to other lists.
  "- [ ] a",
  "- [x] a",
  "- [X] a",
  "* [ ] a",
  "+ [x] a",
  "- [ ] a\n- [x] b",
  "- [ ]a",
  "- [ ] ",
  "- [ ]",
  "- [  ] a",
  "- [y] a",
  "-  [ ]  a",
  "- [ ]\ta",
  "  - [ ] a",
  "    - [ ] a",
  "- [ ] a\n  - [ ] b",
  "- [ ] a\n    - [ ] b",
  "- [ ] a\n   - [ ] b",
  "- [ ] a\n  - [x] b\n    - [ ] c",
  "- [ ] a\n  - [ ] b\n- [ ] c",
  "- [ ] a\n  - b",
  "- [ ] a\n  b",
  "- [ ] a\n\n  b",
  "- [ ] a\n\n- [ ] b",
  "- [ ] a\nb",
  "- [ ] a\n\nb",
  "- [ ] a\n  - [ ] b\n  - c",
  "- [ ] a\n  - [ ] b\n\n  c",
  "- [ ] a\n- b",
  "- a\n- [ ] b",
  "- a\n- [ ] b\n- c",
  "- [ ] a\n- b\n- [x] c",
  "- a\n- [ ] b\n  - [ ] c",
  "- a\n- [ ] b\n  c",
  "- a\n- [ ] b\n\n  c",
  "- a\n- [x]  b",
  "- a\n  - [ ] b",
  "- [ ] **bold** *em*",
  "- [x] [link](x)",
  "a - [ ] b",
  "a\n- [ ] b",
  "a\n-\n[ ] b",
  "ab - [ ] c",
  "a\xa0- [ ] b",
  "> - [ ] a",
  "1. - [ ] a",
  "- [ ] 1. a",
  "- [ ] a\n  1. b",
  "\n\n- [ ] a",
  "- [ ] a\n\n\n",
  "- [ ] a\n\n\nb",
  // Lists beside each other and other blocks.
  "",
  "\n",
  "- a\n1. b",
  "1. a\n- b",
  "> - a\n> - b",
  "- a\n<!-- -->\n- b",
  "# h\n- a\n\n1. b\n\n- [ ] c\n\ntext",
  ...deepMarkdown,
]

// fixtures/dom.json's inputs that hold an element the lists' parse rules read.
const domLists = fixture("dom")
  .parses.map(record => record.html)
  .filter(html => /<\/?(?:ul|ol|li|input|label)\b/i.test(html))
const checkbox = checked => `<label><input type="checkbox"${checked ? ' checked="checked"' : ""}><span></span></label>`
const taskItemHTML = (checked, content) =>
  `<li data-type="taskItem" data-checked="${checked}">${checkbox(checked)}<div>${content}</div></li>`
const listHTML = [
  "<ul><li>a</li><li>b</li></ul>",
  "<ul><li><p>a</p></li></ul>",
  "<ul><li><p>a</p><p>b</p></li></ul>",
  "<ul><li>a<ul><li>b</li></ul></li></ul>",
  "<ul><li><ol><li>x</li></ol></li></ul>",
  "<ul><li>a</li></ul><ol><li>b</li></ol>",
  "<ul><li>a</li>text<li>b</li></ul>",
  "<ul>text</ul>",
  "<ol><li></li></ol>",
  "<p>x</p><ul><li>a</li></ul><p>y</p>",
  "<li>a</li>",
  "<ul><li><h1>h</h1></li></ul>",
  "<ol><li>a</li></ol>",
  ...["3", "", "x", " 4", "4x", "-3", "010", "1.9", "0x1f", "1"].map(start => `<ol start="${start}"><li>a</li></ol>`),
  "<ol start><li>a</li></ol>",
  ...["a", "A", "i", "I", "1", "", "disc"].map(type => `<ol type="${type}"><li>a</li></ol>`),
  ...[
    "list-style-type: lower-roman",
    "list-style-type: upper-roman",
    "list-style-type: lower-alpha",
    "list-style-type: upper-alpha",
    "list-style-type: lower-latin",
    "list-style-type: upper-latin",
    "LIST-STYLE-TYPE : Upper-Roman ",
    "list-style-type: decimal",
    "color: red; list-style-type: lower-alpha; font-weight: bold",
    "list-style-type:lower-alpha;list-style-type:upper-roman",
    "list-style: lower-alpha",
    "list-style-type: ;",
    "list-style-type:",
  ].map(style => `<ol style="${style}"><li>a</li></ol>`),
  '<ol type="a" style="list-style-type: upper-roman"><li>a</li></ol>',
  '<ol type="" style="list-style-type: upper-roman"><li>a</li></ol>',
  '<ol start="2" type="i"><li>a</li><li>b</li></ol>',
  '<ol><li style="list-style-type: lower-alpha">a</li></ol>',
  '<ol style="list-style-type: decimal"><li style="list-style-type: upper-alpha">a</li></ol>',
  '<ol><li>a</li><li style="list-style-type: lower-roman">b</li></ol>',
  '<ol><li><ol><li style="list-style-type: lower-roman">b</li></ol></li></ol>',
  '<ol><p>x</p><li style="list-style-type: upper-roman">a</li></ol>',
  `<ul data-type="taskList">${taskItemHTML(true, "<p>a</p>")}${taskItemHTML(false, "<p>b</p>")}</ul>`,
  ...['data-checked=""', 'data-checked="false"', 'data-checked="x"', 'data-checked="TRUE"', ""].map(
    checked => `<ul data-type="taskList"><li data-type="taskItem" ${checked}><div><p>a</p></div></li></ul>`,
  ),
  '<ul data-type="taskList"><li data-type="taskItem">a</li></ul>',
  '<ul data-type="taskList"><li data-type="taskItem"><p>a</p></li></ul>',
  '<ul data-type="taskList"><li data-type="taskItem"><div>a</div><div>b</div></li></ul>',
  '<ul data-type="taskList"><li data-type="taskItem"><span><div>deep</div></span>x</li></ul>',
  `<ul data-type="taskList"><li data-type="taskItem"><p>a</p><ul data-type="taskList">${taskItemHTML(true, "<p>b</p>")}</ul></li></ul>`,
  `<ul data-type="taskList">${taskItemHTML(false, `<p>a</p><ul data-type="taskList">${taskItemHTML(true, "<p>b</p>")}</ul>`)}</ul>`,
  `<ul data-type="taskList">${taskItemHTML(false, "<p>a</p><ul><li>b</li></ul>")}</ul>`,
  '<ul><li data-type="taskItem">a</li></ul>',
  '<ul data-type="taskList"><li>a</li></ul>',
  '<ul data-type="taskList"></ul>',
  '<ul data-type="tasklist"><li data-type="taskitem">a</li></ul>',
  '<ol data-type="taskList"><li>a</li></ol>',
  '<li data-type="taskItem">orphan</li>',
  '<div data-type="taskList"><li data-type="taskItem">x</li></div>',
  '<ul class="contains-task-list"><li class="task-list-item"><input type="checkbox" disabled> a</li></ul>',
  '<ul data-type="taskList"><li data-type="taskItem" data-checked="true"><label><input type="checkbox"></label><div></div></li></ul>',
  ...deepHTML,
]
// Lines of list markers, indentation, blocks and text, drawn by a seeded generator, for the
// forms no hand-picked input combines.
let seed = 7
const random = () => {
  seed = (seed * 48271) % 2147483647
  return seed / 2147483647
}
const pick = items => items[Math.floor(random() * items.length)]
const repeated = (most, make) => Array.from({ length: 1 + Math.floor(random() * most) }, make)
const pieces = [
  ["- ", "* ", "+ ", "1. ", "2) ", "a. ", "iv. ", "- [ ] ", "- [x] ", "* [X] ", "1. [ ] "],
  ["", "", " ", "  ", "   ", "    ", "\t", "\xa0"],
  ["a", "b c", "**b**", "> q", "# h", "```", "---", "$$", "1.", "- ", "[ ] d", "<b>x</b>"],
]
const generatedMarkdown = Array.from({ length: 300 }, () =>
  repeated(5, () => pick(pieces[1]) + repeated(2, () => pick(pieces[0])).join("") + pick(pieces[2])).join(
    pick(["\n", "\n", "\n\n"]),
  ),
)
const parsesMarkdown = [
  ...named(["lists"], [...fixture("marked").lexed.map(record => record.input), ...listMarkdown, ...generatedMarkdown]),
  ...named(["nested", "bullets", "ordered", "tasks", "paragraphs", "flat"], listMarkdown),
].map(([name, markdown]) => ({
  extensions: name,
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

const ordered = (start, type) => doc(numbers({ start, type }, item(p("a")), item(p("b"))))
const starts = [1, 0, -2, 3, 26, 27, 702, 703, 3999, 1.5, "3", "x", null, true, [], [5], {}]
const types = [null, "1", "a", "A", "i", "I", "x", 1, ""]
const docs = [
  doc(bullets(item(p("a")), item(p("b")))),
  doc(bullets(item(p("a"), bullets(item(p("b")))))),
  doc(bullets(item(p("a"), p("b")))),
  doc(bullets(item(p("a"), p("b"), bullets(item(p("c"))), p("d")))),
  doc(bullets(item(p("a"), paragraph(), paragraph()))),
  doc(bullets(item(paragraph()))),
  doc(bullets(item(paragraph(), p("a")))),
  doc(bullets(item())),
  doc(bullets({ type: "listItem" })),
  doc(bullets()),
  doc({ type: "bulletList" }),
  doc(bullets(item(p("a"))), bullets(item(p("b")))),
  doc(p("x"), bullets(item(p("a"))), p("y")),
  doc(bullets(item({ type: "heading", attrs: { level: 2 }, content: [text("h")] }))),
  doc(bullets(item(bullets(item(p("a")))))),
  doc(bullets(item(paragraph(text("a", { type: "bold" }), { type: "hardBreak" }, text("b", { type: "bold" }))))),
  doc(bullets(item(p("*a* 1. - [ ] b")))),
  doc(bullets(item(p("a\nb"), p("c\n\nd")))),
  doc(bullets(item(p("a"), text("loose")))),
  doc(bullets(item(p("a"), { type: "unknown" }))),
  doc(bullets(item(p("a"), null))),
  doc(bullets(item(p("a")), null)),
  doc(bullets(item(p("a"), { type: "image", attrs: { src: "a.png" } }))),
  doc(bullets(item(p("a"), numbers({ start: 4 }, item(p("b")), item(p("c")))))),
  doc(numbers(null, item(p("a")), item(p("b")))),
  doc(numbers({}, item(p("a")))),
  doc(numbers({ start: 3 }, item(p("a"), numbers({ start: 2, type: "a" }, item(p("b")), item(p("c")))))),
  doc(tasks(task(true, p("a")), task(false, p("b")))),
  ...["x", 0, 1, "", null].map(checked => doc(tasks(task(checked, p("a"))))),
  doc(tasks({ type: "taskItem", content: [p("a")] })),
  doc(tasks(task(false, p("a"), tasks(task(true, p("b")))))),
  doc(tasks(task(false, p("a"), p("b")))),
  doc(tasks(task(false, p("a"), bullets(item(p("b")))))),
  doc(tasks(task(false))),
  doc(tasks({ type: "taskItem", attrs: { checked: true } })),
  doc(tasks()),
  doc(item(p("a"))),
  doc(task(true, p("a"))),
  doc(tasks(item(p("a")))),
  doc(bullets(task(true, p("a")))),
  doc(numbers({ start: 5 }, task(true, p("a")), item(p("b")))),
  doc(bullets(paragraph(text("a")), p("b"))),
  // Lists and items whose content or attributes aren't what a schema gives them.
  doc({ type: "bulletList", content: "x" }),
  doc({ type: "bulletList", content: item(p("a")) }),
  doc(bullets({ type: "listItem", content: "x" })),
  doc(bullets({ type: "listItem", content: { type: "paragraph" } })),
  doc({ type: "orderedList", attrs: "x", content: [item(p("a"))] }),
  doc({ type: "orderedList", attrs: [], content: [item(p("a"))] }),
  doc({ type: "orderedList", attrs: { start: 2, type: "i" }, content: [item(p("a")), item(p("b")), item(p("c"))] }),
  doc(numbers({ start: 10000000, type: "i" }, item(p("a")))),
  doc(numbers({ start: 1e21 }, item(p("a")))),
  doc(tasks({ type: "taskItem", attrs: "x", content: [p("a")] })),
  doc(bullets(item(paragraph(text("a ", { type: "bold" })), paragraph(text("b", { type: "bold" }))))),
  doc(bullets(item(p("a")), item(p("b"), bullets(item(p("c")), item(p("d"))), p("e")), item(p("f")))),
]
const orderedDocs = starts.flatMap(start => types.map(type => ordered(start, type)))
// Text whose marks are of the list types, which the manager renders as it does marks.
const markDocs = [
  { type: "listItem" },
  { type: "bulletList" },
  { type: "orderedList", attrs: { start: 3 } },
  { type: "taskItem", attrs: { checked: true } },
  { type: "taskList" },
].map(mark => doc(paragraph(text("a", mark), text("b"))))
const flatDocs = [
  doc(
    { type: "flatListItemUnordered", attrs: { indent: 1 }, content: [text("flat")] },
    bullets(item(p("a"))),
    { type: "flatListItemTask", attrs: { checked: true }, content: [text("task")] },
  ),
]
// The documents each configuration serializes, besides those its parses make.
const docsOf = {
  lists: [...docs, ...orderedDocs, ...markDocs, ...deepDocs],
  tabs: [...docs, ...deepDocs],
  attributes: [...docs, ...orderedDocs],
  flat: [...docs, ...flatDocs],
}
const docsFor = name => docsOf[name] ?? docs

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it.
const parsed = (records, name) => records.filter(record => record.extensions === name && record.doc).map(record => record.doc)
const documents = (name, docs, records) =>
  unique([...docs, ...parsed(records, name)].map(doc => JSON.stringify(doc))).map(text => [name, JSON.parse(text)])
const names = Object.keys(configurations)
// The configurations whose schemas differ: `tabs` has the schema `lists` has.
const schemas = names.filter(name => name !== "tabs")
const serializeHTML = (name, doc) =>
  outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  })

const htmlInputs = [
  ...named(["lists"], [...domLists, ...listHTML]),
  ...named(["nested", "bullets", "ordered", "tasks", "paragraphs", "flat"], listHTML),
]
// Tiptap's own HTML for each document, parsed back.
const roundTrips = schemas.flatMap(name =>
  docsFor(name).flatMap(doc => {
    const { html } = serializeHTML(name, doc)
    return html ? [[name, html]] : []
  }),
)
const parsesHTML = unique([...htmlInputs, ...roundTrips].map(entry => JSON.stringify(entry)))
  .map(entry => JSON.parse(entry))
  .map(([name, html]) => ({
    extensions: name,
    html,
    ...outcome(() => {
      const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
      return { doc: converters[name].parser.parse(document.body).toJSON() }
    }),
  }))

// The parses whose documents a configuration serializes: `lists` serializes those of its Markdown
// and its HTML, and the others those of their HTML, whose Markdown makes much what `lists`' does.
const serialized = name => (name === "lists" ? [...parsesMarkdown, ...parsesHTML] : parsesHTML)
const serializesMarkdown = names
  .flatMap(name => documents(name, docsFor(name), serialized(name)))
  .map(([name, doc]) => ({ extensions: name, doc, ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })) }))

const serializesHTML = schemas
  .flatMap(name => documents(name, docsFor(name), serialized(name)))
  .map(([name, doc]) => ({ extensions: name, doc, ...serializeHTML(name, doc) }))

// Each configuration's node and mark types in the schema's order, with the fields of their specs
// a schema reads: strings and `selectable` where given, other flags where true, and each
// attribute's default where it has one.
const present = (spec, keys) => keys.filter(key => spec[key] !== undefined).map(key => [key, spec[key]])
const flags = (spec, keys) => keys.filter(key => spec[key]).map(key => [key, true])
const attributes = spec =>
  Object.entries(spec.attrs ?? {}).map(([name, attr]) => ("default" in attr ? { name, default: attr.default } : { name }))
const typeSpecs = (types, strings, booleans) => {
  const specs = []
  types.forEach((name, spec) =>
    specs.push({
      name,
      ...Object.fromEntries([...present(spec, strings), ...flags(spec, booleans)]),
      attrs: attributes(spec),
    }),
  )
  return specs
}
const schemaRecords = schemas.map(name => {
  const { spec } = converters[name].schema
  return {
    extensions: name,
    nodes: typeSpecs(
      spec.nodes,
      ["content", "marks", "group", "whitespace", "selectable"],
      ["inline", "atom", "draggable", "code", "linebreakReplacement", "defining", "isolating"],
    ),
    marks: typeSpecs(spec.marks, ["excludes", "group", "inclusive", "spanning"], ["code"]),
  }
})

writeFixture("tiptap-lists", {
  schemas: schemaRecords,
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
