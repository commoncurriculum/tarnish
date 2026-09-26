// prosemirror-model's API over tarnish. Each class wraps a handle of the native bridge in `h`
// and passes calls through; what's left here is JavaScript's side of the boundary: default
// arguments, the shapes an argument may come in, and one wrapper object per native object.
import OrderedMap from "orderedmap"
import native from "./native.mjs"

const finalized = new FinalizationRegistry(({ map, key }) => {
  if (!map.get(key)?.deref()) map.delete(key)
})

// The live wrapper of the native object `key` names, made by `make` if there is none.
function cached(map, key, make) {
  let found = map.get(key)?.deref()
  if (!found) {
    found = make()
    map.set(key, new WeakRef(found))
    finalized.register(found, { map, key })
  }
  return found
}

const nodes = new Map(), fragments = new Map(), marks = new Map(), matches = new Map()
const schemas = new Map()

export class ReplaceError extends Error {}

export class Node {
  #type
  #attrs
  #content
  #marks

  /// @internal
  constructor(h) {
    this.h = h
  }

  get type() { return (this.#type ??= this.h.nodeType()) }
  get attrs() { return (this.#attrs ??= this.h.attrs()) }
  get content() { return (this.#content ??= this.h.content()) }
  get marks() { return (this.#marks ??= this.h.marks()) }
  get children() { return this.content.content }
  get nodeSize() { return this.h.nodeSize }
  get childCount() { return this.content.childCount }
  child(index) { return this.content.child(index) }
  maybeChild(index) { return this.content.maybeChild(index) }
  forEach(f) { this.content.forEach(f) }
  nodesBetween(from, to, f, startPos = 0) { this.h.nodesBetween(from, to, f, startPos) }
  descendants(f) { this.nodesBetween(0, this.content.size, f) }
  get textContent() { return this.h.textContent() }
  textBetween(from, to, blockSeparator, leafText) { return this.h.textBetween(from, to, blockSeparator, leafText) }
  get firstChild() { return this.content.firstChild }
  get lastChild() { return this.content.lastChild }
  eq(other) { return this == other || this.h.eq(other) }
  sameMarkup(other) { return this.h.sameMarkup(other) }
  hasMarkup(type, attrs, marks) { return this.h.hasMarkup(type, attrs, marks) }
  copy(content = null) { return this.h.copy(content ?? Fragment.empty) }
  mark(marks) { return marks == this.marks ? this : this.h.mark(marks) }
  cut(from, to = this.content.size) { return this.h.cut(from, to) }
  slice(from, to = this.content.size, includeParents = false) { return this.h.slice(from, to, includeParents) }
  replace(from, to, slice) { return this.h.replace(from, to, slice) }
  nodeAt(pos) { return this.h.nodeAt(pos) }
  childAfter(pos) { return this.h.childAfter(pos) }
  childBefore(pos) { return this.h.childBefore(pos) }
  resolve(pos) { return this.h.resolve(pos) }
  resolveNoCache(pos) { return this.h.resolve(pos) }
  rangeHasMark(from, to, type) {
    return type instanceof Mark ? this.h.rangeHasMark(from, to, type) : this.h.rangeHasMarkType(from, to, type)
  }
  get isBlock() { return this.type.isBlock }
  get isTextblock() { return this.type.isTextblock }
  get inlineContent() { return this.type.inlineContent }
  get isInline() { return this.type.isInline }
  get isText() { return this.type.isText }
  get isLeaf() { return this.type.isLeaf }
  get isAtom() { return this.type.isAtom }
  toString() { return this.h.toDebugString() }
  contentMatchAt(index) { return this.h.contentMatchAt(index) }
  canReplace(from, to, replacement = Fragment.empty, start = 0, end = replacement.childCount) {
    return this.h.canReplace(from, to, replacement, start, end)
  }
  canReplaceWith(from, to, type, marks) { return this.h.canReplaceWith(from, to, type, marks) }
  canAppend(other) { return this.h.canAppend(other) }
  check() { this.h.check() }
  toJSON() { return this.h.toJson() }
  static fromJSON(schema, json) { return schema.h.nodeFromJson(json) }
}

Node.prototype.text = undefined

export class TextNode extends Node {
  #text

  get text() { return (this.#text ??= this.h.text()) }
  get textContent() { return this.text }
  textBetween(from, to) { return this.h.textBetween(from, to) }
  withText(text) { return this.h.withText(text) }
  cut(from = 0, to = this.text.length) { return this.h.cut(from, to) }
}

export class Fragment {
  #content

  /// @internal
  constructor(h) {
    this.h = h
  }

  get size() { return this.h.size }
  get content() { return (this.#content ??= this.h.children()) }
  nodesBetween(from, to, f, nodeStart = 0, parent) { this.h.nodesBetween(from, to, f, nodeStart, parent) }
  descendants(f) { this.nodesBetween(0, this.size, f) }
  textBetween(from, to, blockSeparator, leafText) { return this.h.textBetween(from, to, blockSeparator, leafText) }
  append(other) { return this.h.append(other) }
  cut(from, to = this.size) { return this.h.cut(from, to) }
  cutByIndex(from, to) { return this.h.cutByIndex(from, to) }
  replaceChild(index, node) { return this.h.replaceChild(index, node) }
  addToStart(node) { return this.h.addToStart(node) }
  addToEnd(node) { return this.h.addToEnd(node) }
  eq(other) { return this.h.eq(other) }
  get firstChild() { return this.content.length ? this.content[0] : null }
  get lastChild() { return this.content.length ? this.content[this.content.length - 1] : null }
  get childCount() { return this.h.childCount }
  child(index) { return this.h.child(index) }
  maybeChild(index) { return this.content[index] || null }
  forEach(f) { this.h.forEach(f) }
  findDiffStart(other, pos = 0) { return this.h.findDiffStart(other, pos) }
  findDiffEnd(other, pos = this.size, otherPos = other.size) { return this.h.findDiffEnd(other, pos, otherPos) }
  findIndex(pos) { return this.h.findIndex(pos) }
  toString() { return this.h.toDebugString() }
  toStringInner() { return this.h.toStringInner() }
  toJSON() { return this.h.toJson() }
  static fromJSON(schema, value) { return native.fragmentFromJson(schema.h, value) }
  static fromArray(array) { return native.fragmentFromArray(array) }

  static from(nodes) {
    if (!nodes) return Fragment.empty
    if (nodes instanceof Fragment) return nodes
    if (Array.isArray(nodes)) return this.fromArray(nodes)
    if (nodes.attrs) return this.fromArray([nodes])
    throw new RangeError("Can not convert " + nodes + " to a Fragment" +
      (nodes.nodesBetween ? " (looks like multiple versions of prosemirror-model were loaded)" : ""))
  }
}

export class Mark {
  #type
  #attrs

  /// @internal
  constructor(h) {
    this.h = h
  }

  get type() { return (this.#type ??= this.h.markType()) }
  get attrs() { return (this.#attrs ??= this.h.attrs()) }
  addToSet(set) { return this.h.addToSet(set) }
  removeFromSet(set) { return this.h.removeFromSet(set) }
  isInSet(set) { return this.h.isInSet(set) }
  eq(other) { return this == other || this.h.eq(other) }
  toJSON() { return this.h.toJson() }
  static fromJSON(schema, json) { return schema.h.markFromJson(json) }
  static sameSet(a, b) { return a == b || native.marksSameSet(a, b) }

  static setFrom(marks) {
    if (!marks || Array.isArray(marks) && marks.length == 0) return Mark.none
    if (marks instanceof Mark) return [marks]
    return native.marksSetFrom(marks)
  }

  static none = []
}

export class Slice {
  constructor(content, openStart, openEnd) {
    this.content = content
    this.openStart = openStart
    this.openEnd = openEnd
  }

  get size() { return native.sliceSize(this) }
  insertAt(pos, fragment) { return native.sliceInsertAt(this, pos, fragment) }
  removeBetween(from, to) { return native.sliceRemoveBetween(this, from, to) }
  eq(other) { return native.sliceEq(this, other) }
  toString() { return native.sliceToDebugString(this) }
  toJSON() { return native.sliceToJson(this) }
  static fromJSON(schema, json) { return native.sliceFromJson(schema.h, json) }
  static maxOpen(fragment, openIsolating = true) { return native.sliceMaxOpen(fragment, openIsolating) }
}

export class ResolvedPos {
  /// @internal
  constructor(h) {
    this.h = h
    this.pos = h.pos
    this.depth = h.depth
    this.parentOffset = h.parentOffset
  }

  /// @internal
  resolveDepth(val) {
    if (val == null) return this.depth
    if (val < 0) return this.depth + val
    return val
  }

  get parent() { return this.node(this.depth) }
  get doc() { return this.node(0) }
  node(depth) { return this.h.node(this.resolveDepth(depth)) }
  index(depth) { return this.h.index(this.resolveDepth(depth)) }
  indexAfter(depth) { return this.h.indexAfter(this.resolveDepth(depth)) }
  start(depth) { return this.h.start(this.resolveDepth(depth)) }
  end(depth) { return this.h.end(this.resolveDepth(depth)) }
  before(depth) { return this.h.before(this.resolveDepth(depth)) }
  after(depth) { return this.h.after(this.resolveDepth(depth)) }
  get textOffset() { return this.h.textOffset }
  get nodeAfter() { return this.h.nodeAfter() }
  get nodeBefore() { return this.h.nodeBefore() }
  posAtIndex(index, depth) { return this.h.posAtIndex(index, this.resolveDepth(depth)) }
  marks() { return this.h.marks() }
  marksAcross($end) { return this.h.marksAcross($end) }
  sharedDepth(pos) { return this.h.sharedDepth(pos) }
  blockRange(other = this, pred) { return this.h.blockRange(other, pred) }
  sameParent(other) { return this.h.sameParent(other) }
  max(other) { return other.pos > this.pos ? other : this }
  min(other) { return other.pos < this.pos ? other : this }
  toString() { return this.h.toDebugString() }
}

export class NodeRange {
  constructor($from, $to, depth) {
    this.$from = $from
    this.$to = $to
    this.depth = depth
  }

  get start() { return this.$from.before(this.depth + 1) }
  get end() { return this.$to.after(this.depth + 1) }
  get parent() { return this.$from.node(this.depth) }
  get startIndex() { return this.$from.index(this.depth) }
  get endIndex() { return this.$to.indexAfter(this.depth) }
}

export class ContentMatch {
  /// @internal
  constructor(h) {
    this.h = h
  }

  static parse(string, nodeTypes) {
    for (let name in nodeTypes) return native.contentMatchParse(nodeTypes[name].schema.h, string)
  }

  get validEnd() { return this.h.validEnd }
  matchType(type) { return this.h.matchType(type) }
  matchFragment(frag, start = 0, end = frag.childCount) { return this.h.matchFragment(frag, start, end) }
  get inlineContent() { return this.h.inlineContent }
  get defaultType() { return this.h.defaultType() }
  compatible(other) { return this.h.compatible(other) }
  fillBefore(after, toEnd = false, startIndex = 0) { return this.h.fillBefore(after, toEnd, startIndex) }
  findWrapping(target) { return this.h.findWrapping(target) }
  get edgeCount() { return this.h.edgeCount }
  edge(n) { return this.h.edge(n) }
  toString() { return this.h.toDebugString() }
}

// Marks as `Mark.setFrom` takes them: none, one, or an array.
function markArray(marks) {
  return marks instanceof Mark ? [marks] : marks
}

class Attribute {
  constructor(options) {
    this.hasDefault = Object.prototype.hasOwnProperty.call(options, "default")
    this.default = options.default
    this.validate = options.validate
  }

  get isRequired() { return !this.hasDefault }
}

function initAttrs(attrs) {
  let result = Object.create(null)
  if (attrs) for (let name in attrs) result[name] = new Attribute(attrs[name])
  return result
}

export class NodeType {
  /// @internal
  constructor(name, schema, spec, h) {
    this.name = name
    this.schema = schema
    this.spec = spec
    this.h = h
    this.groups = h.groups
    this.attrs = initAttrs(spec.attrs)
    this.defaultAttrs = h.defaultAttrs ?? null
    this.isBlock = h.isBlock
    this.isText = h.isText
    this.inlineContent = h.inlineContent
    this.contentMatch = h.contentMatch()
    this.markSet = null
  }

  get isInline() { return !this.isBlock }
  get isTextblock() { return this.isBlock && this.inlineContent }
  get isLeaf() { return this.h.isLeaf }
  get isAtom() { return this.isLeaf || !!this.spec.atom }
  isInGroup(group) { return this.groups.indexOf(group) > -1 }
  get whitespace() { return this.h.whitespace }
  hasRequiredAttrs() { return this.h.hasRequiredAttrs }
  compatibleContent(other) { return this == other || this.h.compatibleContent(other) }
  computeAttrs(attrs) { return this.h.computeAttrs(attrs) }
  create(attrs = null, content, marks) { return this.h.create(attrs, Fragment.from(content), markArray(marks)) }
  createChecked(attrs = null, content, marks) {
    return this.h.createChecked(attrs, Fragment.from(content), markArray(marks))
  }
  createAndFill(attrs = null, content, marks) {
    return this.h.createAndFill(attrs, Fragment.from(content), markArray(marks))
  }
  validContent(content) { return this.h.validContent(content) }
  checkContent(content) { this.h.checkContent(content) }
  checkAttrs(attrs) { this.h.checkAttrs(attrs) }
  allowsMarkType(markType) { return this.h.allowsMarkType(markType) }
  allowsMarks(marks) { return this.h.allowsMarks(marks) }
  allowedMarks(marks) { return this.h.allowedMarks(marks) }
}

export class MarkType {
  /// @internal
  constructor(name, rank, schema, spec, h) {
    this.name = name
    this.rank = rank
    this.schema = schema
    this.spec = spec
    this.h = h
    this.attrs = initAttrs(spec.attrs)
    this.excluded = null
  }

  get instance() { return this.h.defaultAttrs ? this.create() : null }
  create(attrs = null) { return this.h.create(attrs) }
  removeFromSet(set) { return this.h.removeFromSet(set) }
  isInSet(set) { return this.h.isInSet(set) ?? undefined }
  checkAttrs(attrs) { this.h.checkAttrs(attrs) }
  excludes(other) { return this.h.excludes(other) }
}

function entries(map) {
  let result = []
  map.forEach((name, spec) => result.push([name, spec]))
  return result
}

export class Schema {
  cached = Object.create(null)

  constructor(spec) {
    let instanceSpec = this.spec = {}
    for (let prop in spec) instanceSpec[prop] = spec[prop]
    instanceSpec.nodes = OrderedMap.from(spec.nodes)
    instanceSpec.marks = OrderedMap.from(spec.marks || {})

    this.h = new native.SchemaHandle(entries(this.spec.nodes), entries(this.spec.marks), spec.topNode)
    this.nodeList = []
    this.markList = []
    schemas.set(this.h.id, this)

    this.marks = Object.create(null)
    this.spec.marks.forEach((name, markSpec) => {
      let type = new MarkType(name, this.markList.length, this, markSpec, this.h.markType(this.markList.length))
      this.markList.push(this.marks[name] = type)
    })
    for (let type of this.markList) type.excluded = type.h.excluded.map(index => this.markList[index])

    this.nodes = Object.create(null)
    this.spec.nodes.forEach((name, nodeSpec) => {
      let type = new NodeType(name, this, nodeSpec, this.h.nodeType(this.nodeList.length))
      this.nodeList.push(this.nodes[name] = type)
    })
    for (let type of this.nodeList) {
      let markSet = type.h.markSet
      type.markSet = markSet && markSet.map(index => this.markList[index])
    }

    let linebreak = this.h.linebreakReplacement
    this.linebreakReplacement = linebreak == null ? null : this.nodeList[linebreak]
    this.nodeFromJSON = json => Node.fromJSON(this, json)
    this.markFromJSON = json => Mark.fromJSON(this, json)
    this.topNodeType = this.nodeList[this.h.topNodeType]
    this.cached.wrappings = Object.create(null)
  }

  node(type, attrs = null, content, marks) {
    if (typeof type == "string")
      type = this.nodeType(type)
    else if (!(type instanceof NodeType))
      throw new RangeError("Invalid node type: " + type)
    else if (type.schema != this)
      throw new RangeError("Node type from different schema used (" + type.name + ")")
    return type.createChecked(attrs, content, marks)
  }

  text(text, marks) { return this.h.text(text, markArray(marks)) }

  mark(type, attrs) {
    if (typeof type == "string") type = this.marks[type]
    return type.create(attrs)
  }

  /// @internal
  nodeType(name) { return this.nodeList[this.h.expectNodeType(name)] }
}

native.register({
  wrapNode: (h, id) => cached(nodes, id, () => h.isText ? new TextNode(h) : new Node(h)),
  wrapFragment: (h, id) => cached(fragments, id, () => new Fragment(h)),
  wrapMark: (h, id) => cached(marks, id, () => new Mark(h)),
  wrapContentMatch: (h, key) => cached(matches, key, () => new ContentMatch(h)),
  wrapNodeType: (schema, index) => schemas.get(schema).nodeList[index],
  wrapMarkType: (schema, index) => schemas.get(schema).markList[index],
  wrapResolvedPos: h => new ResolvedPos(h),
  wrapNodeRange: ($from, $to, depth) => new NodeRange($from, $to, depth),
  wrapSlice: (content, openStart, openEnd) => new Slice(content, openStart, openEnd),
  markNone: () => Mark.none,
  RangeError,
  SyntaxError,
  Error,
  ReplaceError,
})

Fragment.empty = native.fragmentEmpty()
Slice.empty = new Slice(Fragment.empty, 0, 0)
