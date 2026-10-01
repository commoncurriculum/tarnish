// Records what the real ProseMirror packages make of transform ops, and of `textBetween` and
// `textContent`, to fixtures/ops.json, for the Rust, Elixir and C tests of tarnish's
// `transform`, `text_between` and `text_content` to expect.
//
// An op is an object naming a `Transform` method in "op", with the method's arguments by name as
// ProseMirror's JSON. `transform` below is what an op means, and crates/tarnish/src/api/ops.rs
// reads ops the same way. Where a method takes a NodeRange, an op gives `from`, `to` and
// optionally `depth`; where ProseMirror's users compute an argument with a helper, an op may
// leave it out for `liftTarget` or `findWrapping` to compute.
import { readFileSync } from "node:fs"
import { Fragment, Node, NodeRange, Schema, Slice } from "prosemirror-model"
import { Step, Transform, findWrapping, liftTarget } from "prosemirror-transform"
import { outcome, writeFixture } from "./fixture.mjs"

const isObject = value => value !== null && typeof value === "object" && !Array.isArray(value)
const field = (json, key) => (isObject(json) && Object.hasOwn(json, key) ? json[key] : undefined)

class Op {
  constructor(schema, json) {
    this.schema = schema
    this.json = json
    this.name = String(field(json, "op"))
  }

  get(key) {
    return field(this.json, key)
  }

  invalid(key) {
    return new RangeError(`Invalid ${key} for ${this.name}`)
  }

  whole(key, fallback) {
    const value = this.get(key)
    if (value == null && fallback !== undefined) return fallback
    if (!Number.isSafeInteger(value) || value < 0) throw this.invalid(key)
    return value
  }

  optionalWhole(key) {
    return this.get(key) == null ? null : this.whole(key)
  }

  range(toIsOptional = false) {
    const from = this.whole("from")
    const to = this.whole("to", toIsOptional ? from : undefined)
    if (to < from) throw new RangeError(`Invalid range from ${from} to ${to} for ${this.name}`)
    return [from, to]
  }

  string(key) {
    const value = this.get(key)
    if (typeof value !== "string") throw this.invalid(key)
    return value
  }

  nodeType(key) {
    return this.schema.nodeType(this.string(key))
  }

  attrs(key) {
    const value = this.get(key)
    if (value != null && !isObject(value)) throw this.invalid(key)
    return value ?? null
  }

  mark(key) {
    return this.schema.markFromJSON(this.get(key))
  }

  markTypeOrMark(key) {
    const value = this.get(key)
    if (typeof value !== "string") return this.schema.markFromJSON(value)
    const type = this.schema.marks[value]
    if (!type) throw new RangeError(`There is no mark type ${value} in this schema`)
    return type
  }

  marks(key) {
    const value = this.get(key)
    if (value == null) return undefined
    if (!Array.isArray(value)) throw this.invalid(key)
    return value.map(mark => this.schema.markFromJSON(mark))
  }

  node(key) {
    return this.schema.nodeFromJSON(this.get(key))
  }

  content(key) {
    const value = this.get(key)
    return value == null || Array.isArray(value) ? Fragment.fromJSON(this.schema, value) : this.schema.nodeFromJSON(value)
  }

  slice(key) {
    return Slice.fromJSON(this.schema, this.get(key))
  }

  step(key) {
    return Step.fromJSON(this.schema, this.get(key))
  }

  wrapper(json, key) {
    const type = field(json, "type")
    const attrs = field(json, "attrs")
    if (typeof type !== "string" || (attrs != null && !isObject(attrs))) throw this.invalid(key)
    return { type: this.schema.nodeType(type), attrs: attrs ?? null }
  }

  wrappers(key) {
    const value = this.get(key)
    if (!Array.isArray(value)) throw this.invalid(key)
    return value.map(wrapper => this.wrapper(wrapper, key))
  }

  typesAfter(key) {
    const value = this.get(key)
    if (value == null) return undefined
    if (!Array.isArray(value)) throw this.invalid(key)
    return value.map(type => (type === null ? null : this.wrapper(type, key)))
  }
}

function blockRange(doc, from, to, depth) {
  const $from = doc.resolve(from)
  const $to = doc.resolve(to)
  if (depth == null) {
    const range = $from.blockRange($to)
    if (!range) throw new RangeError(`No block range from ${from} to ${to}`)
    return range
  }
  if (depth > $from.sharedDepth(to)) throw new RangeError(`No block range from ${from} to ${to} at depth ${depth}`)
  return new NodeRange($from, $to, depth)
}

