// Records @tiptap/extension-list's BulletList, OrderedList, ListItem, TaskList and TaskItem to
// fixtures/tiptap-lists.json, as harness/tiptap.mjs records them: `lists`, Tiptap's own
// extensions with the five lists; `nested`, with TaskItem's `nested`; `attributes` and
// `attributeValues`, with each list's HTMLAttributes, of strings and of other values; `bullets`,
// `ordered` and `tasks`, with only those lists; `paragraphs`, lists of paragraphs; `flat`, the
// lists beside tiptap-extension-flat-list's items, without the items' parse rules, which tarnish
// leaves to the application; and `tabs`, the lists with a manager that indents with tabs.
import { BulletList, ListItem, OrderedList, TaskItem, TaskList } from "@tiptap/extension-list"
import { Document } from "@tiptap/extension-document"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Text } from "@tiptap/extension-text"
import { FlatListCore, FlatListOrdered, FlatListTask, FlatListUnordered } from "tiptap-extension-flat-list"
import {
  doc,
  domInputs,
  markedInputs,
  otherAttributes,
  own,
  paragraph,
  record,
  rendered,
  text,
  withParsed,
} from "./tiptap.mjs"

const lists = [BulletList, OrderedList, ListItem, TaskList, TaskItem]
const withoutParseRules = extension => extension.extend({ parseHTML: () => [] })

const p = value => paragraph(text(value))
const item = (...content) => ({ type: "listItem", content })
const bullets = (...items) => ({ type: "bulletList", content: items })
const numbers = (attrs, ...items) => ({ type: "orderedList", ...(attrs ? { attrs } : {}), content: items })
const task = (checked, ...content) => ({ type: "taskItem", attrs: { checked }, content })
const tasks = (...items) => ({ type: "taskList", content: items })

// Lists nested one to three deep, which tests/recorded/lists.rs folds into the conversions of any
// depth:
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
  doc({ type: "flatListItemOrdered", attrs: { indent: 2, counter: 3 }, content: [text("ordered")] }, { type: "flatListItemTask" }),
]

const fiveLists = [...own, ...lists]
const listsDocs = [...docs, ...orderedDocs, ...markDocs, ...deepDocs]
// Tiptap's own extensions with only some of the lists, which reach what the Markdown and HTML of
// lists parse to, and what the others' documents serialize to.
const some = extensions => ({
  extensions: [...own, ...extensions],
  parsesMarkdown: listMarkdown,
  parsesHTML: listHTML,
  serializesMarkdown: docs,
  serializesHTML: docs,
})

record("tiptap-lists", {
  lists: {
    extensions: fiveLists,
    parsesMarkdown: [...markedInputs(), ...listMarkdown, ...generatedMarkdown],
    // Tiptap's own HTML for each document is parsed back.
    parsesHTML: [...domInputs(/<\/?(?:ul|ol|li|input|label)\b/i), ...listHTML, ...rendered(fiveLists, listsDocs)],
    serializesMarkdown: withParsed(listsDocs),
    serializesHTML: withParsed(listsDocs),
  },
  // `nested` lets a task item hold blocks after its paragraph, which only its parse rule reads.
  nested: {
    extensions: [...own, BulletList, OrderedList, ListItem, TaskList, TaskItem.configure({ nested: true })],
    parsesHTML: listHTML.filter(html => /taskItem/i.test(html)),
  },
  // HTMLAttributes reach only renderHTML.
  attributes: {
    extensions: [
      ...own,
      BulletList.configure({ HTMLAttributes: { class: "bullets", "data-list": "bullet" } }),
      OrderedList.configure({ HTMLAttributes: { class: "numbers", start: "9", type: "A", style: "color: red" } }),
      ListItem.configure({ HTMLAttributes: { class: "item" } }),
      TaskList.configure({ HTMLAttributes: { class: "tasks", "data-type": "mine" } }),
      TaskItem.configure({ HTMLAttributes: { class: "task", "data-checked": "maybe", style: "color: blue" } }),
    ],
    serializesHTML: [...docs, ...orderedDocs],
  },
  attributeValues: {
    extensions: [...own, ...lists.map(extension => extension.configure({ HTMLAttributes: otherAttributes }))],
    serializesHTML: docs,
  },
  bullets: some([BulletList, ListItem]),
  ordered: some([OrderedList, ListItem]),
  tasks: some([TaskList, TaskItem]),
  paragraphs: {
    extensions: [
      ...own,
      BulletList.configure({ itemTypeName: "paragraph" }),
      OrderedList.configure({ itemTypeName: "paragraph" }),
      TaskList.configure({ itemTypeName: "paragraph" }),
    ],
    parsesMarkdown: listMarkdown,
    parsesHTML: listHTML,
    serializesMarkdown: withParsed(docs),
    serializesHTML: withParsed(docs),
  },
  // The flat items have no parse rules here, and no Markdown hooks.
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
    serializesMarkdown: flatDocs,
    serializesHTML: flatDocs,
  },
  // The indentation reaches only the Markdown lists serialize to.
  tabs: { extensions: fiveLists, indentation: { style: "tab", size: 3 }, serializesMarkdown: [...docs, ...deepDocs] },
})