const methods = {
  replace(tr, op) {
    const [from, to] = op.range(true)
    tr.replace(from, to, op.slice("slice"))
  },
  replaceWith(tr, op) {
    const [from, to] = op.range()
    tr.replaceWith(from, to, op.content("content"))
  },
  delete(tr, op) {
    const [from, to] = op.range()
    tr.delete(from, to)
  },
  insert(tr, op) {
    const pos = op.whole("pos")
    tr.insert(pos, op.content("content"))
  },
  replaceRange(tr, op) {
    const [from, to] = op.range()
    tr.replaceRange(from, to, op.slice("slice"))
  },
  replaceRangeWith(tr, op) {
    const [from, to] = op.range()
    tr.replaceRangeWith(from, to, op.node("node"))
  },
  deleteRange(tr, op) {
    const [from, to] = op.range()
    tr.deleteRange(from, to)
  },
  addMark(tr, op) {
    const [from, to] = op.range()
    tr.addMark(from, to, op.mark("mark"))
  },
  removeMark(tr, op) {
    const [from, to] = op.range()
    tr.removeMark(from, to, op.get("mark") == null ? null : op.markTypeOrMark("mark"))
  },
  addNodeMark(tr, op) {
    const pos = op.whole("pos")
    tr.addNodeMark(pos, op.mark("mark"))
  },
  removeNodeMark(tr, op) {
    const pos = op.whole("pos")
    tr.removeNodeMark(pos, op.markTypeOrMark("mark"))
  },
  setNodeMarkup(tr, op) {
    const pos = op.whole("pos")
    const type = op.get("type") == null ? null : op.nodeType("type")
    tr.setNodeMarkup(pos, type, op.attrs("attrs"), op.marks("marks"))
  },
  setNodeAttribute(tr, op) {
    const pos = op.whole("pos")
    tr.setNodeAttribute(pos, op.string("attr"), op.get("value"))
  },
  setDocAttribute(tr, op) {
    tr.setDocAttribute(op.string("attr"), op.get("value"))
  },
  setBlockType(tr, op) {
    const [from, to] = op.range(true)
    tr.setBlockType(from, to, op.nodeType("type"), op.attrs("attrs"))
  },
  lift(tr, op) {
    const [from, to] = op.range()
    const depth = op.optionalWhole("depth")
    const target = op.optionalWhole("target")
    const range = blockRange(tr.doc, from, to, depth)
    const lifted = target ?? liftTarget(range)
    if (lifted == null) throw new RangeError(`Can't lift the range from ${from} to ${to}`)
    tr.lift(range, lifted)
  },
  wrap(tr, op) {
    const [from, to] = op.range()
    const depth = op.optionalWhole("depth")
    const given = op.get("wrappers") == null ? null : op.wrappers("wrappers")
    const nodeType = given ? null : op.nodeType("nodeType")
    const attrs = given ? null : op.attrs("attrs")
    const range = blockRange(tr.doc, from, to, depth)
    const wrappers = given ?? findWrapping(range, nodeType, attrs)
    if (!wrappers) throw new RangeError(`Can't wrap the range from ${from} to ${to} in ${nodeType.name}`)
    tr.wrap(range, wrappers)
  },
  join(tr, op) {
    const pos = op.whole("pos")
    tr.join(pos, op.whole("depth", 1))
  },
  split(tr, op) {
    const pos = op.whole("pos")
    const depth = op.whole("depth", 1)
    tr.split(pos, depth, op.typesAfter("typesAfter"))
  },
  clearIncompatible(tr, op) {
    const pos = op.whole("pos")
    tr.clearIncompatible(pos, op.nodeType("parentType"))
  },
  step(tr, op) {
    tr.step(op.step("step"))
  },
  maybeStep(tr, op) {
    tr.maybeStep(op.step("step"))
  },
}

function transform(doc, ops) {
  if (!Array.isArray(ops)) throw new RangeError("Ops must be an array")
  const tr = new Transform(doc)
  for (const json of ops) {
    const op = new Op(doc.type.schema, json)
    if (!Object.hasOwn(methods, op.name)) throw new RangeError(`Unknown Transform method: ${op.name}`)
    methods[op.name](tr, op)
  }
  return tr
}

const recorded = JSON.parse(readFileSync(new URL("../fixtures/transform.json", import.meta.url), "utf8"))
// The schemas prosemirror-transform's tests use: prosemirror-schema-basic with
// prosemirror-schema-list, then one whose hard_break is the linebreak replacement, one with
// definingForContent blockquotes, one whose doc takes comment marks, and one of text only.
const schemas = recorded.schemas
const built = schemas.map(spec => new Schema(spec))

const text = (text, ...marks) => (marks.length ? { type: "text", marks, text } : { type: "text", text })
function node(type, ...rest) {
  const attrs = isObject(rest[0]) && !("type" in rest[0]) ? rest.shift() : undefined
  const content = rest.map(child => (typeof child === "string" ? text(child) : child))
  return { type, ...(attrs && { attrs }), ...(content.length && { content }) }
}
const doc = (...content) => node("doc", ...content)
const p = (...content) => node("paragraph", ...content)
const blockquote = (...content) => node("blockquote", ...content)
const h = (level, ...content) => node("heading", { level }, ...content)
const ul = (...content) => node("bullet_list", ...content)
const ol = (...content) => node("ordered_list", ...content)
const li = (...content) => node("list_item", ...content)
const code = content => node("code_block", content)
const hr = { type: "horizontal_rule" }
const br = { type: "hard_break" }
const image = src => ({ type: "image", attrs: { src } })
const em = { type: "em" }
const strong = { type: "strong" }
const link = href => ({ type: "link", attrs: { href } })
const comment = id => ({ type: "comment", attrs: { id } })
const slice = (content, openStart, openEnd) => ({ content, ...(openStart && { openStart }), ...(openEnd && { openEnd }) })

const docs = {
  // 0-13 paragraph (1-12 "hello there"), 13-21 paragraph (14-20 "second")
  plain: doc(p("hello there"), p("second")),
  // 0-7 heading, 7-36 paragraph with strong 14-18 and em 23-29, 36-52 blockquote of
  // paragraphs 37-45 and 45-51, 52-63 paragraph with a hard_break at 57, 63-74 code_block,
  // 74-75 horizontal_rule, 75-86 paragraph with an image at 76
  rich: doc(
    h(1, "Title"),
    p("Hello ", text("bold", strong), " and ", text("italic", em), " text."),
    blockquote(p("Quoted"), p("More")),
    p("Last", br, "line"),
    code("let x\n= 1"),
    hr,
    p(image("a.png"), " caption"),
  ),
  // 0-28 bullet_list: items 1-8 (paragraph 2-7) and 8-27 (paragraph 9-14, then a list 14-26 whose
  // item 15-25 has paragraph 16-24), 28-35 ordered_list: item 29-34, paragraph 30-33
  lists: doc(ul(li(p("one")), li(p("two"), ul(li(p("nested"))))), ol(li(p("x")))),
  // 0-8 blockquote of paragraphs 1-4 and 4-7, 8-11 paragraph
  quotes: doc(blockquote(p("a"), p("b")), p("c")),
  // 0-13 paragraph: "go " 1-4 and "here" 4-8 linked, " now" 8-12
  linked: doc(p(text("go ", link("x")), text("here", link("x"), em), " now")),
  // In schema 1: 0-9 paragraph with a hard_break at 4, 9-17 code_block, 17-20 paragraph
  breaks: doc(p("one", br, "two"), code("a\nb\r\nc"), p("x")),
  // In schema 2: 0-24 blockquote of paragraphs 1-12 and 12-23, 24-31 paragraph
  quoted: doc(blockquote({ color: "red" }, p("quote one"), p("quote two")), p("after")),
  // In schema 3: 0-5 p, 5-10 p
  comments: doc(node("p", "abc"), node("p", "def")),
  // In schema 4: text 0-10
  text: doc("plain text"),
}

const cases = []
const add = (schema, start, ...opLists) => {
  for (const ops of opLists) cases.push({ schema, doc: start, ops })
}
const { plain, rich, lists, quotes, linked, breaks, quoted, comments } = docs

add(
  0,
  plain,
  [],
  [{ op: "replace", from: 1, to: 6, slice: slice([text("goodbye")]) }],
  [{ op: "replace", from: 3 }],
  [{ op: "replace", from: 3, to: 16 }],
  [{ op: "replace", from: 6, to: 6, slice: slice([p("X"), p("Y")], 1, 1) }],
  [{ op: "replace", from: 12, to: 14, slice: slice([text(" and ", strong)]) }],
  [{ op: "replace", from: 0, to: 13, slice: slice([h(2, "new")]) }],
  [{ op: "replace", from: 5, to: 999 }],
  [{ op: "replace", from: 1, to: 2, slice: slice([{ type: "nope" }]) }],
  [{ op: "replaceWith", from: 1, to: 6, content: [text("bye")] }],
  [{ op: "replaceWith", from: 1, to: 6, content: text("hi", em) }],
  [{ op: "replaceWith", from: 0, to: 13, content: hr }],
  [{ op: "replaceWith", from: 1, to: 6, content: null }],
  [{ op: "replaceWith", from: 3, to: 3, content: p("inner") }],
  [{ op: "replaceWith", from: 1, to: 6, content: [5] }],
  [{ op: "replaceWith", from: 1, to: 6, content: "text" }],
  [{ op: "replaceWith", from: 1, to: 6 }],
  [{ op: "delete", from: 1, to: 7 }],
  [{ op: "delete", from: 0, to: 13 }],
  [{ op: "delete", from: 0, to: 21 }],
  [{ op: "delete", from: 3, to: 1 }],
  [{ op: "delete", from: "1", to: 2 }],
  [{ op: "delete", from: 1 }],
  [{ op: "delete", from: -1, to: 2 }],
  [{ op: "delete", from: 1.5, to: 2 }],
  [{ op: "delete", from: 0, to: 22 }],
  [{ op: "insert", pos: 6, content: [text(",")] }],
  [{ op: "insert", pos: 13, content: p("middle") }],
  [{ op: "insert", pos: 0, content: hr }],
  [{ op: "insert", pos: 3, content: image("x.png") }],
  [{ op: "insert", pos: 3, content: [text("a", strong), text("b")] }],
  [{ op: "insert", pos: 3, content: { type: "paragraph" } }],
  [{ op: "insert", pos: 21, content: [p("end")] }],
  [{ op: "insert", pos: 30, content: [text("x")] }],
  [{ op: "insert", content: [text("x")] }],
  [{ op: "insert", pos: 3, content: { type: "nope" } }],
  [{ op: "insert", pos: 3, content: text("x", { type: "nope" }) }],
  [{ op: "replaceRange", from: 1, to: 6, slice: slice([text("X")]) }],
  [{ op: "replaceRange", from: 1, to: 20, slice: slice([p("A"), p("B")], 1, 1) }],
  [{ op: "replaceRange", from: 1, to: 6 }],
  [{ op: "replaceRange", from: 1, to: 99, slice: slice([text("X")]) }],
  [{ op: "replaceRangeWith", from: 3, to: 3, node: hr }],
  [{ op: "replaceRangeWith", from: 1, to: 1, node: hr }],
  [{ op: "replaceRangeWith", from: 12, to: 12, node: hr }],
  [{ op: "replaceRangeWith", from: 1, to: 12, node: image("y.png") }],
  [{ op: "replaceRangeWith", from: 0, to: 13, node: p("whole") }],
  [{ op: "replaceRangeWith", from: 1, to: 12 }],
  [{ op: "deleteRange", from: 1, to: 12 }],
  [{ op: "deleteRange", from: 1, to: 14 }],
  [{ op: "deleteRange", from: 3, to: 17 }],
  [{ op: "addMark", from: 1, to: 6, mark: em }],
  [{ op: "addMark", from: 3, to: 17, mark: strong }],
  [{ op: "addMark", from: 1, to: 6, mark: { type: "link", attrs: { href: "u", title: "t" } } }],
  [{ op: "addMark", from: 1, to: 6, mark: { type: "nope" } }],
  [{ op: "addMark", from: 1, to: 6 }],
  [{ op: "addMark", from: 1, to: 6, mark: { type: "link" } }],
  [{ op: "addMark", from: 1, to: 6, mark: { type: "link", attrs: { href: 5 } } }],
  [{ op: "addNodeMark", pos: 21, mark: em }],
  [{ op: "addNodeMark", pos: 99, mark: em }],
  [{ op: "addNodeMark", pos: 0, mark: em }],
  [{ op: "removeNodeMark", pos: 21, mark: "em" }],
  [{ op: "removeNodeMark", pos: 1, mark: null }],
  [{ op: "setDocAttribute", attr: "nope", value: 1 }],
  [{ op: "setDocAttribute", value: 1 }],
  [{ op: "wrap", from: 1, to: 1, nodeType: "blockquote" }],
  [{ op: "wrap", from: 1, to: 20, nodeType: "bullet_list" }],
  [{ op: "wrap", from: 1, to: 1, nodeType: "ordered_list", attrs: { order: 3 } }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [{ type: "blockquote" }] }],
  [{ op: "wrap", from: 1, to: 20, wrappers: [{ type: "bullet_list" }, { type: "list_item" }] }],
  [{ op: "wrap", from: 1, to: 1, depth: 0, nodeType: "blockquote" }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [] }],
  [{ op: "wrap", from: 1, to: 1, nodeType: "heading" }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [{ type: "paragraph" }, { type: "blockquote" }] }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [{ type: "list_item" }] }],
  [{ op: "wrap", from: 1, to: 1 }],
  [{ op: "wrap", from: 1, to: 1, wrappers: "blockquote" }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [null] }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [{ type: "blockquote", attrs: 5 }] }],
  [{ op: "wrap", from: 1, to: 1, wrappers: [{ type: "nope" }] }],
  [{ op: "wrap", from: 1, to: 1, nodeType: "nope" }],
  [{ op: "wrap", from: 1, to: 1, depth: 2, nodeType: "blockquote" }],
  [{ op: "join", pos: 13 }],
  [{ op: "join", pos: 0 }],
  [{ op: "join", pos: 21 }],
  [{ op: "join", pos: 13, depth: "x" }],
  [{ op: "split", pos: 6 }],
  [{ op: "split", pos: 1 }],
  [{ op: "split", pos: 6, typesAfter: [{ type: "heading", attrs: { level: 2 } }] }],
  [{ op: "split", pos: 6, typesAfter: [null] }],
  [{ op: "split", pos: 6, depth: 0 }],
  [{ op: "split", pos: 6, depth: 2 }],
  [{ op: "split", pos: 6, typesAfter: "x" }],
  [{ op: "split", pos: 6, typesAfter: [{ type: "nope" }] }],
  [{ op: "split", pos: 6, typesAfter: [{ type: "blockquote" }] }],
  [{ op: "step", step: { stepType: "replace", from: 1, to: 6, slice: slice([text("hey")]) } }],
  [{ op: "step", step: { stepType: "addMark", from: 1, to: 6, mark: em } }],
  [{ op: "step", step: { stepType: "replace", from: 0, to: 1 } }],
  [{ op: "step", step: { stepType: "nope" } }],
  [{ op: "step" }],
  [{ op: "step", step: { stepType: "replace", from: "a", to: 1 } }],
  [
    { op: "maybeStep", step: { stepType: "replace", from: 0, to: 1 } },
    { op: "addMark", from: 1, to: 6, mark: em },
  ],
  [{ op: "maybeStep", step: { stepType: "replace", from: 1, to: 6 } }],
  [
    { op: "split", pos: 6 },
    { op: "setBlockType", from: 1, type: "heading", attrs: { level: 1 } },
    { op: "addMark", from: 8, to: 14, mark: strong },
    { op: "insert", pos: 0, content: hr },
  ],
  [
    { op: "addMark", from: 1, to: 6, mark: em },
    { op: "delete", from: 1, to: 3 },
    { op: "setBlockType", from: 1, to: 1, type: "blockquote" },
  ],
  [
    { op: "wrap", from: 1, to: 20, nodeType: "bullet_list" },
    { op: "lift", from: 3, to: 3 },
    { op: "join", pos: 13 },
  ],
  [
    { op: "wrap", from: 1, to: 20, nodeType: "bullet_list" },
    { op: "split", pos: 8, depth: 2 },
    { op: "lift", from: 22, to: 22 },
  ],
)

add(
  0,
  rich,
  [{ op: "replace", from: 0, to: 7, slice: slice([p("new")]) }],
  [{ op: "delete", from: 10, to: 40 }],
  [{ op: "replaceRange", from: 38, to: 44, slice: slice([h(2, "H")], 1, 1) }],
  [{ op: "deleteRange", from: 37, to: 51 }],
  [{ op: "addMark", from: 8, to: 35, mark: em }],
  [{ op: "addMark", from: 64, to: 73, mark: strong }],
  [{ op: "addMark", from: 0, to: 86, mark: strong }],
  [{ op: "removeMark", from: 8, to: 35 }],
  [{ op: "removeMark", from: 8, to: 35, mark: "strong" }],
  [{ op: "removeMark", from: 8, to: 35, mark: em }],
  [{ op: "removeMark", from: 15, to: 25, mark: null }],
  [{ op: "removeMark", from: 8, to: 35, mark: "nope" }],
  [{ op: "removeMark", from: 8, to: 35, mark: 5 }],
  [{ op: "addNodeMark", pos: 76, mark: em }],
  [{ op: "removeNodeMark", pos: 76, mark: "em" }],
  [
    { op: "addNodeMark", pos: 76, mark: em },
    { op: "addNodeMark", pos: 76, mark: strong },
    { op: "removeNodeMark", pos: 76, mark: em },
  ],
  [{ op: "setNodeMarkup", pos: 0, type: "paragraph" }],
  [{ op: "setNodeMarkup", pos: 0, attrs: { level: 3 } }],
  [{ op: "setNodeMarkup", pos: 0, type: null, attrs: { level: 2 } }],
  [{ op: "setNodeMarkup", pos: 0, attrs: { level: "x" } }],
  [{ op: "setNodeMarkup", pos: 76, attrs: { src: "b.png", alt: "B" } }],
  [{ op: "setNodeMarkup", pos: 76, attrs: { src: "a.png" }, marks: [em] }],
  [{ op: "setNodeMarkup", pos: 76, marks: [] }],
  [{ op: "setNodeMarkup", pos: 7, type: "heading", attrs: { level: 2 } }],
  [{ op: "setNodeMarkup", pos: 52, type: "code_block" }],
  [{ op: "setNodeMarkup", pos: 36, type: "bullet_list" }],
  [{ op: "setNodeMarkup", pos: 86 }],
  [{ op: "setNodeMarkup", pos: 0, type: "nope" }],
  [{ op: "setNodeMarkup", pos: 0, type: 5 }],
  [{ op: "setNodeMarkup", pos: 0, attrs: [1] }],
  [{ op: "setNodeMarkup", pos: 0, marks: {} }],
  [{ op: "setNodeMarkup", pos: 76, attrs: {} }],
  [{ op: "setNodeAttribute", pos: 0, attr: "level", value: 4 }],
  [{ op: "setNodeAttribute", pos: 76, attr: "alt", value: "an image" }],
  [{ op: "setNodeAttribute", pos: 76, attr: "title" }],
  [{ op: "setNodeAttribute", pos: 0, attr: "unknown", value: true }],
  [{ op: "setNodeAttribute", pos: 0, attr: "level", value: { nested: [1, 2.5, null] } }],
  [{ op: "setNodeAttribute", pos: 86, attr: "level", value: 1 }],
  [{ op: "setNodeAttribute", pos: 0, attr: 5, value: 1 }],
  [{ op: "setBlockType", from: 1, to: 1, type: "paragraph" }],
  [{ op: "setBlockType", from: 0, to: 86, type: "heading", attrs: { level: 2 } }],
  [{ op: "setBlockType", from: 8, type: "code_block" }],
  [{ op: "setBlockType", from: 53, to: 53, type: "code_block" }],
  [{ op: "setBlockType", from: 64, to: 64, type: "paragraph" }],
  [{ op: "setBlockType", from: 38, to: 50, type: "heading", attrs: { level: 3 } }],
  [{ op: "setBlockType", from: 1, to: 1, type: "blockquote" }],
  [{ op: "setBlockType", from: 1, to: 1 }],
  [{ op: "setBlockType", from: 1, to: 1, type: "nope" }],
  [{ op: "setBlockType", from: 5, to: 1, type: "paragraph" }],
  [{ op: "lift", from: 38, to: 38 }],
  [{ op: "lift", from: 38, to: 50 }],
  [{ op: "join", pos: 7 }],
  [{ op: "join", pos: 36 }],
  [{ op: "join", pos: 63 }],
  [{ op: "split", pos: 40, depth: 2 }],
  [{ op: "split", pos: 4, depth: 0 }],
  [{ op: "clearIncompatible", pos: 7, parentType: "code_block" }],
  [{ op: "clearIncompatible", pos: 52, parentType: "code_block" }],
  [{ op: "clearIncompatible", pos: 63, parentType: "paragraph" }],
  [{ op: "clearIncompatible", pos: 75, parentType: "heading" }],
  [{ op: "clearIncompatible", pos: 36, parentType: "paragraph" }],
  [{ op: "clearIncompatible", pos: 36, parentType: "bullet_list" }],
  [{ op: "clearIncompatible", pos: 86, parentType: "paragraph" }],
  [{ op: "clearIncompatible", pos: 0, parentType: "nope" }],
  [{ op: "clearIncompatible", pos: 0 }],
  [
    {
      op: "step",
      step: { stepType: "replaceAround", from: 36, to: 52, gapFrom: 37, gapTo: 51, insert: 0, slice: {}, structure: true },
    },
  ],
  [
    { op: "addMark", from: 1, to: 6, mark: em },
    { op: "setNodeAttribute", pos: 0, attr: "level", value: 2 },
    { op: "lift", from: 38, to: 38 },
    { op: "setBlockType", from: 37, to: 37, type: "code_block" },
    { op: "split", pos: 40 },
    { op: "join", pos: 41 },
  ],
)

add(
  0,
  lists,
  [{ op: "delete", from: 3, to: 17 }],
  [{ op: "deleteRange", from: 3, to: 6 }],
  [{ op: "deleteRange", from: 2, to: 16 }],
  [{ op: "setBlockType", from: 3, to: 3, type: "heading" }],
  [{ op: "lift", from: 17, to: 17 }],
  [{ op: "lift", from: 3, to: 3 }],
  [{ op: "lift", from: 3, to: 12 }],
  [{ op: "lift", from: 31, to: 31 }],
  [{ op: "wrap", from: 17, to: 17, nodeType: "blockquote" }],
  [{ op: "wrap", from: 3, to: 3, nodeType: "bullet_list" }],
  [{ op: "join", pos: 8 }],
  [{ op: "join", pos: 28 }],
  [{ op: "join", pos: 8, depth: 2 }],
  [{ op: "split", pos: 4, depth: 2 }],
  [{ op: "split", pos: 4, depth: 2, typesAfter: [null, { type: "heading" }] }],
  [{ op: "split", pos: 19, depth: 4 }],
  [{ op: "replaceRangeWith", from: 17, to: 17, node: hr }],
  [{ op: "replaceRange", from: 3, to: 31, slice: slice([li(p("mid"))], 2, 2) }],
)

add(
  0,
  quotes,
  [{ op: "replaceRange", from: 1, to: 7, slice: slice([p("new")]) }],
  [{ op: "deleteRange", from: 2, to: 6 }],
  [{ op: "lift", from: 2, to: 2 }],
  [{ op: "lift", from: 2, to: 6 }],
  [{ op: "lift", from: 5, to: 5 }],
  [{ op: "lift", from: 2, to: 2, target: 0 }],
  [{ op: "lift", from: 2, to: 2, target: 1 }],
  [{ op: "lift", from: 2, to: 2, depth: 1 }],
  [{ op: "lift", from: 2, to: 2, depth: 2 }],
  [{ op: "lift", from: 2, to: 2, depth: 0 }],
  [{ op: "lift", from: 9, to: 9 }],
  [{ op: "lift", from: 2, to: 2, depth: 3 }],
  [{ op: "lift", from: 2, to: 2, target: "x" }],
  [{ op: "lift", from: 5, to: 2 }],
  [{ op: "lift", from: 2, to: 99 }],
  [{ op: "wrap", from: 2, to: 6, nodeType: "ordered_list" }],
  [{ op: "join", pos: 4 }],
  [{ op: "join", pos: 8 }],
  [{ op: "split", pos: 3, depth: 2 }],
  [{ op: "split", pos: 3, depth: 3 }],
  [{ op: "setBlockType", from: 0, to: 11, type: "code_block" }],
)

add(
  0,
  linked,
  [{ op: "addMark", from: 1, to: 12, mark: link("y") }],
  [{ op: "addMark", from: 2, to: 6, mark: em }],
  [{ op: "removeMark", from: 1, to: 12, mark: "link" }],
  [{ op: "removeMark", from: 1, to: 12, mark: link("other") }],
  [{ op: "removeMark", from: 1, to: 12, mark: link("x") }],
  [{ op: "setBlockType", from: 1, to: 1, type: "code_block" }],
  [{ op: "split", pos: 6 }],
)

add(
  0,
  p("root ", text("para", em)),
  [{ op: "addMark", from: 0, to: 5, mark: strong }],
  [{ op: "insert", pos: 0, content: [text(">")] }],
  [{ op: "delete", from: 0, to: 9 }],
)

add(
  1,
  breaks,
  [{ op: "setBlockType", from: 1, to: 1, type: "code_block" }],
  [{ op: "setBlockType", from: 10, to: 10, type: "paragraph" }],
  [{ op: "setBlockType", from: 0, to: 20, type: "code_block" }],
  [{ op: "setBlockType", from: 0, to: 20, type: "heading", attrs: { level: 4 } }],
  [{ op: "join", pos: 9 }],
  [{ op: "join", pos: 17 }],
  [{ op: "clearIncompatible", pos: 9, parentType: "paragraph" }],
  [{ op: "clearIncompatible", pos: 0, parentType: "code_block" }],
  [{ op: "setNodeMarkup", pos: 9, type: "paragraph" }],
  [{ op: "insert", pos: 13, content: br }],
  [
    { op: "join", pos: 9 },
    { op: "split", pos: 11 },
    { op: "setBlockType", from: 12, to: 12, type: "code_block" },
  ],
)

add(
  2,
  quoted,
  [{ op: "replaceRange", from: 2, to: 22, slice: slice([blockquote(p("x"))], 2, 2) }],
  [{ op: "replaceRange", from: 0, to: 24, slice: slice([p("replaced")]) }],
  [{ op: "replaceRange", from: 13, to: 22, slice: slice([blockquote({ color: "blue" }, p("y"))], 1, 1) }],
  [{ op: "replaceRange", from: 2, to: 11, slice: slice([p("one"), p("two")], 1, 1) }],
  [{ op: "replaceRangeWith", from: 25, to: 30, node: blockquote(p("q")) }],
  [{ op: "setNodeAttribute", pos: 0, attr: "color", value: "blue" }],
  [{ op: "setNodeMarkup", pos: 0, attrs: { color: "green" } }],
  [{ op: "wrap", from: 25, to: 25, wrappers: [{ type: "blockquote", attrs: { color: "green" } }] }],
  [{ op: "wrap", from: 25, to: 25, nodeType: "blockquote" }],
  [{ op: "lift", from: 2, to: 22 }],
  [{ op: "split", pos: 7, depth: 2, typesAfter: [{ type: "blockquote", attrs: { color: "white" } }] }],
)

add(
  3,
  comments,
  [{ op: "addMark", from: 1, to: 4, mark: comment(1) }],
  [
    { op: "addMark", from: 1, to: 4, mark: comment(1) },
    { op: "addMark", from: 2, to: 8, mark: comment(2) },
  ],
  [
    { op: "addMark", from: 1, to: 9, mark: comment(1) },
    { op: "addMark", from: 1, to: 9, mark: comment(1) },
  ],
  [
    { op: "addMark", from: 1, to: 4, mark: comment(1) },
    { op: "addMark", from: 2, to: 8, mark: comment(2) },
    { op: "removeMark", from: 1, to: 9, mark: "comment" },
  ],
  [
    { op: "addMark", from: 1, to: 4, mark: comment(1) },
    { op: "addMark", from: 2, to: 8, mark: comment(2) },
    { op: "removeMark", from: 3, to: 7, mark: comment(1) },
  ],
  [{ op: "addNodeMark", pos: 0, mark: comment(7) }],
  [
    { op: "addNodeMark", pos: 0, mark: comment(7) },
    { op: "addNodeMark", pos: 0, mark: comment(8) },
    { op: "removeNodeMark", pos: 0, mark: "comment" },
  ],
  [
    { op: "addNodeMark", pos: 5, mark: comment(7) },
    { op: "removeNodeMark", pos: 5, mark: comment(7) },
  ],
  [
    { op: "addNodeMark", pos: 0, mark: comment(7) },
    { op: "split", pos: 2 },
    { op: "join", pos: 3 },
  ],
  [{ op: "addMark", from: 1, to: 4, mark: { type: "comment" } }],
  [{ op: "removeNodeMark", pos: 0, mark: "nope" }],
)

add(
  4,
  docs.text,
  [{ op: "setDocAttribute", attr: "meta", value: { a: 1 } }],
  [{ op: "setDocAttribute", attr: "meta", value: null }],
  [{ op: "setDocAttribute", attr: "meta" }],
  [
    { op: "setDocAttribute", attr: "meta", value: [1, "two"] },
    { op: "insert", pos: 5, content: [text("-")] },
    { op: "delete", from: 0, to: 2 },
  ],
  [{ op: "step", step: { stepType: "docAttr", attr: "meta", value: 5 } }],
  [{ op: "setNodeAttribute", pos: 0, attr: "x", value: 1 }],
  [{ op: "lift", from: 1, to: 1 }],
  [{ op: "wrap", from: 1, to: 3, nodeType: "doc" }],
  [{ op: "split", pos: 3 }],
  [{ op: "replaceWith", from: 0, to: 10, content: [text("new")] }],
)

add(
  0,
  plain,
  {},
  null,
  [{}],
  [5],
  [{ op: "frobnicate" }],
  [{ op: "constructor" }],
  [{ op: 5 }],
  [{ op: "delete", from: 1, to: 3 }, null],
)

const transforms = cases.map(({ schema, doc, ops }) => ({
  schema,
  doc,
  ops,
  ...outcome(() => {
    const tr = transform(Node.fromJSON(built[schema], doc), ops)
    return { result: tr.doc.toJSON(), steps: tr.steps.map(step => step.toJSON()) }
  }),
}))

const leaves = doc(p("a", br, "b"), hr, p(image("i.png")), blockquote(p("q"), hr))
const textBetween = [
  [0, rich, 0, 86],
  [0, rich, 0, 86, "\n"],
  [0, rich, 0, 86, "\n", "[leaf]"],
  [0, rich, 0, 86, "", "*"],
  [0, rich, 0, 86, null, "*"],
  [0, rich, 0, 86, "\n\n", ""],
  [0, rich, 10, 40, " | "],
  [0, rich, 40, 10, "\n"],
  [0, rich, 86, 86],
  [0, rich, 0, 87],
  [0, rich, 50, 200, "\n"],
  [0, lists, 0, 35, "\n"],
  [0, quotes, 2, 9, "/"],
  [0, leaves, 0, 15, "\n", "→"],
  [0, leaves, 0, 15, "\n"],
  [0, leaves, 3, 11, " ", "□"],
  [0, doc(p("a😀b"), p("é")), 1, 8, "¶"],
  [0, doc(p("a😀b"), p("é")), 2, 4],
  [0, doc(p("a😀b"), p("é")), 1, 3],
  [0, doc(p("a😀b"), p("é")), 3, 8, "|"],
  [0, p("in ", text("a", em), " paragraph"), 1, 4],
  [0, text("hello"), 1, 3],
  [0, text("hello"), 0, 100],
  [1, breaks, 0, 20, "\n", "⏎"],
  [4, docs.text, 2, 7],
  [4, docs.text, 0, 11],
].map(([schema, doc, from, to, blockSeparator, leafText]) => ({
  schema,
  doc,
  from,
  to,
  ...(blockSeparator != null && { blockSeparator }),
  ...(leafText != null && { leafText }),
  ...outcome(() => {
    const node = Node.fromJSON(built[schema], doc)
    const result = node.textBetween(from, to, blockSeparator ?? undefined, leafText ?? undefined)
    // Jason, among other JSON readers, refuses a lone surrogate, so text holding one is recorded
    // as the text JSON.stringify writes for it.
    return result.isWellFormed() ? { result } : { resultJSON: JSON.stringify(result) }
  }),
}))

const textContent = [
  [0, plain],
  [0, rich],
  [0, lists],
  [0, leaves],
  [0, p("in ", text("a", em), " paragraph")],
  [0, text("hello")],
  [0, image("i.png")],
  [1, breaks],
  [4, docs.text],
].map(([schema, doc]) => ({ schema, doc, result: Node.fromJSON(built[schema], doc).textContent }))

writeFixture("ops", { schemas, transforms, textBetween, textContent })
